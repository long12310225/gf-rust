//! Port of `net/gipv4`: IPv4 conversion, validation and host inspection.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

use gf_errors::gerror;
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};

fn parse_ipv4(ip: &str) -> Option<Ipv4Addr> {
    if let Ok(addr) = ip.parse::<Ipv4Addr>() {
        return Some(addr);
    }
    ip.parse::<Ipv6Addr>()
        .ok()
        .and_then(|addr| addr.to_ipv4_mapped())
}

/// Validates `ip` as an IPv4 address.
pub fn validate(ip: &str) -> bool {
    parse_ipv4(ip).is_some()
}

/// Converts an IPv4 address to a big-endian integer; invalid input is `0`.
pub fn ip_to_long_big_endian(ip: &str) -> u32 {
    parse_ipv4(ip).map_or(0, |addr| u32::from_be_bytes(addr.octets()))
}

/// Converts a big-endian integer to its IPv4 address text.
pub fn long_to_ip_big_endian(long: u32) -> String {
    Ipv4Addr::from(long.to_be_bytes()).to_string()
}

/// Converts an IPv4 address to a little-endian integer; invalid input is `0`.
pub fn ip_to_long_little_endian(ip: &str) -> u32 {
    parse_ipv4(ip).map_or(0, |addr| u32::from_le_bytes(addr.octets()))
}

/// Converts a little-endian integer to its IPv4 address text.
pub fn long_to_ip_little_endian(long: u32) -> String {
    Ipv4Addr::from(long.to_le_bytes()).to_string()
}

/// Deprecated alias of [`ip_to_long_big_endian`].
pub fn ip2long(ip: &str) -> u32 {
    ip_to_long_big_endian(ip)
}

/// Deprecated alias of [`long_to_ip_big_endian`].
pub fn long2ip(long: u32) -> String {
    long_to_ip_big_endian(long)
}

/// Parses `address` into `(host, port)`; failure yields `("", 0)`.
pub fn parse_address(address: &str) -> (String, i32) {
    let (host, port) = if let Some(rest) = address.strip_prefix('[') {
        match rest.split_once("]:") {
            Some((host, port)) => (host, port),
            None => return (String::new(), 0),
        }
    } else {
        match address.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, port),
            _ => return (String::new(), 0),
        }
    };
    match port.parse::<i32>() {
        Ok(port) => (host.to_owned(), port),
        Err(_) => (String::new(), 0),
    }
}

/// Returns the first three segments of an IPv4 address, or `""`.
pub fn get_segment(ip: &str) -> String {
    if !validate(ip) {
        return String::new();
    }
    let mut segments = ip.split('.');
    let first = segments.next();
    let second = segments.next();
    let third = segments.next();
    match (first, second, third) {
        (Some(first), Some(second), Some(third)) => format!("{first}.{second}.{third}"),
        _ => String::new(),
    }
}

/// Reports whether `ip` is private or loopback.
///
/// Loopback: `127.0.0.0/8`, `::1`. Private: `10/8`, `172.16/12`, `192.168/16`,
/// and IPv6 ULA `fc00::/7`.
pub fn is_intranet(ip: &str) -> bool {
    match ip.parse::<IpAddr>() {
        Ok(IpAddr::V4(addr)) => {
            let octets = addr.octets();
            addr.is_loopback()
                || addr.is_private()
                || octets[0] == 10
                || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 168)
        }
        Ok(IpAddr::V6(addr)) => addr.is_loopback() || (addr.segments()[0] & 0xFE00) == 0xFC00,
        Err(_) => false,
    }
}

fn interfaces() -> Result<Vec<NetworkInterface>, gerror::Error> {
    NetworkInterface::show().map_err(|err| wrap(err, "net.Interfaces failed"))
}

fn wrap(err: impl std::error::Error + Send + Sync + 'static, text: &str) -> gerror::Error {
    match gerror::wrap_std(Some(Box::new(err)), text) {
        Some(err) => err,
        None => gerror::Error::new(text),
    }
}

/// Returns all non-loopback IPv4 addresses of this host.
pub fn get_ip_array() -> Result<Vec<String>, gerror::Error> {
    let mut ips = Vec::new();
    for interface in interfaces()? {
        for addr in interface.addr {
            if let Addr::V4(addr) = addr
                && !addr.ip.is_loopback()
            {
                ips.push(addr.ip.to_string());
            }
        }
    }
    Ok(ips)
}

/// Returns all intranet IPv4 addresses of this host.
pub fn get_intranet_ip_array() -> Result<Vec<String>, gerror::Error> {
    let mut ips = Vec::new();
    for interface in interfaces()? {
        if interface.name.starts_with("w-") {
            continue;
        }
        for addr in interface.addr {
            let Addr::V4(addr) = addr else {
                continue;
            };
            if addr.ip.is_loopback() {
                continue;
            }
            let ip = addr.ip.to_string();
            if is_intranet(&ip) {
                ips.push(ip);
            }
        }
    }
    Ok(ips)
}

/// Returns the first intranet IPv4 address of this host.
pub fn get_intranet_ip() -> Result<String, gerror::Error> {
    let ips = get_intranet_ip_array()?;
    match ips.into_iter().next() {
        Some(ip) => Ok(ip),
        None => Err(gerror::Error::new("no intranet ip found")),
    }
}

