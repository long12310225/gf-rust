//! Port of `encoding/gcompress` (gzip and zlib).

use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use gf_errors::gcode::CODE_INVALID_PARAMETER;
use gf_errors::gerror;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

fn wrap(err: impl std::error::Error + Send + Sync + 'static, text: &str) -> gerror::Error {
    match gerror::wrap_std(Some(Box::new(err)), text) {
        Some(err) => err,
        None => gerror::Error::new(text),
    }
}

/// Compresses `data` with gzip; `level` is 1..=9, default 6.
pub fn gzip(data: &[u8], level: Option<u32>) -> Result<Vec<u8>, gerror::Error> {
    let compression = match level {
        Some(level) if level <= 9 => Compression::new(level),
        Some(level) => {
            return Err(gerror::Error::newf(
                "gzip.NewWriterLevel failed for level \"%d\"",
                vec![gf_core::Value::Int(i64::from(level))],
            ));
        }
        None => Compression::default(),
    };
    let mut encoder = GzEncoder::new(Vec::new(), compression);
    encoder
        .write_all(data)
        .map_err(|err| wrap(err, "writer.Write failed"))?;
    encoder
        .finish()
        .map_err(|err| wrap(err, "writer.Close failed"))
}

/// Decompresses gzip data.
pub fn un_gzip(data: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let mut decoder = GzDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|err| wrap(err, "io.ReadAll failed"))?;
    Ok(out)
}

/// Compresses `data` with zlib; short inputs (< 13 bytes) are returned as-is.
pub fn zlib(data: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    if data.len() < 13 {
        return Ok(data.to_vec());
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .map_err(|err| wrap(err, "zlib.Writer.Write failed"))?;
    encoder
        .finish()
        .map_err(|err| wrap(err, "zlib.Writer.Close failed"))
}

/// Decompresses zlib data; short inputs (< 13 bytes) are returned as-is.
pub fn un_zlib(data: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    if data.len() < 13 {
        return Ok(data.to_vec());
    }
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|err| wrap(err, "io.ReadAll failed"))?;
    Ok(out)
}

/// Compresses the content of `src` into `dst` with gzip.
pub fn gzip_file(src_path: &str, dst_path: &str, level: Option<u32>) -> Result<(), gerror::Error> {
    let data = std::fs::read(src_path)
        .map_err(|err| wrap(err, &format!("os.Open failed for name \"{src_path}\"")))?;
    let compressed = gzip(&data, level)?;
    std::fs::write(dst_path, compressed)
        .map_err(|err| wrap(err, &format!("os.Create failed for name \"{dst_path}\"")))?;
    Ok(())
}

/// Decompresses the content of `src` into `dst` with gzip.
pub fn un_gzip_file(src_path: &str, dst_path: &str) -> Result<(), gerror::Error> {
    let data = std::fs::read(src_path)
        .map_err(|err| wrap(err, &format!("os.Open failed for name \"{src_path}\"")))?;
    let decompressed = un_gzip(&data)?;
    std::fs::write(dst_path, decompressed)
        .map_err(|err| wrap(err, &format!("os.Create failed for name \"{dst_path}\"")))?;
    Ok(())
}

