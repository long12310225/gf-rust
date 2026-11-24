//! Port of `crypto/gaes`.

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockCipher, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::{Aes128, Aes192, Aes256};
use cfb_mode::Decryptor as CfbDecryptor;
use cfb_mode::Encryptor as CfbEncryptor;
use cfb_mode::cipher::{AsyncStreamCipher, KeyIvInit};
use gf_errors::gcode::CODE_INVALID_PARAMETER;
use gf_errors::gerror;

/// The default IV, a full 16-byte block.
pub const IV_DEFAULT_VALUE: &str = "I Love Go Frame!";

const BLOCK_SIZE: usize = 16;

fn invalid(text: &str) -> gerror::Error {
    gerror::Error::new_code(&CODE_INVALID_PARAMETER, &[text])
}

fn invalidf(format: &str, args: Vec<gf_core::Value>) -> gerror::Error {
    gerror::Error::new_codef(&CODE_INVALID_PARAMETER, format, args)
}

fn key_error(key: &[u8]) -> gerror::Error {
    invalidf(
        "aes.NewCipher failed for key \"%s\"",
        vec![gf_core::Value::String(
            String::from_utf8_lossy(key).into_owned(),
        )],
    )
}

fn iv_value(iv: Option<&[u8]>) -> Result<Vec<u8>, gerror::Error> {
    let iv = iv.unwrap_or(IV_DEFAULT_VALUE.as_bytes());
    if iv.len() != BLOCK_SIZE {
        return Err(invalid("invalid iv length"));
    }
    Ok(iv.to_vec())
}

/// Applies PKCS#7 padding to `src` for `block_size`.
pub fn pkcs7_padding(src: &[u8], block_size: usize) -> Vec<u8> {
    let padding = block_size - src.len() % block_size;
    let mut out = Vec::with_capacity(src.len() + padding);
    out.extend_from_slice(src);
    out.extend(std::iter::repeat_n(padding as u8, padding));
    out
}

/// Removes PKCS#7 padding, with the Go package's error texts.
pub fn pkcs7_unpadding(src: &[u8], block_size: usize) -> Result<Vec<u8>, gerror::Error> {
    if block_size == 0 {
        return Err(invalidf(
            "invalid blockSize: %d",
            vec![gf_core::Value::Int(block_size as i64)],
        ));
    }
    let length = src.len();
    if !length.is_multiple_of(block_size) || length == 0 {
        return Err(invalid("invalid data len"));
    }
    let unpadding = usize::from(src[length - 1]);
    if unpadding > block_size || unpadding == 0 {
        return Err(invalid("invalid unpadding"));
    }
    let padding = &src[length - unpadding..];
    if padding.iter().any(|&byte| usize::from(byte) != unpadding) {
        return Err(invalid("invalid padding"));
    }
    Ok(src[..length - unpadding].to_vec())
}

/// Applies PKCS#5 padding (default block size 8).
pub fn pkcs5_padding(src: &[u8], block_size: Option<usize>) -> Vec<u8> {
    pkcs7_padding(src, block_size.unwrap_or(8))
}

/// Removes PKCS#5 padding (default block size 8).
pub fn pkcs5_unpadding(src: &[u8], block_size: Option<usize>) -> Result<Vec<u8>, gerror::Error> {
    pkcs7_unpadding(src, block_size.unwrap_or(8))
}

