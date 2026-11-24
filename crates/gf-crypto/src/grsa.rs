//! Port of `crypto/grsa`.
//!
//! Key parsing accepts PKCS#1 and PKCS#8/PKIX PEM blocks. Encryption uses
//! PKCS#1 v1.5 by default and OAEP (SHA-256) in the `*_oaep` functions; labels
//! are not supported because the RustCrypto `Oaep` type does not expose them in
//! the same shape (ADR-023).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use gf_core::Value;
use gf_errors::gcode::{CODE_INTERNAL_ERROR, CODE_INVALID_PARAMETER};
use gf_errors::gerror;
use rand::rngs::OsRng;
use rsa::pkcs1::{
    DecodeRsaPrivateKey, DecodeRsaPublicKey, EncodeRsaPrivateKey, EncodeRsaPublicKey,
};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use rsa::traits::PublicKeyParts;
use rsa::{Oaep, Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;

/// Default RSA key size.
pub const DEFAULT_RSA_KEY_BITS: usize = 2048;
/// PKCS#1 private key type name.
pub const KEY_TYPE_PKCS1: &str = "PKCS#1";
/// PKCS#8 private key type name.
pub const KEY_TYPE_PKCS8: &str = "PKCS#8";

const PEM_RSA_PRIVATE_KEY: &str = "RSA PRIVATE KEY";
const PEM_PRIVATE_KEY: &str = "PRIVATE KEY";
const PEM_RSA_PUBLIC_KEY: &str = "RSA PUBLIC KEY";
const PEM_PUBLIC_KEY: &str = "PUBLIC KEY";

fn invalid(text: &str) -> gerror::Error {
    gerror::Error::new_code(&CODE_INVALID_PARAMETER, &[text])
}

fn invalidf(format: &str, args: Vec<Value>) -> gerror::Error {
    gerror::Error::new_codef(&CODE_INVALID_PARAMETER, format, args)
}

fn wrap(err: impl std::error::Error + Send + Sync + 'static, text: &str) -> gerror::Error {
    match gerror::wrap_std(Some(Box::new(err)), text) {
        Some(err) => err,
        None => gerror::Error::new(text),
    }
}

fn internal(err: impl std::error::Error + Send + Sync + 'static, text: &str) -> gerror::Error {
    let mut error = wrap(err, text);
    error.set_code(CODE_INTERNAL_ERROR.clone());
    error
}

fn parse_public(public_key: &[u8]) -> Result<RsaPublicKey, gerror::Error> {
    let pem = pem::parse(public_key).map_err(|_| invalid("invalid public key"))?;
    if let Ok(key) = RsaPublicKey::from_public_key_der(pem.contents()) {
        return Ok(key);
    }
    if let Ok(key) = RsaPublicKey::from_pkcs1_der(pem.contents()) {
        return Ok(key);
    }
    Err(wrap(
        std::io::Error::other("unsupported public key encoding"),
        "failed to parse public key",
    ))
}

fn parse_private(private_key: &[u8]) -> Result<RsaPrivateKey, gerror::Error> {
    let pem = pem::parse(private_key).map_err(|_| invalid("invalid private key"))?;
    if let Ok(key) = RsaPrivateKey::from_pkcs8_der(pem.contents()) {
        return Ok(key);
    }
    if let Ok(key) = RsaPrivateKey::from_pkcs1_der(pem.contents()) {
        return Ok(key);
    }
    Err(wrap(
        std::io::Error::other("unsupported private key encoding"),
        "failed to parse private key",
    ))
}

fn encrypt_pkcs1_raw(plain_text: &[u8], public_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let key = parse_public(public_key)?;
    let max_size = key.size().saturating_sub(11);
    if plain_text.len() > max_size {
        return Err(invalidf(
            "plaintext too long: max %d bytes for this key, got %d bytes",
            vec![
                Value::Int(max_size as i64),
                Value::Int(plain_text.len() as i64),
            ],
        ));
    }
    key.encrypt(&mut OsRng, Pkcs1v15Encrypt, plain_text)
        .map_err(|err| wrap(err, "rsa.EncryptPKCS1v15 failed"))
}

fn decrypt_pkcs1_raw(cipher_text: &[u8], private_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let key = parse_private(private_key)?;
    key.decrypt(Pkcs1v15Encrypt, cipher_text)
        .map_err(|err| wrap(err, "rsa.DecryptPKCS1v15 failed"))
}

fn encrypt_oaep_padded(plain_text: &[u8], public_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let key = parse_public(public_key)?;
    let max_size = key.size().saturating_sub(2 * 32 + 2);
    if plain_text.len() > max_size {
        return Err(invalidf(
            "plaintext too long: max %d bytes for this key with OAEP, got %d bytes",
            vec![
                Value::Int(max_size as i64),
                Value::Int(plain_text.len() as i64),
            ],
        ));
    }
    key.encrypt(&mut OsRng, Oaep::new::<Sha256>(), plain_text)
        .map_err(|err| wrap(err, "rsa.EncryptOAEP failed"))
}

fn decrypt_oaep_padded(cipher_text: &[u8], private_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let key = parse_private(private_key)?;
    key.decrypt(Oaep::new::<Sha256>(), cipher_text)
        .map_err(|err| wrap(err, "rsa.DecryptOAEP failed"))
}

/// Encrypts with PKCS#1 v1.5 padding, auto-detecting the key format.
pub fn encrypt(plain_text: &[u8], public_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    encrypt_pkcs1_raw(plain_text, public_key)
}

/// Decrypts with PKCS#1 v1.5 padding, auto-detecting the key format.
pub fn decrypt(cipher_text: &[u8], private_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    decrypt_pkcs1_raw(cipher_text, private_key)
}

/// Encrypts with a base64-encoded public key and returns base64 text.
pub fn encrypt_base64(plain_text: &[u8], public_key_base64: &str) -> Result<String, gerror::Error> {
    let public_key = STANDARD
        .decode(public_key_base64)
        .map_err(|err| wrap(err, "failed to decode public key"))?;
    let encrypted = encrypt(plain_text, &public_key)?;
    Ok(STANDARD.encode(encrypted))
}

/// Decrypts base64 text with a base64-encoded private key.
pub fn decrypt_base64(
    cipher_text_base64: &str,
    private_key_base64: &str,
) -> Result<Vec<u8>, gerror::Error> {
    let private_key = STANDARD
        .decode(private_key_base64)
        .map_err(|err| wrap(err, "failed to decode private key"))?;
    let cipher_text = STANDARD
        .decode(cipher_text_base64)
        .map_err(|err| wrap(err, "failed to decode cipher text"))?;
    decrypt(&cipher_text, &private_key)
}

/// Encrypts with a PKIX (X.509) public key.
pub fn encrypt_pkix(plain_text: &[u8], public_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let parsed = pem::parse(public_key).map_err(|_| invalid("invalid public key"))?;
    let key = RsaPublicKey::from_public_key_der(parsed.contents())
        .map_err(|err| wrap(err, "failed to parse PKIX public key"))?;
    let max_size = key.size().saturating_sub(11);
    if plain_text.len() > max_size {
        return Err(invalidf(
            "plaintext too long: max %d bytes for this key, got %d bytes",
            vec![
                Value::Int(max_size as i64),
                Value::Int(plain_text.len() as i64),
            ],
        ));
    }
    key.encrypt(&mut OsRng, Pkcs1v15Encrypt, plain_text)
        .map_err(|err| wrap(err, "rsa.EncryptPKCS1v15 failed"))
}

/// Deprecated alias of [`encrypt_pkix`].
pub fn encrypt_pkcs8(plain_text: &[u8], public_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    encrypt_pkix(plain_text, public_key)
}

/// Encrypts with a PKCS#1 public key.
pub fn encrypt_pkcs1(plain_text: &[u8], public_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let parsed = pem::parse(public_key).map_err(|_| invalid("invalid public key"))?;
    let key = RsaPublicKey::from_pkcs1_der(parsed.contents())
        .map_err(|err| wrap(err, "failed to parse PKCS#1 public key"))?;
    let max_size = key.size().saturating_sub(11);
    if plain_text.len() > max_size {
        return Err(invalidf(
            "plaintext too long: max %d bytes for this key, got %d bytes",
            vec![
                Value::Int(max_size as i64),
                Value::Int(plain_text.len() as i64),
            ],
        ));
    }
    key.encrypt(&mut OsRng, Pkcs1v15Encrypt, plain_text)
        .map_err(|err| wrap(err, "rsa.EncryptPKCS1v15 failed"))
}

/// Decrypts with a PKCS#8 private key.
pub fn decrypt_pkcs8(cipher_text: &[u8], private_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let parsed = pem::parse(private_key).map_err(|_| invalid("invalid private key"))?;
    let key = RsaPrivateKey::from_pkcs8_der(parsed.contents())
        .map_err(|err| wrap(err, "failed to parse PKCS#8 private key"))?;
    key.decrypt(Pkcs1v15Encrypt, cipher_text)
        .map_err(|err| wrap(err, "rsa.DecryptPKCS1v15 failed"))
}

/// Decrypts with a PKCS#1 private key.
pub fn decrypt_pkcs1(cipher_text: &[u8], private_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let parsed = pem::parse(private_key).map_err(|_| invalid("invalid private key"))?;
    let key = RsaPrivateKey::from_pkcs1_der(parsed.contents())
        .map_err(|err| wrap(err, "failed to parse private key"))?;
    key.decrypt(Pkcs1v15Encrypt, cipher_text)
        .map_err(|err| wrap(err, "rsa.DecryptPKCS1v15 failed"))
}

/// Encrypts with a PKIX public key and returns base64 text.
pub fn encrypt_pkix_base64(
    plain_text: &[u8],
    public_key_base64: &str,
) -> Result<String, gerror::Error> {
    let public_key = STANDARD
        .decode(public_key_base64)
        .map_err(|err| wrap(err, "failed to decode public key"))?;
    let encrypted = encrypt_pkix(plain_text, &public_key)?;
    Ok(STANDARD.encode(encrypted))
}

/// Deprecated alias of [`encrypt_pkix_base64`].
pub fn encrypt_pkcs8_base64(
    plain_text: &[u8],
    public_key_base64: &str,
) -> Result<String, gerror::Error> {
    encrypt_pkix_base64(plain_text, public_key_base64)
}

/// Encrypts with a PKCS#1 public key and returns base64 text.
pub fn encrypt_pkcs1_base64(
    plain_text: &[u8],
    public_key_base64: &str,
) -> Result<String, gerror::Error> {
    let public_key = STANDARD
        .decode(public_key_base64)
        .map_err(|err| wrap(err, "failed to decode public key"))?;
    let encrypted = encrypt_pkcs1(plain_text, &public_key)?;
    Ok(STANDARD.encode(encrypted))
}

/// Decrypts base64 text with a PKCS#8 private key.
pub fn decrypt_pkcs8_base64(
    cipher_text_base64: &str,
    private_key_base64: &str,
) -> Result<Vec<u8>, gerror::Error> {
    let private_key = STANDARD
        .decode(private_key_base64)
        .map_err(|err| wrap(err, "failed to decode private key"))?;
    let cipher_text = STANDARD
        .decode(cipher_text_base64)
        .map_err(|err| wrap(err, "failed to decode cipher text"))?;
    decrypt_pkcs8(&cipher_text, &private_key)
}

/// Decrypts base64 text with a PKCS#1 private key.
pub fn decrypt_pkcs1_base64(
    cipher_text_base64: &str,
    private_key_base64: &str,
) -> Result<Vec<u8>, gerror::Error> {
    let private_key = STANDARD
        .decode(private_key_base64)
        .map_err(|err| wrap(err, "failed to decode private key"))?;
    let cipher_text = STANDARD
        .decode(cipher_text_base64)
        .map_err(|err| wrap(err, "failed to decode cipher text"))?;
    decrypt_pkcs1(&cipher_text, &private_key)
}

/// Detects whether `private_key` is PKCS#1 or PKCS#8.
pub fn get_private_key_type(private_key: &[u8]) -> Result<&'static str, gerror::Error> {
    let parsed = pem::parse(private_key).map_err(|_| invalid("invalid private key"))?;
    if RsaPrivateKey::from_pkcs1_der(parsed.contents()).is_ok() {
        return Ok(KEY_TYPE_PKCS1);
    }
    if RsaPrivateKey::from_pkcs8_der(parsed.contents()).is_ok() {
        return Ok(KEY_TYPE_PKCS8);
    }
    Err(invalid("unknown private key format"))
}

