//! Port of `os/gres`.
//!
//! GoFrame packs a directory tree into a gzip-compressed zip archive, embeds the
//! archive (base64 or raw) and exposes it through a [`Resource`] that looks like
//! a read-only file system: `get`, `get_with_index`, `scan_dir`, `export`.
//!
//! Go's `File` wraps a `*zip.File`; the port extracts every entry when the
//! archive is unpacked, because the `zip` crate's entries borrow the archive and
//! the resource tree needs owned values (ADR-044). `PackToGoFile`, which emits a
//! Go source file, has no Rust counterpart and is not ported.
//!
//! Divergences, recorded in ADR-044:
//!
//! - `gres.Option` is `PackOption` (the name `Option` is reserved for the
//!   standard enum), `File::FileInfo()` is `file_info()` returning [`FileInfo`];
//! - `PackToGoFile`/`PackToGoFileWithOption` are not ported;
//! - a [`File`] read handle is a [`FileReader`] over the extracted bytes.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gf_encoding::{gbase64, gcompress};
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_text::gstr;

use crate::gfile;
use crate::gtime;

/// The directory separator resources use, like Go's `gres.Separator`.
pub const SEPARATOR: &str = "/";

/// The default instance name, like Go's `DefaultName`.
pub const DEFAULT_NAME: &str = "default";

/// Metadata of one resource entry, like Go's `os.FileInfo`.
#[derive(Clone, Debug, Default)]
pub struct FileInfo {
    name: String,
    size: u64,
    is_dir: bool,
    mod_time: Option<SystemTime>,
}

impl FileInfo {
    /// The base name of the entry.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The uncompressed size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Whether the entry is a directory.
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }

    /// The entry's modification time.
    pub fn mod_time(&self) -> Option<SystemTime> {
        self.mod_time
    }
}

/// One file or directory inside a resource, like Go's `gres.File`.
#[derive(Clone, Debug)]
pub struct File {
    name: String,
    content: Vec<u8>,
    info: FileInfo,
}

impl File {
    /// The full resource path of the entry.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The entry's content; empty for a directory.
    pub fn content(&self) -> &[u8] {
        &self.content
    }

    /// The entry's metadata.
    pub fn file_info(&self) -> &FileInfo {
        &self.info
    }

    /// Opens a reader over the content, like Go's `File.Open`.
    pub fn open(&self) -> FileReader {
        FileReader {
            cursor: Cursor::new(self.content.clone()),
        }
    }

    /// Exports this entry (and its subtree) under `dst`, like `File.Export`.
    pub fn export(
        &self,
        dst: &str,
        option: Option<ExportOption>,
        resource: &Resource,
    ) -> Result<(), Error> {
        resource.export(&self.name, dst, option)
    }

    /// Encodes the entry as Go's `File.MarshalJSON` does.
    pub fn marshal_json(&self) -> Result<String, Error> {
        let mut map = indexmap::IndexMap::new();
        map.insert("name".to_owned(), gf_core::Value::String(self.name.clone()));
        map.insert("size".to_owned(), gf_core::Value::UInt(self.info.size));
        map.insert(
            "time".to_owned(),
            gf_core::Value::String(
                self.info
                    .mod_time
                    .map(time_text)
                    .unwrap_or_else(|| "0001-01-01 00:00:00".to_owned()),
            ),
        );
        map.insert("file".to_owned(), gf_core::Value::Bool(!self.info.is_dir));
        gf_json::marshal_value(&gf_core::Value::Map(map))
    }
}

/// A read handle over a resource entry, like the `io.ReadCloser` Go returns.
pub struct FileReader {
    cursor: Cursor<Vec<u8>>,
}

impl Read for FileReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.cursor.read(buffer)
    }
}

impl Seek for FileReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.cursor.seek(position)
    }
}

