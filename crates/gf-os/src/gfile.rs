//! Port of `os/gfile`.
//!
//! Go's package is a thin, friendly layer over `os`/`path/filepath`. The port
//! keeps every function that does not need unported units, and replaces Go's
//! `OpenFile(path, flag int, perm)` with [`open_with`] over
//! [`std::fs::OpenOptions`] because Rust has no integer file flags (ADR-040).
//!
//! Divergences, all recorded in ADR-040:
//!
//! - `OpenFile`/`OpenWithFlag`/`OpenWithFlagPerm` are `open_with`, `open` and
//!   `open_append`; permissions come from the process umask and [`chmod`].
//! - `MainPkgPath` is not ported: Rust has no `package main` source path.
//! - `GetContentsWithCache`/`GetBytesWithCache` wait for `gcache` and
//!   `gfsnotify` (waves 16-17).
//! - `Home` reads the environment (`HOME`, `USERPROFILE`, `HOMEDRIVE` +
//!   `HOMEPATH`) instead of the platform user database.
//! - globbing and pattern matching are implemented here (`path.Match` and
//!   `filepath.Match` semantics) because `std` has no glob.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::SystemTime;

use gf_errors::gerror::{self, Error};
use gf_text::gstr;

use crate::gtime;

mod cache;
mod copy;
mod globstar;
mod replace;
mod scan;
mod search;

pub use cache::{CacheDuration, get_bytes_with_cache, get_contents_with_cache};
pub use copy::{CopyOption, copy, copy_dir, copy_file};
pub use globstar::match_glob;
pub use replace::{replace_dir, replace_dir_func, replace_file, replace_file_func};
pub use scan::{scan_dir, scan_dir_file, scan_dir_file_func, scan_dir_func};
pub use search::search;

/// Wraps an IO error with GoFrame's operation text, like `gerror.Wrapf`.
pub(crate) fn wrap_io(error: std::io::Error, text: String) -> Error {
    let inner = gerror::new(error.to_string());
    gerror::wrap(Some(inner), text.clone()).unwrap_or_else(|| gerror::new(text))
}

/// The file mode used when opening files, like Go's `DefaultPermOpen`.
pub const DEFAULT_PERM_OPEN: u32 = 0o666;

/// The file mode used when copying files, like Go's `DefaultPermCopy`.
pub const DEFAULT_PERM_COPY: u32 = 0o755;

/// The path separator of the current platform, like Go's `gfile.Separator`.
pub const SEPARATOR: char = std::path::MAIN_SEPARATOR;

/// The separator as a string, for callers that build paths by hand.
pub fn separator() -> String {
    SEPARATOR.to_string()
}

/// The maximum depth directory scanning recurses into, like Go's
/// `maxScanDepth`.
pub(crate) const MAX_SCAN_DEPTH: usize = 100_000;

/// Creates `path` and every missing parent, like Go's `Mkdir`.
pub fn mkdir(path: &str) -> Result<(), Error> {
    fs::create_dir_all(path)
        .map_err(|error| wrap_io(error, format!("os.MkdirAll failed for path \"{path}\"")))
}

/// Creates (or truncates) a file, creating its directory recursively, like
/// Go's `Create`.
pub fn create(path: &str) -> Result<File, Error> {
    let dir = dir(path);
    if !exists(&dir) {
        mkdir(&dir)?;
    }
    File::create(path)
        .map_err(|error| wrap_io(error, format!("os.Create failed for name \"{path}\"")))
}

/// Opens a file or directory read-only, like Go's `Open`.
pub fn open(path: &str) -> Result<File, Error> {
    File::open(path).map_err(|error| wrap_io(error, format!("os.Open failed for name \"{path}\"")))
}

/// Opens a file with the given options, replacing Go's integer `O_*` flags.
pub fn open_with(path: &str, options: &OpenOptions) -> Result<File, Error> {
    options
        .open(path)
        .map_err(|error| wrap_io(error, format!("os.OpenFile failed with name \"{path}\"")))
}

/// Opens a file for appending, creating it when missing.
pub fn open_append(path: &str) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    open_with(path, &options)
}

/// The characters Go's `filepath` treats as a separator on this platform.
fn separator_mask() -> String {
    if cfg!(windows) {
        "/\\".to_owned()
    } else {
        "/".to_owned()
    }
}

/// The byte index of the last path separator in `path`.
fn last_separator(path: &str) -> Option<usize> {
    path.char_indices()
        .filter(|(_, character)| *character == '/' || (cfg!(windows) && *character == '\\'))
        .map(|(index, _)| index)
        .next_back()
}