/// Detects the type of a base64-encoded private key.
pub fn get_private_key_type_base64(
    private_key_base64: &str,
) -> Result<&'static str, gerror::Error> {
    let private_key = STANDARD
        .decode(private_key_base64)
        .map_err(|err| wrap(err, "failed to decode private key"))?;
    get_private_key_type(&private_key)
}

/// Generates an RSA key pair in PKCS#1 form.
pub fn generate_key_pair(bits: usize) -> Result<(Vec<u8>, Vec<u8>), gerror::Error> {
    let key = RsaPrivateKey::new(&mut OsRng, bits)
        .map_err(|err| internal(err, "failed to generate rsa key"))?;
    let private_der = key
        .to_pkcs1_der()
        .map_err(|err| wrap(err, "failed to marshal private key"))?;
    let public_der = key
        .to_public_key()
        .to_pkcs1_der()
        .map_err(|err| invalid(&err.to_string()))?;
    let private_key = pem::encode(&pem::Pem::new(PEM_RSA_PRIVATE_KEY, private_der.as_bytes()));
    let public_key = pem::encode(&pem::Pem::new(PEM_RSA_PUBLIC_KEY, public_der.as_bytes()));
    Ok((private_key.into_bytes(), public_key.into_bytes()))
}

/// Generates an RSA key pair in PKCS#8/PKIX form.
pub fn generate_key_pair_pkcs8(bits: usize) -> Result<(Vec<u8>, Vec<u8>), gerror::Error> {
    let key = RsaPrivateKey::new(&mut OsRng, bits)
        .map_err(|err| internal(err, "failed to generate rsa key"))?;
    let private_der = key
        .to_pkcs8_der()
        .map_err(|err| wrap(err, "failed to marshal private key to PKCS#8"))?;
    let public_der = key
        .to_public_key()
        .to_public_key_der()
        .map_err(|err| wrap(err, "failed to marshal public key"))?;
    let private_key = pem::encode(&pem::Pem::new(PEM_PRIVATE_KEY, private_der.as_bytes()));
    let public_key = pem::encode(&pem::Pem::new(PEM_PUBLIC_KEY, public_der.as_bytes()));
    Ok((private_key.into_bytes(), public_key.into_bytes()))
}