/// Rendering of a time for JSON output.
fn time_text(time: SystemTime) -> String {
    let millis = time
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0);
    gtime::Time::new_from_timestamp_milli(millis).iso8601()
}

/// Options for [`Resource::export`], like Go's `ExportOption`.
#[derive(Clone, Debug, Default)]
pub struct ExportOption {
    /// Strips this prefix from every exported path.
    pub remove_prefix: String,
}

/// Options for packing, like Go's `gres.Option`.
#[derive(Clone, Debug, Default)]
pub struct PackOption {
    /// Prefix added to every packed entry.
    pub prefix: String,
    /// Keeps the passed path as the entry prefix.
    pub keep_path: bool,
}

/// A read-only resource tree, like Go's `gres.Resource`.
#[derive(Default)]
pub struct Resource {
    tree: RwLock<BTreeMap<String, Arc<File>>>,
}

impl std::fmt::Debug for Resource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let files = self
            .tree
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        formatter
            .debug_struct("Resource")
            .field("files", &files)
            .finish()
    }
}

impl Resource {
    /// Creates an empty resource tree.
    pub fn new() -> Resource {
        Resource::default()
    }

    /// Unpacks `content` and adds every entry under `prefix`, like `Add`.
    pub fn add(&self, content: &[u8], prefix: Option<&str>) -> Result<(), Error> {
        let files = unpack_content(content)?;
        let prefix = prefix.unwrap_or_default();
        let mut tree = self
            .tree
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for file in files {
            tree.insert(format!("{prefix}{}", file.name), Arc::new(file));
        }
        Ok(())
    }

    /// Reads `path` from disk and adds it, like `Load`.
    pub fn load(&self, path: &str, prefix: Option<&str>) -> Result<(), Error> {
        let real_path = gfile::search(path, &[])?;
        self.add(&gfile::get_bytes(&real_path), prefix)
    }

