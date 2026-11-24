//! Port of `internal/fileinfo`: a virtual file description.
//!
//! Go builds this from `os.FileInfo`; here it is a plain value that callers
//! construct, with `mode` keeping Go's `os.FileMode` bit layout.

use std::time::SystemTime;

use gf_core::Value;

/// Go's `os.ModeDir` bit.
pub const MODE_DIR: u32 = 1 << 31;

/// A virtual file description.
#[derive(Clone, Debug)]
pub struct Info {
    name: String,
    size: i64,
    mode: u32,
    mod_time: SystemTime,
}

impl Info {
    /// Creates a file description.
    pub fn new(name: impl Into<String>, size: i64, mode: u32, mod_time: SystemTime) -> Info {
        Info {
            name: name.into(),
            size,
            mode,
            mod_time,
        }
    }

    /// Returns the base name of the file.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the file size in bytes.
    pub fn size(&self) -> i64 {
        self.size
    }

    /// Returns whether the file describes a directory.
    pub fn is_dir(&self) -> bool {
        self.mode & MODE_DIR != 0
    }

    /// Returns the file mode bits.
    pub fn mode(&self) -> u32 {
        self.mode
    }

    /// Returns the modification time.
    pub fn mod_time(&self) -> SystemTime {
        self.mod_time
    }

    /// Returns the underlying data source, which the Go original reports as
    /// `nil`.
    pub fn sys(&self) -> Value {
        Value::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_constructed_values() {
        let time = SystemTime::UNIX_EPOCH;
        let info = Info::new("virtual.txt", 123, 0o644, time);
        assert_eq!(info.name(), "virtual.txt");
        assert_eq!(info.size(), 123);
        assert_eq!(info.mode(), 0o644);
        assert!(!info.is_dir());
        assert_eq!(info.mod_time(), time);
        assert!(info.sys().is_nil());
    }

    #[test]
    fn mode_dir_bit_marks_directories() {
        let info = Info::new("dir", 0, MODE_DIR | 0o755, SystemTime::UNIX_EPOCH);
        assert!(info.is_dir());
        assert_eq!(info.mode() & MODE_DIR, MODE_DIR);
    }
}
