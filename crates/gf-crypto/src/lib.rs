//! The port of GoFrame's `crypto/` hash packages.

use std::io::Read;

use gf_errors::gerror;

pub mod gaes;
#[cfg(test)]
mod gaes_tests;

pub mod gcrc32;
pub mod gdes;
pub mod gmd5;
pub mod grsa;
pub mod gsha1;
pub mod gsha256;
pub mod gsha512;

pub(crate) fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0F)]));
    }
    out
}

pub(crate) fn wrap(
    err: impl std::error::Error + Send + Sync + 'static,
    text: &str,
) -> gerror::Error {
    match gerror::wrap_std(Some(Box::new(err)), text) {
        Some(err) => err,
        None => gerror::Error::new(text),
    }
}

pub(crate) fn hash_file(
    path: &str,
    algorithm: &'static ring::digest::Algorithm,
) -> Result<String, gerror::Error> {
    let mut file = std::fs::File::open(path)
        .map_err(|err| wrap(err, &format!("os.Open failed for name \"{path}\"")))?;
    let mut context = ring::digest::Context::new(algorithm);
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| wrap(err, "io.Copy failed"))?;
        if read == 0 {
            break;
        }
        context.update(&buffer[..read]);
    }
    Ok(to_hex(context.finish().as_ref()))
}