/// Joins path pieces with the platform separator, like Go's `Join`.
///
/// Unlike [`std::path::Path::join`] it does not reset on an absolute piece; it
/// simply concatenates, exactly like GoFrame.
pub fn join(paths: &[&str]) -> String {
    let mut joined = String::new();
    for path in paths {
        if !joined.is_empty() {
            joined.push(SEPARATOR);
        }
        joined.push_str(&gstr::trim_right(path, Some(&separator_mask())));
    }
    joined
}

/// Reports whether `path` exists, like Go's `Exists`.
pub fn exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

/// Reports whether `path` is a directory, like Go's `IsDir`.
pub fn is_dir(path: &str) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
}

/// Returns the absolute path of the current working directory, like `Pwd`.
pub fn pwd() -> String {
    std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Changes the current working directory, like Go's `Chdir`.
pub fn chdir(dir: &str) -> Result<(), Error> {
    std::env::set_current_dir(dir)
        .map_err(|error| wrap_io(error, format!("os.Chdir failed with dir \"{dir}\"")))
}

/// Reports whether `path` is a file (anything that is not a directory), like
/// Go's `IsFile`.
pub fn is_file(path: &str) -> bool {
    stat(path).map(|meta| !meta.is_dir()).unwrap_or(false)
}

/// Returns the metadata of `path`, like Go's `Stat`.
pub fn stat(path: &str) -> Result<fs::Metadata, Error> {
    fs::metadata(path)
        .map_err(|error| wrap_io(error, format!("os.Stat failed for file \"{path}\"")))
}

/// Renames `src` to `dst`, like Go's `Move` (and its alias `Rename`).
pub fn rename(src: &str, dst: &str) -> Result<(), Error> {
    fs::rename(src, dst).map_err(|error| {
        wrap_io(
            error,
            format!("os.Rename failed from \"{src}\" to \"{dst}\""),
        )
    })
}

/// Returns the names of the entries of a directory, like Go's `DirNames`.
pub fn dir_names(path: &str) -> Result<Vec<String>, Error> {
    let entries = fs::read_dir(path)
        .map_err(|error| wrap_io(error, format!("Read dir files failed from path \"{path}\"")))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| gerror::new(error.to_string()))?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    Ok(names)
}

/// Returns the files matching a shell pattern, like Go's `Glob`.
///
/// The pattern supports `*`, `?` and `[class]` per path segment, not `**`;
/// when `only_names` is set the last path element is returned instead of the
/// full path.
pub fn glob(pattern: &str, only_names: bool) -> Result<Vec<String>, Error> {
    let mut matches = glob_matches(pattern)?;
    matches.sort();
    if only_names {
        matches = matches.iter().map(|path| basename(path)).collect();
    }
    Ok(matches)
}

/// Removes `path` recursively, doing nothing for an empty path, like Go's
/// deprecated `Remove`.
pub fn remove(path: &str) -> Result<(), Error> {
    if path.is_empty() {
        return Ok(());
    }
    remove_all(path)
}

/// Removes one file or empty directory, like Go's `RemoveFile`.
pub fn remove_file(path: &str) -> Result<(), Error> {
    fs::remove_file(path)
        .map_err(|error| wrap_io(error, format!("os.Remove failed for path \"{path}\"")))
}

/// Removes `path` and its children, like Go's `RemoveAll`.
pub fn remove_all(path: &str) -> Result<(), Error> {
    fs::remove_dir_all(path)
        .or_else(|error| {
            // `remove_dir_all` refuses files; Go's RemoveAll removes them.
            if error.kind() == std::io::ErrorKind::NotADirectory {
                fs::remove_file(path)
            } else if error.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| wrap_io(error, format!("os.RemoveAll failed for path \"{path}\"")))
}

/// Reports whether `path` can be opened for reading, like Go's `IsReadable`.
pub fn is_readable(path: &str) -> bool {
    match File::open(path) {
        Ok(file) => {
            drop(file);
            true
        }
        Err(_) => false,
    }
}

/// Reports whether `path` can be written, like Go's `IsWritable`.
///
/// A directory is probed by creating and removing a temporary file inside it.
pub fn is_writable(path: &str) -> bool {
    if is_dir(path) {
        let probe = join(&[
            path,
            &SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos().to_string())
                .unwrap_or_default(),
        ]);
        match create(&probe) {
            Ok(file) => {
                drop(file);
                let _ = remove(&probe);
                true
            }
            Err(_) => false,
        }
    } else {
        match OpenOptions::new().write(true).open(path) {
            Ok(file) => {
                drop(file);
                true
            }
            Err(_) => false,
        }
    }
}

/// Changes the permissions of `path`, like Go's `Chmod`.
pub fn chmod(path: &str, mode: u32) -> Result<(), Error> {
    let map =
        |error: std::io::Error| wrap_io(error, format!("os.Chmod failed with path \"{path}\""));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(map)
    }
    #[cfg(not(unix))]
    {
        // Only the read-only bit is portable on Windows.
        let current = fs::metadata(path).map_err(map)?.permissions();
        let mut permissions = current;
        permissions.set_readonly(mode & 0o200 == 0);
        fs::set_permissions(path, permissions).map_err(map)
    }
}