    /// Returns the entry at `path`, like Go's `Get`.
    pub fn get(&self, path: &str) -> Option<Arc<File>> {
        if path.is_empty() {
            return None;
        }
        let path = normalize(path);
        self.tree
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&path)
            .cloned()
    }

    /// Returns the entry at `path`, falling back to index files when it is a
    /// directory, like Go's `GetWithIndex`.
    pub fn get_with_index(&self, path: &str, index_files: &[&str]) -> Option<Arc<File>> {
        let path = normalize(path);
        let file = self.get(&path)?;
        if !index_files.is_empty() && file.file_info().is_dir() {
            for name in index_files {
                if let Some(index) = self.get(&format!("{path}/{name}")) {
                    return Some(index);
                }
            }
        }
        Some(file)
    }

    /// Returns the content at `path`, like Go's `GetContent`.
    pub fn get_content(&self, path: &str) -> Vec<u8> {
        self.get(path)
            .map(|file| file.content().to_vec())
            .unwrap_or_default()
    }

    /// Reports whether `path` exists.
    pub fn contains(&self, path: &str) -> bool {
        self.get(path).is_some()
    }

    /// Reports whether the tree holds no entry.
    pub fn is_empty(&self) -> bool {
        self.tree
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
    }

    /// Returns the direct and recursive children of `path`, like `ScanDir`.
    pub fn scan_dir(&self, path: &str, pattern: &str, recursive: bool) -> Vec<Arc<File>> {
        self.do_scan_dir(path, pattern, recursive, false)
    }

    /// Returns the files under `path`, like Go's `ScanDirFile`.
    pub fn scan_dir_file(&self, path: &str, pattern: &str, recursive: bool) -> Vec<Arc<File>> {
        self.do_scan_dir(path, pattern, recursive, true)
    }

    /// The shared scan implementation, like Go's `doScanDir`.
    fn do_scan_dir(
        &self,
        path: &str,
        pattern: &str,
        recursive: bool,
        only_file: bool,
    ) -> Vec<Arc<File>> {
        let path = normalize(path);
        let patterns: Vec<String> = gstr::split(pattern, ",")
            .into_iter()
            .map(|item| item.trim().to_owned())
            .collect();
        let tree = self
            .tree
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut files = Vec::new();
        let mut first = true;
        let length = path.len();
        for (key, value) in tree.range(path.clone()..) {
            if first {
                if !value.file_info().is_dir() {
                    break;
                }
                first = false;
            }
            if only_file && value.file_info().is_dir() {
                continue;
            }
            if key.len() <= length {
                continue;
            }
            if key[..length] != path {
                break;
            }
            // Avoid matching `/i18n-dir` for `/i18n`.
            if !first && key.as_bytes()[length] != b'/' {
                continue;
            }
            if !recursive && key[length + 1..].contains('/') {
                continue;
            }
            for candidate in &patterns {
                if gfile::path_match(candidate, &gfile::basename(key)).unwrap_or(false) {
                    files.push(Arc::clone(value));
                    break;
                }
            }
        }
        files
    }

    /// Exports `src` and its subtree under `dst`, like Go's `Export`.
    pub fn export(&self, src: &str, dst: &str, option: Option<ExportOption>) -> Result<(), Error> {
        let option = option.unwrap_or_default();
        let Some(source) = self.get(src) else {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("resource \"{src}\" does not exist"),
                Vec::new(),
            ));
        };
        let files = if source.file_info().is_dir() {
            self.do_scan_dir(src, "*", true, false)
        } else {
            vec![source]
        };
        for file in files {
            let mut name = file.name().to_owned();
            if !option.remove_prefix.is_empty() {
                name = gstr::trim_left_str(&name, &option.remove_prefix);
            }
            name = gstr::trim(&name, Some(r"\/"));
            if name.is_empty() {
                continue;
            }
            let path = gfile::join(&[dst, &name]);
            if file.file_info().is_dir() {
                gfile::mkdir(&path)?;
            } else {
                gfile::put_bytes(&path, file.content())?;
            }
        }
        Ok(())
    }

    /// Prints every entry like Go's `Dump`.
    pub fn dump(&self) {
        let tree = self
            .tree
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (key, value) in tree.iter() {
            let info = value.file_info();
            let time = info
                .mod_time
                .map(time_text)
                .unwrap_or_else(|| "0001-01-01T00:00:00Z".to_owned());
            println!("{time} {:>8} {key}", gfile::format_size(info.size as i64));
        }
        println!("TOTAL FILES: {}", tree.len());
    }
}

/// Collapses `\\` and `//`, and drops trailing slashes, like Go's `Get` does.
fn normalize(path: &str) -> String {
    let mut normalized = path.replace('\\', "/").replace("//", "/");
    if normalized != "/" {
        while normalized.len() > 1 && normalized.ends_with('/') {
            normalized.pop();
        }
    }
    normalized
}

/// The process-wide instances, like Go's `instances` map.
fn instances() -> &'static Mutex<BTreeMap<String, Arc<Resource>>> {
    static INSTANCES: OnceLock<Mutex<BTreeMap<String, Arc<Resource>>>> = OnceLock::new();
    INSTANCES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Returns the shared resource instance for `name`, like Go's `Instance`.
pub fn instance(name: Option<&str>) -> Arc<Resource> {
    let key = match name {
        Some(name) if !name.is_empty() => name,
        _ => DEFAULT_NAME,
    };
    let mut instances = instances()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(resource) = instances.get(key) {
        return Arc::clone(resource);
    }
    let resource = Arc::new(Resource::new());
    instances.insert(key.to_owned(), Arc::clone(&resource));
    resource
}

/// The default resource object, like Go's `defaultResource`.
fn default_resource() -> &'static Arc<Resource> {
    static DEFAULT: OnceLock<Arc<Resource>> = OnceLock::new();
    DEFAULT.get_or_init(|| instance(None))
}