fn collect_paths(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), gerror::Error> {
    out.push(path.to_path_buf());
    if path.is_dir() {
        let entries = fs::read_dir(path).map_err(|err| {
            wrap(
                err,
                &format!("os.ReadDir failed for path \"{}\"", path.display()),
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|err| wrap(err, "os.ReadDir failed"))?;
            collect_paths(&entry.path(), out)?;
        }
    }
    Ok(())
}

fn write_zip_entries<W: Write + Seek>(
    paths: &str,
    writer: W,
    prefix: Option<&str>,
    exclude: Option<&Path>,
) -> Result<(), gerror::Error> {
    let mut zip = ZipWriter::new(writer);
    let file_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let dir_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);
    for raw in paths.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let input = Path::new(raw);
        if !input.exists() {
            return Err(gerror::Error::new(format!(
                "the given path \"{raw}\" does not exist"
            )));
        }
        let input = input
            .canonicalize()
            .map_err(|err| wrap(err, &format!("path resolve failed for \"{raw}\"")))?;
        if let Some(exclude) = exclude
            && exclude == input
        {
            continue;
        }
        let input_is_dir = input.is_dir();
        let base_name = input
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let header_prefix = if input_is_dir {
            match prefix {
                Some(prefix) if !prefix.is_empty() => {
                    prefix.trim_end_matches(['/', '\\']).to_owned()
                }
                _ => base_name.clone(),
            }
        } else {
            prefix
                .unwrap_or_default()
                .trim_end_matches(['/', '\\'])
                .to_owned()
        };
        let mut entries = Vec::new();
        if input_is_dir {
            let children = fs::read_dir(&input)
                .map_err(|err| wrap(err, &format!("os.ReadDir failed for path \"{raw}\"")))?;
            for child in children {
                let child = child.map_err(|err| wrap(err, "os.ReadDir failed"))?;
                collect_paths(&child.path(), &mut entries)?;
            }
            entries.sort();
        } else {
            entries.push(input.clone());
        }
        for entry in entries {
            if let Some(exclude) = exclude
                && entry == exclude
            {
                continue;
            }
            let relative = entry.strip_prefix(&input).unwrap_or(&entry);
            let name = if input_is_dir {
                let relative = relative.to_string_lossy().replace('\\', "/");
                if header_prefix.is_empty() {
                    relative
                } else {
                    format!("{header_prefix}/{relative}")
                }
            } else {
                let file_name = entry
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if header_prefix.is_empty() {
                    file_name
                } else {
                    format!("{header_prefix}/{file_name}")
                }
            };
            if entry.is_dir() {
                let directory = format!("{}/", name.trim_end_matches('/'));
                zip.add_directory(&directory, dir_options)
                    .map_err(|err| wrap(err, "zip.Writer.CreateHeader failed"))?;
                continue;
            }
            zip.start_file(&name, file_options)
                .map_err(|err| wrap(err, "zip.Writer.CreateHeader failed"))?;
            let mut file = fs::File::open(&entry).map_err(|err| {
                wrap(
                    err,
                    &format!("os.Open failed for name \"{}\"", entry.display()),
                )
            })?;
            std::io::copy(&mut file, &mut zip).map_err(|err| {
                wrap(
                    err,
                    &format!("io.Copy failed from \"{}\" to \"{name}\"", entry.display()),
                )
            })?;
        }
    }
    zip.finish()
        .map_err(|err| wrap(err, "zip.Writer.Close failed"))?;
    Ok(())
}

/// Compresses files or folders (comma separated) into `dst_file_path` as zip.
pub fn zip_path(
    file_or_folder_paths: &str,
    dst_file_path: &str,
    prefix: Option<&str>,
) -> Result<(), gerror::Error> {
    let dst = Path::new(dst_file_path);
    let exclude = dst.canonicalize().ok();
    let file = fs::File::create(dst).map_err(|err| {
        wrap(
            err,
            &format!("os.Create failed for name \"{dst_file_path}\""),
        )
    })?;
    let writer = std::io::BufWriter::new(file);
    write_zip_entries(file_or_folder_paths, writer, prefix, exclude.as_deref())
}

/// Compresses files or folders into an existing writer as zip.
pub fn zip_path_writer<W: Write + Seek>(
    file_or_folder_paths: &str,
    writer: W,
    prefix: Option<&str>,
) -> Result<(), gerror::Error> {
    write_zip_entries(file_or_folder_paths, writer, prefix, None)
}

/// Compresses files or folders into zip bytes.
pub fn zip_path_content(
    file_or_folder_paths: &str,
    prefix: Option<&str>,
) -> Result<Vec<u8>, gerror::Error> {
    let mut cursor = Cursor::new(Vec::new());
    zip_path_writer(file_or_folder_paths, &mut cursor, prefix)?;
    Ok(cursor.into_inner())
}

fn is_local_name(name: &str) -> bool {
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    for component in name.split('/') {
        if component == ".." || component.contains(':') {
            return false;
        }
    }
    true
}

fn escape_error(name: &str) -> gerror::Error {
    gerror::Error::new_codef(
        &CODE_INVALID_PARAMETER,
        "zip file name \"%s\" escapes the destination directory",
        vec![gf_core::Value::String(name.to_owned())],
    )
}