fn cbc_encrypt<C>(plain: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>, gerror::Error>
where
    C: BlockEncrypt + KeyInit,
{
    let cipher = C::new_from_slice(key).map_err(|_| key_error(key))?;
    let mut out = plain.to_vec();
    let mut previous = [0u8; BLOCK_SIZE];
    previous.copy_from_slice(iv);
    for chunk in out.as_chunks_mut::<BLOCK_SIZE>().0 {
        for (index, byte) in chunk.iter_mut().enumerate() {
            *byte ^= previous[index];
        }
        cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
        previous.copy_from_slice(chunk);
    }
    Ok(out)
}

fn cbc_decrypt<C>(cipher_text: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>, gerror::Error>
where
    C: BlockDecrypt + KeyInit,
{
    let cipher = C::new_from_slice(key).map_err(|_| key_error(key))?;
    let mut out = cipher_text.to_vec();
    let mut previous = [0u8; BLOCK_SIZE];
    previous.copy_from_slice(iv);
    for chunk in out.as_chunks_mut::<BLOCK_SIZE>().0 {
        let mut encrypted = [0u8; BLOCK_SIZE];
        encrypted.copy_from_slice(chunk);
        cipher.decrypt_block(GenericArray::from_mut_slice(chunk));
        for (index, byte) in chunk.iter_mut().enumerate() {
            *byte ^= previous[index];
        }
        previous = encrypted;
    }
    Ok(out)
}

/// Encrypts with AES-CBC (alias of [`encrypt_cbc`]).
pub fn encrypt(plain_text: &[u8], key: &[u8], iv: Option<&[u8]>) -> Result<Vec<u8>, gerror::Error> {
    encrypt_cbc(plain_text, key, iv)
}

/// Decrypts AES-CBC (alias of [`decrypt_cbc`]).
pub fn decrypt(
    cipher_text: &[u8],
    key: &[u8],
    iv: Option<&[u8]>,
) -> Result<Vec<u8>, gerror::Error> {
    decrypt_cbc(cipher_text, key, iv)
}

/// Encrypts `plain_text` with AES-CBC and PKCS#7 padding.
pub fn encrypt_cbc(
    plain_text: &[u8],
    key: &[u8],
    iv: Option<&[u8]>,
) -> Result<Vec<u8>, gerror::Error> {
    let iv = iv_value(iv)?;
    let padded = pkcs7_padding(plain_text, BLOCK_SIZE);
    match key.len() {
        16 => cbc_encrypt::<Aes128>(&padded, key, &iv),
        24 => cbc_encrypt::<Aes192>(&padded, key, &iv),
        32 => cbc_encrypt::<Aes256>(&padded, key, &iv),
        _ => Err(key_error(key)),
    }
}

/// Decrypts `cipher_text` produced by [`encrypt_cbc`].
pub fn decrypt_cbc(
    cipher_text: &[u8],
    key: &[u8],
    iv: Option<&[u8]>,
) -> Result<Vec<u8>, gerror::Error> {
    let iv = iv_value(iv)?;
    if cipher_text.len() < BLOCK_SIZE {
        return Err(invalid("cipherText too short"));
    }
    if !cipher_text.len().is_multiple_of(BLOCK_SIZE) {
        return Err(invalid("cipherText is not a multiple of the block size"));
    }
    let plain = match key.len() {
        16 => cbc_decrypt::<Aes128>(cipher_text, key, &iv)?,
        24 => cbc_decrypt::<Aes192>(cipher_text, key, &iv)?,
        32 => cbc_decrypt::<Aes256>(cipher_text, key, &iv)?,
        _ => return Err(key_error(key)),
    };
    pkcs7_unpadding(&plain, BLOCK_SIZE)
}

/// Pads `data` with zeroes to a multiple of `block_size`, returning the count.
pub fn zero_padding(data: &[u8], block_size: usize) -> (Vec<u8>, usize) {
    let padding = block_size - data.len() % block_size;
    let mut out = Vec::with_capacity(data.len() + padding);
    out.extend_from_slice(data);
    out.extend(std::iter::repeat_n(0u8, padding));
    (out, padding)
}

/// Removes `un_padding` trailing bytes.
pub fn zero_unpadding(plain_text: &[u8], un_padding: usize) -> Vec<u8> {
    plain_text[..plain_text.len() - un_padding].to_vec()
}

/// Encrypts with AES-CFB, writing the zero-padding count into `padding`.
pub fn encrypt_cfb(
    plain_text: &[u8],
    key: &[u8],
    padding: &mut usize,
    iv: Option<&[u8]>,
) -> Result<Vec<u8>, gerror::Error> {
    let iv = iv_value(iv)?;
    let (mut buffer, pad) = zero_padding(plain_text, BLOCK_SIZE);
    *padding = pad;
    match key.len() {
        16 => apply_cfb_encrypt::<Aes128>(&mut buffer, key, &iv)?,
        24 => apply_cfb_encrypt::<Aes192>(&mut buffer, key, &iv)?,
        32 => apply_cfb_encrypt::<Aes256>(&mut buffer, key, &iv)?,
        _ => return Err(key_error(key)),
    }
    Ok(buffer)
}

fn apply_cfb_encrypt<C>(buffer: &mut [u8], key: &[u8], iv: &[u8]) -> Result<(), gerror::Error>
where
    C: BlockCipher + BlockEncrypt + BlockDecrypt + KeyInit,
{
    let stream = CfbEncryptor::<C>::new_from_slices(key, iv).map_err(|_| key_error(key))?;
    stream.encrypt(buffer);
    Ok(())
}

fn apply_cfb_decrypt<C>(buffer: &mut [u8], key: &[u8], iv: &[u8]) -> Result<(), gerror::Error>
where
    C: BlockCipher + BlockEncrypt + BlockDecrypt + KeyInit,
{
    let stream = CfbDecryptor::<C>::new_from_slices(key, iv).map_err(|_| key_error(key))?;
    stream.decrypt(buffer);
    Ok(())
}

/// Decrypts AES-CFB and removes `un_padding` trailing bytes.
pub fn decrypt_cfb(
    cipher_text: &[u8],
    key: &[u8],
    un_padding: usize,
    iv: Option<&[u8]>,
) -> Result<Vec<u8>, gerror::Error> {
    let iv = iv_value(iv)?;
    if cipher_text.len() < BLOCK_SIZE {
        return Err(invalid("cipherText too short"));
    }
    let mut buffer = cipher_text.to_vec();
    match key.len() {
        16 => apply_cfb_decrypt::<Aes128>(&mut buffer, key, &iv)?,
        24 => apply_cfb_decrypt::<Aes192>(&mut buffer, key, &iv)?,
        32 => apply_cfb_decrypt::<Aes256>(&mut buffer, key, &iv)?,
        _ => return Err(key_error(key)),
    }
    Ok(zero_unpadding(&buffer, un_padding))
}