/// Adds packed content to the default resource, like `gres.Add`.
pub fn add(content: &[u8], prefix: Option<&str>) -> Result<(), Error> {
    default_resource().add(content, prefix)
}

/// Loads a packed file into the default resource, like `gres.Load`.
pub fn load(path: &str, prefix: Option<&str>) -> Result<(), Error> {
    default_resource().load(path, prefix)
}

/// Returns a file from the default resource, like `gres.Get`.
pub fn get(path: &str) -> Option<Arc<File>> {
    default_resource().get(path)
}

/// Returns a file, falling back to index files, like `GetWithIndex`.
pub fn get_with_index(path: &str, index_files: &[&str]) -> Option<Arc<File>> {
    default_resource().get_with_index(path, index_files)
}

/// Returns the content of `path` from the default resource.
pub fn get_content(path: &str) -> Vec<u8> {
    default_resource().get_content(path)
}

/// Reports whether the default resource contains `path`.
pub fn contains(path: &str) -> bool {
    default_resource().contains(path)
}

/// Reports whether the default resource is empty.
pub fn is_empty() -> bool {
    default_resource().is_empty()
}

/// Scans a directory of the default resource.
pub fn scan_dir(path: &str, pattern: &str, recursive: bool) -> Vec<Arc<File>> {
    default_resource().scan_dir(path, pattern, recursive)
}

/// Scans files of the default resource.
pub fn scan_dir_file(path: &str, pattern: &str, recursive: bool) -> Vec<Arc<File>> {
    default_resource().scan_dir_file(path, pattern, recursive)
}

/// Exports from the default resource.
pub fn export(src: &str, dst: &str, option: Option<ExportOption>) -> Result<(), Error> {
    default_resource().export(src, dst, option)
}

/// Prints the default resource.
pub fn dump() {
    default_resource().dump();
}

// ---------------------------------------------------------------------------
// Packing and unpacking
// ---------------------------------------------------------------------------

/// Packs `src_paths` into gzipped zip bytes, like Go's `Pack`.
pub fn pack(src_paths: &str, key_prefix: Option<&str>) -> Result<Vec<u8>, Error> {
    let mut option = PackOption::default();
    if let Some(prefix) = key_prefix.filter(|prefix| !prefix.is_empty()) {
        option.prefix = prefix.to_owned();
    }
    pack_with_option(src_paths, option)
}

/// Packs `src_paths` with options, like Go's `PackWithOption`.
pub fn pack_with_option(src_paths: &str, option: PackOption) -> Result<Vec<u8>, Error> {
    let zip_bytes = zip_paths(src_paths, &option)?;
    gcompress::gzip(&zip_bytes, Some(9))
}

/// Packs `src_paths` into `dst_path`, like Go's `PackToFile`.
pub fn pack_to_file(
    src_paths: &str,
    dst_path: &str,
    key_prefix: Option<&str>,
) -> Result<(), Error> {
    let data = pack(src_paths, key_prefix)?;
    gfile::put_bytes(dst_path, &data)
}

/// Packs `src_paths` into `dst_path` with options, like
/// `PackToFileWithOption`.
pub fn pack_to_file_with_option(
    src_paths: &str,
    dst_path: &str,
    option: PackOption,
) -> Result<(), Error> {
    let data = pack_with_option(src_paths, option)?;
    gfile::put_bytes(dst_path, &data)
}

/// Unpacks the file at `path`, like Go's `Unpack`.
pub fn unpack(path: &str) -> Result<Vec<File>, Error> {
    let real_path = gfile::search(path, &[])?;
    unpack_content(&gfile::get_bytes(&real_path))
}

