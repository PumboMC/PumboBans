//! Import of vanilla ban lists: `banned-players.json` and `banned-ips.json`
//! (Mojang's server, Paper, Spigot, Pumpkin all write this format), or the same
//! records read through a server API.
//!
//! Dates are `yyyy-MM-dd HH:mm:ss Z`; `expires` is a date or `forever`.

use pumbo_common::id::{Uuid, parse_ip};
use serde::Deserialize;

use crate::dates::parse_vanilla_date;
use crate::engine::ImportEntry;
use crate::model::{Kind, Target};
use crate::net;

/// One entry of `banned-players.json`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct VanillaPlayerBan {
    pub uuid: String,
    pub name: String,
    pub created: String,
    pub source: String,
    pub expires: String,
    pub reason: String,
}

/// One entry of `banned-ips.json`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct VanillaIpBan {
    pub ip: String,
    pub created: String,
    pub source: String,
    pub expires: String,
    pub reason: String,
}

pub fn parse_players(json: &str) -> Result<Vec<VanillaPlayerBan>, String> {
    serde_json::from_str(json).map_err(|e| format!("banned-players.json: {e}"))
}

pub fn parse_ips(json: &str) -> Result<Vec<VanillaIpBan>, String> {
    serde_json::from_str(json).map_err(|e| format!("banned-ips.json: {e}"))
}

fn expires(s: &str) -> Option<u64> {
    if s.trim().is_empty() || s.trim().eq_ignore_ascii_case("forever") { None } else { parse_vanilla_date(s) }
}

fn operator(source: &str) -> String {
    let s = source.trim();
    if s.is_empty() || s == "(Unknown)" { "Server".into() } else { s.to_string() }
}

/// Entries for [`crate::Engine::import`]. Unreadable UUIDs fall back to a name
/// punishment; entries without either are dropped (and counted).
pub fn player_entries(list: &[VanillaPlayerBan], now: u64) -> (Vec<ImportEntry>, usize) {
    let mut out = Vec::new();
    let mut dropped = 0;
    for b in list {
        let target = match Uuid::parse(&b.uuid) {
            Some(u) => Target::player(u),
            None if !b.name.trim().is_empty() => Target::name(&b.name),
            None => {
                dropped += 1;
                continue;
            }
        };
        out.push(ImportEntry {
            kind: Kind::Ban,
            target,
            name: Some(b.name.clone()).filter(|n| !n.is_empty()),
            operator: operator(&b.source),
            reason: b.reason.clone(),
            created: parse_vanilla_date(&b.created).unwrap_or(now),
            expires: expires(&b.expires),
            source: "import:vanilla".into(),
        });
    }
    (out, dropped)
}

/// Entries of IP bans; each address becomes a single-address range (IPv6 too,
/// since the list names exact addresses).
pub fn ip_entries(list: &[VanillaIpBan], now: u64) -> (Vec<ImportEntry>, usize) {
    let mut out = Vec::new();
    let mut dropped = 0;
    for b in list {
        let Some(ip) = parse_ip(&b.ip) else {
            dropped += 1;
            continue;
        };
        out.push(ImportEntry {
            kind: Kind::Ban,
            target: Target::Address { net: net::punish_range(ip, 32, 128) },
            name: None,
            operator: operator(&b.source),
            reason: b.reason.clone(),
            created: parse_vanilla_date(&b.created).unwrap_or(now),
            expires: expires(&b.expires),
            source: "import:vanilla".into(),
        });
    }
    (out, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYERS: &str = r#"[
      {"uuid":"069a79f4-44e9-4726-a5be-fca90e38aaf5","name":"Notch","created":"2026-10-01 10:00:00 +0000","source":"Server","expires":"forever","reason":"Banned by an operator."},
      {"uuid":"not-a-uuid","name":"Griefer","created":"2026-10-01 10:00:00 +0000","source":"(Unknown)","expires":"2026-12-01 10:00:00 +0000","reason":"grief"},
      {"uuid":"","name":"","created":"","source":"","expires":"forever","reason":""}
    ]"#;
    const IPS: &str = r#"[
      {"ip":"1.2.3.4","created":"2026-10-01 10:00:00 +0000","source":"Admin","expires":"forever","reason":"bots"},
      {"ip":"2001:db8::1","created":"bad date","source":"Admin","expires":"forever","reason":"v6"},
      {"ip":"nonsense","created":"","source":"","expires":"forever","reason":""}
    ]"#;

    #[test]
    fn players() {
        let list = parse_players(PLAYERS).unwrap();
        let (entries, dropped) = player_entries(&list, 7);
        assert_eq!(dropped, 1);
        assert_eq!(entries.len(), 2);
        let first = entries.first().unwrap();
        assert!(matches!(first.target, Target::Player { .. }));
        assert_eq!(first.expires, None);
        assert_eq!(first.operator, "Server");
        let second = entries.get(1).unwrap();
        assert_eq!(second.target, Target::name("griefer"));
        assert!(second.expires.is_some());
        assert_eq!(second.operator, "Server");
    }

    #[test]
    fn ips() {
        let (entries, dropped) = ip_entries(&parse_ips(IPS).unwrap(), 7);
        assert_eq!(dropped, 1);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries.get(1).unwrap().created, 7);
        assert!(parse_ips("{").is_err());
    }
}
