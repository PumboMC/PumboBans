//! Punishments and players on top of [`pumbo_common::store::Store`].
//!
//! Tables (all `text -> bytes`):
//!
//! | table | key | value |
//! | --- | --- | --- |
//! | `punishments` | id, 20 digits | JSON [`Punishment`] |
//! | `active` | id | empty: lasting and not lifted when last written |
//! | `index` | `<index key>\|<id>` | empty |
//! | `players` | UUID | JSON [`PlayerRecord`] |
//! | `names` | `name_key` | UUID of the last account with that name |
//! | `addresses` | `<ip_key>\|<uuid>` | empty |
//! | `meta` | `schema`, `next-id` | u64 |
//!
//! Index keys: the target (`u:<uuid>`, `a:<range>`, `n:<name>`), the linked
//! account (`v:<uuid>`) and the operator (`o:<uuid>` or `o:~<name>`). Every
//! punishment is written together with its index entries in one transaction.
//! Index entries are only ever added, so rewriting them is harmless.

use std::collections::BTreeSet;
use std::net::IpAddr;

use pumbo_common::id::{Uuid, ip_key, name_key};
use pumbo_common::store::{Durability, ReadExt, ReadOps, Result, Store, StoreError, WriteExt, WriteOps};

use crate::model::{PlayerRecord, Punishment};

const PUNISHMENTS: &str = "punishments";
const ACTIVE: &str = "active";
const INDEX: &str = "index";
const PLAYERS: &str = "players";
const NAMES: &str = "names";
const ADDRESSES: &str = "addresses";
const META: &str = "meta";

/// Version of the table layout.
pub const SCHEMA: u64 = 1;

fn id_key(id: u64) -> String {
    format!("{id:020}")
}

/// Index keys of a punishment.
pub fn index_keys(p: &Punishment) -> Vec<String> {
    let mut keys = vec![p.target.key(), p.operator.key()];
    if let Some(v) = p.victim_uuid {
        keys.push(format!("v:{v}"));
    }
    keys
}

fn listed_active(p: &Punishment) -> bool {
    p.kind.is_lasting() && p.revoked.is_none()
}

#[derive(Debug)]
pub struct BansStore {
    store: Store,
}

impl BansStore {
    /// Wraps a store, refusing one written by a newer schema.
    pub fn new(store: Store) -> Result<Self> {
        store.write(|tx| match tx.get_u64(META, "schema")? {
            None => tx.put_u64(META, "schema", SCHEMA),
            Some(v) if v > SCHEMA => Err(StoreError::new(format!(
                "the database was written by a newer PumboBans (schema {v}, this version reads {SCHEMA})"
            ))),
            Some(_) => Ok(()),
        })?;
        Ok(Self { store })
    }

    pub fn in_memory() -> Self {
        Self { store: Store::in_memory() }
    }

    /// A store whose every write fails (a broken disk), for tests.
    #[cfg(test)]
    pub(crate) fn failing_for_tests() -> Self {
        Self { store: Store::new(tests::FailingWrites(pumbo_common::store::MemoryBackend::default())) }
    }

    fn write_punishment(tx: &mut dyn WriteOps, p: &Punishment) -> Result<()> {
        let key = id_key(p.id);
        tx.put_json(PUNISHMENTS, &key, p)?;
        for k in index_keys(p) {
            tx.put(INDEX, &format!("{k}|{key}"), b"")?;
        }
        if listed_active(p) {
            tx.put(ACTIVE, &key, b"")?;
        } else {
            tx.remove(ACTIVE, &key)?;
        }
        Ok(())
    }

    /// Gives `p` the next id and writes it.
    pub fn insert(&self, p: &mut Punishment) -> Result<()> {
        let id = self.store.write(|tx| {
            let id = tx.get_u64(META, "next-id")?.unwrap_or(1);
            tx.put_u64(META, "next-id", id + 1)?;
            let mut copy = p.clone();
            copy.id = id;
            Self::write_punishment(tx, &copy)?;
            Ok(id)
        })?;
        p.id = id;
        Ok(())
    }

    /// Rewrites an existing punishment.
    pub fn update(&self, p: &Punishment) -> Result<()> {
        self.store.write(|tx| {
            if tx.get(PUNISHMENTS, &id_key(p.id))?.is_none() {
                return Err(StoreError::new(format!("no punishment {}", p.id)));
            }
            Self::write_punishment(tx, p)
        })
    }

    pub fn get(&self, id: u64) -> Result<Option<Punishment>> {
        self.store.get_json(PUNISHMENTS, &id_key(id))
    }

    /// Punishments listed as active, and how many of them could not be read.
    pub fn active(&self) -> Result<(Vec<Punishment>, usize)> {
        self.store.read(|tx| {
            let mut ids = Vec::new();
            tx.scan(ACTIVE, "", &mut |k, _| {
                ids.push(k.to_string());
                true
            })?;
            let mut out = Vec::new();
            let mut damaged = 0;
            for k in ids {
                match tx.get_json::<Punishment>(PUNISHMENTS, &k) {
                    Ok(Some(p)) => out.push(p),
                    _ => damaged += 1,
                }
            }
            Ok((out, damaged))
        })
    }