/// Unpacks packed content (hex, base64 or raw gzip), like `UnpackContent`.
pub fn unpack_content(content: &[u8]) -> Result<Vec<File>, Error> {
    let data = if is_hex_str(content) {
        let bytes = hex_str_to_bytes(content);
        gcompress::un_gzip(&bytes)?
    } else if is_base64(content) {
        let bytes = gbase64::decode(content)?;
        gcompress::un_gzip(&bytes)?
    } else {
        gcompress::un_gzip(content)?
    };
    let mut archive = zip::ZipArchive::new(Cursor::new(data)).map_err(|error| {
        gerror::wrap(
            Some(gerror::new(error.to_string())),
            "create zip reader failed".to_owned(),
        )
        .unwrap_or_else(|| gerror::new("create zip reader failed"))
    })?;
    let mut files = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| gerror::new(error.to_string()))?;
        // Zip marks directories with a trailing slash; GoFrame's resource
        // keys never carry one (its `Get` trims them), so drop it here.
        let name = entry.name().trim_end_matches('/').to_owned();
        let is_dir = entry.is_dir();
        let size = entry.size();
        let mod_time = entry.last_modified().map(zip_time_to_system_time);
        let mut content = Vec::new();
        if !is_dir {
            entry
                .read_to_end(&mut content)
                .map_err(|error| gerror::new(error.to_string()))?;
        }
        files.push(File {
            info: FileInfo {
                name: gfile::basename(&name),
                size,
                is_dir,
                mod_time,
            },
            name,
            content,
        });
    }
    Ok(files)
}

/// Converts a zip timestamp to a [`SystemTime`].
fn zip_time_to_system_time(time: zip::DateTime) -> SystemTime {
    let year = i64::from(time.year());
    let month = i64::from(time.month());
    let day = i64::from(time.day());
    // Days from the civil epoch, after Howard Hinnant's algorithm.
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds = days * 86_400
        + i64::from(time.hour()) * 3_600
        + i64::from(time.minute()) * 60
        + i64::from(time.second());
    if seconds >= 0 {
        UNIX_EPOCH + Duration::from_secs(seconds as u64)
    } else {
        UNIX_EPOCH - Duration::from_secs((-seconds) as u64)
    }
}

/// Reports whether `text` is a hex string, like Go's `isHexStr`.
fn is_hex_str(text: &[u8]) -> bool {
    text.iter().copied().all(|byte| {
        byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) || (b'A'..=b'F').contains(&byte)
    })
}

/// Reports whether `text` is a base64 string, like Go's `isBase64`.
fn is_base64(text: &[u8]) -> bool {
    text.iter().copied().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'_' | b'/' | b'=')
    })
}

/// Decodes a hex string, like Go's `hexStrToBytes` (invalid pairs decode as 0).
fn hex_str_to_bytes(text: &[u8]) -> Vec<u8> {
    let bytes = text;
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    let value = |byte: u8| -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    };
    let mut index = 0;
    while index + 1 < bytes.len() {
        let high = value(bytes[index]).unwrap_or(0);
        let low = value(bytes[index + 1]).unwrap_or(0);
        decoded.push(high << 4 | low);
        index += 2;
    }
    decoded
}

/// Builds the zip archive for `paths`, like Go's `zipPathWriter`.
fn zip_paths(paths: &str, option: &PackOption) -> Result<Vec<u8>, Error> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for path in paths.split(',') {
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        let absolute = gfile::search(path, &[])?;
        let files = if gfile::is_dir(&absolute) {
            gfile::scan_dir(&absolute, "*", true)?
        } else {
            vec![absolute.clone()]
        };
        let mut header_prefix = option.prefix.clone();
        if header_prefix != "/" {
            header_prefix = gstr::trim_right(&header_prefix, Some(r"\/"));
        }
        if !header_prefix.is_empty() && gfile::is_dir(&absolute) {
            header_prefix.push('/');
        }
        if header_prefix.is_empty() {
            if option.keep_path {
                header_prefix = path.to_owned();
            } else {
                header_prefix = gfile::basename(&absolute);
            }
        }
        header_prefix = header_prefix.replace('\\', "/").replace("//", "/");
        for file in &files {
            let mut sub_path = file[absolute.len()..].to_owned();
            if !sub_path.is_empty() {
                sub_path = gfile::dir(&sub_path);
            }
            let entry = format!("{header_prefix}{sub_path}");
            add_file_entry(&mut writer, file, &entry)?;
        }
        if !header_prefix.is_empty() {
            let mut current = header_prefix.clone();
            loop {
                let entry = format!("{current}/");
                add_directory_entry(&mut writer, &entry)?;
                if current == "/" || !current.contains('/') {
                    break;
                }
                current = gfile::dir(&current);
            }
        }
    }
    let cursor = writer
        .finish()
        .map_err(|error| gerror::new(error.to_string()))?;
    Ok(cursor.into_inner())
}

