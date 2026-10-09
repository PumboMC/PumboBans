//! The data model: punishments, their targets and the players PumboBans has seen.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use pumbo_common::id::{Cidr, Uuid, name_key};

use crate::net;

/// What a punishment does. A temporary ban is a [`Kind::Ban`] with an end time,
/// an IP ban is a ban whose [`Target`] is an address.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Ban,
    Mute,
    Warn,
    Kick,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Ban, Kind::Mute, Kind::Warn, Kind::Kick];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Ban => "ban",
            Kind::Mute => "mute",
            Kind::Warn => "warn",
            Kind::Kick => "kick",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        match s.trim().to_lowercase().as_str() {
            "ban" | "tempban" | "ipban" => Some(Kind::Ban),
            "mute" | "tempmute" | "ipmute" => Some(Kind::Mute),
            "warn" | "warning" => Some(Kind::Warn),
            "kick" => Some(Kind::Kick),
            _ => None,
        }
    }

    /// Kicks happen once; everything else stays in force until it ends.
    pub fn is_lasting(self) -> bool {
        self != Kind::Kick
    }
}

/// Who or what a punishment is aimed at.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Target {
    /// A player account (UUID).
    Player {
        #[serde(with = "net::uuid_str")]
        uuid: Uuid,
    },
    /// An address or an address range (IPv6 is a /64 by default).
    Address {
        #[serde(with = "net::cidr_str")]
        net: Cidr,
    },
    /// A nickname, compared case-insensitively. Used for players the server has
    /// never seen (with `-f`) and for imported entries without a UUID.
    Name { name: String },
}

impl Target {
    pub fn player(uuid: Uuid) -> Target {
        Target::Player { uuid }
    }

    pub fn name(name: &str) -> Target {
        Target::Name { name: name_key(name) }
    }

    /// Index key: `u:<uuid>`, `a:<range>`, `n:<name>`.
    pub fn key(&self) -> String {
        match self {
            Target::Player { uuid } => format!("u:{uuid}"),
            Target::Address { net } => format!("a:{}", net::label(net)),
            Target::Name { name } => format!("n:{name}"),
        }
    }

    pub fn is_address(&self) -> bool {
        matches!(self, Target::Address { .. })
    }
}

/// Who did something: a player (with UUID) or the console / another plugin
/// (name only).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Actor {
    #[serde(default, skip_serializing_if = "Option::is_none", with = "net::opt_uuid_str")]
    pub uuid: Option<Uuid>,
    pub name: String,
}

impl Actor {
    pub fn player(uuid: Uuid, name: &str) -> Actor {
        Actor { uuid: Some(uuid), name: name.to_string() }
    }

    pub fn console(name: &str) -> Actor {
        Actor { uuid: None, name: name.to_string() }
    }

    /// Index key for staff history: `o:<uuid>` or `o:~<name>`.
    pub fn key(&self) -> String {
        match self.uuid {
            Some(u) => format!("o:{u}"),
            None => format!("o:~{}", name_key(&self.name)),
        }
    }
}

/// How a punishment was lifted.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Revocation {
    pub by: Actor,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One punishment. Nothing is ever deleted: lifted punishments keep their
/// [`Revocation`], expired ones simply have `expires` in the past.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Punishment {
    pub id: u64,
    pub kind: Kind,
    pub target: Target,
    /// Display name of the punished player at the time (also for address
    /// punishments placed through a player).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub victim_name: Option<String>,
    /// The account behind an address or name punishment: the player it was placed
    /// through, or the first account that tried to log in under a punished name.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "net::opt_uuid_str")]
    pub victim_uuid: Option<Uuid>,
    pub operator: Actor,
    pub reason: String,
    /// Unix time in milliseconds.
    pub created: u64,
    /// End time; `None` is permanent. Kicks have `Some(created)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<u64>,
    #[serde(default)]
    pub silent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Where the punishment applies. Always `*` (everywhere) in 0.1; per-server
    /// scopes come with PumboProx.
    #[serde(default = "global_scope")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked: Option<Revocation>,
    /// Origin when not typed by staff: `escalation`, `import:vanilla`, `ipc:<plugin>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

fn global_scope() -> String {
    "*".to_string()
}

impl Punishment {
    /// In force right now: not lifted, not expired, and not a kick.
    pub fn is_active(&self, now: u64) -> bool {
        self.kind.is_lasting() && self.revoked.is_none() && self.expires.is_none_or(|e| e > now)
    }

    pub fn is_permanent(&self) -> bool {
        self.expires.is_none()
    }

    /// Milliseconds left; `None` for permanent punishments.
    pub fn remaining(&self, now: u64) -> Option<u64> {
        self.expires.map(|e| e.saturating_sub(now))
    }

    /// The length it was given with; `None` for permanent.
    pub fn length(&self) -> Option<u64> {
        self.expires.map(|e| e.saturating_sub(self.created))
    }

    /// Whether this punishment is aimed at the given account directly (its UUID,
    /// or a name punishment for one of its names or linked to it).
    pub fn targets_account(&self, uuid: Uuid, name: &str) -> bool {
        match &self.target {
            Target::Player { uuid: u } => *u == uuid,
            Target::Name { name: n } => *n == name_key(name) || self.victim_uuid == Some(uuid),
            Target::Address { .. } => false,
        }
    }

