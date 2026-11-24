//! Port of `crypto/gsha512`.

use gf_errors::gerror;
use ring::digest;

use crate::{hash_file, to_hex};

/// Encrypts bytes with SHA512.
pub fn encrypt_bytes(data: &[u8]) -> String {
    to_hex(digest::digest(&digest::SHA512, data).as_ref())
}

/// Encrypts a string with SHA512.
pub fn encrypt_string(data: &str) -> String {
    encrypt_bytes(data.as_bytes())
}

/// Encrypts the content of `path` with SHA512.
pub fn encrypt_file(path: &str) -> Result<String, gerror::Error> {
    hash_file(path, &digest::SHA512)
}

/// Encrypts the content of `path` with SHA512, panicking on failure.
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
            "c7b81ef31111986759f12df55baf7ea79f9d23557f32656fd271813adc37ab605b793e7c0170180b219a7a66a43a156e04b7563eeab61c4ad04c650b132da269"
        );
    }

    #[test]
    fn file_digest() {
        let dir = std::env::temp_dir().join("gf-sha512-test");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("test.text");
        std::fs::write(&path, b"pibigstar").expect("write");
        let path = path.to_string_lossy().into_owned();
        assert_eq!(
            encrypt_file(&path).expect("hash"),
            "c7b81ef31111986759f12df55baf7ea79f9d23557f32656fd271813adc37ab605b793e7c0170180b219a7a66a43a156e04b7563eeab61c4ad04c650b132da269"
        );
        std::fs::remove_file(&path).ok();
    }
}