fn un_zip_reader<R: Read + Seek>(
    reader: R,
    dst_folder_path: &str,
    prefix: Option<&str>,
) -> Result<(), gerror::Error> {
    let mut archive = ZipArchive::new(reader).map_err(|err| wrap(err, "zip.NewReader failed"))?;
    fs::create_dir_all(dst_folder_path).map_err(|err| {
        wrap(
            err,
            &format!("os.MkdirAll failed for path \"{dst_folder_path}\""),
        )
    })?;
    let root = fs::canonicalize(dst_folder_path).map_err(|err| {
        wrap(
            err,
            &format!("os.OpenRoot failed for path \"{dst_folder_path}\""),
        )
    })?;
    let prefix = prefix.unwrap_or_default().replace('\\', "/");
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| wrap(err, "zip.Reader.Open failed"))?;
        let original = entry.name().to_owned();
        let mut name = original.replace('\\', "/");
        name = name.trim_matches('/').to_owned();
        if !prefix.is_empty() {
            if !name.starts_with(&prefix) {
                continue;
            }
            name = name[prefix.len()..].to_owned();
        }
        name = name.trim_start_matches('/').to_owned();
        if name.is_empty() {
            continue;
        }
        if !is_local_name(&name) {
            return Err(escape_error(&original));
        }
        let target = root.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|err| wrap(err, &format!("MkdirAll failed for name \"{name}\"")))?;
            let canonical = fs::canonicalize(&target)
                .map_err(|err| wrap(err, &format!("MkdirAll failed for name \"{name}\"")))?;
            if !canonical.starts_with(&root) {
                return Err(escape_error(&original));
            }
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                wrap(
                    err,
                    &format!("MkdirAll failed for name \"{}\"", parent.display()),
                )
            })?;
            let canonical = fs::canonicalize(parent).map_err(|err| {
                wrap(
                    err,
                    &format!("MkdirAll failed for name \"{}\"", parent.display()),
                )
            })?;
            if !canonical.starts_with(&root) {
                return Err(escape_error(&original));
            }
        }
        let mut output = fs::File::create(&target)
            .map_err(|err| wrap(err, &format!("root.OpenFile failed for name \"{name}\"")))?;
        std::io::copy(&mut entry, &mut output).map_err(|err| {
            wrap(
                err,
                &format!("io.Copy failed from \"{original}\" to \"{name}\""),
            )
        })?;
    }
    Ok(())
}

/// Extracts a zip file into `dst_folder_path`.
pub fn un_zip_file(
    zipped_file_path: &str,
    dst_folder_path: &str,
    prefix: Option<&str>,
) -> Result<(), gerror::Error> {
    let file = fs::File::open(zipped_file_path).map_err(|err| {
        wrap(
            err,
            &format!("zip.OpenReader failed for name \"{dst_folder_path}\""),
        )
    })?;
    un_zip_reader(std::io::BufReader::new(file), dst_folder_path, prefix)
}

