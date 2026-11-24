use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::gaes::*;

fn b64(text: &str) -> Vec<u8> {
    STANDARD.decode(text).expect("vector")
}

#[test]
fn cbc_vectors_match_go() {
    let content = b"pibigstar";
    let key16 = b"1234567891234567";
    let key24 = b"123456789123456789123456";
    let key32 = b"12345678912345678912345678912345";
    let iv = b"Hello My GoFrame";

    assert_eq!(
        encrypt_cbc(content, key16, None).expect("enc"),
        b64("v1jqsGHId/H8onlVHR8Vaw==")
    );
    assert_eq!(
        encrypt_cbc(content, key24, None).expect("enc"),
        b64("0TXOaj5KMoLhNWmJ3lxY1A==")
    );
    assert_eq!(
        encrypt_cbc(content, key32, None).expect("enc"),
        b64("qM/Waw1kkWhrwzek24rCSA==")
    );
    assert_eq!(
        encrypt_cbc(content, key16, Some(iv)).expect("enc"),
        b64("DqQUXiHgW/XFb6Qs98+hrA==")
    );
    assert_eq!(
        encrypt_cbc(content, key32, Some(iv)).expect("enc"),
        b64("ZuLgAOii+lrD5KJoQ7yQ8Q==")
    );

    let cipher_text = encrypt(content, key16, None).expect("enc");
    assert_eq!(decrypt(&cipher_text, key16, None).expect("dec"), content);
    assert_eq!(
        decrypt(&b64("ZuLgAOii+lrD5KJoQ7yQ8Q=="), key32, Some(iv)).expect("dec"),
        content
    );
}

#[test]
fn cfb_vectors_match_go() {
    let content = b"pibigstar";
    let key16 = b"1234567891234567";
    let iv = b"Hello My GoFrame";
    let mut padding = 0;
    let cipher_text = encrypt_cfb(content, key16, &mut padding, Some(iv)).expect("enc");
    assert_eq!(padding, 7);
    assert_eq!(cipher_text, b64("oSmget3aBDT1nJnBp8u6kA=="));
    assert_eq!(
        decrypt_cfb(&cipher_text, key16, padding, Some(iv)).expect("dec"),
        content
    );
}

#[test]
fn errors_match_go() {
    assert!(encrypt_cbc(b"x", b"1234", None).is_err());
    assert!(decrypt_cbc(b"test", b"1234567891234567", None).is_err());
    assert!(decrypt_cbc(b"12345678912345670", b"1234567891234567", None).is_err());

    let cipher_text = b64("ZuLgAOii+lrD5KJoQ7yQ8Q==");
    let wrong = b"12345678912345678912345678912346";
    let iv = b"Hello My GoFrame";
    let err = decrypt(&cipher_text, wrong, Some(iv)).expect_err("wrong key");
    let text = err.to_string();
    assert!(
        text == "invalid unpadding" || text == "invalid padding",
        "{text}"
    );
}

#[test]
fn pkcs7_padding_round_trip() {
    let padded = pkcs7_padding(b"abc", 16);
    assert_eq!(padded.len(), 16);
    assert_eq!(pkcs7_unpadding(&padded, 16).expect("unpad"), b"abc");
    assert!(pkcs7_unpadding(b"", 16).is_err());
    assert!(pkcs7_unpadding(b"abc", 0).is_err());
}
