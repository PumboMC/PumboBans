//! The punishment engine: decides who may join and who may talk, places and
//! lifts punishments, escalates warnings and answers history queries.
//!
//! Active punishments are cached in memory (loaded once at start, kept in step
//! with every write), so the login and chat checks need no database read except
//! the player's own record. The database stays the source of truth: a write that
//! fails changes nothing in the cache either.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use pumbo_common::id::{Cidr, Uuid, name_key};
use pumbo_common::lang::Lang;
use pumbo_common::rich::{Click, Text};
use pumbo_common::store::{Durability, StoreError};
use pumbo_common::style;
use pumbo_common::text::{Args, escape};
use pumbo_common::time::Term;

use crate::config::{BansCfg, Strictness, parse_length, within};
use crate::model::{Actor, Kind, PlayerRecord, Punishment, Revocation, Target};
use crate::net;
use crate::render::{self, Langs};
use crate::store::BansStore;

const DAY: u64 = 86_400_000;

/// Most accounts looked at through shared addresses for one check (`stern`,
/// `strict`, alt notices). Protects logins from huge shared NATs.
pub const MAX_LINKED_ACCOUNTS: usize = 64;

/// Who is connecting or talking.
#[derive(Clone, Debug, Default)]
pub struct Subject {
    pub uuid: Option<Uuid>,
    pub name: Option<String>,
    pub ip: Option<IpAddr>,
}

impl Subject {
    pub fn new(uuid: Uuid, name: &str, ip: Option<IpAddr>) -> Subject {
        Subject { uuid: Some(uuid), name: Some(name.to_string()), ip }
    }
}

/// A resolved punishment target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// An account; `exempt` are the exemptions it has (from the live permission
    /// check when online, from the last login otherwise).
    Player { uuid: Uuid, name: String, exempt: Vec<Kind> },
    /// An address range, optionally placed through a player.
    Address { net: Cidr, via: Option<(Uuid, String)> },
    /// A name that never joined.
    Name { name: String },
}

impl Resolved {
    pub fn label(&self) -> String {
        match self {
            Resolved::Player { name, .. } => name.clone(),
            Resolved::Address { net, via: Some((_, n)) } => format!("{n} ({})", net::label(net)),
            Resolved::Address { net, via: None } => net::label(net),
            Resolved::Name { name } => name.clone(),
        }
    }

    pub fn uuid(&self) -> Option<Uuid> {
        match self {
            Resolved::Player { uuid, .. } => Some(*uuid),
            Resolved::Address { via, .. } => via.as_ref().map(|(u, _)| *u),
            Resolved::Name { .. } => None,
        }
    }

    fn target(&self) -> Target {
        match self {
            Resolved::Player { uuid, .. } => Target::player(*uuid),
            Resolved::Address { net, .. } => Target::Address { net: *net },
            Resolved::Name { name } => Target::name(name),
        }
    }
}

/// A request to punish.
#[derive(Clone, Debug)]
pub struct PunishRequest {
    pub kind: Kind,
    pub target: Resolved,
    /// `None`: permanent for bans and mutes, the configured length for warnings.
    pub length: Option<Term>,
    pub reason: Option<String>,
    /// Template name without `#`.
    pub template: Option<String>,
    pub silent: bool,
    pub operator: Actor,
    pub bypass_exempt: bool,
    /// Longest length the operator may give (`None` = unlimited).
    pub length_limit: Option<Term>,
    pub source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PunishError {
    Exempt,
    AlreadyPunished(Box<Punishment>),
    UnknownTemplate(String),
    ReasonRequired,
    LimitExceeded(Term),
    Storage(String),
}

/// A placed punishment and what followed from it.
#[derive(Clone, Debug)]
pub struct Punished {
    pub punishment: Punishment,
    /// Punishments replaced by this one (`allow-override`).
    pub replaced: Vec<Punishment>,
    /// For warnings: the number of active warnings including this one.
    pub warn_count: usize,
    /// Punishment placed by warning escalation.
    pub escalated: Option<Punishment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevokeError {
    NotPunished,
    NoSuchId,
    WrongKind(Box<Punishment>),
    Storage(String),
}

/// Result of a login check.
#[derive(Clone, Debug)]
pub enum LoginDecision {
    Allow {
        /// Notice for staff: the player shares an address with banned accounts.
        alt_notice: Option<Text>,
    },
    Deny {
        screen: Text,
        punishment: Option<Box<Punishment>>,
    },
}

/// An account that shares an address with another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alt {
    pub uuid: Uuid,
    pub name: String,
    pub address: IpAddr,
    pub banned: bool,
    pub muted: bool,
}

/// An entry to import (vanilla lists or other plugins).
#[derive(Clone, Debug)]
pub struct ImportEntry {
    pub kind: Kind,
    pub target: Target,
    pub name: Option<String>,
    pub operator: String,
    pub reason: String,
    pub created: u64,
    pub expires: Option<u64>,
    pub source: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub added: usize,
    pub skipped: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub expired: usize,
    pub addresses_forgotten: usize,
}

pub struct Engine {
    pub cfg: BansCfg,
    pub langs: Langs,
    store: Option<BansStore>,
    /// Lasting punishments that are not lifted (some may have expired since).
    active: BTreeMap<u64, Punishment>,
    /// Why the store is missing, for `/pumbobans version` and logs.
    pub storage_error: Option<String>,
    /// Human-readable platform, e.g. "Pumpkin 0.2.0 / MC 26.3".
    pub platform: String,
}

impl Engine {
    /// Builds the engine. A store that failed to open leaves the engine in the
    /// fail-closed state (see `enforcement.fail-closed`).
    pub fn new(
        cfg: BansCfg,
        langs: Langs,
        store: Result<BansStore, StoreError>,
        platform: &str,
    ) -> (Engine, Vec<String>) {
        let mut warnings = Vec::new();
        let (store, storage_error) = match store {
            Ok(s) => (Some(s), None),
            Err(e) => {
                warnings.push(format!("cannot open the database: {e}"));
                (None, Some(e.0))
            }
        };
        let mut engine =
            Engine { cfg, langs, store, active: BTreeMap::new(), storage_error, platform: platform.to_string() };
        if let Some(store) = &engine.store {
            match store.active() {
                Ok((list, damaged)) => {
                    if damaged > 0 {
                        warnings.push(format!("{damaged} active punishments could not be read and are ignored"));
                    }
                    engine.active = list.into_iter().map(|p| (p.id, p)).collect();
                }
                Err(e) => {
                    warnings.push(format!("cannot read active punishments: {e}"));
                    engine.storage_error = Some(e.0);
                    engine.store = None;
                }
            }
        }
        (engine, warnings)
    }

    pub fn storage_ok(&self) -> bool {
        self.store.is_some()
    }

    pub fn lang(&self) -> &Lang {
        &self.langs.default
    }

    pub fn lang_for(&self, locale: Option<&str>) -> &Lang {
        self.langs.for_locale(locale)
    }

    pub fn active_count(&self, now: u64) -> usize {
        self.active.values().filter(|p| p.is_active(now)).count()
    }

    fn store(&self) -> Result<&BansStore, StoreError> {
        self.store.as_ref().ok_or_else(|| StoreError::new("the database is unavailable"))
    }