/// Performs as [`get_intranet_ip`], panicking on failure.
pub fn must_get_intranet_ip() -> String {
    match get_intranet_ip() {
        Ok(ip) => ip,
        Err(err) => panic!("{err}"),
    }
}

/// Returns all MAC addresses of this host.
pub fn get_mac_array() -> Result<Vec<String>, gerror::Error> {
    let mut macs = Vec::new();
    for interface in interfaces()? {
        if let Some(mac) = interface.mac_addr {
            let mac = mac.to_string();
            if !mac.is_empty() {
                macs.push(mac);
            }
        }
    }
    Ok(macs)
}

/// Returns the first MAC address of this host.
pub fn get_mac() -> Result<String, gerror::Error> {
    Ok(get_mac_array()?.into_iter().next().unwrap_or_default())
}

fn lookup_ipv4(hostname: &str) -> Result<Vec<String>, gerror::Error> {
    let addrs = (hostname, 0)
        .to_socket_addrs()
        .map_err(|err| wrap(err, "net.LookupIP failed"))?;
    Ok(addrs
        .filter_map(|addr| match addr.ip() {
            IpAddr::V4(addr) => Some(addr.to_string()),
            IpAddr::V6(_) => None,
        })
        .collect())
}

/// Returns the first IPv4 address of `hostname`.
pub fn get_host_by_name(hostname: &str) -> Result<String, gerror::Error> {
    Ok(lookup_ipv4(hostname)?
        .into_iter()
        .next()
        .unwrap_or_default())
}

/// Returns all IPv4 addresses of `hostname`.
pub fn get_hosts_by_name(hostname: &str) -> Result<Vec<String>, gerror::Error> {
    lookup_ipv4(hostname)
}

/// Returns the host name of `ip_address` without the trailing dot.
pub fn get_name_by_addr(ip_address: &str) -> Result<String, gerror::Error> {
    let ip: IpAddr = ip_address
        .parse()
        .map_err(|err| wrap(err, "net.LookupAddr failed"))?;
    let name = dns_lookup::lookup_addr(&ip).map_err(|err| wrap(err, "net.LookupAddr failed"))?;
    Ok(name.trim_end_matches('.').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const IPV4: &str = "192.168.1.1";
    const LONG_BIG_ENDIAN: u32 = 3_232_235_777;
    const LONG_LITTLE_ENDIAN: u32 = 16_885_952;

    #[test]
    fn endian_conversions_match_go() {
        assert_eq!(ip_to_long_big_endian(IPV4), LONG_BIG_ENDIAN);
        assert_eq!(ip2long(IPV4), LONG_BIG_ENDIAN);
        assert_eq!(long_to_ip_big_endian(LONG_BIG_ENDIAN), IPV4);
        assert_eq!(long2ip(LONG_BIG_ENDIAN), IPV4);
        assert_eq!(ip_to_long_little_endian(IPV4), LONG_LITTLE_ENDIAN);
        assert_eq!(long_to_ip_little_endian(LONG_LITTLE_ENDIAN), IPV4);

        assert_eq!(ip_to_long_big_endian("not an ip"), 0);
        assert_eq!(ip_to_long_big_endian("256.1.1.1"), 0);
        assert_eq!(ip_to_long_big_endian("10.0.0.1/24"), 0);
    }

    #[test]
    fn validate_and_segment() {
        assert!(validate("192.168.1.1"));
        assert!(validate("::ffff:192.168.1.1"));
        assert!(!validate("::1"));
        assert!(!validate(""));
        assert_eq!(get_segment("192.168.2.102"), "192.168.2");
        assert_eq!(get_segment("bad"), "");
    }

    #[test]
    fn parse_address_matches_go() {
        assert_eq!(
            parse_address("192.168.1.1:80"),
            ("192.168.1.1".to_owned(), 80)
        );
        assert_eq!(parse_address("[::1]:8080"), ("::1".to_owned(), 8080));
        assert_eq!(parse_address("192.168.1.1"), (String::new(), 0));
        assert_eq!(parse_address("1:2:3"), (String::new(), 0));
        assert_eq!(parse_address("host:abc"), (String::new(), 0));
    }

    #[test]
    fn intranet_matches_the_go_cases() {
        for (ip, expected) in [
            ("127.0.0.1", true),
            ("127.0.0.2", true),
            ("127.255.255.255", true),
            ("10.0.0.1", true),
            ("172.16.0.1", true),
            ("172.31.255.255", true),
            ("192.168.0.1", true),
            ("192.168.255.255", true),
            ("::1", true),
            ("fc00::1", true),
            ("8.8.8.8", false),
            ("172.15.0.1", false),
            ("172.32.0.1", false),
            ("192.169.0.1", false),
            ("fe80::1", false),
            ("256.256.256.256", false),
            ("10.0.0.1/24", false),
            ("", false),
        ] {
            assert_eq!(is_intranet(ip), expected, "{ip}");
        }
    }

    #[test]
    fn host_interfaces_are_reported() {
        let ips = get_ip_array().expect("interfaces");
        for ip in &ips {
            assert!(validate(ip), "{ip}");
        }
        let macs = get_mac_array().expect("interfaces");
        for mac in &macs {
            assert_eq!(mac.matches(':').count(), 5, "{mac}");
        }
        let _ = get_mac().expect("interfaces");
        let _ = get_intranet_ip_array().expect("interfaces");
        // A machine without a private address still reports this error.
        let _ = get_intranet_ip();
    }
}