    pub fn covers_address(&self, ip: IpAddr) -> bool {
        match &self.target {
            Target::Address { net } => net.contains(ip),
            _ => false,
        }
    }

    /// Who it was aimed at, for messages: the player's name, the address or the name.
    pub fn target_label(&self) -> String {
        match &self.target {
            Target::Player { uuid } => self.victim_name.clone().unwrap_or_else(|| uuid.to_string()),
            Target::Address { net } => match &self.victim_name {
                Some(n) => format!("{n} ({})", net::label(net)),
                None => net::label(net),
            },
            Target::Name { name } => self.victim_name.clone().unwrap_or_else(|| name.clone()),
        }
    }

    /// Ordering for "which of several matching punishments is shown": permanent
    /// first, then the one that ends last, then the oldest.
    pub fn strength(&self) -> (bool, u64, std::cmp::Reverse<u64>) {
        (self.expires.is_none(), self.expires.unwrap_or(u64::MAX), std::cmp::Reverse(self.id))
    }
}

/// When something was first and last seen.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Seen<T> {
    pub value: T,
    pub first: u64,
    pub last: u64,
}

/// What PumboBans remembers about an account. Addresses are personal data and
/// are dropped after the retention period from the config.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PlayerRecord {
    #[serde(with = "net::uuid_str")]
    pub uuid: Uuid,
    /// Last known name, as typed by the player.
    pub name: String,
    #[serde(default)]
    pub names: Vec<Seen<String>>,
    #[serde(default)]
    pub addresses: Vec<Seen<IpAddr>>,
    pub first_seen: u64,
    pub last_seen: u64,
    /// Exemption permissions the player had at the last login, so punishments
    /// placed while they are offline respect them too.
    #[serde(default)]
    pub exempt: Vec<Kind>,
    /// Client language at the last login (`pl_pl`), for the ban screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

impl PlayerRecord {
    pub fn new(uuid: Uuid, name: &str, now: u64) -> PlayerRecord {
        PlayerRecord {
            uuid,
            name: name.to_string(),
            names: Vec::new(),
            addresses: Vec::new(),
            first_seen: now,
            last_seen: now,
            exempt: Vec::new(),
            locale: None,
        }
    }

    /// Notes a login; returns true when a new address was added.
    pub fn touch(&mut self, name: &str, ip: Option<IpAddr>, now: u64) -> bool {
        self.name = name.to_string();
        self.last_seen = now;
        match self.names.iter_mut().find(|s| s.value == name) {
            Some(s) => s.last = now,
            None => self.names.push(Seen { value: name.to_string(), first: now, last: now }),
        }
        let Some(ip) = ip else { return false };
        match self.addresses.iter_mut().find(|s| s.value == ip) {
            Some(s) => {
                s.last = now;
                false
            }
            None => {
                self.addresses.push(Seen { value: ip, first: now, last: now });
                true
            }
        }
    }

    pub fn is_exempt(&self, kind: Kind) -> bool {
        self.exempt.contains(&kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumbo_common::id::parse_ip;

    fn sample(expires: Option<u64>) -> Punishment {
        Punishment {
            id: 1,
            kind: Kind::Ban,
            target: Target::player(Uuid(7)),
            victim_name: Some("Steve".into()),
            victim_uuid: None,
            operator: Actor::console("Console"),
            reason: "test".into(),
            created: 100,
            expires,
            silent: false,
            template: None,
            scope: "*".into(),
            revoked: None,
            source: None,
        }
    }

    #[test]
    fn activity() {
        assert!(sample(None).is_active(10_000));
        assert!(sample(Some(200)).is_active(199));
        assert!(!sample(Some(200)).is_active(200));
        let mut kick = sample(Some(100));
        kick.kind = Kind::Kick;
        assert!(!kick.is_active(50));
        let mut revoked = sample(None);
        revoked.revoked = Some(Revocation { by: Actor::console("x"), at: 150, reason: None });
        assert!(!revoked.is_active(160));
    }

    #[test]
    fn json_shape_is_stable() {
        let p = sample(Some(200));
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"kind\":\"ban\""));
        assert!(json.contains("\"type\":\"player\""));
        let back: Punishment = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn name_targets_match_case_insensitively_and_by_link() {
        let mut p = sample(None);
        p.target = Target::name("Griefer");
        assert!(p.targets_account(Uuid(1), "gRIEFER"));
        assert!(!p.targets_account(Uuid(1), "other"));
        p.victim_uuid = Some(Uuid(1));
        assert!(p.targets_account(Uuid(1), "renamed"));
    }

    #[test]
    fn record_touch() {
        let mut r = PlayerRecord::new(Uuid(1), "A", 1);
        assert!(r.touch("A", parse_ip("1.1.1.1"), 2));
        assert!(!r.touch("A", parse_ip("1.1.1.1"), 3));
        assert!(r.touch("B", parse_ip("2.2.2.2"), 4));
        assert_eq!(r.names.len(), 2);
        assert_eq!(r.addresses.len(), 2);
        assert_eq!(r.addresses.first().map(|s| s.last), Some(3));
        assert_eq!(r.name, "B");
    }

    #[test]
    fn strength_prefers_permanent_then_latest_end() {
        let a = sample(None);
        let mut b = sample(Some(10_000));
        b.id = 2;
        assert!(a.strength() > b.strength());
        let mut c = sample(Some(20_000));
        c.id = 3;
        assert!(c.strength() > b.strength());
    }
}