    fn store_mut(&mut self) -> Result<&BansStore, StoreError> {
        self.store()
    }

    /// Replaces config and messages (`/pumbo bans reload`); the database stays.
    pub fn reconfigure(&mut self, cfg: BansCfg, langs: Langs) {
        self.cfg = cfg;
        self.langs = langs;
    }

    fn retention_ms(&self) -> Option<u64> {
        match self.cfg.privacy.address_retention_days {
            0 => None,
            d => Some(u64::from(d) * DAY),
        }
    }

    // ------------------------------------------------------------------
    // Players

    pub fn player(&self, uuid: Uuid) -> Result<Option<PlayerRecord>, StoreError> {
        self.store()?.player(uuid)
    }

    pub fn uuid_by_name(&self, name: &str) -> Result<Option<Uuid>, StoreError> {
        self.store()?.uuid_by_name(name)
    }

    /// Addresses of a record that are still within the retention period.
    fn live_addresses(&self, rec: &PlayerRecord, now: u64) -> Vec<IpAddr> {
        let keep = self.retention_ms();
        rec.addresses.iter().filter(|s| keep.is_none_or(|k| now.saturating_sub(s.last) <= k)).map(|s| s.value).collect()
    }

    /// Notes a login of an account from an address.
    pub fn record_login(&mut self, uuid: Uuid, name: &str, ip: Option<IpAddr>, now: u64) -> Result<(), StoreError> {
        let store = self.store_mut()?;
        let mut rec = store.player(uuid)?.unwrap_or_else(|| PlayerRecord::new(uuid, name, now));
        rec.touch(name, ip, now);
        store.put_player_with(&rec, &[], Durability::Eventual)
    }

    /// Stores which exemption permissions an online player has (so punishments
    /// placed while the player is offline respect them) and the client language
    /// (for the ban screen at the next login). Writes only on change.
    pub fn note_join(
        &mut self,
        uuid: Uuid,
        name: &str,
        exempt: Vec<Kind>,
        locale: Option<String>,
        now: u64,
    ) -> Result<(), StoreError> {
        let store = self.store_mut()?;
        let existing = store.player(uuid)?;
        let mut exempt = exempt;
        exempt.sort();
        exempt.dedup();
        if existing.as_ref().is_some_and(|r| r.exempt == exempt && (locale.is_none() || r.locale == locale)) {
            return Ok(());
        }
        let mut rec = existing.unwrap_or_else(|| PlayerRecord::new(uuid, name, now));
        rec.exempt = exempt;
        if locale.is_some() {
            rec.locale = locale;
        }
        store.put_player(&rec, &[])
    }

    // ------------------------------------------------------------------
    // Enforcement

    /// Accounts that share an address with `addresses` (excluding `uuid`).
    fn linked_accounts(&self, uuid: Option<Uuid>, addresses: &[IpAddr]) -> Result<Vec<Uuid>, StoreError> {
        let store = self.store()?;
        let mut out = BTreeSet::new();
        for ip in addresses {
            for u in store.uuids_by_address(*ip)? {
                if Some(u) != uuid {
                    out.insert(u);
                }
                if out.len() >= MAX_LINKED_ACCOUNTS {
                    return Ok(out.into_iter().collect());
                }
            }
        }
        Ok(out.into_iter().collect())
    }

    /// The punishment of `kind` that applies to `s` right now, following the
    /// configured address strictness. With several, the strongest one.
    pub fn find_active(&self, kind: Kind, s: &Subject, now: u64) -> Result<Option<Punishment>, StoreError> {
        let candidates: Vec<&Punishment> =
            self.active.values().filter(|p| p.kind == kind && p.is_active(now)).collect();
        if candidates.is_empty() {
            return Ok(None);
        }
        let mut best: Option<&Punishment> = None;
        fn keep<'a>(best: &mut Option<&'a Punishment>, p: &'a Punishment) {
            if best.is_none_or(|b| p.strength() > b.strength()) {
                *best = Some(p);
            }
        }
        let uuid = match (s.uuid, &s.name) {
            (Some(u), _) => Some(u),
            (None, Some(n)) => self.store.as_ref().and_then(|st| st.uuid_by_name(n).ok().flatten()),
            (None, None) => None,
        };
        let name = s.name.clone().unwrap_or_default();

        // The account itself, the name, and the address it connects from.
        for p in &candidates {
            let hit = match &p.target {
                Target::Player { uuid: u } => Some(*u) == uuid,
                Target::Name { name: n } => {
                    (!name.is_empty() && *n == name_key(&name)) || (uuid.is_some() && p.victim_uuid == uuid)
                }
                Target::Address { net } => s.ip.is_some_and(|ip| net.contains(ip)),
            };
            if hit {
                keep(&mut best, p);
            }
        }

        let strictness = self.cfg.enforcement.address_strictness;
        let has_address = candidates.iter().any(|p| p.target.is_address());
        let needs_alts = matches!(strictness, Strictness::Stern | Strictness::Strict);
        if strictness == Strictness::Lenient || self.store.is_none() || (!has_address && !needs_alts) {
            return Ok(best.cloned());
        }

