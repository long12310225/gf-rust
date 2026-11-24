//! Port of `crypto/gdes`.

use des::cipher::generic_array::GenericArray;
use des::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use des::{Des, TdesEde3};
use gf_core::Value;
use gf_errors::gcode::CODE_INVALID_PARAMETER;
use gf_errors::gerror;

/// No padding.
pub const NOPADDING: i32 = 0;
/// PKCS#5 padding.
pub const PKCS5PADDING: i32 = 1;

const BLOCK_SIZE: usize = 8;

fn invalid(text: &str) -> gerror::Error {
    gerror::Error::new_code(&CODE_INVALID_PARAMETER, &[text])
}

fn invalidf(format: &str, args: Vec<Value>) -> gerror::Error {
    gerror::Error::new_codef(&CODE_INVALID_PARAMETER, format, args)
}

fn key_error(key: &[u8], triple: bool) -> gerror::Error {
    let name = if triple {
        "des.NewTripleDESCipher"
    } else {
        "des.NewCipher"
    };
    invalidf(
        &format!("{name} failed for key \"%s\""),
        vec![Value::String(String::from_utf8_lossy(key).into_owned())],
    )
}

/// Applies PKCS#5 padding with a block size of 8.
pub fn padding_pkcs5(text: &[u8], block_size: usize) -> Vec<u8> {
    let padding = block_size - text.len() % block_size;
    let mut out = Vec::with_capacity(text.len() + padding);
    out.extend_from_slice(text);
    out.extend(std::iter::repeat_n(padding as u8, padding));
    out
}

/// Removes PKCS#5 padding.
pub fn unpadding_pkcs5(text: &[u8]) -> Vec<u8> {
    if text.is_empty() {
        return Vec::new();
    }
    let padding = usize::from(text[text.len() - 1]);
    if padding == 0 || padding > text.len() {
        return Vec::new();
    }
    text[..text.len() - padding].to_vec()
}

/// Pads `text` according to the Go padding constant.
pub fn padding(text: &[u8], padding: i32) -> Result<Vec<u8>, gerror::Error> {
    match padding {
        NOPADDING => {
            if !text.len().is_multiple_of(BLOCK_SIZE) {
                return Err(invalid("invalid text length"));
            }
            Ok(text.to_vec())
        }
        PKCS5PADDING => Ok(padding_pkcs5(text, BLOCK_SIZE)),
        other => Err(invalidf(
            "unsupported padding type \"%d\"",
            vec![Value::Int(i64::from(other))],
        )),
    }
}

/// Removes padding according to the Go padding constant.
pub fn unpadding(text: &[u8], padding: i32) -> Result<Vec<u8>, gerror::Error> {
    match padding {
        NOPADDING => {
            if !text.len().is_multiple_of(BLOCK_SIZE) {
                return Err(invalid("invalid text length"));
            }
            Ok(text.to_vec())
        }
        PKCS5PADDING => Ok(unpadding_pkcs5(text)),
        other => Err(invalidf(
            "unsupported padding type \"%d\"",
            vec![Value::Int(i64::from(other))],
        )),
    }
}

fn expand_triple_key(key: &[u8]) -> Vec<u8> {
    if key.len() == 16 {
        let mut expanded = key.to_vec();
        expanded.extend_from_slice(&key[..8]);
        expanded
    } else {
        key.to_vec()
    }
}

fn ecb_encrypt<C: BlockEncrypt + KeyInit>(
    text: &[u8],
    key: &[u8],
    triple: bool,
) -> Result<Vec<u8>, gerror::Error> {
    let cipher = C::new_from_slice(key).map_err(|_| key_error(key, triple))?;
    let block_size = C::block_size();
    let mut out = text.to_vec();
    for chunk in out.chunks_exact_mut(block_size) {
        cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
    }
    Ok(out)
}

fn ecb_decrypt<C: BlockDecrypt + KeyInit>(
    text: &[u8],
    key: &[u8],
    triple: bool,
) -> Result<Vec<u8>, gerror::Error> {
    let cipher = C::new_from_slice(key).map_err(|_| key_error(key, triple))?;
    let block_size = C::block_size();
    let mut out = text.to_vec();
    for chunk in out.chunks_exact_mut(block_size) {
        cipher.decrypt_block(GenericArray::from_mut_slice(chunk));
    }
    Ok(out)
}