/// Generates a 2048-bit key pair in PKCS#1 form.
pub fn generate_default_key_pair() -> Result<(Vec<u8>, Vec<u8>), gerror::Error> {
    generate_key_pair(DEFAULT_RSA_KEY_BITS)
}

/// Extracts the PKCS#1 public key from a PKCS#1 private key.
pub fn extract_pkcs1_public_key(private_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let parsed = pem::parse(private_key).map_err(|_| invalid("invalid private key"))?;
    let key = RsaPrivateKey::from_pkcs1_der(parsed.contents())
        .map_err(|err| wrap(err, "failed to parse private key"))?;
    let public_der = key
        .to_public_key()
        .to_pkcs1_der()
        .map_err(|err| wrap(err, "failed to marshal public key"))?;
    Ok(pem::encode(&pem::Pem::new(PEM_RSA_PUBLIC_KEY, public_der.as_bytes())).into_bytes())
}

/// Encrypts with OAEP (SHA-256).
pub fn encrypt_oaep(plain_text: &[u8], public_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    encrypt_oaep_padded(plain_text, public_key)
}

/// Decrypts OAEP (SHA-256).
pub fn decrypt_oaep(cipher_text: &[u8], private_key: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    decrypt_oaep_padded(cipher_text, private_key)
}

/// Encrypts with OAEP and returns base64 text.
pub fn encrypt_oaep_base64(
    plain_text: &[u8],
    public_key_base64: &str,
) -> Result<String, gerror::Error> {
    let public_key = STANDARD
        .decode(public_key_base64)
        .map_err(|err| wrap(err, "failed to decode public key"))?;
    let encrypted = encrypt_oaep_padded(plain_text, &public_key)?;
    Ok(STANDARD.encode(encrypted))
}

