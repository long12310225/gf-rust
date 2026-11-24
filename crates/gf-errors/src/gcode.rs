//! Port of `errors/gcode`: universal error codes.

use std::borrow::Cow;
use std::fmt;

use gf_core::Value;

/// A universal error code.
pub trait Code {
    /// Returns the integer number of the code.
    fn code(&self) -> i32;

    /// Returns the brief message of the code.
    fn message(&self) -> &str;

    /// Returns the extension detail of the code.
    fn detail(&self) -> Option<&Value>;

    /// Renders the code the way its Go `String` method does.
    fn as_string(&self) -> String {
        let detail = self.detail();
        if let Some(detail) = detail {
            return format!("{}:{} {}", self.code(), self.message(), detail);
        }
        if !self.message().is_empty() {
            return format!("{}:{}", self.code(), self.message());
        }
        format!("{}", self.code())
    }
}

/// The concrete error code the framework uses.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalCode {
    code: i32,
    message: Cow<'static, str>,
    detail: Option<Box<Value>>,
}

impl LocalCode {
    /// Creates an error code.
    pub fn new(code: i32, message: impl Into<String>, detail: Option<Value>) -> LocalCode {
        LocalCode {
            code,
            message: Cow::Owned(message.into()),
            detail: detail.map(Box::new),
        }
    }
}

impl Code for LocalCode {
    fn code(&self) -> i32 {
        self.code
    }

    fn message(&self) -> &str {
        &self.message
    }

    fn detail(&self) -> Option<&Value> {
        self.detail.as_deref()
    }
}

impl fmt::Display for LocalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_string())
    }
}

impl LocalCode {
    const fn constant(code: i32, message: &'static str) -> LocalCode {
        LocalCode {
            code,
            message: Cow::Borrowed(message),
            detail: None,
        }
    }
}

/// No error code specified.
pub const CODE_NIL: LocalCode = LocalCode::constant(-1, "");
/// It is OK.
pub const CODE_OK: LocalCode = LocalCode::constant(0, "OK");
/// An error occurred internally.
pub const CODE_INTERNAL_ERROR: LocalCode = LocalCode::constant(50, "Internal Error");
/// Data validation failed.
pub const CODE_VALIDATION_FAILED: LocalCode = LocalCode::constant(51, "Validation Failed");
/// Database operation error.
pub const CODE_DB_OPERATION_ERROR: LocalCode = LocalCode::constant(52, "Database Operation Error");
/// The given parameter for the current operation is invalid.
pub const CODE_INVALID_PARAMETER: LocalCode = LocalCode::constant(53, "Invalid Parameter");
/// A parameter for the current operation is missing.
pub const CODE_MISSING_PARAMETER: LocalCode = LocalCode::constant(54, "Missing Parameter");
/// The function cannot be used like this.
pub const CODE_INVALID_OPERATION: LocalCode = LocalCode::constant(55, "Invalid Operation");
/// The configuration is invalid for the current operation.
pub const CODE_INVALID_CONFIGURATION: LocalCode = LocalCode::constant(56, "Invalid Configuration");
/// The configuration is missing for the current operation.
pub const CODE_MISSING_CONFIGURATION: LocalCode = LocalCode::constant(57, "Missing Configuration");
/// The operation is not implemented yet.
pub const CODE_NOT_IMPLEMENTED: LocalCode = LocalCode::constant(58, "Not Implemented");
/// The operation is not supported yet.
pub const CODE_NOT_SUPPORTED: LocalCode = LocalCode::constant(59, "Not Supported");
/// The operation was attempted and failed.
pub const CODE_OPERATION_FAILED: LocalCode = LocalCode::constant(60, "Operation Failed");
/// Not authorized.
pub const CODE_NOT_AUTHORIZED: LocalCode = LocalCode::constant(61, "Not Authorized");
/// Security reason.
pub const CODE_SECURITY_REASON: LocalCode = LocalCode::constant(62, "Security Reason");
/// Server is busy.
pub const CODE_SERVER_BUSY: LocalCode = LocalCode::constant(63, "Server Is Busy");
/// Unknown error.
pub const CODE_UNKNOWN: LocalCode = LocalCode::constant(64, "Unknown Error");
/// The resource does not exist.
pub const CODE_NOT_FOUND: LocalCode = LocalCode::constant(65, "Not Found");
/// Invalid request.
pub const CODE_INVALID_REQUEST: LocalCode = LocalCode::constant(66, "Invalid Request");
/// A necessary package was not imported.
pub const CODE_NECESSARY_PACKAGE_NOT_IMPORT: LocalCode =
    LocalCode::constant(67, "Necessary Package Not Import");
/// An internal panic occurred.
pub const CODE_INTERNAL_PANIC: LocalCode = LocalCode::constant(68, "Internal Panic");
/// Business validation failed.
pub const CODE_BUSINESS_VALIDATION_FAILED: LocalCode =
    LocalCode::constant(300, "Business Validation Failed");

/// Creates an error code.
pub fn new(code: i32, message: impl Into<String>, detail: Option<Value>) -> LocalCode {
    LocalCode::new(code, message, detail)
}

/// Creates an error code with the code and message of `code` and a new detail.
pub fn with_code(code: &dyn Code, detail: Option<Value>) -> LocalCode {
    LocalCode::new(code.code(), code.message().to_owned(), detail)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    #[test]
    fn default_codes_render_like_go() {
        assert_eq!(CODE_NIL.to_string(), "-1");
        assert_eq!(CODE_INTERNAL_ERROR.to_string(), "50:Internal Error");
    }

    #[test]
    fn new_keeps_code_message_and_detail() {
        let code = new(
            1,
            "custom error",
            Some(Value::String("detailed description".into())),
        );
        assert_eq!(code.code(), 1);
        assert_eq!(code.message(), "custom error");
        assert_eq!(
            code.detail(),
            Some(&Value::String("detailed description".into()))
        );
    }

    #[test]
    fn with_code_overrides_only_the_detail() {
        let code = with_code(
            &CODE_INTERNAL_ERROR,
            Some(Value::String("CodeInternalError".into())),
        );
        assert_eq!(code.code(), CODE_INTERNAL_ERROR.code());
        assert_eq!(
            code.detail(),
            Some(&Value::String("CodeInternalError".into()))
        );
    }

    #[test]
    fn string_omits_missing_parts() {
        let with_detail = new(
            100,
            "test message",
            Some(Value::String("test detail".into())),
        );
        assert_eq!(with_detail.to_string(), "100:test message test detail");

        let message_only = new(100, "test message", None);
        assert_eq!(message_only.to_string(), "100:test message");

        let bare = new(100, "", None);
        assert_eq!(bare.to_string(), "100");

        let mut rendered = String::new();
        write!(&mut rendered, "{}", new(7, "m", Some(Value::Int(2)))).expect("write to string");
        assert_eq!(rendered, "7:m 2");
    }
}
