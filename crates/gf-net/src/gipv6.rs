//! Port of `net/gipv6`.

use gf_text::gregex;

const IPV6_PATTERN: &str = r"^([\da-fA-F]{1,4}:){7}[\da-fA-F]{1,4}$|^:((:[\da-fA-F]{1,4}){1,6}|:)$|^[\da-fA-F]{1,4}:((:[\da-fA-F]{1,4}){1,5}|:)$|^([\da-fA-F]{1,4}:){2}((:[\da-fA-F]{1,4}){1,4}|:)$|^([\da-fA-F]{1,4}:){3}((:[\da-fA-F]{1,4}){1,3}|:)$|^([\da-fA-F]{1,4}:){4}((:[\da-fA-F]{1,4}){1,2}|:)$|^([\da-fA-F]{1,4}:){5}:([\da-fA-F]{1,4})?$|^([\da-fA-F]{1,4}:){6}:$";

/// Checks whether `ip` is a valid IPv6 address, using Go's pattern.
pub fn validate(ip: &str) -> bool {
    gregex::is_match_string(IPV6_PATTERN, ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_addresses() {
        for ip in [
            "::",
            "::1",
            "1::",
            "2001:db8::1",
            "2001:0db8:85a3:0000:0000:8a2e:0370:7334",
            "2001:db8:0:0:0:0:2:1",
            "FE80::1",
            "1:2:3:4:5:6:7:8",
        ] {
            assert!(validate(ip), "{ip}");
        }
    }

    #[test]
    fn rejects_invalid_addresses() {
        for ip in [
            "",
            "1:2:3:4:5:6:7",
            "1:2:3:4:5:6:7:8:9",
            "::ffff:192.168.1.1",
            "192.168.1.1",
            "gggg::1",
            "12345::1",
            "1:2:3:4:5:6:7:8::",
        ] {
            assert!(!validate(ip), "{ip}");
        }
    }
}
