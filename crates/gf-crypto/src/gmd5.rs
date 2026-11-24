//! Port of `crypto/gmd5`.

use std::io::Read;

use gf_errors::gerror;
use md5::{Digest, Md5};

use crate::{to_hex, wrap};

fn md5_file(path: &str) -> Result<String, gerror::Error> {
    let mut file = std::fs::File::open(path)
        .map_err(|err| wrap(err, &format!("os.Open failed for name \"{path}\"")))?;
    let mut hasher = Md5::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| wrap(err, "io.Copy failed"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(to_hex(&hasher.finalize()))
}

/// Encrypts bytes with MD5.
pub fn encrypt_bytes(data: &[u8]) -> Result<String, gerror::Error> {
    Ok(to_hex(&Md5::digest(data)))
}

/// Encrypts bytes with MD5, panicking on failure.
pub fn must_encrypt_bytes(data: &[u8]) -> String {
    match encrypt_bytes(data) {
        Ok(digest) => digest,
        Err(err) => panic!("{err}"),
    }
}

/// Encrypts a string with MD5.
pub fn encrypt_string(data: &str) -> Result<String, gerror::Error> {
    encrypt_bytes(data.as_bytes())
}

/// Encrypts a string with MD5, panicking on failure.
pub fn must_encrypt_string(data: &str) -> String {
    match encrypt_string(data) {
        Ok(digest) => digest,
        Err(err) => panic!("{err}"),
    }
}

/// Encrypts the content of `path` with MD5.
pub fn encrypt_file(path: &str) -> Result<String, gerror::Error> {
    md5_file(path)
}

/// Encrypts the content of `path` with MD5, panicking on failure.
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
    fn known_digests() {
        assert_eq!(
            encrypt_string("pibigstar").expect("hash"),
            "d175a1ff66aedde64344785f7f7a3df8"
        );
        assert_eq!(
            encrypt_bytes(b"Hello Go Frame").expect("hash"),
            "e6e6e1cd41895beebff16d5452dfce12"
        );
        assert_eq!(
            must_encrypt_string("123456"),
            "e10adc3949ba59abbe56e057f20f883e"
        );
    }

    #[test]
    fn files_and_errors() {
        let dir = std::env::temp_dir().join("gf-md5-test");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("test.text");
        std::fs::write(&path, b"Hello Go Frame").expect("write");
        let path = path.to_string_lossy().into_owned();
        assert_eq!(
            encrypt_file(&path).expect("hash"),
            "e6e6e1cd41895beebff16d5452dfce12"
        );
        let missing = dir.join("missing").to_string_lossy().into_owned();
        assert!(encrypt_file(&missing).is_err());
        std::fs::remove_file(&path).ok();
    }
}