    /// Drops ids from the active list (they stay in the history).
    pub fn retire(&self, ids: &[u64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.store.write(|tx| {
            for id in ids {
                tx.remove(ACTIVE, &id_key(*id))?;
            }
            Ok(())
        })
    }

    /// Ids filed under an index key, ascending.
    pub fn ids_by_key(&self, key: &str) -> Result<Vec<u64>> {
        self.store.read(|tx| ids_under(tx, key))
    }

    /// Distinct index keys starting with `prefix` (e.g. every `a:` range).
    pub fn index_keys_with_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        self.store.read(|tx| {
            let mut keys = BTreeSet::new();
            tx.scan(INDEX, prefix, &mut |k, _| {
                if let Some((key, _)) = k.rsplit_once('|') {
                    keys.insert(key.to_string());
                }
                true
            })?;
            Ok(keys.into_iter().collect())
        })
    }

    pub fn count(&self) -> Result<u64> {
        self.store.read(|tx| tx.len(PUNISHMENTS))
    }

    pub fn player(&self, uuid: Uuid) -> Result<Option<PlayerRecord>> {
        self.store.get_json(PLAYERS, &uuid.to_string())
    }

    /// Writes a player record and points its name and addresses at it; the
    /// `forgotten` addresses are unlinked.
    pub fn put_player(&self, rec: &PlayerRecord, forgotten: &[IpAddr]) -> Result<()> {
        self.put_player_with(rec, forgotten, Durability::Immediate)
    }

    /// Like [`BansStore::put_player`]; logins are written with
    /// [`Durability::Eventual`] (a lost login record costs nothing, an fsync per
    /// login costs a lot under a bot flood).
    pub fn put_player_with(&self, rec: &PlayerRecord, forgotten: &[IpAddr], durability: Durability) -> Result<()> {
        let uuid = rec.uuid.to_string();
        self.store.write_with(durability, |tx| {
            tx.put_json(PLAYERS, &uuid, rec)?;
            tx.put(NAMES, &name_key(&rec.name), uuid.as_bytes())?;
            for a in &rec.addresses {
                tx.put(ADDRESSES, &format!("{}|{uuid}", ip_key(a.value)), b"")?;
            }
            for ip in forgotten {
                // Another remembered address in the same IPv6 /64 keeps the link.
                let key = ip_key(*ip);
                if !rec.addresses.iter().any(|a| ip_key(a.value) == key) {
                    tx.remove(ADDRESSES, &format!("{key}|{uuid}"))?;
                }
            }
            Ok(())
        })
    }

    pub fn uuid_by_name(&self, name: &str) -> Result<Option<Uuid>> {
        let raw = self.store.read(|tx| tx.get(NAMES, &name_key(name)))?;
        Ok(raw.and_then(|b| String::from_utf8(b).ok()).and_then(|s| Uuid::parse(&s)))
    }

    /// Accounts linked to an address (IPv6 by /64).
    pub fn uuids_by_address(&self, ip: IpAddr) -> Result<Vec<Uuid>> {
        let prefix = format!("{}|", ip_key(ip));
        self.store.read(|tx| {
            let mut out = Vec::new();
            tx.scan(ADDRESSES, &prefix, &mut |k, _| {
                if let Some(u) = k.get(prefix.len()..).and_then(Uuid::parse) {
                    out.push(u);
                }
                true
            })?;
            Ok(out)
        })
    }

    pub fn player_uuids(&self) -> Result<Vec<Uuid>> {
        self.store.read(|tx| {
            let mut out = Vec::new();
            tx.scan(PLAYERS, "", &mut |k, _| {
                if let Some(u) = Uuid::parse(k) {
                    out.push(u);
                }
                true
            })?;
            Ok(out)
        })
    }
}

