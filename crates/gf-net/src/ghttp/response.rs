//! Port of `net/ghttp/internal/response`.
//!
//! Go's `response` package wraps `http.ResponseWriter` to track headers,
//! written bytes and hijacking, and adds a buffered writer that can flush a
//! whole response at once (with a default status body).
//!
//! Rust has no `http.ResponseWriter`, so the port defines a small
//! [`ResponseWriter`] trait (ADR-063):
//!
//! - [`Writer`] wraps one writer, remembers whether the header was written and
//!   how many bytes were written, and only forwards the first `write_header`;
//! - [`BufferWriter`] buffers body writes, remembers the status, and on
//!   [`BufferWriter::flush`] writes the status header (once), the default
//!   status text when there is no body, then the buffered body;
//! - hijacking is a flag (`mark_hijacked` / `is_hijacked`) because the
//!   connection object is server-specific in Go.

use gf_errors::gerror::Error;

/// The response writer contract the port needs, replacing Go's
/// `http.ResponseWriter`.
///
/// The future `ghttp` server implements this over its TCP connection.
pub trait ResponseWriter: Send {
    /// Writes the status header.
    fn write_header(&mut self, status: i32);
    /// Writes body bytes, returning the number written.
    fn write(&mut self, data: &[u8]) -> Result<usize, Error>;
    /// Flushes any buffered data to the client.
    fn flush(&mut self) -> Result<(), Error>;
}

/// Returns the status text for `status`, like Go's `http.StatusText`.
///
/// The port covers the common HTTP status codes; unknown codes return
/// `Unknown Status`, mirroring Go's empty string.
pub fn status_text(status: i32) -> String {
    let text = match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Request Entity Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown Status",
    };
    text.to_owned()
}

/// Wraps a [`ResponseWriter`] with header/byte tracking, like Go's `Writer`.
pub struct Writer {
    inner: Box<dyn ResponseWriter>,
    hijacked: bool,
    wrote_header: bool,
    bytes_written: i64,
}

impl Writer {
    /// Creates a writer over `inner`, like `NewWriter`.
    pub fn new(inner: impl ResponseWriter + 'static) -> Writer {
        Writer {
            inner: Box::new(inner),
            hijacked: false,
            wrote_header: false,
            bytes_written: 0,
        }
    }

    /// Writes the header once, like Go's `WriteHeader`.
    pub fn write_header(&mut self, status: i32) {
        if self.wrote_header {
            return;
        }
        self.inner.write_header(status);
        self.wrote_header = true;
    }

    /// The number of body bytes written, like `BytesWritten`.
    pub fn bytes_written(&self) -> i64 {
        self.bytes_written
    }

    /// Writes body bytes and records them, like Go's `Write`.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        let written = self.inner.write(data)?;
        self.bytes_written += written as i64;
        self.wrote_header = true;
        Ok(written)
    }

    /// Reports whether the header was written, like `IsHeaderWrote`.
    pub fn is_header_wrote(&self) -> bool {
        self.wrote_header
    }

    /// Reports whether the connection was hijacked, like `IsHijacked`.
    pub fn is_hijacked(&self) -> bool {
        self.hijacked
    }

    /// Marks the connection as hijacked (replaces Go's `Hijack`).
    pub fn mark_hijacked(&mut self) {
        self.hijacked = true;
    }

    /// Flushes the underlying writer, like Go's `Flush`.
    pub fn flush(&mut self) -> Result<(), Error> {
        self.inner.flush()?;
        self.wrote_header = true;
        Ok(())
    }
}

/// The buffered response writer, like Go's `BufferWriter`.
pub struct BufferWriter {
    writer: Writer,
    status: i32,
    buffer: Vec<u8>,
}

impl BufferWriter {
    /// Creates a buffered writer over `writer`, like `NewBufferWriter`.
    pub fn new(writer: impl ResponseWriter + 'static) -> BufferWriter {
        BufferWriter {
            writer: Writer::new(writer),
            status: 0,
            buffer: Vec::new(),
        }
    }

    /// The underlying writer, like `RawWriter`.
    pub fn raw_writer(&self) -> &Writer {
        &self.writer
    }

    /// The underlying writer, mutably.
    pub fn raw_writer_mut(&mut self) -> &mut Writer {
        &mut self.writer
    }

    /// The status set by [`BufferWriter::write_header`].
    pub fn status(&self) -> i32 {
        self.status
    }

    /// Buffers body bytes, like Go's `Write`.
    pub fn write(&mut self, data: &[u8]) -> usize {
        self.buffer.extend_from_slice(data);
        data.len()
    }

    /// Buffers a string, like `WriteString`.
    pub fn write_string(&mut self, data: &str) -> usize {
        self.write(data.as_bytes())
    }

    /// The buffered bytes, like `Buffer`.
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// The buffered bytes as a string, like `BufferString`.
    pub fn buffer_string(&self) -> String {
        String::from_utf8_lossy(&self.buffer).into_owned()
    }