fn cbc_encrypt<C: BlockEncrypt + KeyInit>(
    text: &[u8],
    key: &[u8],
    iv: &[u8],
    triple: bool,
) -> Result<Vec<u8>, gerror::Error> {
    let cipher = C::new_from_slice(key).map_err(|_| key_error(key, triple))?;
    let block_size = C::block_size();
    let mut out = text.to_vec();
    let mut previous = iv.to_vec();
    for chunk in out.chunks_exact_mut(block_size) {
        for (index, byte) in chunk.iter_mut().enumerate() {
            *byte ^= previous[index];
        }
        cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
        previous.copy_from_slice(chunk);
    }
    Ok(out)
}

fn cbc_decrypt<C: BlockDecrypt + KeyInit>(
    text: &[u8],
    key: &[u8],
    iv: &[u8],
    triple: bool,
) -> Result<Vec<u8>, gerror::Error> {
    let cipher = C::new_from_slice(key).map_err(|_| key_error(key, triple))?;
    let block_size = C::block_size();
    let mut out = text.to_vec();
    let mut previous = iv.to_vec();
    for chunk in out.chunks_exact_mut(block_size) {
        let mut encrypted = vec![0u8; block_size];
        encrypted.copy_from_slice(chunk);
        cipher.decrypt_block(GenericArray::from_mut_slice(chunk));
        for (index, byte) in chunk.iter_mut().enumerate() {
            *byte ^= previous[index];
        }
        previous.copy_from_slice(&encrypted);
    }
    Ok(out)
}

/// Encrypts with DES-ECB.
pub fn encrypt_ecb(
    plain_text: &[u8],
    key: &[u8],
    padding_mode: i32,
) -> Result<Vec<u8>, gerror::Error> {
    let text = padding(plain_text, padding_mode)?;
    ecb_encrypt::<Des>(&text, key, false)
}

/// Decrypts DES-ECB.
pub fn decrypt_ecb(
    cipher_text: &[u8],
    key: &[u8],
    padding_mode: i32,
) -> Result<Vec<u8>, gerror::Error> {
    let text = ecb_decrypt::<Des>(cipher_text, key, false)?;
    unpadding(&text, padding_mode)
}

/// Encrypts with TripleDES-ECB.
pub fn encrypt_ecb_triple(
    plain_text: &[u8],
    key: &[u8],
    padding_mode: i32,
) -> Result<Vec<u8>, gerror::Error> {
    if key.len() != 16 && key.len() != 24 {
        return Err(invalid("key length error"));
    }
    let text = padding(plain_text, padding_mode)?;
    let key = expand_triple_key(key);
    ecb_encrypt::<TdesEde3>(&text, &key, true)
}

/// Decrypts TripleDES-ECB.
pub fn decrypt_ecb_triple(
    cipher_text: &[u8],
    key: &[u8],
    padding_mode: i32,
) -> Result<Vec<u8>, gerror::Error> {
    if key.len() != 16 && key.len() != 24 {
        return Err(invalid("key length error"));
    }
    let key = expand_triple_key(key);
    let text = ecb_decrypt::<TdesEde3>(cipher_text, &key, true)?;
    unpadding(&text, padding_mode)
}

/// Encrypts with DES-CBC.
pub fn encrypt_cbc(
    plain_text: &[u8],
    key: &[u8],
    iv: &[u8],
    padding_mode: i32,
) -> Result<Vec<u8>, gerror::Error> {
    if iv.len() != BLOCK_SIZE {
        return Err(invalid("invalid iv length"));
    }
    let text = padding(plain_text, padding_mode)?;
    cbc_encrypt::<Des>(&text, key, iv, false)
}

/// Decrypts DES-CBC.
pub fn decrypt_cbc(
    cipher_text: &[u8],
    key: &[u8],
    iv: &[u8],
    padding_mode: i32,
) -> Result<Vec<u8>, gerror::Error> {
    if iv.len() != BLOCK_SIZE {
        return Err(invalid("iv length invalid"));
    }
    let text = cbc_decrypt::<Des>(cipher_text, key, iv, false)?;
    unpadding(&text, padding_mode)
}

/// Encrypts with TripleDES-CBC.
pub fn encrypt_cbc_triple(
    plain_text: &[u8],
    key: &[u8],
    iv: &[u8],
    padding_mode: i32,
) -> Result<Vec<u8>, gerror::Error> {
    if key.len() != 16 && key.len() != 24 {
        return Err(invalid("key length invalid"));
    }
    if iv.len() != BLOCK_SIZE {
        return Err(invalid("invalid iv length"));
    }
    let text = padding(plain_text, padding_mode)?;
    let key = expand_triple_key(key);
    cbc_encrypt::<TdesEde3>(&text, &key, iv, true)
}