/// Extracts zip bytes into `dst_folder_path`.
pub fn un_zip_content(
    zipped_content: &[u8],
    dst_folder_path: &str,
    prefix: Option<&str>,
) -> Result<(), gerror::Error> {
    un_zip_reader(Cursor::new(zipped_content), dst_folder_path, prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_gzip(data: &[u8]) -> bool {
        data.len() > 2 && data[0] == 0x1f && data[1] == 0x8b
    }

    #[test]
    fn gzip_round_trip_and_levels() {
        let data = b"Hello Go Frame Hello Go Frame Hello Go Frame".repeat(20);
        let compressed = gzip(&data, None).expect("gzip");
        assert!(is_gzip(&compressed));
        assert!(compressed.len() < data.len());
        assert_eq!(un_gzip(&compressed).expect("ungzip"), data);

        for level in [1u32, 9] {
            let compressed = gzip(&data, Some(level)).expect("gzip");
            assert_eq!(un_gzip(&compressed).expect("ungzip"), data);
        }
        assert!(gzip(&data, Some(10)).is_err());
        assert!(un_gzip(b"not gzip").is_err());
    }

    #[test]
    fn zlib_round_trip_and_short_passthrough() {
        let data = b"Hello Go Frame, this is a longer payload".repeat(4);
        let compressed = zlib(&data).expect("zlib");
        assert_eq!(un_zlib(&compressed).expect("unzlib"), data);

        assert_eq!(zlib(b"short").expect("zlib"), b"short");
        assert_eq!(un_zlib(b"short").expect("unzlib"), b"short");
        assert!(un_zlib(&[0u8; 20]).is_err());
    }

    #[test]
    fn files_round_trip() {
        let dir = std::env::temp_dir().join("gf-gcompress-test");
        std::fs::create_dir_all(&dir).expect("dir");
        let src = dir.join("src.txt");
        let gz = dir.join("src.gz");
        let out = dir.join("out.txt");
        std::fs::write(&src, b"file content").expect("write");
        let src = src.to_string_lossy().into_owned();
        let gz = gz.to_string_lossy().into_owned();
        let out = out.to_string_lossy().into_owned();

        gzip_file(&src, &gz, None).expect("gzip file");
        un_gzip_file(&gz, &out).expect("ungzip file");
        assert_eq!(std::fs::read(&out).expect("read"), b"file content");
        assert!(gzip_file("missing", &gz, None).is_err());
        std::fs::remove_file(&src).ok();
        std::fs::remove_file(&gz).ok();
        std::fs::remove_file(&out).ok();
    }

    #[test]
    fn zip_round_trip_and_prefix() {
        let dir = std::env::temp_dir().join("gf-zip-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).expect("dir");
        std::fs::write(dir.join("a.txt"), b"a").expect("write");
        std::fs::write(dir.join("sub").join("b.txt"), b"b").expect("write");

        let dst = dir.join("out.zip");
        zip_path(
            dir.to_string_lossy().as_ref(),
            dst.to_string_lossy().as_ref(),
            None,
        )
        .expect("zip");
        let extract = dir.join("extract");
        un_zip_file(
            dst.to_string_lossy().as_ref(),
            extract.to_string_lossy().as_ref(),
            None,
        )
        .expect("unzip");
        assert_eq!(
            std::fs::read(extract.join("gf-zip-test").join("a.txt")).expect("a"),
            b"a"
        );
        assert_eq!(
            std::fs::read(extract.join("gf-zip-test").join("sub").join("b.txt")).expect("b"),
            b"b"
        );

        let content = zip_path_content(dir.join("sub").to_string_lossy().as_ref(), None)
            .expect("zip content");
        let prefix_extract = dir.join("prefix");
        un_zip_content(
            &content,
            prefix_extract.to_string_lossy().as_ref(),
            Some("sub"),
        )
        .expect("unzip prefix");
        assert_eq!(
            std::fs::read(prefix_extract.join("b.txt")).expect("b"),
            b"b"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn malicious_zip(name: &str) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            zip.start_file(
                name,
                SimpleFileOptions::default()
                    .compression_method(CompressionMethod::Deflated)
                    .unix_permissions(0o644),
            )
            .expect("entry");
            zip.write_all(b"evil").expect("write");
            zip.finish().expect("finish");
        }
        cursor.into_inner()
    }

    #[test]
    fn zip_slip_is_rejected() {
        use gf_errors::gcode::Code;

        let root = std::env::temp_dir().join("gf-zip-slip");
        let _ = std::fs::remove_dir_all(&root);
        for name in ["../evil.txt", "a/../../evil.txt", r"..\evil.txt"] {
            let dst = root.join("out");
            let err = un_zip_content(&malicious_zip(name), dst.to_string_lossy().as_ref(), None)
                .expect_err(name);
            assert_eq!(err.code().code(), CODE_INVALID_PARAMETER.code());
            assert!(!root.join("evil.txt").exists());
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn zip_conflicts_fail() {
        let root = std::env::temp_dir().join("gf-zip-conflict");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("dst")).expect("dir");
        std::fs::write(root.join("dst").join("a"), b"").expect("file");

        let mut content = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut content);
            zip.add_directory("a/", SimpleFileOptions::default())
                .expect("dir entry");
            zip.finish().expect("finish");
        }
        assert!(
            un_zip_content(
                &content.into_inner(),
                root.join("dst").to_string_lossy().as_ref(),
                None
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