/// Adds one file to the zip archive, keeping the entry name `prefix`.
fn add_file_entry<W: Write + Seek>(
    writer: &mut zip::ZipWriter<W>,
    path: &str,
    prefix: &str,
) -> Result<(), Error> {
    let content = gfile::get_bytes(path);
    let name = format!("{}/{}", prefix.replace("//", "/"), gfile::basename(path));
    let name = normalize_entry(&name);
    if gfile::is_dir(path) {
        add_directory_entry(writer, &name)?;
        return Ok(());
    }
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer
        .start_file(name, options)
        .map_err(|error| gerror::new(error.to_string()))?;
    writer
        .write_all(&content)
        .map_err(|error| gerror::new(error.to_string()))
}

/// Adds one directory entry to the zip archive.
fn add_directory_entry<W: Write + Seek>(
    writer: &mut zip::ZipWriter<W>,
    entry: &str,
) -> Result<(), Error> {
    let entry = normalize_entry(entry);
    if entry.is_empty() {
        return Ok(());
    }
    let options = zip::write::SimpleFileOptions::default();
    writer
        .add_directory(format!("{entry}/"), options)
        .map_err(|error| gerror::new(error.to_string()))
}

/// Normalizes a zip entry name to forward slashes without empty segments.
///
/// A leading slash is preserved: packing with the `/` prefix produces absolute
/// entry names, exactly like Go's `createFileHeader` after its `/{2,}`
/// collapse.
fn normalize_entry(name: &str) -> String {
    let normalized = name.replace('\\', "/");
    let absolute = normalized.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in normalized.split('/') {
        match segment {
            "" | "." => {}
            other => segments.push(other),
        }
    }
    let joined = segments.join("/");
    if absolute && !joined.is_empty() {
        format!("/{joined}")
    } else {
        joined
    }
}

impl gf_i18n::gi18n::Resource for Resource {
    fn contains(&self, path: &str) -> bool {
        Resource::contains(self, path)
    }