fn ids_under(tx: &dyn ReadOps, key: &str) -> Result<Vec<u64>> {
    let prefix = format!("{key}|");
    let mut out = Vec::new();
    tx.scan(INDEX, &prefix, &mut |k, _| {
        if let Some(id) = k.get(prefix.len()..).and_then(|s| s.parse::<u64>().ok()) {
            out.push(id);
        }
        true
    })?;
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use pumbo_common::id::{Cidr, parse_ip};
    use pumbo_common::store::{Backend, MemoryBackend};

    use super::*;
    use crate::model::{Actor, Kind, Revocation, Target};

    /// Reads like memory, refuses every write: a broken disk.
    pub struct FailingWrites(pub MemoryBackend);

    impl Backend for FailingWrites {
        fn read(&self, f: &mut dyn FnMut(&dyn ReadOps) -> Result<()>) -> Result<()> {
            self.0.read(f)
        }

        fn write(&self, _: Durability, _: &mut dyn FnMut(&mut dyn WriteOps) -> Result<()>) -> Result<()> {
            Err(StoreError::new("simulated write failure"))
        }
    }

    fn punishment(target: Target) -> Punishment {
        Punishment {
            id: 0,
            kind: Kind::Ban,
            target,
            victim_name: Some("Steve".into()),
            victim_uuid: None,
            operator: Actor::console("Console"),
            reason: "r".into(),
            created: 10,
            expires: None,
            silent: false,
            template: None,
            scope: "*".into(),
            revoked: None,
            source: None,
        }
    }

    fn exercise(store: &BansStore) {
        let mut a = punishment(Target::player(Uuid(1)));
        store.insert(&mut a).unwrap();
        let mut b = punishment(Target::Address { net: Cidr::parse("10.0.0.0/8").unwrap() });
        store.insert(&mut b).unwrap();
        let mut c = punishment(Target::name("Griefer"));
        c.kind = Kind::Kick;
        store.insert(&mut c).unwrap();
        assert_eq!((a.id, b.id, c.id), (1, 2, 3));
        assert_eq!(store.count().unwrap(), 3);

        let active: Vec<u64> = store.active().unwrap().0.iter().map(|p| p.id).collect();
        assert_eq!(active, vec![1, 2]);
        assert_eq!(store.ids_by_key("u:00000000-0000-0000-0000-000000000001").unwrap(), vec![1]);
        assert_eq!(store.index_keys_with_prefix("a:").unwrap(), vec!["a:10.0.0.0/8".to_string()]);
        assert_eq!(store.ids_by_key("o:~console").unwrap(), vec![1, 2, 3]);

        a.revoked = Some(Revocation { by: Actor::console("Console"), at: 20, reason: None });
        store.update(&a).unwrap();
        let active: Vec<u64> = store.active().unwrap().0.iter().map(|p| p.id).collect();
        assert_eq!(active, vec![2]);
        assert_eq!(store.get(1).unwrap().unwrap().revoked.unwrap().at, 20);

        c.victim_uuid = Some(Uuid(9));
        store.update(&c).unwrap();
        assert_eq!(store.ids_by_key("v:00000000-0000-0000-0000-000000000009").unwrap(), vec![3]);
        store.retire(&[2]).unwrap();
        assert!(store.active().unwrap().0.is_empty());
        assert!(store.get(2).unwrap().is_some());
        let mut missing = punishment(Target::player(Uuid(5)));
        missing.id = 99;
        assert!(store.update(&missing).is_err());

        let ip1 = parse_ip("1.1.1.1").unwrap();
        let ip2 = parse_ip("2.2.2.2").unwrap();
        let mut rec = PlayerRecord::new(Uuid(1), "Steve", 1);
        rec.touch("Steve", Some(ip1), 1);
        rec.touch("Steve", Some(ip2), 2);
        store.put_player(&rec, &[]).unwrap();
        let mut other = PlayerRecord::new(Uuid(2), "Alex", 1);
        other.touch("Alex", Some(ip1), 3);
        store.put_player(&other, &[]).unwrap();
        assert_eq!(store.uuid_by_name("STEVE").unwrap(), Some(Uuid(1)));
        let mut sharing = store.uuids_by_address(ip1).unwrap();
        sharing.sort();
        assert_eq!(sharing, vec![Uuid(1), Uuid(2)]);
        rec.addresses.retain(|s| s.value != ip1);
        store.put_player(&rec, &[ip1]).unwrap();
        assert_eq!(store.uuids_by_address(ip1).unwrap(), vec![Uuid(2)]);
        assert_eq!(store.player(Uuid(1)).unwrap().unwrap().addresses.len(), 1);
        let mut all = store.player_uuids().unwrap();
        all.sort();
        assert_eq!(all, vec![Uuid(1), Uuid(2)]);

        // IPv6 addresses link by /64.
        let v6a = parse_ip("2001:db8:0:1::1").unwrap();
        let v6b = parse_ip("2001:db8:0:1::2").unwrap();
        let mut r3 = PlayerRecord::new(Uuid(3), "V6", 1);
        r3.touch("V6", Some(v6a), 1);
        store.put_player(&r3, &[]).unwrap();
        assert_eq!(store.uuids_by_address(v6b).unwrap(), vec![Uuid(3)]);
    }

    #[test]
    fn memory() {
        exercise(&BansStore::new(Store::in_memory()).unwrap());
    }

    #[test]
    fn redb_and_reopen() {
        let path = std::env::temp_dir().join(format!("pumbo-bans-store-{}.redb", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let store = BansStore::new(Store::open(&path).unwrap()).unwrap();
            exercise(&store);
        }
        let store = BansStore::new(Store::open(&path).unwrap()).unwrap();
        assert_eq!(store.count().unwrap(), 3);
        let mut p = punishment(Target::player(Uuid(3)));
        store.insert(&mut p).unwrap();
        assert_eq!(p.id, 4);
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn newer_schema_is_refused() {
        let store = Store::in_memory();
        store.write(|tx| tx.put_u64(META, "schema", SCHEMA + 1)).unwrap();
        assert!(BansStore::new(store).is_err());
    }
}