        // normal: every address this account has used.
        let store = self.store()?;
        let mut own: Vec<IpAddr> = Vec::new();
        if let Some(u) = uuid
            && let Some(rec) = store.player(u)?
        {
            own = self.live_addresses(&rec, now);
        }
        if let Some(ip) = s.ip
            && !own.contains(&ip)
        {
            own.push(ip);
        }
        fn by_address<'a>(candidates: &[&'a Punishment], addrs: &[IpAddr], best: &mut Option<&'a Punishment>) {
            for p in candidates {
                if let Target::Address { net } = &p.target
                    && addrs.iter().any(|a| net.contains(*a))
                {
                    keep(best, p);
                }
            }
        }
        by_address(&candidates, &own, &mut best);
        if !needs_alts {
            return Ok(best.cloned());
        }

        // stern: addresses of the accounts that share an address with this one;
        // strict: also the account punishments of those accounts.
        for alt in self.linked_accounts(uuid, &own)? {
            let Some(rec) = store.player(alt)? else { continue };
            by_address(&candidates, &self.live_addresses(&rec, now), &mut best);
            if strictness == Strictness::Strict {
                for p in &candidates {
                    if p.targets_account(alt, &rec.name) {
                        keep(&mut best, p);
                    }
                }
            }
        }
        Ok(best.cloned())
    }

    /// Login check, done before the player enters the world. Records the login
    /// (name and address) for history, alt detection and `normal` strictness.
    pub fn check_login(&mut self, uuid: Uuid, name: &str, ip: Option<IpAddr>, now: u64) -> LoginDecision {
        if self.store.is_none() {
            return if self.cfg.enforcement.fail_closed {
                LoginDecision::Deny { screen: render::unavailable_screen(self.lang()), punishment: None }
            } else {
                LoginDecision::Allow { alt_notice: None }
            };
        }
        let subject = Subject::new(uuid, name, ip);
        let found = match self.find_active(Kind::Ban, &subject, now) {
            Ok(f) => f,
            Err(_) if self.cfg.enforcement.fail_closed => {
                return LoginDecision::Deny { screen: render::unavailable_screen(self.lang()), punishment: None };
            }
            Err(_) => None,
        };
        if let Some(mut p) = found {
            // A name punishment sticks to the first account that tries the name.
            if matches!(p.target, Target::Name { .. }) && p.victim_uuid.is_none() {
                p.victim_uuid = Some(uuid);
                if let Ok(store) = self.store_mut()
                    && store.update(&p).is_ok()
                {
                    self.active.insert(p.id, p.clone());
                }
            }
            if self.cfg.privacy.record_refused_logins {
                let _ = self.record_login(uuid, name, ip, now);
            }
            // The language the player used last time, if known.
            let locale = self.store.as_ref().and_then(|st| st.player(uuid).ok().flatten()).and_then(|r| r.locale);
            let screen = render::ban_screen(self.lang_for(locale.as_deref()), &self.cfg, &p, now);
            return LoginDecision::Deny { screen, punishment: Some(Box::new(p)) };
        }
        let _ = self.record_login(uuid, name, ip, now);
        let alt_notice = if self.cfg.notifications.alt_join { self.alt_notice(uuid, name, now) } else { None };
        LoginDecision::Allow { alt_notice }
    }

    /// Check of a connection before its account is known (PumboProx pre-login,
    /// before encryption): bans of the address it connects from. Account and
    /// name bans follow in [`Engine::check_login`] once the UUID is known, so
    /// that a name ban can still remember the account.
    pub fn check_address(&self, name: &str, ip: Option<IpAddr>, now: u64) -> LoginDecision {
        if self.store.is_none() {
            return if self.cfg.enforcement.fail_closed {
                LoginDecision::Deny { screen: render::unavailable_screen(self.lang()), punishment: None }
            } else {
                LoginDecision::Allow { alt_notice: None }
            };
        }
        let mut best: Option<&Punishment> = None;
        if let Some(ip) = ip {
            for p in self.active.values().filter(|p| p.kind == Kind::Ban && p.is_active(now) && p.covers_address(ip)) {
                if best.is_none_or(|b| p.strength() > b.strength()) {
                    best = Some(p);
                }
            }
        }
        let Some(p) = best else { return LoginDecision::Allow { alt_notice: None } };
        let locale = self
            .store
            .as_ref()
            .and_then(|st| st.uuid_by_name(name).ok().flatten().and_then(|u| st.player(u).ok().flatten()))
            .and_then(|r| r.locale);
        let screen = render::ban_screen(self.lang_for(locale.as_deref()), &self.cfg, p, now);
        LoginDecision::Deny { screen, punishment: Some(Box::new(p.clone())) }
    }

    /// "X joined from an address used by banned Y, Z".
    fn alt_notice(&self, uuid: Uuid, name: &str, now: u64) -> Option<Text> {
        let alts = self.alts(uuid, now).ok()?;
        let banned: Vec<String> = alts.iter().filter(|a| a.banned).map(|a| a.name.clone()).collect();
        if banned.is_empty() {
            return None;
        }
        let args = Args::new().with("player", escape(name)).with("accounts", escape(&banned.join(", ")));
        let text = style::info(self.lang(), "notify-alt-join", &args);
        let click = Click::Suggest(format!("/{} alts {name}", crate::commands::ALIAS));
        Some(Text { lines: text.lines.into_iter().map(|l| l.on_click(click.clone())).collect() })
    }

    /// The active mute of a player, if any.
    pub fn mute_for(&self, s: &Subject, now: u64) -> Option<Punishment> {
        // A broken database does not silence everybody: the cache still answers.
        self.find_active(Kind::Mute, s, now).ok().flatten()
    }

    // ------------------------------------------------------------------
    // Placing punishments

    fn same_target_active(&self, kind: Kind, target: &Resolved, now: u64) -> Vec<Punishment> {
        let t = target.target();
        self.active
            .values()
            .filter(|p| p.kind == kind && p.is_active(now))
            .filter(|p| match (&t, target) {
                (Target::Player { uuid }, Resolved::Player { name, .. }) => p.targets_account(*uuid, name),
                (Target::Address { net }, _) => match &p.target {
                    Target::Address { net: other } => other == net,
                    _ => false,
                },
                (Target::Name { name }, _) => match &p.target {
                    Target::Name { name: other } => other == name,
                    _ => false,
                },
                _ => false,
            })
            .cloned()
            .collect()
    }

    /// How many earlier punishments with this template the target has (lifted
    /// ones do not count), which picks the step on the template's ladder.
    fn template_step(&self, kind: Kind, target: &Resolved, template: &str) -> Result<usize, StoreError> {
        let history = self.history_for_resolved(target)?;
        Ok(history
            .iter()
            .filter(|p| p.kind == kind && p.revoked.is_none() && p.template.as_deref() == Some(template))
            .count())
    }

    pub fn punish(&mut self, req: PunishRequest, now: u64) -> Result<Punished, PunishError> {
        if self.store.is_none() {
            return Err(PunishError::Storage("the database is unavailable".into()));
        }
        // Exemptions (accounts only; addresses and names have no permissions).
        if let Resolved::Player { exempt, .. } = &req.target
            && exempt.contains(&req.kind)
            && !req.bypass_exempt
        {
            return Err(PunishError::Exempt);
        }

        let mut reason = req.reason.clone().map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        let mut length = req.length;
        if let Some(name) = &req.template {
            let Some(t) = self.cfg.template(name).cloned() else {
                return Err(PunishError::UnknownTemplate(name.clone()));
            };
            reason = Some(match reason {
                Some(extra) => format!("{} ({extra})", t.reason),
                None => t.reason.clone(),
            });
            if length.is_none() && !t.durations.is_empty() {
                let step = self.template_step(req.kind, &req.target, &t.name).map_err(|e| PunishError::Storage(e.0))?;
                let idx = step.min(t.durations.len().saturating_sub(1));
                length = t.durations.get(idx).and_then(|d| parse_length(d));
            }
        }
        if reason.is_none() && self.cfg.punishments.require_reason {
            return Err(PunishError::ReasonRequired);
        }

        let length = match req.kind {
            Kind::Kick => Term::Limited(std::time::Duration::ZERO),
            Kind::Warn => length.unwrap_or_else(|| self.cfg.warning_length()),
            Kind::Ban | Kind::Mute => length.unwrap_or(Term::Permanent),
        };
        if req.kind != Kind::Kick
            && let Some(limit) = req.length_limit
            && !within(length, limit)
        {
            return Err(PunishError::LimitExceeded(limit));
        }

        let mut replaced = Vec::new();
        if matches!(req.kind, Kind::Ban | Kind::Mute) {
            let existing = self.same_target_active(req.kind, &req.target, now);
            if let Some(first) = existing.first() {
                if !self.cfg.punishments.allow_override {
                    return Err(PunishError::AlreadyPunished(Box::new(first.clone())));
                }
                for mut old in existing {
                    old.revoked =
                        Some(Revocation { by: req.operator.clone(), at: now, reason: Some("replaced".into()) });
                    self.store_mut().and_then(|s| s.update(&old)).map_err(|e| PunishError::Storage(e.0))?;
                    self.active.remove(&old.id);
                    replaced.push(old);
                }
            }
        }

        let (victim_name, victim_uuid) = match &req.target {
            Resolved::Player { name, .. } => (Some(name.clone()), None),
            Resolved::Address { via, .. } => (via.as_ref().map(|(_, n)| n.clone()), via.as_ref().map(|(u, _)| *u)),
            Resolved::Name { name } => (Some(name.clone()), None),
        };
        let mut p = Punishment {
            id: 0,
            kind: req.kind,
            target: req.target.target(),
            victim_name,
            victim_uuid,
            operator: req.operator.clone(),
            reason: reason.unwrap_or_default(),
            created: now,
            expires: if req.kind == Kind::Kick { Some(now) } else { length.expires_at(now) },
            silent: req.silent,
            template: req.template.as_ref().map(|t| t.trim_start_matches('#').to_lowercase()),
            scope: "*".into(),
            revoked: None,
            source: req.source.clone(),
        };
        self.store_mut().and_then(|s| s.insert(&mut p)).map_err(|e| PunishError::Storage(e.0))?;
        if p.kind.is_lasting() {
            self.active.insert(p.id, p.clone());
        }

        let mut warn_count = 0;
        let mut escalated = None;
        if p.kind == Kind::Warn {
            warn_count = self.active_warns_for(&req.target, now).len();
            escalated = self.escalate(&req, warn_count, now);
        }
        Ok(Punished { punishment: p, replaced, warn_count, escalated })
    }

    /// Places the escalation step for `count` active warnings, if one is configured.
    fn escalate(&mut self, req: &PunishRequest, count: usize, now: u64) -> Option<Punishment> {
        let steps = &self.cfg.warnings.escalation;
        let exact = steps.iter().find(|s| s.count as usize == count);
        let step = match exact {
            Some(s) => s.clone(),
            None => {
                let highest = steps.last()?;
                if self.cfg.warnings.repeat_highest && count > highest.count as usize {
                    highest.clone()
                } else {
                    return None;
                }
            }
        };
        let kind = Kind::parse(&step.action)?;
        let length = if step.duration.is_empty() { Some(Term::Permanent) } else { parse_length(&step.duration) };
        let follow = PunishRequest {
            kind,
            target: req.target.clone(),
            length,
            reason: Some(step.reason.clone()).filter(|r| !r.is_empty()),
            template: None,
            silent: req.silent,
            operator: req.operator.clone(),
            // The warning itself already passed the exemption check.
            bypass_exempt: true,
            length_limit: None,
            source: Some("escalation".into()),
        };
        // An escalation onto someone who is already banned or muted is skipped.
        self.punish(follow, now).ok().map(|r| r.punishment)
    }

    // ------------------------------------------------------------------
    // Lifting punishments

    fn revoke_one(
        &mut self,
        mut p: Punishment,
        by: &Actor,
        reason: Option<String>,
        now: u64,
    ) -> Result<Punishment, StoreError> {
        p.revoked = Some(Revocation { by: by.clone(), at: now, reason });
        self.store_mut()?.update(&p)?;
        self.active.remove(&p.id);
        Ok(p)
    }

    /// Lifts the active punishments of `kind` on a target: an account (its UUID
    /// and name punishments, and address punishments placed through it), an
    /// address (every range that covers it) or a name.
    pub fn revoke(
        &mut self,
        kind: Kind,
        target: &Resolved,
        by: &Actor,
        reason: Option<String>,
        now: u64,
    ) -> Result<Vec<Punishment>, RevokeError> {
        if self.store.is_none() {
            return Err(RevokeError::Storage("the database is unavailable".into()));
        }
        let matching: Vec<Punishment> = self
            .active
            .values()
            .filter(|p| p.kind == kind && p.is_active(now))
            .filter(|p| match target {
                Resolved::Player { uuid, name, .. } => {
                    p.targets_account(*uuid, name) || (p.target.is_address() && p.victim_uuid == Some(*uuid))
                }
                Resolved::Address { net, .. } => match &p.target {
                    Target::Address { net: other } => net::overlaps(other, net),
                    _ => false,
                },
                Resolved::Name { name } => match &p.target {
                    Target::Name { name: n } => *n == name_key(name),
                    _ => false,
                },
            })
            .cloned()
            .collect();
        if matching.is_empty() {
            return Err(RevokeError::NotPunished);
        }
        let mut out = Vec::new();
        for p in matching {
            out.push(self.revoke_one(p, by, reason.clone(), now).map_err(|e| RevokeError::Storage(e.0))?);
        }
        Ok(out)
    }

    /// Lifts one punishment by id (`expect` limits it to a kind).
    pub fn revoke_id(
        &mut self,
        id: u64,
        expect: Option<Kind>,
        by: &Actor,
        reason: Option<String>,
        now: u64,
    ) -> Result<Punishment, RevokeError> {
        let p = self.store().map_err(|e| RevokeError::Storage(e.0))?.get(id).map_err(|e| RevokeError::Storage(e.0))?;
        let Some(p) = p else { return Err(RevokeError::NoSuchId) };
        if expect.is_some_and(|k| k != p.kind) {
            return Err(RevokeError::WrongKind(Box::new(p)));
        }
        if !p.is_active(now) {
            return Err(RevokeError::NotPunished);
        }
        self.revoke_one(p, by, reason, now).map_err(|e| RevokeError::Storage(e.0))
    }

    // ------------------------------------------------------------------
    // Queries

    pub fn get(&self, id: u64) -> Result<Option<Punishment>, StoreError> {
        self.store()?.get(id)
    }

    fn collect_ids(&self, keys: &[String]) -> Result<Vec<Punishment>, StoreError> {
        let store = self.store()?;
        let mut ids = BTreeSet::new();
        for k in keys {
            ids.extend(store.ids_by_key(k)?);
        }
        let mut out = Vec::new();
        for id in ids.into_iter().rev() {
            if let Some(p) = store.get(id)? {
                out.push(p);
            }
        }
        Ok(out)
    }

    /// Everything filed against an account: its UUID, punishments linked to it
    /// and punishments of every name it used. Newest first.
    pub fn history_of_player(&self, uuid: Uuid) -> Result<Vec<Punishment>, StoreError> {
        let mut keys = vec![format!("u:{uuid}"), format!("v:{uuid}")];
        if let Some(rec) = self.store()?.player(uuid)? {
            keys.push(format!("n:{}", name_key(&rec.name)));
            for n in &rec.names {
                keys.push(format!("n:{}", name_key(&n.value)));
            }
        }
        self.collect_ids(&keys)
    }

    /// Every address punishment whose range covers or overlaps `net`.
    pub fn history_of_address(&self, range: &Cidr) -> Result<Vec<Punishment>, StoreError> {
        let keys: Vec<String> = self
            .store()?
            .index_keys_with_prefix("a:")?
            .into_iter()
            .filter(|k| k.get(2..).and_then(Cidr::parse).is_some_and(|n| net::overlaps(&n, range)))
            .collect();
        self.collect_ids(&keys)
    }

    pub fn history_of_name(&self, name: &str) -> Result<Vec<Punishment>, StoreError> {
        self.collect_ids(&[format!("n:{}", name_key(name))])
    }

    /// Punishments given by an operator, newest first.
    pub fn history_of_operator(&self, op: &Actor) -> Result<Vec<Punishment>, StoreError> {
        self.collect_ids(&[op.key()])
    }

    pub fn history_for_resolved(&self, target: &Resolved) -> Result<Vec<Punishment>, StoreError> {
        match target {
            Resolved::Player { uuid, .. } => self.history_of_player(*uuid),
            Resolved::Address { net, .. } => self.history_of_address(net),
            Resolved::Name { name } => self.history_of_name(name),
        }
    }

    /// Active punishments of a kind, newest first.
    pub fn active_list(&self, kind: Kind, now: u64) -> Vec<Punishment> {
        self.active.values().rev().filter(|p| p.kind == kind && p.is_active(now)).cloned().collect()
    }

    /// Active warnings aimed at a target, newest first.
    pub fn active_warns_for(&self, target: &Resolved, now: u64) -> Vec<Punishment> {
        self.active
            .values()
            .rev()
            .filter(|p| p.kind == Kind::Warn && p.is_active(now))
            .filter(|p| match target {
                Resolved::Player { uuid, name, .. } => p.targets_account(*uuid, name),
                Resolved::Address { net, .. } => matches!(&p.target, Target::Address { net: n } if n == net),
                Resolved::Name { name } => matches!(&p.target, Target::Name { name: n } if *n == name_key(name)),
            })
            .cloned()
            .collect()
    }

    /// Accounts that share a (still remembered) address with `uuid`.
    pub fn alts(&self, uuid: Uuid, now: u64) -> Result<Vec<Alt>, StoreError> {
        let store = self.store()?;
        let Some(rec) = store.player(uuid)? else { return Ok(Vec::new()) };
        let mut out: Vec<Alt> = Vec::new();
        for ip in self.live_addresses(&rec, now) {
            for other in store.uuids_by_address(ip)? {
                if other == uuid || out.iter().any(|a| a.uuid == other) {
                    continue;
                }
                let Some(orec) = store.player(other)? else { continue };
                // The other side must still remember the address too.
                if !self.live_addresses(&orec, now).contains(&ip) {
                    continue;
                }
                let s = Subject::new(other, &orec.name, None);
                let direct = |kind: Kind| {
                    self.active
                        .values()
                        .any(|p| p.kind == kind && p.is_active(now) && p.targets_account(other, &orec.name))
                        || self.find_active(kind, &s, now).ok().flatten().is_some()
                };
                out.push(Alt {
                    uuid: other,
                    name: orec.name.clone(),
                    address: ip,
                    banned: direct(Kind::Ban),
                    muted: direct(Kind::Mute),
                });
                if out.len() >= MAX_LINKED_ACCOUNTS {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------------
    // Maintenance

    /// Retires expired punishments from the active set and forgets addresses
    /// past the retention period. Cheap enough to run every few minutes.
    pub fn sweep(&mut self, now: u64, forget_addresses: bool) -> Result<SweepReport, StoreError> {
        let mut report = SweepReport::default();
        let expired: Vec<u64> = self.active.values().filter(|p| !p.is_active(now)).map(|p| p.id).collect();
        if !expired.is_empty() {
            self.store_mut()?.retire(&expired)?;
            for id in &expired {
                self.active.remove(id);
            }
            report.expired = expired.len();
        }
        if forget_addresses && let Some(keep) = self.retention_ms() {
            let uuids = self.store()?.player_uuids()?;
            for u in uuids {
                let Some(mut rec) = self.store()?.player(u)? else { continue };
                let old: Vec<IpAddr> =
                    rec.addresses.iter().filter(|s| now.saturating_sub(s.last) > keep).map(|s| s.value).collect();
                if old.is_empty() {
                    continue;
                }
                rec.addresses.retain(|s| !old.contains(&s.value));
                self.store_mut()?.put_player(&rec, &old)?;
                report.addresses_forgotten += old.len();
            }
        }
        Ok(report)
    }

    /// Forgets every address of an account (GDPR erasure request). Punishments
    /// of addresses stay: they are records of what was done.
    pub fn purge_addresses(&mut self, uuid: Uuid) -> Result<usize, StoreError> {
        let Some(mut rec) = self.store()?.player(uuid)? else { return Ok(0) };
        let old: Vec<IpAddr> = rec.addresses.iter().map(|s| s.value).collect();
        rec.addresses.clear();
        self.store_mut()?.put_player(&rec, &old)?;
        Ok(old.len())
    }

    /// Imports punishments; skips expired ones and targets that already have an
    /// active punishment of the same kind.
    pub fn import(&mut self, entries: Vec<ImportEntry>, now: u64) -> Result<ImportReport, StoreError> {
        let mut report = ImportReport::default();
        for e in entries {
            if e.expires.is_some_and(|x| x <= now) {
                report.skipped += 1;
                continue;
            }
            let duplicate = self.active.values().any(|p| p.kind == e.kind && p.is_active(now) && p.target == e.target);
            if duplicate {
                report.skipped += 1;
                continue;
            }
            let mut p = Punishment {
                id: 0,
                kind: e.kind,
                target: e.target,
                victim_name: e.name,
                victim_uuid: None,
                operator: Actor::console(&e.operator),
                reason: e.reason,
                created: e.created,
                expires: e.expires,
                silent: true,
                template: None,
                scope: "*".into(),
                revoked: None,
                source: Some(e.source),
            };
            self.store_mut()?.insert(&mut p)?;
            if p.kind.is_lasting() {
                self.active.insert(p.id, p);
            }
            report.added += 1;
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pumbo_common::config::load;
    use pumbo_common::id::parse_ip;
    use pumbo_common::lang::COMMON;
    use pumbo_common::store::{MemoryBackend, Store};

    use super::*;
    use crate::store::tests::FailingWrites;

    const MINUTE: u64 = 60_000;
    const HOUR: u64 = 3_600_000;

    fn english() -> Lang {
        Lang::load(&[COMMON, crate::LANG], "en", None).0
    }

    fn ms(n: u64) -> Term {
        Term::Limited(Duration::from_millis(n))
    }

    const T0: u64 = 1_800_000_000_000;

    fn engine_with(cfg_text: &str) -> Engine {
        let (cfg, w) = load::<BansCfg>(cfg_text);
        assert!(w.is_empty(), "{w:?}");
        Engine::new(cfg, Langs::single(english()), Ok(BansStore::in_memory()), "test").0
    }

    fn engine() -> Engine {
        engine_with("")
    }

    fn uuid(n: u128) -> Uuid {
        Uuid(n)
    }

    fn ip(s: &str) -> Option<IpAddr> {
        parse_ip(s)
    }

    fn player(n: u128, name: &str) -> Resolved {
        Resolved::Player { uuid: uuid(n), name: name.into(), exempt: Vec::new() }
    }

    fn req(kind: Kind, target: Resolved, length: Option<Term>) -> PunishRequest {
        PunishRequest {
            kind,
            target,
            length,
            reason: Some("because".into()),
            template: None,
            silent: false,
            operator: Actor::player(uuid(999), "Mod"),
            bypass_exempt: false,
            length_limit: None,
            source: None,
        }
    }

    fn denied(d: &LoginDecision) -> Option<u64> {
        match d {
            LoginDecision::Deny { punishment, .. } => Some(punishment.as_ref().map(|p| p.id).unwrap_or(0)),
            LoginDecision::Allow { .. } => None,
        }
    }

    #[test]
    fn uuid_ban_blocks_login_and_unban_lifts_it() {
        let mut e = engine();
        e.record_login(uuid(1), "Steve", ip("1.1.1.1"), T0).unwrap();
        let r = e.punish(req(Kind::Ban, player(1, "Steve"), None), T0).unwrap();
        assert_eq!(r.punishment.id, 1);
        let d = e.check_login(uuid(1), "Steve", ip("9.9.9.9"), T0 + 1);
        assert_eq!(denied(&d), Some(1));
        if let LoginDecision::Deny { screen, .. } = d {
            let s = screen.plain();
            assert!(s.contains("because") && s.contains("Mod") && s.contains("#1"), "{s}");
        }
        assert_eq!(denied(&e.check_login(uuid(2), "Alex", ip("9.9.9.9"), T0)), None);
        let lifted = e.revoke(Kind::Ban, &player(1, "Steve"), &Actor::console("Console"), None, T0 + 2).unwrap();
        assert_eq!(lifted.len(), 1);
        assert_eq!(denied(&e.check_login(uuid(1), "Steve", ip("1.1.1.1"), T0 + 3)), None);
        // Still in the history, marked as lifted.
        let h = e.history_of_player(uuid(1)).unwrap();
        assert_eq!(h.len(), 1);
        assert!(h.first().unwrap().revoked.is_some());
    }

    #[test]
    fn tempban_expires() {
        let mut e = engine();
        e.punish(req(Kind::Ban, player(1, "Steve"), Some(ms(10 * MINUTE))), T0).unwrap();
        assert!(denied(&e.check_login(uuid(1), "Steve", None, T0 + 9 * MINUTE)).is_some());
        assert!(denied(&e.check_login(uuid(1), "Steve", None, T0 + 10 * MINUTE)).is_none());
        let swept = e.sweep(T0 + 11 * MINUTE, false).unwrap();
        assert_eq!(swept.expired, 1);
        assert_eq!(e.active_count(T0), 0);
    }

    #[test]
    fn ip_ban_blocks_other_names_from_the_address() {
        let mut e = engine();
        let net = net::punish_range(ip("5.5.5.5").unwrap(), 32, 64);
        e.punish(req(Kind::Ban, Resolved::Address { net, via: Some((uuid(1), "Steve".into())) }, None), T0).unwrap();
        assert!(denied(&e.check_login(uuid(2), "Alt", ip("5.5.5.5"), T0)).is_some());
        assert!(denied(&e.check_login(uuid(3), "Other", ip("5.5.5.6"), T0)).is_none());
        // /unban Steve also lifts the address ban placed through Steve.
        let lifted = e.revoke(Kind::Ban, &player(1, "Steve"), &Actor::console("Console"), None, T0).unwrap();
        assert_eq!(lifted.len(), 1);
        assert!(denied(&e.check_login(uuid(2), "Alt", ip("5.5.5.5"), T0)).is_none());
    }

    #[test]
    fn address_check_before_the_account_is_known() {
        let mut e = engine();
        let net = net::punish_range(ip("5.5.5.5").unwrap(), 32, 64);
        e.punish(req(Kind::Ban, Resolved::Address { net, via: None }, Some(ms(HOUR))), T0).unwrap();
        e.punish(req(Kind::Ban, player(1, "Steve"), None), T0).unwrap();
        assert!(denied(&e.check_address("Any", ip("5.5.5.5"), T0)).is_some());
        // Account bans wait for the UUID (check_login).
        assert!(denied(&e.check_address("Steve", ip("6.6.6.6"), T0)).is_none());
        assert!(denied(&e.check_address("Any", ip("5.5.5.5"), T0 + HOUR)).is_none());
        let (cfg, _) = load::<BansCfg>("");
        let (down, _) = Engine::new(cfg, Langs::single(english()), Err(StoreError::new("gone")), "test");
        assert!(denied(&down.check_address("Any", None, T0)).is_some());
    }

    #[test]
    fn ipv6_ban_covers_the_64() {
        let mut e = engine();
        let net = net::punish_range(ip("2001:db8:0:1::5").unwrap(), 32, 64);
        e.punish(req(Kind::Ban, Resolved::Address { net, via: None }, None), T0).unwrap();
        assert!(denied(&e.check_login(uuid(2), "A", ip("[2001:db8:0:1:ffff::1]:5"), T0)).is_some());
        assert!(denied(&e.check_login(uuid(3), "B", ip("2001:db8:0:2::1"), T0)).is_none());
    }

    #[test]
    fn strictness_levels() {
        // Steve used 7.7.7.7 earlier; Alex shares 8.8.8.8 with Steve; Bob shares nothing.
        let setup = |e: &mut Engine| {
            e.record_login(uuid(1), "Steve", ip("7.7.7.7"), T0).unwrap();
            e.record_login(uuid(1), "Steve", ip("8.8.8.8"), T0).unwrap();
            e.record_login(uuid(2), "Alex", ip("8.8.8.8"), T0).unwrap();
            e.record_login(uuid(3), "Bob", ip("3.3.3.3"), T0).unwrap();
            let net = net::punish_range(ip("7.7.7.7").unwrap(), 32, 64);
            e.punish(req(Kind::Ban, Resolved::Address { net, via: None }, None), T0).unwrap();
        };
        // lenient: Steve from a new address is fine.
        let mut e = engine_with("enforcement:\n  address-strictness: lenient\n");
        setup(&mut e);
        assert!(denied(&e.check_login(uuid(1), "Steve", ip("9.9.9.9"), T0)).is_none());
        assert!(denied(&e.check_login(uuid(4), "New", ip("7.7.7.7"), T0)).is_some());

        // normal: Steve used the address before, so he is banned everywhere; Alex is not.
        let mut e = engine();
        setup(&mut e);
        assert!(denied(&e.check_login(uuid(1), "Steve", ip("9.9.9.9"), T0)).is_some());
        assert!(denied(&e.check_login(uuid(2), "Alex", ip("6.6.6.6"), T0)).is_none());

        // stern: Alex shares 8.8.8.8 with Steve, who used the banned address.
        let mut e = engine_with("enforcement:\n  address-strictness: stern\n");
        setup(&mut e);
        assert!(denied(&e.check_login(uuid(2), "Alex", ip("6.6.6.6"), T0)).is_some());
        assert!(denied(&e.check_login(uuid(3), "Bob", ip("3.3.3.3"), T0)).is_none());

        // strict: an account ban of Alex also covers Steve (shared address).
        let mut e = engine_with("enforcement:\n  address-strictness: strict\n");
        e.record_login(uuid(1), "Steve", ip("8.8.8.8"), T0).unwrap();
        e.record_login(uuid(2), "Alex", ip("8.8.8.8"), T0).unwrap();
        e.punish(req(Kind::Ban, player(2, "Alex"), None), T0).unwrap();
        assert!(denied(&e.check_login(uuid(1), "Steve", ip("1.2.3.4"), T0)).is_some());
        let mut e = engine();
        e.record_login(uuid(1), "Steve", ip("8.8.8.8"), T0).unwrap();
        e.record_login(uuid(2), "Alex", ip("8.8.8.8"), T0).unwrap();
        e.punish(req(Kind::Ban, player(2, "Alex"), None), T0).unwrap();
        assert!(denied(&e.check_login(uuid(1), "Steve", ip("1.2.3.4"), T0)).is_none());
    }

    #[test]
    fn retention_limits_normal_strictness() {
        let mut e = engine_with("privacy:\n  address-retention-days: 1\n");
        e.record_login(uuid(1), "Steve", ip("7.7.7.7"), T0).unwrap();
        let net = net::punish_range(ip("7.7.7.7").unwrap(), 32, 64);
        e.punish(req(Kind::Ban, Resolved::Address { net, via: None }, None), T0).unwrap();
        assert!(denied(&e.check_login(uuid(1), "Steve", ip("9.9.9.9"), T0 + HOUR)).is_some());
        let later = T0 + 3 * DAY;
        // The old address is past retention: no longer linked.
        assert!(denied(&e.check_login(uuid(1), "Steve", ip("9.9.9.9"), later)).is_none());
        let report = e.sweep(later + 2 * DAY, true).unwrap();
        assert!(report.addresses_forgotten >= 1);
        assert!(e.player(uuid(1)).unwrap().unwrap().addresses.iter().all(|a| a.value != ip("7.7.7.7").unwrap()));
    }

    #[test]
    fn name_ban_links_to_the_first_account() {
        let mut e = engine();
        e.punish(req(Kind::Ban, Resolved::Name { name: "Griefer".into() }, None), T0).unwrap();
        assert!(denied(&e.check_login(uuid(5), "gRiEfEr", None, T0)).is_some());
        let p = e.active_list(Kind::Ban, T0).into_iter().next().unwrap();
        assert_eq!(p.victim_uuid, Some(uuid(5)));
        // The same account under a new name is still banned (online-mode rename).
        assert!(denied(&e.check_login(uuid(5), "Renamed", None, T0)).is_some());
        // Unban by name.
        e.revoke(Kind::Ban, &Resolved::Name { name: "GRIEFER".into() }, &Actor::console("Console"), None, T0).unwrap();
        assert!(denied(&e.check_login(uuid(5), "Griefer", None, T0)).is_none());
    }

    #[test]
    fn duplicates_and_override() {
        let mut e = engine();
        e.punish(req(Kind::Ban, player(1, "Steve"), None), T0).unwrap();
        let again = e.punish(req(Kind::Ban, player(1, "Steve"), Some(ms(HOUR))), T0);
        assert!(matches!(again, Err(PunishError::AlreadyPunished(_))));
        let mut e = engine_with("punishments:\n  allow-override: true\n");
        e.punish(req(Kind::Ban, player(1, "Steve"), None), T0).unwrap();
        let r = e.punish(req(Kind::Ban, player(1, "Steve"), Some(ms(HOUR))), T0).unwrap();
        assert_eq!(r.replaced.len(), 1);
        assert_eq!(e.active_list(Kind::Ban, T0).len(), 1);
        assert!(denied(&e.check_login(uuid(1), "Steve", None, T0 + 2 * HOUR)).is_none());
    }

    #[test]
    fn exemptions() {
        let mut e = engine();
        let target = Resolved::Player { uuid: uuid(1), name: "Admin".into(), exempt: vec![Kind::Ban] };
        assert_eq!(e.punish(req(Kind::Ban, target.clone(), None), T0).unwrap_err(), PunishError::Exempt);
        assert!(e.punish(req(Kind::Mute, target.clone(), None), T0).is_ok());
        let mut bypass = req(Kind::Ban, target, None);
        bypass.bypass_exempt = true;
        assert!(e.punish(bypass, T0).is_ok());
    }

    #[test]
    fn limits_and_reasons() {
        let mut e = engine_with("punishments:\n  require-reason: true\n");
        let mut r = req(Kind::Ban, player(1, "Steve"), Some(ms(2 * DAY)));
        r.length_limit = Some(ms(DAY));
        assert_eq!(e.punish(r.clone(), T0).unwrap_err(), PunishError::LimitExceeded(ms(DAY)));
        r.length = None; // permanent is longer than any limit
        assert!(matches!(e.punish(r.clone(), T0), Err(PunishError::LimitExceeded(_))));
        r.length = Some(ms(HOUR));
        r.reason = Some("   ".into());
        assert_eq!(e.punish(r.clone(), T0).unwrap_err(), PunishError::ReasonRequired);
        r.reason = Some("ok".into());
        assert!(e.punish(r, T0).is_ok());
    }

    #[test]
    fn templates_climb_the_ladder() {
        let mut e = engine();
        let mut r = req(Kind::Ban, player(1, "Steve"), None);
        r.template = Some("cheating".into());
        r.reason = None;
        let first = e.punish(r.clone(), T0).unwrap().punishment;
        assert_eq!(first.reason, "Cheating");
        assert_eq!(first.length(), Some(7 * DAY));
        // Expire it, then the second one is longer.
        let later = T0 + 8 * DAY;
        let second = e.punish(r.clone(), later).unwrap().punishment;
        assert_eq!(second.length(), Some(30 * DAY));
        e.revoke(Kind::Ban, &player(1, "Steve"), &Actor::console("Console"), None, later).unwrap();
        // A lifted one does not count: still the 2nd step... then the 3rd (perm) after another.
        let third = e.punish(r.clone(), later + 1).unwrap().punishment;
        assert_eq!(third.length(), Some(30 * DAY));
        let much_later = later + 40 * DAY;
        let fourth = e.punish(r.clone(), much_later).unwrap().punishment;
        assert!(fourth.is_permanent());
        let mut extra = r.clone();
        extra.target = player(2, "Alex");
        extra.reason = Some("killaura".into());
        assert_eq!(e.punish(extra, T0).unwrap().punishment.reason, "Cheating (killaura)");
        let mut unknown = r;
        unknown.template = Some("nope".into());
        assert_eq!(e.punish(unknown, T0).unwrap_err(), PunishError::UnknownTemplate("nope".into()));
    }

    #[test]
    fn warnings_escalate() {
        // Default config: 3 warnings -> mute 1h, 5 -> ban 1d, repeat above 5.
        let mut e = engine();
        for i in 1..=2 {
            let r = e.punish(req(Kind::Warn, player(1, "Steve"), None), T0 + i).unwrap();
            assert_eq!(r.warn_count, i as usize);
            assert!(r.escalated.is_none());
        }
        let third = e.punish(req(Kind::Warn, player(1, "Steve"), None), T0 + 3).unwrap();
        let mute = third.escalated.unwrap();
        assert_eq!(mute.kind, Kind::Mute);
        assert_eq!(mute.length(), Some(HOUR));
        assert_eq!(mute.source.as_deref(), Some("escalation"));
        assert!(e.mute_for(&Subject::new(uuid(1), "Steve", None), T0 + 4).is_some());
        e.punish(req(Kind::Warn, player(1, "Steve"), None), T0 + 4).unwrap();
        let fifth = e.punish(req(Kind::Warn, player(1, "Steve"), None), T0 + 5).unwrap();
        assert_eq!(fifth.escalated.unwrap().kind, Kind::Ban);
        // Already banned: the 6th warning's repeat is skipped, not an error.
        let sixth = e.punish(req(Kind::Warn, player(1, "Steve"), None), T0 + 6).unwrap();
        assert!(sixth.escalated.is_none());
        // Warnings expire after 30 days and stop counting.
        let r = e.punish(req(Kind::Warn, player(1, "Steve"), None), T0 + 31 * DAY).unwrap();
        assert_eq!(r.warn_count, 1);
        // Lifting a warning by id.
        let id = r.punishment.id;
        assert!(e.revoke_id(id, Some(Kind::Warn), &Actor::console("Console"), None, T0 + 31 * DAY).is_ok());
        assert_eq!(
            e.revoke_id(id, Some(Kind::Warn), &Actor::console("Console"), None, T0 + 31 * DAY).unwrap_err(),
            RevokeError::NotPunished
        );
        assert!(matches!(
            e.revoke_id(1, Some(Kind::Ban), &Actor::console("C"), None, T0),
            Err(RevokeError::WrongKind(_))
        ));
        assert_eq!(e.revoke_id(999, None, &Actor::console("C"), None, T0).unwrap_err(), RevokeError::NoSuchId);
    }

    #[test]
    fn mutes_follow_ip_and_account() {
        let mut e = engine();
        let net = net::punish_range(ip("4.4.4.4").unwrap(), 32, 64);
        e.punish(req(Kind::Mute, Resolved::Address { net, via: None }, Some(ms(HOUR))), T0).unwrap();
        assert!(e.mute_for(&Subject::new(uuid(9), "Any", ip("4.4.4.4")), T0).is_some());
        assert!(e.mute_for(&Subject::new(uuid(9), "Any", ip("4.4.4.5")), T0).is_none());
        assert!(e.mute_for(&Subject::new(uuid(9), "Any", ip("4.4.4.4")), T0 + HOUR).is_none());
    }

    #[test]
    fn fail_closed_and_fail_open() {
        let (cfg, _) = load::<BansCfg>("");
        let (mut e, w) = Engine::new(cfg, Langs::single(english()), Err(StoreError::new("disk gone")), "test");
        assert_eq!(w.len(), 1);
        assert!(!e.storage_ok());
        assert!(denied(&e.check_login(uuid(1), "Steve", None, T0)).is_some());
        assert!(matches!(e.punish(req(Kind::Ban, player(1, "S"), None), T0), Err(PunishError::Storage(_))));
        let (cfg, _) = load::<BansCfg>("enforcement:\n  fail-closed: false\n");
        let (mut e, _) = Engine::new(cfg, Langs::single(english()), Err(StoreError::new("disk gone")), "test");
        assert!(denied(&e.check_login(uuid(1), "Steve", None, T0)).is_none());
    }

    #[test]
    fn failed_write_changes_nothing() {
        // A store that cannot even write its schema does not open.
        assert!(BansStore::new(Store::new(FailingWrites(MemoryBackend::default()))).is_err());
        let (cfg, _) = load::<BansCfg>("");
        let (mut e, _) = Engine::new(cfg, Langs::single(english()), Ok(BansStore::failing_for_tests()), "test");
        assert!(matches!(e.punish(req(Kind::Ban, player(1, "S"), None), T0), Err(PunishError::Storage(_))));
        assert_eq!(e.active_count(T0), 0);
    }

    #[test]
    fn history_lists_and_alts() {
        let mut e = engine();
        e.record_login(uuid(1), "Steve", ip("8.8.8.8"), T0).unwrap();
        e.record_login(uuid(2), "Alex", ip("8.8.8.8"), T0).unwrap();
        e.record_login(uuid(3), "Bob", ip("3.3.3.3"), T0).unwrap();
        e.punish(req(Kind::Ban, player(2, "Alex"), None), T0).unwrap();
        e.punish(req(Kind::Kick, player(1, "Steve"), None), T0).unwrap();
        e.punish(req(Kind::Warn, player(1, "Steve"), None), T0).unwrap();
        let net = net::punish_range(ip("8.8.8.8").unwrap(), 32, 64);
        e.punish(req(Kind::Mute, Resolved::Address { net, via: Some((uuid(1), "Steve".into())) }, None), T0).unwrap();
        let h = e.history_of_player(uuid(1)).unwrap();
        assert_eq!(h.iter().map(|p| p.id).collect::<Vec<_>>(), vec![4, 3, 2]);
        assert_eq!(e.history_of_address(&Cidr::parse("8.8.0.0/16").unwrap()).unwrap().len(), 1);
        assert_eq!(e.history_of_operator(&Actor::player(uuid(999), "Mod")).unwrap().len(), 4);
        assert_eq!(e.active_list(Kind::Ban, T0).len(), 1);
        let alts = e.alts(uuid(1), T0).unwrap();
        assert_eq!(alts.len(), 1);
        let alt = alts.first().unwrap();
        assert_eq!((alt.name.as_str(), alt.banned), ("Alex", true));
        // Alt notice when Steve joins: Alex (sharing 8.8.8.8) is banned.
        match e.check_login(uuid(1), "Steve", ip("8.8.8.8"), T0) {
            LoginDecision::Allow { alt_notice: Some(n) } => assert!(n.plain().contains("Alex")),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(e.purge_addresses(uuid(1)).unwrap(), 1);
        assert!(e.alts(uuid(1), T0).unwrap().is_empty());
    }

    #[test]
    fn exemption_snapshot_is_stored() {
        let mut e = engine();
        e.note_join(uuid(1), "Admin", vec![Kind::Ban, Kind::Ban, Kind::Kick], Some("pl_pl".into()), T0).unwrap();
        let rec = e.player(uuid(1)).unwrap().unwrap();
        assert_eq!(rec.exempt, vec![Kind::Ban, Kind::Kick]);
        assert_eq!(rec.locale.as_deref(), Some("pl_pl"));
        e.note_join(uuid(1), "Admin", vec![], None, T0).unwrap();
        let rec = e.player(uuid(1)).unwrap().unwrap();
        assert!(rec.exempt.is_empty());
        assert_eq!(rec.locale.as_deref(), Some("pl_pl"));
    }

    #[test]
    fn import_skips_expired_and_duplicates() {
        let mut e = engine();
        let entry = |target: Target, expires: Option<u64>| ImportEntry {
            kind: Kind::Ban,
            target,
            name: Some("X".into()),
            operator: "Server".into(),
            reason: "old".into(),
            created: T0 - DAY,
            expires,
            source: "import:vanilla".into(),
        };
        let r = e
            .import(
                vec![
                    entry(Target::player(uuid(1)), None),
                    entry(Target::player(uuid(1)), None),
                    entry(Target::player(uuid(2)), Some(T0 - 1)),
                    entry(Target::Address { net: Cidr::parse("1.2.3.4").unwrap() }, Some(T0 + DAY)),
                ],
                T0,
            )
            .unwrap();
        assert_eq!(r, ImportReport { added: 2, skipped: 2 });
        assert!(denied(&e.check_login(uuid(1), "X", None, T0)).is_some());
        assert!(denied(&e.check_login(uuid(7), "Y", ip("1.2.3.4"), T0)).is_some());
    }
}
