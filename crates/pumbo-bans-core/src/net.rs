//! Address ranges of IP punishments, on top of [`pumbo_common::id::Cidr`], and
//! serde helpers for the shared identifier types.

use std::net::IpAddr;

use pumbo_common::id::{Cidr, parse_ip};

/// The range an IP punishment of one address covers: `/v4` for IPv4 (32 = the
/// address itself), `/v6` for IPv6 (64 = the customer's network by default).
pub fn punish_range(ip: IpAddr, v4: u8, v6: u8) -> Cidr {
    match ip {
        IpAddr::V4(_) => Cidr::new(ip, v4),
        IpAddr::V6(_) => Cidr::new(ip, v6),
    }
}

/// Parses an address or range typed by staff. A single address is widened to
/// the configured prefix; an explicit `/n` is kept.
pub fn parse_range(text: &str, v4: u8, v6: u8) -> Option<Cidr> {
    if text.contains('/') && !text.trim_start().starts_with('/') {
        return Cidr::parse(text);
    }
    parse_ip(text).map(|ip| punish_range(ip, v4, v6))
}

/// Whether the range is a single address.
pub fn is_single(c: &Cidr) -> bool {
    match c.network() {
        IpAddr::V4(_) => c.prefix() == 32,
        IpAddr::V6(_) => c.prefix() == 128,
    }
}

/// Whether two ranges share an address.
pub fn overlaps(a: &Cidr, b: &Cidr) -> bool {
    if a.prefix() <= b.prefix() { a.contains(b.network()) } else { b.contains(a.network()) }
}

/// `1.2.3.4` for a single address, `2001:db8::/64` for a range.
pub fn label(c: &Cidr) -> String {
    if is_single(c) { c.network().to_string() } else { c.to_string() }
}

/// Serialises a [`pumbo_common::id::Uuid`] as its hyphenated string.
pub mod uuid_str {
    use pumbo_common::id::Uuid;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(u: &Uuid, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&u.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Uuid, D::Error> {
        let s = String::deserialize(d)?;
        Uuid::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid UUID {s}")))
    }
}

/// Like [`uuid_str`] for `Option<Uuid>`.
pub mod opt_uuid_str {
    use pumbo_common::id::Uuid;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(u: &Option<Uuid>, s: S) -> Result<S::Ok, S::Error> {
        match u {
            Some(u) => s.serialize_some(&u.to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Uuid>, D::Error> {
        match Option::<String>::deserialize(d)? {
            Some(s) => Uuid::parse(&s).map(Some).ok_or_else(|| serde::de::Error::custom(format!("invalid UUID {s}"))),
            None => Ok(None),
        }
    }
}

/// Serialises a [`Cidr`] as `network/prefix`.
pub mod cidr_str {
    use pumbo_common::id::Cidr;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(c: &Cidr, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&c.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Cidr, D::Error> {
        let s = String::deserialize(d)?;
        Cidr::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid address range {s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        parse_ip(s).unwrap()
    }

    #[test]
    fn ranges() {
        let one = punish_range(ip("1.2.3.4"), 32, 64);
        assert!(is_single(&one));
        assert_eq!(label(&one), "1.2.3.4");
        assert!(one.contains(ip("1.2.3.4:5555")));
        assert!(!one.contains(ip("1.2.3.5")));
        let v6 = punish_range(ip("2001:db8:0:1:aaaa::1"), 32, 64);
        assert_eq!(label(&v6), "2001:db8:0:1::/64");
        assert!(v6.contains(ip("[2001:db8:0:1:ffff::9]:25565")));
        assert!(!v6.contains(ip("2001:db8:0:2::1")));
        let wide = parse_range("10.1.2.3/16", 32, 64).unwrap();
        assert_eq!(label(&wide), "10.1.0.0/16");
        assert!(overlaps(&wide, &punish_range(ip("10.1.200.1"), 32, 64)));
        assert!(!overlaps(&wide, &one));
        assert_eq!(parse_range("/127.0.0.1:5", 32, 64).map(|c| label(&c)), Some("127.0.0.1".into()));
        assert!(parse_range("Steve", 32, 64).is_none());
        // IPv4-mapped connections hit IPv4 bans.
        assert!(one.contains("::ffff:1.2.3.4".parse().unwrap()));
    }
}
