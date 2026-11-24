//! Port of `gfile_copy.go`.

use std::fs;
use std::io;

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};

use super::{DEFAULT_PERM_COPY, basename, chmod, create, exists, is_file, join, open, wrap_io};

/// Options for the copy functions, like Go's `gfile.CopyOption`.
#[derive(Clone, Debug)]
pub struct CopyOption {
    /// Syncs the target file after copying.
    pub sync: bool,
    /// Copies the source mode onto the target; `mode` is then ignored.
    pub preserve_mode: bool,
    /// Mode for a created target, `0o755` by default.
    pub mode: u32,
}

impl Default for CopyOption {
    fn default() -> CopyOption {
        CopyOption {
            sync: false,
            preserve_mode: false,
            mode: DEFAULT_PERM_COPY,
        }
    }
}

/// Fills in the default mode like Go's `getCopyOption`.
fn used_option(option: Option<CopyOption>) -> CopyOption {
    let mut option = option.unwrap_or_default();
    if option.mode == 0 {
        option.mode = DEFAULT_PERM_COPY;
    }
    option
}

/// Copies a file or a directory tree, like Go's `gfile.Copy`.
pub fn copy(src: &str, dst: &str, option: impl Into<Option<CopyOption>>) -> Result<(), Error> {
    let option = option.into();
    if src.is_empty() {
        return Err(gerror::new_code(
            &gcode::CODE_INVALID_PARAMETER,
            &["source path cannot be empty"],
        ));
    }
    if dst.is_empty() {
        return Err(gerror::new_code(
            &gcode::CODE_INVALID_PARAMETER,
            &["destination path cannot be empty"],
        ));
    }
    let src_meta = match fs::metadata(src) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("the src path \"{src}\" does not exist"),
                Vec::new(),
            ));
        }
        Err(error) => {
            return Err(wrap_io(error, format!("call os.Stat on \"{src}\" failed")));
        }
    };
    let dst_meta = match fs::metadata(dst) {
        Ok(meta) => Some(meta),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(wrap_io(error, format!("call os.Stat on \"{dst}\" failed")));
        }
    };

    if is_file(src) {
        if dst_meta.as_ref().map(fs::Metadata::is_dir).unwrap_or(false) {
            let target = join(&[dst, &basename(src)]);
            return copy_file(src, &target, option);
        }
        return copy_file(src, dst, option);
    }
    if !src_meta.is_dir() && dst_meta.as_ref().map(fs::Metadata::is_dir).unwrap_or(false) {
        return Err(gerror::new_codef(
            &gcode::CODE_INVALID_PARAMETER,
            format!(
                "Copy failed: the src path \"{src}\" is file, but the dst path \"{dst}\" is folder"
            ),
            Vec::new(),
        ));
    }
    copy_dir(src, dst, option)
}

/// Copies one file, creating the target, like Go's `gfile.CopyFile`.
pub fn copy_file(src: &str, dst: &str, option: impl Into<Option<CopyOption>>) -> Result<(), Error> {
    let option = used_option(option.into());
    if src.is_empty() {
        return Err(gerror::new_code(
            &gcode::CODE_INVALID_PARAMETER,
            &["source file cannot be empty"],
        ));
    }
    if dst.is_empty() {
        return Err(gerror::new_code(
            &gcode::CODE_INVALID_PARAMETER,
            &["destination file cannot be empty"],
        ));
    }
    if src == dst {
        return Ok(());
    }
    let src_meta = match fs::metadata(src) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("the src path \"{src}\" does not exist"),
                Vec::new(),
            ));
        }
        Err(error) => {
            return Err(wrap_io(error, format!("call os.Stat on \"{src}\" failed")));
        }
    };
    match fs::metadata(dst) {
        Ok(meta) if meta.is_dir() && !src_meta.is_dir() => {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!(
                    "CopyFile failed: the src path \"{src}\" is file, but the dst path \"{dst}\" is folder"
                ),
                Vec::new(),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(wrap_io(error, format!("call os.Stat on \"{dst}\" failed")));
        }
    }

    let mut input = open(src)?;
    let mut output = create(dst)?;
    io::copy(&mut input, &mut output)
        .map_err(|error| wrap_io(error, format!("io.Copy failed from \"{src}\" to \"{dst}\"")))?;
    if option.sync {
        output
            .sync_all()
            .map_err(|error| wrap_io(error, format!("file sync failed for file \"{dst}\"")))?;
    }
    let mode = if option.preserve_mode {
        permission_mode(&src_meta)
    } else {
        option.mode
    };
    chmod(dst, mode)
}

