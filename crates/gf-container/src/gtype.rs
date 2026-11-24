//! Port of Go's `container/gtype`.
//!
//! Ports `gtype.go`, `gtype_any.go`, `gtype_interface.go`, `gtype_bool.go`,
//! `gtype_byte.go`, `gtype_bytes.go`, `gtype_float32.go`, `gtype_float64.go`,
//! `gtype_int.go`, `gtype_int32.go`, `gtype_int64.go`, `gtype_string.go`,
//! `gtype_uint.go`, `gtype_uint32.go` and `gtype_uint64.go`.
//!
//! Numeric values use atomics like Go's `sync/atomic` calls: `set` returns the
//! previous value, `add` returns the new value and `cas` reports whether the
//! swap happened. Numbers print their decimal text, like Go; floats print the
//! shortest round-trip digits in Go's `'g'` shape (`1e+21`, not the 22 digit
//! expansion) through [`gf_core::float`].

use std::string::String as StdString;
use std::sync::RwLock;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use gf_core::Value;
use gf_core::float::{format_g, format_g32};
use gf_errors::gerror::Error;
use gf_util::gconv;

/// Decodes `data` as text, like Go's `string(b)`.
fn text_of(data: &[u8]) -> StdString {
    StdString::from_utf8_lossy(data).into_owned()
}

/// Trims quotes from both ends, like Go's `bytes.Trim(b, "\"")`.
fn trim_quotes(data: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = data.len();
    while start < end && data[start] == b'"' {
        start += 1;
    }
    while end > start && data[end - 1] == b'"' {
        end -= 1;
    }
    &data[start..end]
}

/// Encodes a `bool` as the `0`/`1` word Go stores.
fn flag(value: bool) -> u32 {
    if value { 1 } else { 0 }
}

/// Parses unsigned decimal text, falling back to `gconv`.
fn parse_uint64(text: &str) -> u64 {
    match text.trim().parse::<u64>() {
        Ok(value) => value,
        Err(_) => gconv::uint64_value(&Value::String(text.to_owned())),
    }
}

/// `Any` is an alias of [`Interface`], like Go's `type Any = Interface`.
pub type Any = Interface;

/// A synchronized dynamic value, mirroring Go's `gtype.Interface`.
pub struct Interface {
    value: RwLock<Option<Value>>,
}

impl Interface {
    /// Creates a dynamic value; `None` and nil values leave it empty.
    pub fn new(value: Option<Value>) -> Interface {
        let stored = value.filter(|value| !value.is_nil());
        Interface {
            value: RwLock::new(stored),
        }
    }

    /// Returns the current value, or [`Value::Null`] when empty.
    pub fn val(&self) -> Value {
        self.value
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .unwrap_or(Value::Null)
    }

    /// Stores `value` and returns the previous value.
    ///
    /// # Panics
    ///
    /// Panics on [`Value::Null`], like Go's `atomic.Value` storing nil.
    pub fn set(&self, value: Value) -> Value {
        if value.is_nil() {
            // INFALLIBLE: Go's `atomic.Value` panics when storing nil; mirror it.
            panic!("gtype: set(nil) on Interface");
        }
        let mut guard = self
            .value
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.replace(value).unwrap_or(Value::Null)
    }

    /// Returns the value as a string.
    pub fn string(&self) -> StdString {
        gconv::string(&self.val())
    }

    /// Encodes the value as JSON, like Go's `json.Marshal(v.Val())`.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        gf_json::marshal_value(&self.val()).map(StdString::into_bytes)
    }

    /// Replaces the value from JSON text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        let value = gf_json::unmarshal_value(data)?;
        self.set(value);
        Ok(())
    }

    /// Replaces the value with `value`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(value.clone());
    }

    /// Returns an independent copy of the value.
    pub fn deep_copy(&self) -> Interface {
        Interface::new(
            self.value
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        )
    }
}

impl Clone for Interface {
    fn clone(&self) -> Interface {
        Interface::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Interface {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates the dynamic value [`Any`], like Go's `gtype.New`.
pub fn new(value: Option<Value>) -> Any {
    Interface::new(value)
}

/// Creates an [`Interface`], like Go's `NewInterface`.
pub fn new_interface(value: Option<Value>) -> Interface {
    Interface::new(value)
}

/// Creates an [`Any`], like Go's `NewAny`.
pub fn new_any(value: Option<Value>) -> Any {
    Interface::new(value)
}

/// A synchronized `bool`, mirroring Go's `gtype.Bool`.
pub struct Bool {
    value: AtomicU32,
}

impl Bool {
    /// Creates a bool; `None` keeps it `false`.
    pub fn new(value: Option<bool>) -> Bool {
        Bool {
            value: AtomicU32::new(flag(value.unwrap_or(false))),
        }
    }

    /// Returns the current value.
    pub fn val(&self) -> bool {
        self.value.load(Ordering::SeqCst) > 0
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: bool) -> bool {
        self.value.swap(flag(value), Ordering::SeqCst) > 0
    }

    /// Replaces the value when it equals `old`.
    pub fn cas(&self, old: bool, new: bool) -> bool {
        self.value
            .compare_exchange(flag(old), flag(new), Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Returns `true` or `false`.
    pub fn string(&self) -> StdString {
        self.val().to_string()
    }

    /// Encodes the value as the JSON literal `true` or `false`.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(self.string().into_bytes())
    }

    /// Replaces the value from JSON text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(gconv::bool_value(&Value::String(text_of(trim_quotes(
            data,
        )))));
        Ok(())
    }

    /// Replaces the value with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::bool_value(value));
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> Bool {
        Bool::new(Some(self.val()))
    }
}

