//! Port of `crypto/gsha1`.

use gf_errors::gerror;
use ring::digest;

use crate::{hash_file, to_hex};

/// Encrypts bytes with SHA1.
pub fn encrypt_bytes(data: &[u8]) -> String {
    to_hex(digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, data).as_ref())
}

/// Encrypts a string with SHA1.
pub fn encrypt_string(data: &str) -> String {
    encrypt_bytes(data.as_bytes())
}

/// Encrypts the content of `path` with SHA1.
pub fn encrypt_file(path: &str) -> Result<String, gerror::Error> {
    hash_file(path, &digest::SHA1_FOR_LEGACY_USE_ONLY)
}

/// Encrypts the content of `path` with SHA1, panicking on failure.
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
            "5b4c1c2a08ca85ddd031ef8627414f4cb2620b41"
        );
        assert_eq!(
            encrypt_string(""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn file_digest() {
        let dir = std::env::temp_dir().join("gf-sha1-test");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("test.text");
        std::fs::write(&path, b"pibigstar").expect("write");
        let path = path.to_string_lossy().into_owned();
        assert_eq!(
            encrypt_file(&path).expect("hash"),
            "5b4c1c2a08ca85ddd031ef8627414f4cb2620b41"
        );
        std::fs::remove_file(&path).ok();
    }
}