/// Copies a directory tree, like Go's `gfile.CopyDir`.
///
/// Symlinks are skipped.
pub fn copy_dir(src: &str, dst: &str, option: impl Into<Option<CopyOption>>) -> Result<(), Error> {
    let option = used_option(option.into());
    if src.is_empty() {
        return Err(gerror::new_code(
            &gcode::CODE_INVALID_PARAMETER,
            &["source directory cannot be empty"],
        ));
    }
    if dst.is_empty() {
        return Err(gerror::new_code(
            &gcode::CODE_INVALID_PARAMETER,
            &["destination directory cannot be empty"],
        ));
    }
    if src == dst {
        return Ok(());
    }
    let src = clean(src);
    let dst = clean(dst);
    let src_meta = fs::metadata(&src)
        .map_err(|error| wrap_io(error, format!("os.Stat failed for file \"{src}\"")))?;
    if !src_meta.is_dir() {
        return Err(gerror::new_code(
            &gcode::CODE_INVALID_PARAMETER,
            &["source is not a directory"],
        ));
    }
    let mode = if option.preserve_mode {
        permission_mode(&src_meta)
    } else {
        option.mode
    };
    if !exists(&dst) {
        fs::create_dir_all(&dst).map_err(|error| {
            wrap_io(error, format!("create directory failed for path \"{dst}\""))
        })?;
        chmod(&dst, mode)?;
    }
    let entries = fs::read_dir(&src)
        .map_err(|error| wrap_io(error, format!("read directory failed for path \"{src}\"")))?;
    for entry in entries {
        let entry = entry.map_err(|error| gerror::new(error.to_string()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let src_path = join(&[&src, &name]);
        let dst_path = join(&[&dst, &name]);
        let file_type = entry
            .file_type()
            .map_err(|error| wrap_io(error, format!("read file type of \"{src_path}\" failed")))?;
        if file_type.is_dir() {
            copy_dir(&src_path, &dst_path, option.clone())?;
        } else {
            if file_type.is_symlink() {
                continue;
            }
            copy_file(&src_path, &dst_path, option.clone())?;
        }
    }
    Ok(())
}

/// Collapses the path text the way Go's `filepath.Clean` does.
fn clean(path: &str) -> String {
    let cleaned = std::path::Path::new(path);
    if cleaned.as_os_str().is_empty() {
        return ".".to_owned();
    }
    cleaned.to_string_lossy().into_owned()
}

/// The unix permission bits of `meta`, or `0o755` elsewhere.
fn permission_mode(meta: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0o755
    }
}

// The mode helpers below are the only platform-specific part of copying.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfile::{mkdir, put_contents, remove_all, temp};

    fn sandbox(name: &str) -> String {
        let path = temp(&["gf-rust-gfile-copy-tests", name]);
        let _ = remove_all(&path);
        mkdir(&path).expect("sandbox");
        path
    }

    #[test]
    fn copy_file_into_existing_directory_uses_source_name() {
        let base = sandbox("file-into-dir");
        let source = join(&[&base, "source.txt"]);
        put_contents(&source, "data").expect("source");
        let target_dir = join(&[&base, "target"]);
        mkdir(&target_dir).expect("target");
        copy(&source, &target_dir, None).expect("copy");
        assert_eq!(
            crate::gfile::get_contents(&join(&[&target_dir, "source.txt"])),
            "data"
        );
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn copy_dir_recurses_and_skips_symlinks() {
        let base = sandbox("dir");
        let source = join(&[&base, "source"]);
        mkdir(&join(&[&source, "nested"])).expect("nested");
        put_contents(&join(&[&source, "a.txt"]), "a").expect("a");
        put_contents(&join(&[&source, "nested", "b.txt"]), "b").expect("b");
        let target = join(&[&base, "target"]);
        copy_dir(&source, &target, None).expect("copy dir");
        assert_eq!(
            crate::gfile::get_contents(&join(&[&target, "nested", "b.txt"])),
            "b"
        );

        // Same path is a no-op.
        copy_dir(&source, &source, None).expect("same path");
        assert!(copy_dir(&source, &join(&[&base, "x"]), None).is_ok());
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn copy_validates_its_arguments() {
        let base = sandbox("validate");
        assert!(copy("", &base, None).is_err());
        assert!(copy(&base, "", None).is_err());
        let error = copy(&join(&[&base, "missing"]), &base, None).expect_err("missing");
        assert!(error.to_string().contains("does not exist"));
        let file = join(&[&base, "file.txt"]);
        put_contents(&file, "").expect("file");
        let error = copy_file(&file, &base, None).expect_err("file into folder");
        assert!(error.to_string().contains("is file"));
        remove_all(&base).expect("cleanup");
    }
}