/// Returns the absolute form of `path`, like Go's `Abs`.
pub fn abs(path: &str) -> String {
    let path = Path::new(path);
    if path.is_absolute() {
        return clean_path(path);
    }
    Path::new(&pwd()).join(path).to_string_lossy().into_owned()
}

/// Returns the absolute path of `path` when it exists, or an empty string,
/// like Go's `RealPath`.
pub fn real_path(path: &str) -> String {
    let absolute = abs(path);
    if exists(&absolute) {
        absolute
    } else {
        String::new()
    }
}

/// Returns the absolute path of the running binary, like Go's `SelfPath`.
pub fn self_path() -> String {
    std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Returns the binary name of the running process, like Go's `SelfName`.
pub fn self_name() -> String {
    basename(&self_path())
}

/// Returns the directory of the running binary, like Go's `SelfDir`.
pub fn self_dir() -> String {
    dir(&self_path())
}

/// Returns the last path element, like Go's `Basename`.
pub fn basename(path: &str) -> String {
    let trimmed = gstr::trim_right(path, Some(&separator_mask()));
    if trimmed.is_empty() {
        if path.is_empty() {
            return ".".to_owned();
        }
        return separator();
    }
    match last_separator(&trimmed) {
        Some(index) => trimmed[index + 1..].to_owned(),
        None => trimmed.to_owned(),
    }
}

/// Returns the last path element without its extension, like Go's `Name`.
pub fn name(path: &str) -> String {
    let base = basename(path);
    match base.rfind('.') {
        Some(index) => base[..index].to_owned(),
        None => base,
    }
}

/// Returns the directory part of `path`, like Go's `Dir`.
pub fn dir(path: &str) -> String {
    if path == "." {
        return dir(&real_path(path));
    }
    let trimmed = gstr::trim_right(path, Some(&separator_mask()));
    if trimmed.is_empty() {
        return separator();
    }
    match last_separator(&trimmed) {
        None => ".".to_owned(),
        Some(0) => separator(),
        Some(index) => trimmed[..index].to_owned(),
    }
}

/// Reports whether `path` is empty, like Go's `IsEmpty`.
///
/// A missing path counts as empty.
pub fn is_empty(path: &str) -> bool {
    let Ok(meta) = stat(path) else {
        return true;
    };
    if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return true;
        };
        return entries.count() == 0;
    }
    meta.len() == 0
}

/// Returns the file extension including the dot, like Go's `Ext`.
pub fn ext(path: &str) -> String {
    let base = basename(path);
    let Some(index) = base.rfind('.') else {
        return String::new();
    };
    // A leading dot is part of the name, not an extension separator.
    if index == 0 && base.len() > 1 {
        return String::new();
    }
    let extension = &base[index..];
    match extension.find('?') {
        Some(position) => extension[..position].to_owned(),
        None => extension.to_owned(),
    }
}

/// Returns the file extension without the dot, like Go's `ExtName`.
pub fn ext_name(path: &str) -> String {
    ext(path).trim_start_matches('.').to_owned()
}

/// Returns the system temporary directory, joining `names`, like Go's `Temp`.
pub fn temp(names: &[&str]) -> String {
    let mut path = std::env::temp_dir().to_string_lossy().into_owned();
    for name in names {
        path = join(&[&path, name]);
    }
    path
}

/// Returns the current user's home directory, like Go's `Home`.
pub fn home(names: &[&str]) -> Result<String, Error> {
    let mut path = home_path()?;
    for name in names {
        path = join(&[&path, name]);
    }
    Ok(path)
}

/// Resolves the home directory from the environment, like `getHomePath`.
fn home_path() -> Result<String, Error> {
    for key in ["HOME", "USERPROFILE"] {
        if let Some(value) = std::env::var(key).ok().filter(|value| !value.is_empty()) {
            return Ok(value);
        }
    }
    let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
    let path = std::env::var("HOMEPATH").unwrap_or_default();
    if !drive.is_empty() && !path.is_empty() {
        return Ok(format!("{drive}{path}"));
    }
    Err(gerror::new(
        "environment keys HOME, USERPROFILE, HOMEDRIVE and HOMEPATH are empty",
    ))
}

// ---------------------------------------------------------------------------
// File contents
// ---------------------------------------------------------------------------

/// The buffer size used by [`get_next_char_offset`], like Go's
/// `DefaultReadBuffer`.
pub const DEFAULT_READ_BUFFER: usize = 1024;

