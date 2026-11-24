//! Port of `crypto/gsha256`.

use gf_errors::gerror;
use ring::digest;

use crate::{hash_file, to_hex};

/// Encrypts bytes with SHA256.
pub fn encrypt_bytes(data: &[u8]) -> String {
    to_hex(digest::digest(&digest::SHA256, data).as_ref())
}

/// Encrypts a string with SHA256.
pub fn encrypt_string(data: &str) -> String {
    encrypt_bytes(data.as_bytes())
}

/// Encrypts the content of `path` with SHA256.
pub fn encrypt_file(path: &str) -> Result<String, gerror::Error> {
    hash_file(path, &digest::SHA256)
}

/// Encrypts the content of `path` with SHA256, panicking on failure.
pub fn must_encrypt_file(path: &str) -> String {
    match encrypt_file(path) {
        Ok(digest) => digest,
        Err(err) => panic!("{err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digest() {
        assert_eq!(
            encrypt_string("pibigstar"),
            "b5568f1b35aeb9eb7528336dea6c211a2cdcec1f333d98141b8adf346717907e"
        );
        assert_eq!(
            encrypt_string(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn file_digest() {
        let dir = std::env::temp_dir().join("gf-sha256-test");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("test.text");
        std::fs::write(&path, b"pibigstar").expect("write");
        let path = path.to_string_lossy().into_owned();
        assert_eq!(
            must_encrypt_file(&path),
            "b5568f1b35aeb9eb7528336dea6c211a2cdcec1f333d98141b8adf346717907e"
        );
        std::fs::remove_file(&path).ok();
    }
}