impl Clone for Bool {
    fn clone(&self) -> Bool {
        Bool::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Bool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates a [`Bool`], like Go's `NewBool`.
pub fn new_bool(value: Option<bool>) -> Bool {
    Bool::new(value)
}

/// A synchronized `byte` (`u8`), mirroring Go's `gtype.Byte`.
pub struct Byte {
    value: AtomicU32,
}

impl Byte {
    /// Creates a byte; `None` keeps it `0`.
    pub fn new(value: Option<u8>) -> Byte {
        Byte {
            value: AtomicU32::new(u32::from(value.unwrap_or(0))),
        }
    }

    /// Returns the current value.
    pub fn val(&self) -> u8 {
        self.value.load(Ordering::SeqCst) as u8
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: u8) -> u8 {
        self.value.swap(u32::from(value), Ordering::SeqCst) as u8
    }

    /// Adds `delta` and returns the new value.
    pub fn add(&self, delta: u8) -> u8 {
        let raw = self.value.fetch_add(u32::from(delta), Ordering::SeqCst);
        raw.wrapping_add(u32::from(delta)) as u8
    }

    /// Replaces the value when it equals `old`.
    pub fn cas(&self, old: u8, new: u8) -> bool {
        self.value
            .compare_exchange(
                u32::from(old),
                u32::from(new),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    /// Returns the decimal text of the value.
    pub fn string(&self) -> StdString {
        self.val().to_string()
    }

    /// Encodes the value as a JSON number.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(self.string().into_bytes())
    }

    /// Replaces the value from JSON number text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(parse_uint64(&text_of(data)) as u8);
        Ok(())
    }

    /// Replaces the value with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::uint64_value(value) as u8);
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> Byte {
        Byte::new(Some(self.val()))
    }
}

impl Clone for Byte {
    fn clone(&self) -> Byte {
        Byte::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Byte {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates a [`Byte`], like Go's `NewByte`.
pub fn new_byte(value: Option<u8>) -> Byte {
    Byte::new(value)
}

/// A synchronized `[]byte`, mirroring Go's `gtype.Bytes`.
pub struct Bytes {
    value: RwLock<Vec<u8>>,
}

impl Bytes {
    /// Creates a byte slice; `None` keeps it empty.
    pub fn new(value: Option<Vec<u8>>) -> Bytes {
        Bytes {
            value: RwLock::new(value.unwrap_or_default()),
        }
    }

    /// Returns a copy of the current bytes; empty when unset.
    pub fn val(&self) -> Vec<u8> {
        self.value
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: Vec<u8>) -> Vec<u8> {
        let mut guard = self
            .value
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(&mut *guard, value)
    }

    /// Returns the bytes as a string.
    pub fn string(&self) -> StdString {
        // INFALLIBLE: lossy decoding never fails, like Go's `string([]byte)`.
        StdString::from_utf8_lossy(&self.val()).into_owned()
    }

    /// Encodes the bytes as a BASE64 JSON string, like Go.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(format!("\"{}\"", STANDARD.encode(self.val())).into_bytes())
    }

    /// Replaces the bytes from a BASE64 JSON string, like Go.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        let decoded = STANDARD
            .decode(trim_quotes(data))
            .map_err(|err| Error::new(format!("base64.StdEncoding.Decode failed: {err}")))?;
        self.set(decoded);
        Ok(())
    }

    /// Replaces the bytes with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::bytes(value));
    }

    /// Returns an independent copy of the bytes.
    pub fn deep_copy(&self) -> Bytes {
        Bytes::new(Some(self.val()))
    }
}

impl Clone for Bytes {
    fn clone(&self) -> Bytes {
        Bytes::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Bytes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates a [`Bytes`], like Go's `NewBytes`.
pub fn new_bytes(value: Option<Vec<u8>>) -> Bytes {
    Bytes::new(value)
}

/// A synchronized `float32`, mirroring Go's `gtype.Float32`.
pub struct Float32 {
    value: AtomicU32,
}

impl Float32 {
    /// Creates a float32; `None` keeps it `0`.
    pub fn new(value: Option<f32>) -> Float32 {
        Float32 {
            value: AtomicU32::new(value.unwrap_or(0.0).to_bits()),
        }
    }

    /// Returns the current value.
    pub fn val(&self) -> f32 {
        f32::from_bits(self.value.load(Ordering::SeqCst))
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: f32) -> f32 {
        f32::from_bits(self.value.swap(value.to_bits(), Ordering::SeqCst))
    }

    /// Adds `delta` and returns the new value.
    pub fn add(&self, delta: f32) -> f32 {
        let mut old = self.value.load(Ordering::SeqCst);
        loop {
            let new = f32::from_bits(old) + delta;
            match self.value.compare_exchange(
                old,
                new.to_bits(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return new,
                Err(current) => old = current,
            }
        }
    }

    /// Replaces the value when it equals `old`.
    pub fn cas(&self, old: f32, new: f32) -> bool {
        self.value
            .compare_exchange(
                old.to_bits(),
                new.to_bits(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    /// Returns the shortest text of the value, in Go's `'g'` format.
    pub fn string(&self) -> StdString {
        format_g32(self.val())
    }

    /// Encodes the value as a JSON number.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(self.string().into_bytes())
    }

    /// Replaces the value from JSON number text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(gconv::float64_value(&Value::String(text_of(data))) as f32);
        Ok(())
    }

    /// Replaces the value with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::float64_value(value) as f32);
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> Float32 {
        Float32::new(Some(self.val()))
    }
}

impl Clone for Float32 {
    fn clone(&self) -> Float32 {
        Float32::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Float32 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates a [`Float32`], like Go's `NewFloat32`.
pub fn new_float32(value: Option<f32>) -> Float32 {
    Float32::new(value)
}

/// A synchronized `float64`, mirroring Go's `gtype.Float64`.
pub struct Float64 {
    value: AtomicU64,
}

impl Float64 {
    /// Creates a float64; `None` keeps it `0`.
    pub fn new(value: Option<f64>) -> Float64 {
        Float64 {
            value: AtomicU64::new(value.unwrap_or(0.0).to_bits()),
        }
    }

    /// Returns the current value.
    pub fn val(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::SeqCst))
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: f64) -> f64 {
        f64::from_bits(self.value.swap(value.to_bits(), Ordering::SeqCst))
    }

    /// Adds `delta` and returns the new value.
    pub fn add(&self, delta: f64) -> f64 {
        let mut old = self.value.load(Ordering::SeqCst);
        loop {
            let new = f64::from_bits(old) + delta;
            match self.value.compare_exchange(
                old,
                new.to_bits(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return new,
                Err(current) => old = current,
            }
        }
    }

    /// Replaces the value when it equals `old`.
    pub fn cas(&self, old: f64, new: f64) -> bool {
        self.value
            .compare_exchange(
                old.to_bits(),
                new.to_bits(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    /// Returns the shortest text of the value, in Go's `'g'` format.
    pub fn string(&self) -> StdString {
        format_g(self.val())
    }

    /// Encodes the value as a JSON number.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(self.string().into_bytes())
    }

    /// Replaces the value from JSON number text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(gconv::float64_value(&Value::String(text_of(data))));
        Ok(())
    }

    /// Replaces the value with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::float64_value(value));
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> Float64 {
        Float64::new(Some(self.val()))
    }
}

impl Clone for Float64 {
    fn clone(&self) -> Float64 {
        Float64::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Float64 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates a [`Float64`], like Go's `NewFloat64`.
pub fn new_float64(value: Option<f64>) -> Float64 {
    Float64::new(value)
}

/// A synchronized `int` (`i64`), mirroring Go's `gtype.Int`.
pub struct Int {
    value: AtomicI64,
}

impl Int {
    /// Creates an int; `None` keeps it `0`.
    pub fn new(value: Option<i64>) -> Int {
        Int {
            value: AtomicI64::new(value.unwrap_or(0)),
        }
    }

    /// Returns the current value.
    pub fn val(&self) -> i64 {
        self.value.load(Ordering::SeqCst)
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: i64) -> i64 {
        self.value.swap(value, Ordering::SeqCst)
    }

    /// Adds `delta` and returns the new value.
    pub fn add(&self, delta: i64) -> i64 {
        self.value
            .fetch_add(delta, Ordering::SeqCst)
            .wrapping_add(delta)
    }

    /// Replaces the value when it equals `old`.
    pub fn cas(&self, old: i64, new: i64) -> bool {
        self.value
            .compare_exchange(old, new, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Returns the decimal text of the value.
    pub fn string(&self) -> StdString {
        self.val().to_string()
    }

    /// Encodes the value as a JSON number.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(self.string().into_bytes())
    }

    /// Replaces the value from JSON number text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(gconv::int64_value(&Value::String(text_of(data))));
        Ok(())
    }

    /// Replaces the value with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::int64_value(value));
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> Int {
        Int::new(Some(self.val()))
    }
}

impl Clone for Int {
    fn clone(&self) -> Int {
        Int::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Int {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates an [`Int`], like Go's `NewInt`.
pub fn new_int(value: Option<i64>) -> Int {
    Int::new(value)
}

/// A synchronized `int32`, mirroring Go's `gtype.Int32`.
pub struct Int32 {
    value: AtomicI32,
}

impl Int32 {
    /// Creates an int32; `None` keeps it `0`.
    pub fn new(value: Option<i32>) -> Int32 {
        Int32 {
            value: AtomicI32::new(value.unwrap_or(0)),
        }
    }

    /// Returns the current value.
    pub fn val(&self) -> i32 {
        self.value.load(Ordering::SeqCst)
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: i32) -> i32 {
        self.value.swap(value, Ordering::SeqCst)
    }

    /// Adds `delta` and returns the new value.
    pub fn add(&self, delta: i32) -> i32 {
        self.value
            .fetch_add(delta, Ordering::SeqCst)
            .wrapping_add(delta)
    }

    /// Replaces the value when it equals `old`.
    pub fn cas(&self, old: i32, new: i32) -> bool {
        self.value
            .compare_exchange(old, new, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Returns the decimal text of the value.
    pub fn string(&self) -> StdString {
        self.val().to_string()
    }

    /// Encodes the value as a JSON number.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(self.string().into_bytes())
    }

    /// Replaces the value from JSON number text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(gconv::int64_value(&Value::String(text_of(data))) as i32);
        Ok(())
    }

    /// Replaces the value with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::int64_value(value) as i32);
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> Int32 {
        Int32::new(Some(self.val()))
    }
}

impl Clone for Int32 {
    fn clone(&self) -> Int32 {
        Int32::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Int32 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates an [`Int32`], like Go's `NewInt32`.
pub fn new_int32(value: Option<i32>) -> Int32 {
    Int32::new(value)
}

/// A synchronized `int64`, mirroring Go's `gtype.Int64`.
pub struct Int64 {
    value: AtomicI64,
}

impl Int64 {
    /// Creates an int64; `None` keeps it `0`.
    pub fn new(value: Option<i64>) -> Int64 {
        Int64 {
            value: AtomicI64::new(value.unwrap_or(0)),
        }
    }

    /// Returns the current value.
    pub fn val(&self) -> i64 {
        self.value.load(Ordering::SeqCst)
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: i64) -> i64 {
        self.value.swap(value, Ordering::SeqCst)
    }

    /// Adds `delta` and returns the new value.
    pub fn add(&self, delta: i64) -> i64 {
        self.value
            .fetch_add(delta, Ordering::SeqCst)
            .wrapping_add(delta)
    }

    /// Replaces the value when it equals `old`.
    pub fn cas(&self, old: i64, new: i64) -> bool {
        self.value
            .compare_exchange(old, new, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Returns the decimal text of the value.
    pub fn string(&self) -> StdString {
        self.val().to_string()
    }

    /// Encodes the value as a JSON number.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(self.string().into_bytes())
    }

    /// Replaces the value from JSON number text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(gconv::int64_value(&Value::String(text_of(data))));
        Ok(())
    }

    /// Replaces the value with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::int64_value(value));
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> Int64 {
        Int64::new(Some(self.val()))
    }
}

impl Clone for Int64 {
    fn clone(&self) -> Int64 {
        Int64::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Int64 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates an [`Int64`], like Go's `NewInt64`.
pub fn new_int64(value: Option<i64>) -> Int64 {
    Int64::new(value)
}

/// A synchronized `string`, mirroring Go's `gtype.String`.
pub struct String {
    value: RwLock<StdString>,
}

impl String {
    /// Creates a string; `None` keeps it empty.
    pub fn new(value: Option<StdString>) -> String {
        String {
            value: RwLock::new(value.unwrap_or_default()),
        }
    }

    /// Returns the current string; empty when unset.
    pub fn val(&self) -> StdString {
        self.value
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: StdString) -> StdString {
        let mut guard = self
            .value
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(&mut *guard, value)
    }

    /// Returns the current string.
    pub fn string(&self) -> StdString {
        self.val()
    }

    /// Encodes the string as a JSON string.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        gf_json::marshal(&Value::String(self.val()))
    }

    /// Replaces the string from JSON text, trimming quotes like Go.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(text_of(trim_quotes(data)));
        Ok(())
    }

    /// Replaces the string with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::string(value));
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> String {
        String::new(Some(self.val()))
    }
}

impl Clone for String {
    fn clone(&self) -> String {
        String::new(Some(self.val()))
    }
}

impl std::fmt::Debug for String {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates a [`String`], like Go's `NewString`.
pub fn new_string(value: Option<StdString>) -> String {
    String::new(value)
}

/// A synchronized `uint` (`u64`), mirroring Go's `gtype.Uint`.
pub struct Uint {
    value: AtomicU64,
}

impl Uint {
    /// Creates a uint; `None` keeps it `0`.
    pub fn new(value: Option<u64>) -> Uint {
        Uint {
            value: AtomicU64::new(value.unwrap_or(0)),
        }
    }

    /// Returns the current value.
    pub fn val(&self) -> u64 {
        self.value.load(Ordering::SeqCst)
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: u64) -> u64 {
        self.value.swap(value, Ordering::SeqCst)
    }

    /// Adds `delta` and returns the new value.
    pub fn add(&self, delta: u64) -> u64 {
        self.value
            .fetch_add(delta, Ordering::SeqCst)
            .wrapping_add(delta)
    }

    /// Replaces the value when it equals `old`.
    pub fn cas(&self, old: u64, new: u64) -> bool {
        self.value
            .compare_exchange(old, new, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Returns the decimal text of the value.
    pub fn string(&self) -> StdString {
        self.val().to_string()
    }

    /// Encodes the value as a JSON number.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(self.string().into_bytes())
    }

    /// Replaces the value from JSON number text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(parse_uint64(&text_of(data)));
        Ok(())
    }

    /// Replaces the value with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::uint64_value(value));
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> Uint {
        Uint::new(Some(self.val()))
    }
}

impl Clone for Uint {
    fn clone(&self) -> Uint {
        Uint::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Uint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates a [`Uint`], like Go's `NewUint`.
pub fn new_uint(value: Option<u64>) -> Uint {
    Uint::new(value)
}

/// A synchronized `uint32`, mirroring Go's `gtype.Uint32`.
pub struct Uint32 {
    value: AtomicU32,
}

impl Uint32 {
    /// Creates a uint32; `None` keeps it `0`.
    pub fn new(value: Option<u32>) -> Uint32 {
        Uint32 {
            value: AtomicU32::new(value.unwrap_or(0)),
        }
    }

    /// Returns the current value.
    pub fn val(&self) -> u32 {
        self.value.load(Ordering::SeqCst)
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: u32) -> u32 {
        self.value.swap(value, Ordering::SeqCst)
    }

    /// Adds `delta` and returns the new value.
    pub fn add(&self, delta: u32) -> u32 {
        self.value
            .fetch_add(delta, Ordering::SeqCst)
            .wrapping_add(delta)
    }

    /// Replaces the value when it equals `old`.
    pub fn cas(&self, old: u32, new: u32) -> bool {
        self.value
            .compare_exchange(old, new, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Returns the decimal text of the value.
    pub fn string(&self) -> StdString {
        self.val().to_string()
    }

    /// Encodes the value as a JSON number.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(self.string().into_bytes())
    }

    /// Replaces the value from JSON number text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(parse_uint64(&text_of(data)) as u32);
        Ok(())
    }

    /// Replaces the value with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::uint64_value(value) as u32);
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> Uint32 {
        Uint32::new(Some(self.val()))
    }
}

impl Clone for Uint32 {
    fn clone(&self) -> Uint32 {
        Uint32::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Uint32 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates a [`Uint32`], like Go's `NewUint32`.
pub fn new_uint32(value: Option<u32>) -> Uint32 {
    Uint32::new(value)
}

/// A synchronized `uint64`, mirroring Go's `gtype.Uint64`.
pub struct Uint64 {
    value: AtomicU64,
}

impl Uint64 {
    /// Creates a uint64; `None` keeps it `0`.
    pub fn new(value: Option<u64>) -> Uint64 {
        Uint64 {
            value: AtomicU64::new(value.unwrap_or(0)),
        }
    }

    /// Returns the current value.
    pub fn val(&self) -> u64 {
        self.value.load(Ordering::SeqCst)
    }

    /// Stores `value` and returns the previous value.
    pub fn set(&self, value: u64) -> u64 {
        self.value.swap(value, Ordering::SeqCst)
    }

    /// Adds `delta` and returns the new value.
    pub fn add(&self, delta: u64) -> u64 {
        self.value
            .fetch_add(delta, Ordering::SeqCst)
            .wrapping_add(delta)
    }

    /// Replaces the value when it equals `old`.
    pub fn cas(&self, old: u64, new: u64) -> bool {
        self.value
            .compare_exchange(old, new, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Returns the decimal text of the value.
    pub fn string(&self) -> StdString {
        self.val().to_string()
    }

    /// Encodes the value as a JSON number.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        Ok(self.string().into_bytes())
    }

    /// Replaces the value from JSON number text.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        self.set(parse_uint64(&text_of(data)));
        Ok(())
    }

    /// Replaces the value with `value` converted by `gconv`.
    pub fn unmarshal_value(&mut self, value: &Value) {
        self.set(gconv::uint64_value(value));
    }

    /// Returns an independent copy.
    pub fn deep_copy(&self) -> Uint64 {
        Uint64::new(Some(self.val()))
    }
}

impl Clone for Uint64 {
    fn clone(&self) -> Uint64 {
        Uint64::new(Some(self.val()))
    }
}

impl std::fmt::Debug for Uint64 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.val(), formatter)
    }
}

/// Creates a [`Uint64`], like Go's `NewUint64`.
pub fn new_uint64(value: Option<u64>) -> Uint64 {
    Uint64::new(value)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use indexmap::IndexMap;

    use super::*;

    /// A non-scalar value for the `any` tests.
    fn temp(age: i64) -> Value {
        let mut map = IndexMap::new();
        map.insert("name".to_owned(), Value::String("gf".to_owned()));
        map.insert("age".to_owned(), Value::Int(age));
        Value::Map(map)
    }

    /// Marshals a value the way Go's `json.Marshal` does.
    fn json_of(value: &Value) -> Vec<u8> {
        gf_json::marshal_value(value)
            .expect("marshal value")
            .into_bytes()
    }

    #[test]
    fn test_any() {
        let t1 = temp(18);
        let t2 = temp(19);
        let i = new(Some(t1.clone()));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(t2.clone()), t1);
        assert_eq!(i_clone.val(), t2);

        let i1 = new(None);
        assert_eq!(i1.val(), Value::Null);

        let i2 = new(Some(Value::String("gf".to_owned())));
        assert_eq!(i2.string(), "gf");
        let copy_val = i2.deep_copy();
        i2.set(Value::String("goframe".to_owned()));
        assert_ne!(copy_val.val(), i2.val());
        assert_eq!(copy_val.val(), Value::String("gf".to_owned()));
    }

    #[test]
    fn test_any_json() {
        let s = "i love gf";
        let i = new(Some(Value::String(s.to_owned())));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&i.val());
        assert_eq!(b1, b2);

        let mut i2 = new(None);
        i2.unmarshal_json(&b2).expect("unmarshal");
        assert_eq!(i2.val(), Value::String(s.to_owned()));
    }

    #[test]
    fn test_any_unmarshal_value() {
        let mut i = new(None);
        i.unmarshal_value(&Value::String("123".to_owned()));
        assert_eq!(i.val(), Value::String("123".to_owned()));
    }

    #[test]
    fn test_interface() {
        let t1 = temp(18);
        let t2 = temp(19);
        let i = new_interface(Some(t1.clone()));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(t2.clone()), t1);
        assert_eq!(i_clone.val(), t2);

        let i1 = new_interface(None);
        assert_eq!(i1.val(), Value::Null);

        let i2 = new_interface(Some(Value::String("gf".to_owned())));
        assert_eq!(i2.string(), "gf");
        let copy_val = i2.deep_copy();
        i2.set(Value::String("goframe".to_owned()));
        assert_ne!(copy_val.val(), i2.val());
        assert_eq!(copy_val.val(), Value::String("gf".to_owned()));
    }

    #[test]
    fn test_interface_json() {
        let s = "i love gf";
        let i = new_interface(Some(Value::String(s.to_owned())));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&i.val());
        assert_eq!(b1, b2);

        let mut i2 = new_interface(None);
        i2.unmarshal_json(&b2).expect("unmarshal");
        assert_eq!(i2.val(), Value::String(s.to_owned()));
    }

    #[test]
    fn test_interface_unmarshal_value() {
        let mut i = new_interface(None);
        i.unmarshal_value(&Value::String("123".to_owned()));
        assert_eq!(i.val(), Value::String("123".to_owned()));
    }

    #[test]
    #[should_panic(expected = "set(nil) on Interface")]
    fn test_interface_set_null_panics() {
        let i = new_interface(None);
        i.set(Value::Null);
    }

    #[test]
    fn test_bool() {
        let i = new_bool(Some(true));
        let i_clone = i.clone();
        assert!(i_clone.set(false));
        assert!(!i_clone.val());

        let i1 = new_bool(Some(false));
        let i_clone1 = i1.clone();
        assert!(!i_clone1.set(true));
        assert!(i_clone1.val());

        assert!(!i_clone1.cas(false, true));
        assert_eq!(i_clone1.string(), "true");
        assert!(i_clone1.cas(true, false));
        assert_eq!(i_clone1.string(), "false");

        let copy_val = i1.deep_copy();
        i_clone.set(true);
        assert_ne!(copy_val.val(), i_clone.val());

        let i2 = new_bool(None);
        assert!(!i2.val());
    }

    #[test]
    fn test_bool_json() {
        let i = new_bool(Some(true));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&Value::Bool(true));
        assert_eq!(b1, b2);

        let i = new_bool(Some(false));
        assert_eq!(
            i.marshal_json().expect("marshal"),
            json_of(&Value::Bool(false))
        );

        let mut target = new_bool(None);
        target.unmarshal_json(b"true").expect("unmarshal");
        assert!(target.val());
        target.unmarshal_json(b"false").expect("unmarshal");
        assert!(!target.val());
        target.unmarshal_json(b"1").expect("unmarshal");
        assert!(target.val());
        target.unmarshal_json(b"0").expect("unmarshal");
        assert!(!target.val());

        for value in [true, false] {
            let i = new_bool(Some(value));
            let b1 = i.marshal_json().expect("marshal");
            let mut i2 = new_bool(None);
            i2.unmarshal_json(&b1).expect("unmarshal");
            assert_eq!(i2.val(), i.val());
        }
    }

    #[test]
    fn test_bool_unmarshal_value() {
        let mut i = new_bool(None);
        i.unmarshal_value(&Value::String("true".to_owned()));
        assert!(i.val());

        i.unmarshal_value(&Value::String("false".to_owned()));
        assert!(!i.val());
    }

    #[test]
    fn test_byte() {
        let i = new_byte(Some(0));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(1), 0);
        assert_eq!(i_clone.val(), 1);
        let counter = Arc::new(i);

        let mut handles = Vec::new();
        for adds in [32u8, 32, 32, 31] {
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..adds {
                    counter.add(1);
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(counter.val(), 127);

        let i1 = new_byte(None);
        assert_eq!(i1.val(), 0);

        let i2 = new_byte(Some(64));
        assert_eq!(i2.string(), "64");
        assert!(!i2.cas(63, 65));
        assert!(i2.cas(64, 65));

        let copy_val = i2.deep_copy();
        i2.set(66);
        assert_eq!(copy_val.val(), 65);
        assert_eq!(i2.val(), 66);
    }

    #[test]
    fn test_byte_json() {
        let i = new_byte(Some(49));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&Value::UInt(49));
        assert_eq!(b1, b2);

        let mut i2 = new_byte(None);
        i2.unmarshal_json(b"49").expect("unmarshal");
        assert_eq!(i2.val(), 49);
    }

    #[test]
    fn test_byte_unmarshal_value() {
        let mut i = new_byte(None);
        i.unmarshal_value(&Value::String("2".to_owned()));
        assert_eq!(i.val(), 2);
    }

    #[test]
    fn test_bytes() {
        let i = new_bytes(Some(b"abc".to_vec()));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(b"123".to_vec()), b"abc".to_vec());
        assert_eq!(i_clone.val(), b"123".to_vec());

        let i1 = new_bytes(None);
        assert!(i1.val().is_empty());

        let i2 = new_bytes(Some(b"abc".to_vec()));
        assert_eq!(i2.string(), "abc");

        let copy_val = i2.deep_copy();
        i2.set(b"def".to_vec());
        assert_ne!(copy_val.val(), i2.val());
        assert_eq!(copy_val.val(), b"abc".to_vec());
    }

    #[test]
    fn test_bytes_json() {
        let b = b"i love gf".to_vec();
        let i = new_bytes(Some(b.clone()));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&Value::Bytes(b.clone()));
        assert_eq!(b1, b2);

        let mut i2 = new_bytes(None);
        i2.unmarshal_json(&b2).expect("unmarshal");
        assert_eq!(i2.val(), b);
    }

    #[test]
    fn test_bytes_unmarshal_value() {
        let mut i = new_bytes(None);
        i.unmarshal_value(&Value::String("123".to_owned()));
        assert_eq!(i.val(), b"123".to_vec());
    }

    #[test]
    fn test_float32() {
        let i = new_float32(Some(0.0));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(0.1), 0.0);
        assert_eq!(i_clone.val(), 0.1);

        let i1 = new_float32(None);
        assert_eq!(i1.val(), 0.0);

        let i2 = new_float32(Some(1.23));
        assert_eq!(i2.add(3.21), 4.44);
        assert!(!i2.cas(4.45, 5.55));
        assert!(i2.cas(4.44, 5.55));
        assert_eq!(i2.string(), "5.55");

        let copy_val = i2.deep_copy();
        i2.set(6.66);
        assert_ne!(copy_val.val(), i2.val());
    }

    #[test]
    fn test_float32_json() {
        let v = f32::MAX;
        let i = new_float32(Some(v));
        let b1 = i.marshal_json().expect("marshal");
        assert_eq!(b1, i.string().into_bytes());

        let mut i2 = new_float32(None);
        i2.unmarshal_json(&b1).expect("unmarshal");
        assert_eq!(i2.val(), v);
    }

    #[test]
    fn test_float32_unmarshal_value() {
        let mut i = new_float32(None);
        i.unmarshal_value(&Value::String("123.456".to_owned()));
        assert_eq!(i.val(), 123.456);
    }

    #[test]
    fn test_float64() {
        let i = new_float64(Some(0.0));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(0.1), 0.0);
        assert_eq!(i_clone.val(), 0.1);

        let i1 = new_float64(None);
        assert_eq!(i1.val(), 0.0);

        let i2 = new_float64(Some(1.1));
        assert_eq!(i2.add(3.3), 4.4);
        assert!(!i2.cas(4.5, 5.5));
        assert!(i2.cas(4.4, 5.5));
        assert_eq!(i2.string(), "5.5");

        let copy_val = i2.deep_copy();
        i2.set(6.6);
        assert_ne!(copy_val.val(), i2.val());
    }

    #[test]
    fn test_float64_json() {
        let v = f64::MAX;
        let i = new_float64(Some(v));
        let b1 = i.marshal_json().expect("marshal");
        assert_eq!(b1, i.string().into_bytes());

        let mut i2 = new_float64(None);
        i2.unmarshal_json(&b1).expect("unmarshal");
        assert_eq!(i2.val(), v);
    }

    #[test]
    fn test_float64_unmarshal_value() {
        let mut i = new_float64(None);
        i.unmarshal_value(&Value::String("123.456".to_owned()));
        assert_eq!(i.val(), 123.456);
    }

    // The text form follows Go's `strconv.FormatFloat(v, 'g', -1, bits)`
    // (ADR-037): extreme exponents are exponential, not the flat expansion.
    // The expected strings were produced by Go 1.25.
    #[test]
    fn float_text_matches_go_g_format() {
        let f64_cases: [(f64, &str); 6] = [
            (1e21, "1e+21"),
            (1e-7, "1e-07"),
            (1_000_000.0, "1e+06"),
            (0.0001, "0.0001"),
            (123.456, "123.456"),
            (f64::MAX, "1.7976931348623157e+308"),
        ];
        for (value, expected) in f64_cases {
            let float = new_float64(Some(value));
            assert_eq!(float.string(), expected, "Float64({value}) text");
            assert_eq!(
                float.marshal_json().expect("marshal"),
                expected.as_bytes(),
                "Float64({value}) JSON"
            );
        }

        let f32_cases: [(f32, &str); 3] =
            [(1e30, "1e+30"), (f32::MAX, "3.4028235e+38"), (0.1, "0.1")];
        for (value, expected) in f32_cases {
            let float = new_float32(Some(value));
            assert_eq!(float.string(), expected, "Float32({value}) text");
            assert_eq!(
                float.marshal_json().expect("marshal"),
                expected.as_bytes(),
                "Float32({value}) JSON"
            );
        }
    }

    #[test]
    fn test_int() {
        let i = new_int(Some(0));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(1), 0);
        assert_eq!(i_clone.val(), 1);
        let counter = Arc::new(i);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..125 {
                    counter.add(1);
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(counter.val(), 1000);

        let i1 = new_int(None);
        assert_eq!(i1.val(), 0);

        let i2 = new_int(Some(11));
        assert_eq!(i2.add(1), 12);
        assert!(!i2.cas(11, 13));
        assert!(i2.cas(12, 13));
        assert_eq!(i2.string(), "13");

        let copy_val = i2.deep_copy();
        i2.set(14);
        assert_eq!(copy_val.val(), 13);
        assert_eq!(i2.val(), 14);
    }

    #[test]
    fn test_int_json() {
        let v = 666;
        let i = new_int(Some(v));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&Value::Int(v));
        assert_eq!(b1, b2);

        let mut i2 = new_int(None);
        i2.unmarshal_json(&b2).expect("unmarshal");
        assert_eq!(i2.val(), v);
    }

    #[test]
    fn test_int_unmarshal_value() {
        let mut i = new_int(None);
        i.unmarshal_value(&Value::String("123".to_owned()));
        assert_eq!(i.val(), 123);
    }

    #[test]
    fn test_int32() {
        let i = new_int32(Some(0));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(1), 0);
        assert_eq!(i_clone.val(), 1);
        let counter = Arc::new(i);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..125 {
                    counter.add(1);
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(counter.val(), 1000);

        let i1 = new_int32(None);
        assert_eq!(i1.val(), 0);

        let i2 = new_int32(Some(11));
        assert_eq!(i2.add(1), 12);
        assert!(!i2.cas(11, 13));
        assert!(i2.cas(12, 13));
        assert_eq!(i2.string(), "13");

        let copy_val = i2.deep_copy();
        i2.set(14);
        assert_eq!(copy_val.val(), 13);
        assert_eq!(i2.val(), 14);
    }

    #[test]
    fn test_int32_json() {
        let v = i32::MAX;
        let i = new_int32(Some(v));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&Value::Int(i64::from(v)));
        assert_eq!(b1, b2);

        let mut i2 = new_int32(None);
        i2.unmarshal_json(&b2).expect("unmarshal");
        assert_eq!(i2.val(), v);
    }

    #[test]
    fn test_int32_unmarshal_value() {
        let mut i = new_int32(None);
        i.unmarshal_value(&Value::String("123".to_owned()));
        assert_eq!(i.val(), 123);
    }

    #[test]
    fn test_int64() {
        let i = new_int64(Some(0));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(1), 0);
        assert_eq!(i_clone.val(), 1);
        let counter = Arc::new(i);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..125 {
                    counter.add(1);
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(counter.val(), 1000);

        let i1 = new_int64(None);
        assert_eq!(i1.val(), 0);

        let i2 = new_int64(Some(11));
        assert_eq!(i2.add(1), 12);
        assert!(!i2.cas(11, 13));
        assert!(i2.cas(12, 13));
        assert_eq!(i2.string(), "13");

        let copy_val = i2.deep_copy();
        i2.set(14);
        assert_eq!(copy_val.val(), 13);
        assert_eq!(i2.val(), 14);
    }

    #[test]
    fn test_int64_json() {
        let v = i64::MAX;
        let i = new_int64(Some(v));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&Value::Int(v));
        assert_eq!(b1, b2);

        let mut i2 = new_int64(None);
        i2.unmarshal_json(&b2).expect("unmarshal");
        assert_eq!(i2.val(), v);
    }

    #[test]
    fn test_int64_unmarshal_value() {
        let mut i = new_int64(None);
        i.unmarshal_value(&Value::String("123".to_owned()));
        assert_eq!(i.val(), 123);
    }

    #[test]
    fn test_string() {
        let i = new_string(Some("abc".to_owned()));
        let i_clone = i.clone();
        assert_eq!(i_clone.set("123".to_owned()), "abc");
        assert_eq!(i_clone.val(), "123");
        assert_eq!(i_clone.string(), "123");

        let copy_val = i_clone.deep_copy();
        i_clone.set("124".to_owned());
        assert_ne!(copy_val.val(), i_clone.val());
        assert_eq!(copy_val.val(), "123");

        let i1 = new_string(None);
        assert!(i1.val().is_empty());
    }

    #[test]
    fn test_string_json() {
        let s = "i love gf";
        let i1 = new_string(Some(s.to_owned()));
        let b1 = i1.marshal_json().expect("marshal");
        let b2 = json_of(&Value::String(s.to_owned()));
        assert_eq!(b1, b2);

        let mut i2 = new_string(None);
        i2.unmarshal_json(&b2).expect("unmarshal");
        assert_eq!(i2.val(), s);
    }

    #[test]
    fn test_string_unmarshal_value() {
        let mut i = new_string(None);
        i.unmarshal_value(&Value::String("123".to_owned()));
        assert_eq!(i.val(), "123");
    }

    #[test]
    fn test_uint() {
        let i = new_uint(Some(0));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(1), 0);
        assert_eq!(i_clone.val(), 1);
        let counter = Arc::new(i);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..125 {
                    counter.add(1);
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(counter.val(), 1000);

        let i1 = new_uint(None);
        assert_eq!(i1.val(), 0);

        let i2 = new_uint(Some(11));
        assert_eq!(i2.add(1), 12);
        assert!(!i2.cas(11, 13));
        assert!(i2.cas(12, 13));
        assert_eq!(i2.string(), "13");

        let copy_val = i2.deep_copy();
        i2.set(14);
        assert_eq!(copy_val.val(), 13);
        assert_eq!(i2.val(), 14);
    }

    #[test]
    fn test_uint_json() {
        let v = 666;
        let i = new_uint(Some(v));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&Value::UInt(v));
        assert_eq!(b1, b2);

        let mut i2 = new_uint(None);
        i2.unmarshal_json(&b2).expect("unmarshal");
        assert_eq!(i2.val(), v);
    }

    #[test]
    fn test_uint_unmarshal_value() {
        let mut i = new_uint(None);
        i.unmarshal_value(&Value::String("123".to_owned()));
        assert_eq!(i.val(), 123);
    }

    #[test]
    fn test_uint32() {
        let i = new_uint32(Some(0));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(1), 0);
        assert_eq!(i_clone.val(), 1);
        let counter = Arc::new(i);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..125 {
                    counter.add(1);
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(counter.val(), 1000);

        let i1 = new_uint32(None);
        assert_eq!(i1.val(), 0);

        let i2 = new_uint32(Some(11));
        assert_eq!(i2.add(1), 12);
        assert!(!i2.cas(11, 13));
        assert!(i2.cas(12, 13));
        assert_eq!(i2.string(), "13");

        let copy_val = i2.deep_copy();
        i2.set(14);
        assert_eq!(copy_val.val(), 13);
        assert_eq!(i2.val(), 14);
    }

    #[test]
    fn test_uint32_json() {
        let v = u32::MAX;
        let i = new_uint32(Some(v));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&Value::UInt(u64::from(v)));
        assert_eq!(b1, b2);

        let mut i2 = new_uint32(None);
        i2.unmarshal_json(&b2).expect("unmarshal");
        assert_eq!(i2.val(), v);
    }

    #[test]
    fn test_uint32_unmarshal_value() {
        let mut i = new_uint32(None);
        i.unmarshal_value(&Value::String("123".to_owned()));
        assert_eq!(i.val(), 123);
    }

    #[test]
    fn test_uint64() {
        let i = new_uint64(Some(0));
        let i_clone = i.clone();
        assert_eq!(i_clone.set(1), 0);
        assert_eq!(i_clone.val(), 1);
        let counter = Arc::new(i);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..125 {
                    counter.add(1);
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(counter.val(), 1000);

        let i1 = new_uint64(None);
        assert_eq!(i1.val(), 0);

        let i2 = new_uint64(Some(11));
        assert_eq!(i2.add(1), 12);
        assert!(!i2.cas(11, 13));
        assert!(i2.cas(12, 13));
        assert_eq!(i2.string(), "13");

        let copy_val = i2.deep_copy();
        i2.set(14);
        assert_eq!(copy_val.val(), 13);
        assert_eq!(i2.val(), 14);
    }

    #[test]
    fn test_uint64_json() {
        let v = u64::MAX;
        let i = new_uint64(Some(v));
        let b1 = i.marshal_json().expect("marshal");
        let b2 = json_of(&Value::UInt(v));
        assert_eq!(b1, b2);

        let mut i2 = new_uint64(None);
        i2.unmarshal_json(&b2).expect("unmarshal");
        assert_eq!(i2.val(), v);
    }

    #[test]
    fn test_uint64_unmarshal_value() {
        let mut i = new_uint64(None);
        i.unmarshal_value(&Value::String("123".to_owned()));
        assert_eq!(i.val(), 123);
    }
}