/// Returns the content of `path` as text, or an empty string, like
/// `GetContents`.
pub fn get_contents(path: &str) -> String {
    String::from_utf8_lossy(&get_bytes(path)).into_owned()
}

/// Returns the content of `path` as bytes, or an empty vector, like
/// `GetBytes`.
pub fn get_bytes(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_default()
}

/// Truncates `path` to `size` bytes, like Go's `Truncate`.
pub fn truncate(path: &str, size: usize) -> Result<(), Error> {
    OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.set_len(size as u64))
        .map_err(|error| {
            wrap_io(
                error,
                format!("os.Truncate failed for file \"{path}\", size \"{size}\""),
            )
        })
}

/// Writes `content` to `path`, creating it (and its directory), like
/// `PutContents`.
pub fn put_contents(path: &str, content: &str) -> Result<(), Error> {
    put_bytes(path, content.as_bytes())
}

/// Appends `content` to `path`, creating it when missing, like
/// `PutContentsAppend`.
pub fn put_contents_append(path: &str, content: &str) -> Result<(), Error> {
    put_bytes_append(path, content.as_bytes())
}

/// Writes `content` to `path`, creating it (and its directory), like
/// `PutBytes`.
pub fn put_bytes(path: &str, content: &[u8]) -> Result<(), Error> {
    let dir = dir(path);
    if !exists(&dir) {
        mkdir(&dir)?;
    }
    let mut file = File::create(path)
        .map_err(|error| wrap_io(error, format!("Write data to file \"{path}\" failed")))?;
    file.write_all(content)
        .map_err(|error| wrap_io(error, format!("Write data to file \"{path}\" failed")))
}

/// Appends `content` to `path`, creating it (and its directory), like
/// `PutBytesAppend`.
pub fn put_bytes_append(path: &str, content: &[u8]) -> Result<(), Error> {
    let dir = dir(path);
    if !exists(&dir) {
        mkdir(&dir)?;
    }
    let mut file = open_append(path)?;
    file.write_all(content)
        .map_err(|error| wrap_io(error, format!("Write data to file \"{path}\" failed")))
}

/// Returns the offset of `character` in `reader`, starting at `start`, or
/// `-1`, like `GetNextCharOffset`.
pub fn get_next_char_offset<R: Read + Seek>(reader: &mut R, character: u8, start: u64) -> i64 {
    let mut buffer = vec![0u8; DEFAULT_READ_BUFFER];
    let mut offset = start;
    loop {
        let read = match reader.seek(SeekFrom::Start(offset)) {
            Ok(_) => reader.read(&mut buffer).unwrap_or(0),
            Err(_) => 0,
        };
        if read == 0 {
            return -1;
        }
        if let Some(position) = buffer[..read].iter().position(|byte| *byte == character) {
            return (offset + position as u64) as i64;
        }
        offset += read as u64;
    }
}

/// [`get_next_char_offset`] for a path, like `GetNextCharOffsetByPath`.
pub fn get_next_char_offset_by_path(path: &str, character: u8, start: u64) -> i64 {
    match File::open(path) {
        Ok(mut file) => get_next_char_offset(&mut file, character, start),
        Err(_) => -1,
    }
}

/// Reads from `start` up to and including `character`, like
/// `GetBytesTilChar`.
pub fn get_bytes_til_char<R: Read + Seek>(
    reader: &mut R,
    character: u8,
    start: u64,
) -> (Vec<u8>, i64) {
    let offset = get_next_char_offset(reader, character, start);
    if offset == -1 {
        return (Vec::new(), -1);
    }
    (
        get_bytes_by_two_offsets(reader, start, offset as u64 + 1),
        offset,
    )
}

/// [`get_bytes_til_char`] for a path, like `GetBytesTilCharByPath`.
pub fn get_bytes_til_char_by_path(path: &str, character: u8, start: u64) -> (Vec<u8>, i64) {
    match File::open(path) {
        Ok(mut file) => get_bytes_til_char(&mut file, character, start),
        Err(_) => (Vec::new(), -1),
    }
}

