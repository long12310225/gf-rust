//! Port of `crypto/gcrc32`.

/// Returns the IEEE CRC32 checksum of `data`.
pub fn encrypt(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

/// Returns the IEEE CRC32 checksum of `data` as text.
pub fn encrypt_string(data: &str) -> u32 {
    encrypt(data.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_go() {
        assert_eq!(encrypt_string("pibigstar"), 693_191_136);
        assert_eq!(encrypt(b"pibigstar"), 693_191_136);
        assert_eq!(encrypt(b""), 0);
        assert_eq!(
            encrypt(b"d175a1ff66aedde64344785f7f7a3df8"),
            encrypt_string("d175a1ff66aedde64344785f7f7a3df8")
        );
    }
}