/// Decrypts TripleDES-CBC.
pub fn decrypt_cbc_triple(
    cipher_text: &[u8],
    key: &[u8],
    iv: &[u8],
    padding_mode: i32,
) -> Result<Vec<u8>, gerror::Error> {
    if key.len() != 16 && key.len() != 24 {
        return Err(invalid("key length invalid"));
    }
    if iv.len() != BLOCK_SIZE {
        return Err(invalid("iv length invalid"));
    }
    let key = expand_triple_key(key);
    let text = cbc_decrypt::<TdesEde3>(cipher_text, &key, iv, true)?;
    unpadding(&text, padding_mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::to_hex;

    #[test]
    fn des_ecb_matches_go() {
        let key = b"11111111";
        let text = b"12345678";
        let cipher_text = encrypt_ecb(text, key, NOPADDING).expect("encrypt");
        assert_eq!(to_hex(&cipher_text), "858b176da8b12503");
        assert_eq!(
            decrypt_ecb(&cipher_text, key, NOPADDING).expect("decrypt"),
            text
        );

        let cipher_text = encrypt_ecb(b"12345678", key, PKCS5PADDING).expect("encrypt");
        assert_eq!(to_hex(&cipher_text), "858b176da8b12503ad6a88b4fa37833d");
        assert_eq!(
            decrypt_ecb(&cipher_text, key, PKCS5PADDING).expect("decrypt"),
            b"12345678"
        );
    }

    #[test]
    fn triple_des_ecb_matches_go() {
        let key = b"1111111111111234";
        let text = b"1234567812345678";
        let cipher_text = encrypt_ecb_triple(text, key, NOPADDING).expect("encrypt");
        assert_eq!(to_hex(&cipher_text), "a23ee24b98c26263a23ee24b98c26263");
        assert_eq!(
            decrypt_ecb_triple(&cipher_text, key, NOPADDING).expect("decrypt"),
            text
        );

        let key = b"111111111111123412345678";
        let cipher_text = encrypt_ecb_triple(b"123456789", key, PKCS5PADDING).expect("encrypt");
        assert_eq!(to_hex(&cipher_text), "37989b1effc07a6d00ff89a7d052e79f");
    }

    #[test]
    fn des_cbc_matches_go() {
        let key = b"11111111";
        let iv = b"12345678";
        let text = b"1234567812345678";
        let cipher_text = encrypt_cbc(text, key, iv, NOPADDING).expect("encrypt");
        assert_eq!(to_hex(&cipher_text), "40826a5800608c87585ca7c9efabee47");
        assert_eq!(
            decrypt_cbc(&cipher_text, key, iv, NOPADDING).expect("decrypt"),
            text
        );

        let cipher_text = encrypt_cbc(b"12345678", key, iv, PKCS5PADDING).expect("encrypt");
        assert_eq!(to_hex(&cipher_text), "40826a5800608c87100a25d86ac7c52c");
    }

    #[test]
    fn triple_des_cbc_matches_go() {
        let key = b"1111111112345678";
        let iv = b"12345678";
        let text = b"1234567812345678";
        let cipher_text = encrypt_cbc_triple(text, key, iv, NOPADDING).expect("encrypt");
        assert_eq!(to_hex(&cipher_text), "bfde1394e265d5f738d5cab170c77c88");
        assert_eq!(
            decrypt_cbc_triple(&cipher_text, key, iv, NOPADDING).expect("decrypt"),
            text
        );

        let key = b"111111111234567812345678";
        let cipher_text = encrypt_cbc_triple(b"12345678", key, iv, PKCS5PADDING).expect("encrypt");
        assert_eq!(to_hex(&cipher_text), "40826a5800608c87100a25d86ac7c52c");
    }

    #[test]
    fn errors_match_go() {
        assert!(encrypt_ecb(b"12345678", b"short", NOPADDING).is_err());
        assert!(encrypt_ecb(b"12345678", b"11111111", 5).is_err());
        assert!(decrypt_cbc(b"x", b"11111111", b"short", NOPADDING).is_err());
        assert!(encrypt_ecb_triple(b"x", b"short", PKCS5PADDING).is_err());
        assert_eq!(
            unpadding(b"123", NOPADDING)
                .expect_err("length")
                .to_string(),
            "invalid text length"
        );
    }
}