/// Reads the byte range `[start, end)`, like `GetBytesByTwoOffsets`.
pub fn get_bytes_by_two_offsets<R: Read + Seek>(reader: &mut R, start: u64, end: u64) -> Vec<u8> {
    if end < start {
        return Vec::new();
    }
    let mut buffer = vec![0u8; (end - start) as usize];
    match reader.seek(SeekFrom::Start(start)) {
        Ok(_) => match reader.read_exact(&mut buffer) {
            Ok(()) => buffer,
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}

/// [`get_bytes_by_two_offsets`] for a path, like `GetBytesByTwoOffsetsByPath`.
pub fn get_bytes_by_two_offsets_by_path(path: &str, start: u64, end: u64) -> Vec<u8> {
    match File::open(path) {
        Ok(mut file) => get_bytes_by_two_offsets(&mut file, start, end),
        Err(_) => Vec::new(),
    }
}

/// Calls `callback` with each line of `file`, like Go's `ReadLines`.
///
/// Lines are separated by `\n` or `\r\n`; the markers are stripped and a last
/// line without a newline is still visited.
pub fn read_lines(
    file: &str,
    mut callback: impl FnMut(&str) -> Result<(), Error>,
) -> Result<(), Error> {
    let handle = open(file)?;
    for line in BufReader::new(handle).lines() {
        let line = line.map_err(|error| gerror::new(error.to_string()))?;
        callback(&line)?;
    }
    Ok(())
}

/// Calls `callback` with each line of `file` as bytes, like `ReadLinesBytes`.
pub fn read_lines_bytes(
    file: &str,
    mut callback: impl FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let handle = open(file)?;
    let mut reader = BufReader::new(handle);
    loop {
        let mut buffer = Vec::new();
        match reader.read_until(b'\n', &mut buffer) {
            Ok(0) => return Ok(()),
            Ok(_) => {
                let view = &buffer[..];
                callback(view)?;
            }
            Err(error) => return Err(gerror::new(error.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Returns the size of `path` in bytes, or zero, like Go's `Size`.
pub fn size(path: &str) -> i64 {
    fs::metadata(path)
        .map(|meta| meta.len() as i64)
        .unwrap_or(0)
}

/// Returns the formatted size of `path`, like Go's `SizeFormat`.
pub fn size_format(path: &str) -> String {
    format_size(size(path))
}

/// Returns the human readable size of `path`, like Go's `ReadableSize`.
pub fn readable_size(path: &str) -> String {
    format_size(size(path))
}

/// Converts a formatted size to bytes, like Go's `StrToSize`.
pub fn str_to_size(size_str: &str) -> i64 {
    let bytes = size_str.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'.' || byte.is_ascii_digit() {
            index += 1;
        } else {
            break;
        }
    }
    let number = size_str[..index].parse::<f64>().unwrap_or(0.0);
    let unit = size_str[index..].to_lowercase();
    let multiplier = match unit.as_str() {
        "" => 1.0,
        "b" | "bytes" => 1.0,
        "k" | "kb" | "ki" | "kib" | "kilobyte" => 1024.0,
        "m" | "mb" | "mi" | "mib" | "mebibyte" => 1024.0 * 1024.0,
        "g" | "gb" | "gi" | "gib" | "gigabyte" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" | "ti" | "tib" | "terabyte" => 1024.0_f64.powi(4),
        "p" | "pb" | "pi" | "pib" | "petabyte" => 1024.0_f64.powi(5),
        "e" | "eb" | "ei" | "eib" | "exabyte" => 1024.0_f64.powi(6),
        "z" | "zb" | "zi" | "zib" | "zettabyte" => 1024.0_f64.powi(7),
        "y" | "yb" | "yi" | "yib" | "yottabyte" => 1024.0_f64.powi(8),
        "bb" | "brontobyte" => 1024.0_f64.powi(9),
        _ => return -1,
    };
    if unit.is_empty() {
        return number as i64;
    }
    (number * multiplier) as i64
}

/// Formats a byte count like Go's `FormatSize` (`1.50K`, `2.00M`, ...).
pub fn format_size(raw: i64) -> String {
    let bytes = raw as f64;
    const UNITS: [&str; 10] = ["B", "K", "M", "G", "T", "P", "E", "Z", "Y", "BB"];
    let mut divisor = 1.0_f64;
    let mut threshold = 1024.0_f64;
    for unit in UNITS {
        if unit == "BB" {
            return format!("{:.2}BB", bytes / divisor);
        }
        if bytes < threshold {
            return format!("{:.2}{unit}", bytes / divisor);
        }
        divisor *= 1024.0;
        threshold *= 1024.0;
    }
    "TooLarge".to_owned()
}

// ---------------------------------------------------------------------------
// Times
// ---------------------------------------------------------------------------

/// Returns the modification time of `path`, like Go's `MTime`.
pub fn mtime(path: &str) -> gtime::Time {
    match fs::metadata(path).and_then(|meta| meta.modified()) {
        Ok(modified) => match modified.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(elapsed) => gtime::Time::new_from_timestamp_milli(
                i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
            ),
            Err(_) => gtime::Time::new_from_timestamp(0),
        },
        Err(_) => gtime::Time::new_from_timestamp(0),
    }
}

/// Returns the modification time of `path` in unix seconds, like
/// `MTimestamp`; `-1` when the file is missing.
pub fn mtimestamp(path: &str) -> i64 {
    let mtime = mtime(path);
    if mtime.is_zero() {
        -1
    } else {
        mtime.timestamp()
    }
}

/// Returns the modification time of `path` in unix milliseconds, like
/// `MTimestampMilli`; `-1` when the file is missing.
pub fn mtimestamp_milli(path: &str) -> i64 {
    let mtime = mtime(path);
    if mtime.is_zero() {
        -1
    } else {
        mtime.timestamp_milli()
    }
}

// ---------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------

/// Sorts absolute paths as directories first, then files, then text, like
/// Go's `SortFiles`.
pub fn sort_files(files: Vec<String>) -> Vec<String> {
    let mut files = files;
    files.sort_by(|left, right| match file_sort_func(left, right) {
        value if value < 0 => std::cmp::Ordering::Less,
        value if value > 0 => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    files
}

/// The comparison behind [`sort_files`], like Go's `fileSortFunc`.
fn file_sort_func(path1: &str, path2: &str) -> i32 {
    let dir1 = is_dir(path1);
    let dir2 = is_dir(path2);
    if dir1 && !dir2 {
        return -1;
    }
    if !dir1 && dir2 {
        return 1;
    }
    match path1.cmp(path2) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => -1,
    }
}

// ---------------------------------------------------------------------------
// Path cleaning and globbing helpers
// ---------------------------------------------------------------------------

/// Collapses redundant separators and `.`/`..` segments, like `path.Clean`.
fn clean_path(path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    let absolute = path.is_absolute();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => {
                parts.push(prefix.as_os_str().to_string_lossy().into_owned());
            }
            std::path::Component::RootDir => {}
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if parts.last().map(|part| part != "..").unwrap_or(false) {
                    parts.pop();
                } else if !absolute {
                    parts.push("..".to_owned());
                }
            }
            std::path::Component::Normal(name) => {
                parts.push(name.to_string_lossy().into_owned());
            }
        }
    }
    let separator = SEPARATOR.to_string();
    let joined = parts.join(&separator);
    if absolute {
        // Keep the drive prefix (`C:`) followed by the root.
        if joined.ends_with(':') || joined.is_empty() {
            format!("{joined}{separator}")
        } else {
            joined
        }
    } else if joined.is_empty() {
        ".".to_owned()
    } else {
        joined
    }
}

/// Expands a glob pattern into existing paths, like `filepath.Glob`.
fn glob_matches(pattern: &str) -> Result<Vec<String>, Error> {
    if pattern.is_empty() {
        return Ok(Vec::new());
    }
    let normalized = pattern.replace('\\', "/");
    let segments: Vec<&str> = normalized.split('/').collect();
    let first_meta = segments
        .iter()
        .position(|segment| segment.contains(['*', '?', '[']));
    let Some(first_meta) = first_meta else {
        return Ok(if exists(pattern) {
            vec![pattern.to_owned()]
        } else {
            Vec::new()
        });
    };
    let prefix = segments[..first_meta].join(&SEPARATOR.to_string());
    let root = if prefix.is_empty() {
        ".".to_owned()
    } else {
        prefix
    };
    let mut current = vec![root];
    for segment in &segments[first_meta..] {
        let mut next = Vec::new();
        for base in &current {
            if segment.is_empty() {
                next.push(base.clone());
                continue;
            }
            if !segment.contains(['*', '?', '[']) {
                let candidate = Path::new(base).join(segment);
                if exists(&candidate.to_string_lossy()) {
                    next.push(candidate.to_string_lossy().into_owned());
                }
                continue;
            }
            let Ok(entries) = fs::read_dir(base) else {
                continue;
            };
            let mut names: Vec<String> = Vec::new();
            for entry in entries.flatten() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
            names.sort();
            for name in names {
                if path_match(segment, &name)? {
                    next.push(Path::new(base).join(&name).to_string_lossy().into_owned());
                }
            }
        }
        current = next;
    }
    Ok(current)
}

/// Matches `name` against a shell pattern without globstar, like Go's
/// `path.Match`: `*` and `?` never match the separator.
pub fn path_match(pattern: &str, name: &str) -> Result<bool, Error> {
    match_path(pattern.as_bytes(), name.as_bytes())
}

/// The recursive byte matcher behind [`path_match`].
fn match_path(pattern: &[u8], name: &[u8]) -> Result<bool, Error> {
    if pattern.is_empty() {
        return Ok(name.is_empty());
    }
    match pattern[0] {
        b'*' => {
            let mut rest = 1;
            while rest < pattern.len() && pattern[rest] == b'*' {
                rest += 1;
            }
            let tail = &pattern[rest..];
            if tail.is_empty() {
                return Ok(!name.contains(&b'/'));
            }
            for index in 0..=name.len() {
                if name[..index].contains(&b'/') {
                    break;
                }
                if match_path(tail, &name[index..])? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        b'?' => {
            if name.is_empty() || name[0] == b'/' {
                return Ok(false);
            }
            match_path(&pattern[1..], &name[1..])
        }
        b'[' => {
            // Parse the class first so a malformed pattern errors even when
            // the name is exhausted, like Go's `path.Match`.
            let (matched, consumed) =
                match_class(&pattern[1..], name.first().copied().unwrap_or(0))?;
            if name.is_empty() || name[0] == b'/' {
                return Ok(false);
            }
            if !matched {
                return Ok(false);
            }
            match_path(&pattern[1 + consumed..], &name[1..])
        }
        b'\\' => {
            if pattern.len() < 2 {
                return Err(gerror::new("path.Match: trailing backslash"));
            }
            if name.first() == Some(&pattern[1]) {
                match_path(&pattern[2..], &name[1..])
            } else {
                Ok(false)
            }
        }
        byte => {
            if name.first() == Some(&byte) {
                match_path(&pattern[1..], &name[1..])
            } else {
                Ok(false)
            }
        }
    }
}

/// Matches one character class starting after `[`, returning whether it
/// matched and how many bytes the class consumed.
fn match_class(pattern: &[u8], character: u8) -> Result<(bool, usize), Error> {
    let mut index = 0;
    let negate = pattern.get(index) == Some(&b'^');
    if negate {
        index += 1;
    }
    let mut matched = false;
    let mut first = true;
    while index < pattern.len() {
        if pattern[index] == b']' && !first {
            index += 1;
            return Ok((matched != negate, index));
        }
        first = false;
        let low = pattern[index];
        if low == b'\\' {
            index += 1;
            if index >= pattern.len() {
                return Err(gerror::new("path.Match: bad character class"));
            }
        }
        let low = pattern[index];
        index += 1;
        if index + 1 < pattern.len() && pattern[index] == b'-' && pattern[index + 1] != b']' {
            index += 1;
            let high = pattern[index];
            index += 1;
            if (low..=high).contains(&character) {
                matched = true;
            }
        } else if character == low {
            matched = true;
        }
    }
    Err(gerror::new("path.Match: bad character class"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> String {
        let path = temp(&["gf-rust-gfile-tests", name]);
        let _ = remove(&path);
        mkdir(&path).expect("sandbox");
        path
    }

    #[test]
    fn path_helpers_match_go() {
        assert_eq!(basename("/var/www/file.js"), "file.js");
        assert_eq!(basename("file.js"), "file.js");
        assert_eq!(basename(""), ".");
        assert_eq!(name("/var/www/file.js"), "file");
        assert_eq!(name("file.js"), "file");
        assert_eq!(name("file"), "file");
        assert_eq!(dir("/var/www/file.js"), "/var/www");
        assert_eq!(dir("file.js"), ".");
        assert_eq!(ext("main.go"), ".go");
        assert_eq!(ext("api.json?x=1"), ".json");
        assert_eq!(ext("file"), "");
        assert_eq!(ext_name("main.go"), "go");
        assert_eq!(join(&["a", "b", "c"]), format!("a{SEPARATOR}b{SEPARATOR}c"));
        assert_eq!(join(&["a/", "b"]), format!("a{SEPARATOR}b"));
    }

    #[test]
    fn file_operations_round_trip() {
        let base = sandbox("operations");
        let file = join(&[&base, "sub", "file.txt"]);
        assert!(!exists(&file));
        // `create` builds the parent directories, like Go's `gfile.Create`.
        let handle = create(&file).expect("create");
        drop(handle);
        assert!(exists(&file));

        put_contents(&file, "hello").expect("put");
        assert!(exists(&file));
        assert!(is_file(&file));
        assert!(!is_dir(&file));
        assert_eq!(get_contents(&file), "hello");
        assert_eq!(get_bytes(&file), b"hello");
        assert_eq!(size(&file), 5);
        assert_eq!(ext_name(&file), "txt");
        assert_eq!(basename(&file), "file.txt");
        assert_eq!(dir(&file), join(&[&base, "sub"]));
        assert_eq!(size_format(&file), "5.00B");

        put_contents_append(&file, " world").expect("append");
        assert_eq!(get_contents(&file), "hello world");

        truncate(&file, 5).expect("truncate");
        assert_eq!(get_contents(&file), "hello");

        let target = join(&[&base, "target.txt"]);
        copy_file(&file, &target, CopyOption::default()).expect("copy");
        assert_eq!(get_contents(&target), "hello");

        let moved = join(&[&base, "moved.txt"]);
        rename(&target, &moved).expect("rename");
        assert!(!exists(&target));
        assert!(exists(&moved));

        assert!(is_readable(&moved));
        assert!(is_writable(&moved));

        remove_file(&moved).expect("remove file");
        assert!(!exists(&moved));
        remove_all(&base).expect("remove dir");
        assert!(!exists(&base));
    }

    #[test]
    fn dir_names_and_empty() {
        let base = sandbox("names");
        put_contents(&join(&[&base, "b.txt"]), "b").expect("b");
        put_contents(&join(&[&base, "a.txt"]), "a").expect("a");
        mkdir(&join(&[&base, "child"])).expect("child");

        let mut names = dir_names(&base).expect("names");
        names.sort();
        assert_eq!(names, vec!["a.txt", "b.txt", "child"]);
        assert!(!is_empty(&base));
        assert!(is_empty(&join(&[&base, "child"])));
        assert!(is_empty(&join(&[&base, "missing"])));

        let empty_file = join(&[&base, "empty.txt"]);
        put_contents(&empty_file, "").expect("empty file");
        assert!(is_empty(&empty_file));
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn offsets_and_lines() {
        let base = sandbox("lines");
        let file = join(&[&base, "data.txt"]);
        put_contents(&file, "abc\ndef\nghi").expect("put");

        assert_eq!(get_next_char_offset_by_path(&file, b'\n', 0), 3);
        assert_eq!(get_next_char_offset_by_path(&file, b'\n', 4), 7);
        assert_eq!(get_next_char_offset_by_path(&file, b'z', 0), -1);

        let (bytes, offset) = get_bytes_til_char_by_path(&file, b'\n', 0);
        assert_eq!(bytes, b"abc\n");
        assert_eq!(offset, 3);
        assert_eq!(get_bytes_by_two_offsets_by_path(&file, 4, 7), b"def");

        let mut lines = Vec::new();
        read_lines(&file, |line| {
            lines.push(line.to_owned());
            Ok(())
        })
        .expect("read lines");
        assert_eq!(lines, vec!["abc", "def", "ghi"]);

        let mut raw = Vec::new();
        read_lines_bytes(&file, |line| {
            raw.push(line.to_vec());
            Ok(())
        })
        .expect("read bytes lines");
        assert_eq!(raw.len(), 3);

        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn sizes_round_trip() {
        assert_eq!(format_size(0), "0.00B");
        assert_eq!(format_size(1024), "1.00K");
        assert_eq!(format_size(1024 * 1024), "1.00M");
        assert_eq!(format_size(1536), "1.50K");
        assert_eq!(str_to_size("1.5K"), 1536);
        assert_eq!(str_to_size("2M"), 2 * 1024 * 1024);
        assert_eq!(str_to_size("100"), 100);
        assert_eq!(str_to_size("1x"), -1);
    }

    #[test]
    fn glob_and_match() {
        let base = sandbox("glob");
        put_contents(&join(&[&base, "a.go"]), "").expect("a");
        put_contents(&join(&[&base, "b.txt"]), "").expect("b");
        mkdir(&join(&[&base, "sub"])).expect("sub");
        put_contents(&join(&[&base, "sub", "c.go"]), "").expect("c");

        let pattern = join(&[&base, "*.go"]).replace('\\', "/");
        let matches = glob(&pattern, false).expect("glob");
        assert_eq!(matches.len(), 1);
        assert!(matches[0].ends_with("a.go"));

        let names = glob(&pattern, true).expect("glob names");
        assert_eq!(names, vec!["a.go"]);

        assert!(path_match("*.go", "a.go").expect("match"));
        assert!(!path_match("*.go", "a.txt").expect("match"));
        assert!(path_match("[a-c].go", "b.go").expect("class"));
        assert!(!path_match("[^a-c].go", "b.go").expect("negated class"));
        assert!(path_match("a?c", "abc").expect("single"));
        assert!(!path_match("a?c", "a/c").expect("separator"));
        assert!(path_match(r"\*", "*").expect("escape"));
        assert!(path_match("[", "a").is_err());

        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn sorting_puts_directories_first() {
        let base = sandbox("sort");
        let file = join(&[&base, "file.txt"]);
        put_contents(&file, "").expect("file");
        let child = join(&[&base, "child"]);
        mkdir(&child).expect("child");
        let sorted = sort_files(vec![file.clone(), child.clone()]);
        assert_eq!(sorted, vec![child, file]);
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn mtime_and_self_path() {
        let base = sandbox("time");
        let file = join(&[&base, "t.txt"]);
        put_contents(&file, "x").expect("put");
        assert!(mtimestamp(&file) > 0);
        assert!(mtimestamp_milli(&file) > 0);
        assert_eq!(mtimestamp(&join(&[&base, "missing"])), -1);
        assert!(!self_path().is_empty());
        assert!(!self_dir().is_empty());
        assert!(!self_name().is_empty());
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn home_resolves() {
        let home_dir = home(&[]).expect("home");
        assert!(!home_dir.is_empty());
        assert!(home(&["x"]).expect("home x").ends_with("x"));
    }
}