/// Decrypts base64 OAEP text.
pub fn decrypt_oaep_base64(
    cipher_text_base64: &str,
    private_key_base64: &str,
) -> Result<Vec<u8>, gerror::Error> {
    let private_key = STANDARD
        .decode(private_key_base64)
        .map_err(|err| wrap(err, "failed to decode private key"))?;
    let cipher_text = STANDARD
        .decode(cipher_text_base64)
        .map_err(|err| wrap(err, "failed to decode cipher text"))?;
    decrypt_oaep_padded(&cipher_text, &private_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkcs1_and_pkcs8_round_trip() {
        let (private, public) = generate_key_pair(1024).expect("generate");
        assert_eq!(
            get_private_key_type(&private).expect("type"),
            KEY_TYPE_PKCS1
        );
        let cipher_text = encrypt(b"hello", &public).expect("encrypt");
        assert_eq!(decrypt(&cipher_text, &private).expect("decrypt"), b"hello");

        let (private8, public8) = generate_key_pair_pkcs8(1024).expect("generate");
        assert_eq!(
            get_private_key_type(&private8).expect("type"),
            KEY_TYPE_PKCS8
        );
        let cipher_text = encrypt(b"hello", &public8).expect("encrypt");
        assert_eq!(
            decrypt_pkcs8(&cipher_text, &private8).expect("decrypt"),
            b"hello"
        );
        assert_eq!(
            decrypt_pkcs1(&encrypt_pkcs1(b"x", &public).expect("encrypt"), &private)
                .expect("decrypt"),
            b"x"
        );
        assert_eq!(
            decrypt_pkcs1(&encrypt(&b"x"[..], &public).expect("encrypt"), &private)
                .expect("decrypt"),
            b"x"
        );
    }

    #[test]
    fn base64_round_trip() {
        let (private, public) = generate_key_pair(1024).expect("generate");
        let private = STANDARD.encode(&private);
        let public = STANDARD.encode(&public);
        let cipher_text = encrypt_base64(b"hello", &public).expect("encrypt");
        assert_eq!(
            decrypt_base64(&cipher_text, &private).expect("decrypt"),
            b"hello"
        );
        let cipher_text = encrypt_pkcs1_base64(b"hello", &public).expect("encrypt");
        assert_eq!(
            decrypt_pkcs1_base64(&cipher_text, &private).expect("decrypt"),
            b"hello"
        );
        assert_eq!(
            get_private_key_type_base64(&private).expect("type"),
            KEY_TYPE_PKCS1
        );
    }

    #[test]
    fn oaep_round_trip() {
        let (private, public) = generate_key_pair(1024).expect("generate");
        let cipher_text = encrypt_oaep(b"hello", &public).expect("encrypt");
        assert_eq!(
            decrypt_oaep(&cipher_text, &private).expect("decrypt"),
            b"hello"
        );
        let private = STANDARD.encode(&private);
        let public = STANDARD.encode(&public);
        let cipher_text = encrypt_oaep_base64(b"hello", &public).expect("encrypt");
        assert_eq!(
            decrypt_oaep_base64(&cipher_text, &private).expect("decrypt"),
            b"hello"
        );
    }

    #[test]
    fn errors_match_go() {
        assert_eq!(
            encrypt(b"x", b"not a key").expect_err("public").to_string(),
            "invalid public key"
        );
        assert_eq!(
            decrypt(b"x", b"not a key")
                .expect_err("private")
                .to_string(),
            "invalid private key"
        );
        let (private, public) = generate_key_pair(1024).expect("generate");
        let too_long = vec![0u8; 200];
        assert!(
            encrypt(&too_long, &public)
                .expect_err("too long")
                .to_string()
                .starts_with("plaintext too long: max")
        );
        assert!(decrypt(b"short", &private).is_err());

        let public_key = extract_pkcs1_public_key(&private).expect("extract");
        assert_eq!(
            decrypt_pkcs1(
                &encrypt_pkcs1(b"x", &public_key).expect("encrypt"),
                &private
            )
            .expect("decrypt"),
            b"x"
        );
    }
}