    fn files(&self, path: &str) -> Vec<gf_i18n::gi18n::ResourceFile> {
        let normalized = normalize(path);
        let prefix = format!("{}/", normalized.trim_matches('/'));
        Resource::scan_dir_file(self, path, "*", true)
            .into_iter()
            .map(|file| {
                let name = file.name().trim_matches('/');
                let relative = name.strip_prefix(&prefix).unwrap_or(name);
                gf_i18n::gi18n::ResourceFile {
                    name: relative.to_owned(),
                    content: file.content().to_vec(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfile::{mkdir, put_contents, remove_all, temp};

    fn sandbox(name: &str) -> String {
        let path = temp(&["gf-rust-gres-tests", name]);
        let _ = remove_all(&path);
        mkdir(&path).expect("sandbox");
        path
    }

    /// Builds the Go test fixture layout: files/{root/index.html, dir1/...}.
    fn fixture(name: &str) -> String {
        let base = sandbox(name);
        put_contents(
            &gfile::join(&[&base, "files", "root", "index.html"]),
            "<html>",
        )
        .expect("put");
        put_contents(
            &gfile::join(&[&base, "files", "dir1", "test1"]),
            "test1 content",
        )
        .expect("put");
        put_contents(
            &gfile::join(&[&base, "files", "dir1", "sub", "sub-test1.txt"]),
            "sub-test1 content",
        )
        .expect("put");
        put_contents(
            &gfile::join(&[&base, "files", "dir2", "test2"]),
            "test2 content",
        )
        .expect("put");
        put_contents(
            &gfile::join(&[&base, "files", "dir2", "sub", "sub-test2.txt"]),
            "sub-test2 content",
        )
        .expect("put");
        gfile::join(&[&base, "files"])
    }

    #[test]
    fn pack_add_and_get() {
        let source = fixture("basic");
        let data = pack(&source, None).expect("pack");
        let resource = Resource::new();
        resource.add(&data, None).expect("add");

        assert!(resource.contains("files"));
        assert_eq!(
            resource.get_content("files/dir1/test1"),
            b"test1 content".to_vec()
        );
        assert!(resource.get_content("files/dir1").is_empty());
        assert!(resource.get("missing").is_none());

        let file = resource.get("files/dir1/test1").expect("file");
        assert_eq!(file.name(), "files/dir1/test1");
        assert!(!file.file_info().is_dir());
        assert_eq!(file.file_info().name(), "test1");
        assert_eq!(file.content(), b"test1 content");

        let mut reader = file.open();
        let mut buffer = [0u8; 5];
        reader.read_exact(&mut buffer).expect("read");
        assert_eq!(&buffer, b"test1");

        let indexed = resource
            .get_with_index("files", &["index.html", "root/index.html"])
            .expect("index");
        assert_eq!(indexed.name(), "files/root/index.html");

        resource
            .add(&pack(&source, Some("/")).expect("pack"), None)
            .expect("add with prefix");
        // A "/" prefix replaces the source directory name, like Go.s Test_Pack.
        assert!(resource.contains("/dir1/test1"));

        remove_all(&gfile::dir(&source)).expect("cleanup");
    }

    #[test]
    fn scan_dir_matches_go() {
        let source = fixture("scan");
        let resource = Resource::new();
        resource
            .add(&pack(&source, None).expect("pack"), None)
            .expect("add");
        let root = "files/dir1";

        assert_eq!(resource.scan_dir(root, "*", false).len(), 2);
        assert_eq!(resource.scan_dir(root, "*", true).len(), 3);
        let matched = resource.scan_dir(root, "*.*", true);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].name(), "files/dir1/sub/sub-test1.txt");
        assert_eq!(resource.scan_dir_file(root, "*", false).len(), 1);
        assert_eq!(resource.scan_dir_file(root, "*", true).len(), 2);

        remove_all(&gfile::dir(&source)).expect("cleanup");
    }

    #[test]
    fn export_round_trips() {
        let source = fixture("export");
        let resource = Resource::new();
        resource
            .add(&pack(&source, None).expect("pack"), None)
            .expect("add");

        let dst = gfile::temp(&["gf-rust-gres-export"]);
        let _ = remove_all(&dst);
        resource.export("files", &dst, None).expect("export");
        let files = gfile::scan_dir(&dst, "*", true).expect("scan");
        // The fixture holds 11 entries (5 directories and 6 files).
        assert_eq!(files.len(), 11);
        assert_eq!(
            gfile::get_contents(&gfile::join(&[&dst, "files", "dir1", "test1"])),
            "test1 content"
        );

        let stripped = gfile::temp(&["gf-rust-gres-export-stripped"]);
        let _ = remove_all(&stripped);
        resource
            .export(
                "files",
                &stripped,
                Some(ExportOption {
                    remove_prefix: "files".to_owned(),
                }),
            )
            .expect("export stripped");
        assert_eq!(
            gfile::get_contents(&gfile::join(&[&stripped, "dir1", "test1"])),
            "test1 content"
        );

        remove_all(&dst).ok();
        remove_all(&stripped).ok();
        remove_all(&gfile::dir(&source)).ok();
    }

    #[test]
    fn base64_and_raw_unpack() {
        let source = fixture("encoding");
        let data = pack(&source, None).expect("pack");
        // Raw gzip content.
        let raw = unpack_content(&data).expect("raw");
        assert!(!raw.is_empty());
        // Base64 content.
        let encoded = gbase64::encode_to_string(&data);
        let decoded = unpack_content(encoded.as_bytes()).expect("base64");
        assert_eq!(decoded.len(), raw.len());
        // Hex content (the legacy packing).
        let hex: String = data.iter().map(|byte| format!("{byte:02x}")).collect();
        let decoded = unpack_content(hex.as_bytes()).expect("hex");
        assert_eq!(decoded.len(), raw.len());
        assert!(unpack_content(b"not packed").is_err());
        remove_all(&gfile::dir(&source)).expect("cleanup");
    }

    #[test]
    fn pack_to_file_and_load() {
        let source = fixture("files");
        let packed = gfile::temp(&["gf-rust-gres-packed", "data.bin"]);
        pack_to_file(&source, &packed, None).expect("pack to file");
        let resource = Resource::new();
        resource.load(&packed, None).expect("load");
        assert!(resource.contains("files/dir2/test2"));
        assert!(!resource.is_empty());
        remove_all(&packed).ok();
        remove_all(&gfile::dir(&source)).expect("cleanup");
    }

    #[test]
    fn file_marshal_json_and_reader() {
        let source = fixture("json");
        let resource = Resource::new();
        resource
            .add(&pack(&source, None).expect("pack"), None)
            .expect("add");
        let file = resource.get("files/dir2/test2").expect("file");
        let json = file.marshal_json().expect("json");
        assert!(json.contains("\"name\":\"files/dir2/test2\""));
        assert!(json.contains("\"file\":true"));

        let mut reader = file.open();
        let mut buffer = String::new();
        reader.read_to_string(&mut buffer).expect("read");
        assert_eq!(buffer, "test2 content");
        assert_eq!(reader.seek(SeekFrom::Start(0)).expect("seek"), 0);
        remove_all(&gfile::dir(&source)).expect("cleanup");
    }

    #[test]
    fn hex_and_base64_detection() {
        assert!(is_hex_str(b"deadBEEF12"));
        assert!(!is_hex_str(b"xyz"));
        assert!(is_hex_str(b""));
        assert!(is_base64(b"aGVsbG8="));
        assert!(!is_base64(b"hello world"));
        assert_eq!(hex_str_to_bytes(b"0a1b"), vec![0x0a, 0x1b]);
    }

    #[test]
    fn resource_serves_i18n_files() {
        use gf_i18n::gi18n::{Manager, Options, Resource as I18nResource};

        let root = gfile::temp(&["gf-rust-gres-tests", "i18n-resource"]);
        let _ = gfile::remove_all(&root);
        gfile::mkdir(&gfile::join(&[&root, "i18n"])).expect("mkdir");
        gfile::put_contents(
            &gfile::join(&[&root, "i18n", "zh-CN.json"]),
            r#"{"hello":"你好"}"#,
        )
        .expect("write");
        let packed = pack(&gfile::join(&[&root, "i18n"]), Some("i18n")).expect("pack");
        let resource = std::sync::Arc::new(Resource::new());
        resource.add(&packed, None).expect("add");
        assert!(I18nResource::contains(resource.as_ref(), "i18n"));

        let adapter: std::sync::Arc<dyn I18nResource> = resource.clone();
        let files = adapter.files("i18n");
        assert!(!files.is_empty());
        assert_eq!(files[0].name, "zh-CN.json");

        let manager = Manager::new(Options::new("i18n").with_resource(adapter));
        assert_eq!(manager.translate_with_language("{#hello}", "zh-CN"), "你好");
        let _ = gfile::remove_all(&root);
    }
}