    /// The buffered byte count, like `BufferLength`.
    pub fn buffer_length(&self) -> usize {
        self.buffer.len()
    }

    /// Replaces the buffer, like `SetBuffer`.
    pub fn set_buffer(&mut self, data: &[u8]) {
        self.buffer.clear();
        self.buffer.extend_from_slice(data);
    }

    /// Clears the buffer, like `ClearBuffer`.
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    /// Records the status, like Go's `WriteHeader`.
    pub fn write_header(&mut self, status: i32) {
        self.status = status;
    }

    /// Writes the status header and buffered body to the client, like `Flush`.
    pub fn flush(&mut self) -> Result<(), Error> {
        if self.writer.is_hijacked() {
            return Ok(());
        }
        if self.status != 0 && !self.writer.is_header_wrote() {
            self.writer.write_header(self.status);
        }
        // Default status text output when there is nothing else to send.
        if self.status != 200 && self.buffer.is_empty() && self.writer.bytes_written() == 0 {
            let text = status_text(self.status);
            self.buffer.extend_from_slice(text.as_bytes());
        }
        if !self.buffer.is_empty() {
            let data = std::mem::take(&mut self.buffer);
            self.writer.write(&data)?;
            self.writer.flush()?;
        }
        Ok(())
    }
}

/// Ensures the module is never compiled with an ignored error.
#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A recording writer for tests.
    #[derive(Clone, Default)]
    struct Recorder {
        state: Arc<Mutex<RecorderState>>,
    }

    #[derive(Default)]
    struct RecorderState {
        headers: Vec<i32>,
        body: Vec<u8>,
        flushes: usize,
    }

    impl ResponseWriter for Recorder {
        fn write_header(&mut self, status: i32) {
            self.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .headers
                .push(status);
        }

        fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.body.extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> Result<(), Error> {
            self.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .flushes += 1;
            Ok(())
        }
    }

    impl Recorder {
        fn snapshot(&self) -> (Vec<i32>, Vec<u8>, usize) {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (state.headers.clone(), state.body.clone(), state.flushes)
        }
    }

    #[test]
    fn writer_tracks_header_and_bytes() {
        let recorder = Recorder::default();
        let mut writer = Writer::new(recorder.clone());
        assert!(!writer.is_header_wrote());
        writer.write_header(200);
        writer.write_header(404);
        assert!(writer.is_header_wrote());
        assert_eq!(writer.write(b"hello").expect("write"), 5);
        assert_eq!(writer.bytes_written(), 5);
        writer.flush().expect("flush");
        let (headers, body, flushes) = recorder.snapshot();
        assert_eq!(headers, vec![200], "only the first header is forwarded");
        assert_eq!(body, b"hello");
        assert_eq!(flushes, 1);
    }

    #[test]
    fn buffer_writer_buffers_and_flushes() {
        let recorder = Recorder::default();
        let mut writer = BufferWriter::new(recorder.clone());
        assert_eq!(writer.write(b"hello"), 5);
        assert_eq!(writer.write_string(" world"), 6);
        assert_eq!(writer.buffer_length(), 11);
        assert_eq!(writer.buffer_string(), "hello world");
        writer.write_header(200);
        writer.flush().expect("flush");
        let (headers, body, flushes) = recorder.snapshot();
        assert_eq!(headers, vec![200]);
        assert_eq!(body, b"hello world");
        assert_eq!(flushes, 1);
        assert_eq!(writer.buffer_length(), 0);
        assert_eq!(writer.status(), 200);
    }

    #[test]
    fn buffer_writer_default_status_body() {
        let recorder = Recorder::default();
        let mut writer = BufferWriter::new(recorder.clone());
        writer.write_header(404);
        writer.flush().expect("flush");
        let (headers, body, _) = recorder.snapshot();
        assert_eq!(headers, vec![404]);
        assert_eq!(body, b"Not Found");
    }

    #[test]
    fn buffer_writer_set_and_clear() {
        let recorder = Recorder::default();
        let mut writer = BufferWriter::new(recorder.clone());
        writer.set_buffer(b"first");
        assert_eq!(writer.buffer_string(), "first");
        writer.set_buffer(b"second");
        assert_eq!(writer.buffer_string(), "second");
        writer.clear_buffer();
        assert_eq!(writer.buffer_length(), 0);
    }

    #[test]
    fn hijacked_writer_does_not_flush() {
        let recorder = Recorder::default();
        let mut writer = BufferWriter::new(recorder.clone());
        writer.write(b"hello");
        writer.raw_writer_mut().mark_hijacked();
        writer.flush().expect("flush");
        let (headers, body, flushes) = recorder.snapshot();
        assert!(headers.is_empty());
        assert!(body.is_empty());
        assert_eq!(flushes, 0);
    }

    #[test]
    fn status_texts() {
        assert_eq!(status_text(200), "OK");
        assert_eq!(status_text(404), "Not Found");
        assert_eq!(status_text(500), "Internal Server Error");
        assert_eq!(status_text(999), "Unknown Status");
    }
}
