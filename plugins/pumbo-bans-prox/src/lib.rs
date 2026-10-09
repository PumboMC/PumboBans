//! PumboBans for the PumboProx proxy: one punishment database for the whole
//! network behind the proxy.
//!
//! All rules live in `pumbo-bans-core`; this crate turns proxy events into calls
//! of the core and carries out what it returns:
//!
//! - `on-pre-login` (before encryption): bans of the connecting address;
//!   `on-login` (final UUID after the profile plugins): account and name bans,
//!   the login record (names, addresses for `/alts`), alt notices for staff,
//! - `on-chat` and `on-backend-command`: mutes (the proxy drops a cancelled
//!   signed message with a `chat_ack`, plan §2.8), and PumboBans commands that
//!   the client signed because a backend has a command of the same name,
//! - commands `/ban`, `/mute`, ... and `/pumbobans <sub>` (`/pumbo bans <sub>`),
//!   also from the proxy console,
//! - exemptions of offline players through `permissions.has-offline`,
//! - for other plugins: the service `pumbobans:punish@1.0` (contract in
//!   `pumbo_common::bans`), the topics `pumbo:player-punished@1.0` and
//!   `pumbo:punishment-revoked@1.0`, the placeholder `%pumbobans_muted%`.
//!
//! [`setup`] holds the parts that do not need the host.

pub mod setup;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use pumbo_bans_core::commands::{
    self as cmds, Ctx, Online, SHORTCUTS, Sender, exempt_node, permission_nodes, shortcuts,
};
use pumbo_bans_core::effects::{AdminRequest, Audience, Effect};
use pumbo_bans_core::engine::{LoginDecision, Subject};
use pumbo_bans_core::store::BansStore;
use pumbo_bans_core::{Engine, ID, Kind, Punishment, import, ipc, render};
use pumbo_common::clock::now_ms;
use pumbo_common::command::{permission, split_command};
use pumbo_common::id::{Uuid, parse_ip};
use pumbo_common::rich::Text as Rich;
use pumbo_common::store::{Store, StoreError};
use pumbo_common::style::{self, Tone};
use pumbo_common::text::Args;
use pumbo_sdk::contracts::{Contract, PLAYER_PUNISHED, PUNISHMENT_REVOKED};
use pumbo_sdk::{
    BackendCommandEvent, BackendCommandReply, CallReject, ChatReply, Command, CommandEvent, Context, PlayerId,
    PlayerInfo, PreLoginEvent, PreLoginReply, QueryContext, ServiceCall, Text, Verdict, bus, log, permissions,
    placeholders, players, scheduler,
};

/// Retire expired punishments this often.
const SWEEP_MS: u64 = 60_000;
/// Forget addresses past the retention period at most this often.
const FORGET_EVERY_MS: u64 = 3_600_000;
/// Standard subcommands the proxy runs itself under `/pumbo bans`.
const HOST_SUBCOMMANDS: &[&str] = &["reload", "version", "debug"];
/// Shown when a login cannot be checked because the plugin is busy.
const BUSY: &str = "&cPumboBans is busy, please join again.";
/// Push placeholder `%pumbobans_muted%` (`true` / `false`).
const MUTED_KEY: &str = "muted";

struct State {
    engine: Engine,
    sweep_timer: u64,
    last_forget: u64,
    /// Short commands registered at the top level (`ban`, `mute`, ...).
    roots: BTreeSet<String>,
    /// Commands taken from backend commands, run on the next timer tick.
    pending: BTreeMap<u64, (PlayerId, String, Vec<String>)>,
}

pub struct PumboBans {
    state: RefCell<Option<State>>,
    /// Read-only config folder (`plugins/pumbo-bans/`).
    pub config_dir: String,
    /// Data folder (`plugins/data/pumbo-bans/`), holds `bans.redb`.
    pub data_dir: String,
}

impl Default for PumboBans {
    fn default() -> Self {
        PumboBans { state: RefCell::new(None), config_dir: "/config".into(), data_dir: "/data".into() }
    }
}

fn now() -> u64 {
    now_ms()
}

fn uuid_of(p: &PlayerInfo) -> Uuid {
    Uuid::from_high_low(p.profile.id.high, p.profile.id.low)
}

fn sdk_uuid(u: Uuid) -> pumbo_sdk::Uuid {
    let (high, low) = u.high_low();
    pumbo_sdk::Uuid { high, low }
}

fn locale(p: &PlayerInfo) -> Option<String> {
    p.settings.as_ref().map(|s| s.locale.clone())
}

fn text(t: &Rich) -> Text {
    Text::Json(setup::json(t))
}

fn has(id: PlayerId, node: &str) -> bool {
    permissions::has(id, node, &QueryContext::Current)
}

fn send(id: PlayerId, t: &Rich) {
    if !t.is_empty() {
        players::send_message(id, text(t));
    }
}

fn exemptions(id: PlayerId) -> Vec<Kind> {
    Kind::ALL.into_iter().filter(|k| has(id, &exempt_node(*k))).collect()
}

/// Online players; exemptions only for those named in `targets`.
fn online(all: &[PlayerInfo], targets: &[String]) -> Vec<Online> {
    all.iter()
        .map(|p| {
            let name = p.profile.name.clone();
            let exempt =
                if targets.iter().any(|t| t.eq_ignore_ascii_case(&name)) { exemptions(p.id) } else { Vec::new() };
            Online { uuid: uuid_of(p), ip: parse_ip(&p.connection.address), locale: locale(p), name, exempt }
        })
        .collect()
}

fn publish(topic: &Contract, p: &Punishment) {
    if let Err(e) = bus::publish_value(&topic.versioned(), &setup::contract(p)) {
        log::debug(&format!("PumboBans: {} not published: {e}", topic.name));
    }
}

/// The subcommand behind a command name: short commands map through
/// [`SHORTCUTS`] (`checkban` → `check`) and keep their name for usage lines.
fn subcommand(name: &str) -> (String, Option<&'static str>) {
    match SHORTCUTS.iter().find(|(c, _)| *c == name) {
        Some((c, s)) => ((*s).to_string(), Some(*c)),
        None => (name.to_string(), None),
    }
}

impl PumboBans {
    /// Runs `f` with the state; `None` when it is missing or borrowed. Never
    /// `.await` inside `f`.
    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> Option<R> {
        self.state.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f))
    }

    /// Loads config, messages and the database, registers the commands.
    pub fn start(&self, store: Result<BansStore, StoreError>) -> Result<(), String> {
        let (cfg, mut warnings, found) = setup::load_config(&self.config_dir);
        if !found {
            log::info(
                "PumboBans: no config.yml in plugins/pumbo-bans/, using the defaults (template: config.yml of the release)",
            );
        }
        let (langs, w) = setup::load_langs(&self.config_dir, &cfg);
        warnings.extend(w);
        for w in &warnings {
            log::warn(&format!("PumboBans: config: {w}"));
        }
        let (engine, problems) = Engine::new(cfg, langs, store, "PumboProx");
        for p in &problems {
            // Fail closed: without the database nobody joins (unless configured otherwise).
            log::error(&format!("PumboBans: {p}"));
        }
        let tree = cmds::tree();
        let disabled = engine.cfg.commands.disabled.clone();
        let mut roots = BTreeSet::new();
        for (name, sub) in shortcuts(&disabled) {
            let Some(s) = tree.subs().iter().find(|s| s.name == sub) else { continue };
            let spec = Command::new(name).permission(&tree.permission(s)).usage(&format!("/{name} {}", s.usage));
            match spec.register() {
                Ok(()) => {
                    roots.insert(name.to_string());
                }
                Err(e) => log::warn(&format!("PumboBans: /{name} not registered: {e}")),
            }
        }
        for s in tree.subs().iter().filter(|s| !HOST_SUBCOMMANDS.contains(&s.name)) {
            let mut spec = Command::new(s.name)
                .permission(&tree.permission(s))
                .usage(&format!("/pumbobans {} {}", s.name, s.usage))
                .umbrella();
            for a in s.aliases {
                spec = spec.alias(a);
            }
            if let Err(e) = spec.register() {
                log::warn(&format!("PumboBans: /pumbobans {} not registered: {e}", s.name));
            }
        }
        if let Err(e) = Command::new("help").usage("/pumbobans help [page]").umbrella().register() {
            log::warn(&format!("PumboBans: /pumbobans help not registered: {e}"));
        }
        let active = engine.active_count(now());
        let sweep_timer = scheduler::every(SWEEP_MS);
        let short = roots.len();
        *self.state.borrow_mut() = Some(State { engine, sweep_timer, last_forget: 0, roots, pending: BTreeMap::new() });
        log::info(&format!(
            "PumboBans {} loaded ({active} active punishments, /pumbobans and {short} short commands)",
            env!("CARGO_PKG_VERSION")
        ));
        Ok(())
    }

    /// The warnings, or the error of a file that is not valid YAML (then the
    /// current settings stay).
    fn reload(&self) -> Result<Vec<String>, String> {
        let (cfg, mut warnings, _) = setup::load_config(&self.config_dir);
        let (langs, w) = setup::load_langs(&self.config_dir, &cfg);
        warnings.extend(w);
        if let Some(w) = warnings.iter().find(|w| w.fatal) {
            log::warn(&format!("PumboBans: reload refused, the current settings stay: {w}"));
            return Err(w.message.clone());
        }
        self.with(|s| s.engine.reconfigure(cfg, langs));
        Ok(warnings
            .iter()
            .map(|w| {
                log::warn(&format!("PumboBans: config: {w}"));
                w.message.clone()
            })
            .collect())
    }

    /// The command sender: the console, or a player with the nodes it has.
    fn sender(&self, player: Option<PlayerId>) -> Option<Sender> {
        let Some(id) = player else { return Some(Sender::console()) };
        let info = players::get(id)?;
        let nodes = self.with(|s| permission_nodes(&s.engine)).unwrap_or_default();
        let granted: BTreeSet<String> = nodes.into_iter().filter(|n| has(id, n)).collect();
        Some(Sender::player(uuid_of(&info), &info.profile.name, granted, locale(&info)))
    }

    /// Before a punishment of an offline player: its exemptions from the
    /// permission source (`has-offline`), stored as the snapshot the core reads.
    /// An error counts as "not exempt" (plan §6.4).
    async fn refresh_offline_exemptions(&self, words: &[String], online: &[Online]) {
        let punishes = matches!(
            words.first().map(String::as_str),
            Some("ban" | "tempban" | "tban" | "mute" | "tempmute" | "tmute" | "warn")
        );
        let Some(token) = words.iter().skip(1).find(|w| !w.starts_with('-')) else { return };
        if !punishes || online.iter().any(|o| o.name.eq_ignore_ascii_case(token)) {
            return;
        }
        let record = self
            .with(|s| {
                let uuid = Uuid::parse(token).or_else(|| s.engine.uuid_by_name(token).ok().flatten())?;
                s.engine.player(uuid).ok().flatten()
            })
            .flatten();
        let Some(rec) = record else { return };
        let mut exempt = Vec::new();
        for k in Kind::ALL {
            match permissions::has_offline(sdk_uuid(rec.uuid), exempt_node(k), QueryContext::Global).await {
                Ok(true) => exempt.push(k),
                Ok(false) => {}
                Err(e) => log::warn(&format!("PumboBans: exemption of {} unknown ({e:?}), not exempt", rec.name)),
            }
        }
        if let Some(Err(e)) = self.with(|s| s.engine.note_join(rec.uuid, &rec.name, exempt, None, now())) {
            log::warn(&format!("PumboBans: cannot store exemptions of {}: {e}", rec.name));
        }
    }

    async fn run_command(&self, player: Option<PlayerId>, label: Option<&'static str>, words: Vec<String>) {
        let Some(who) = self.sender(player) else { return };
        let all = players::all();
        let targets: Vec<String> = words.iter().take(2).cloned().collect();
        let mut on = online(&all, &targets);
        self.refresh_offline_exemptions(&words, &on).await;
        // The player list may have changed while waiting for the permission source.
        if all.len() != players::all().len() {
            on = online(&players::all(), &targets);
        }
        let effects = self.with(|s| {
            let ctx = Ctx { sender: &who, online: &on, now: now(), label };
            cmds::run(&mut s.engine, &words, &ctx)
        });
        let Some(effects) = effects else {
            reply(player, &Rich::parse("&cPumboBans is busy, please try again."));
            return;
        };
        for req in self.apply(player, effects.0) {
            self.admin(player, &who, req);
        }
    }

    /// Carries out effects; admin requests are returned. `to` gets the replies
    /// (`None`: the console log).
    fn apply(&self, to: Option<PlayerId>, effects: Vec<Effect>) -> Vec<AdminRequest> {
        let all = players::all();
        let id_of = |u: Uuid| all.iter().find(|p| uuid_of(p) == u).map(|p| p.id);
        let mut admin = Vec::new();
        let mut mutes: Vec<Punishment> = Vec::new();
        for e in effects {
            match e {
                Effect::Reply(t) => reply(to, &t),
                Effect::Kick { uuid, screen } => {
                    if let Some(id) = id_of(uuid) {
                        players::kick(id, text(&screen));
                    }
                }
                Effect::Tell { uuid, message } => {
                    if let Some(id) = id_of(uuid) {
                        send(id, &message);
                    }
                }
                Effect::Notify { message, silent, except, audience } => {
                    notify(&all, &message, silent, except, audience);
                }
                Effect::Log(line) => log::info(&line),
                Effect::Admin(r) => admin.push(r),
                Effect::Placed(p) => {
                    publish(&PLAYER_PUNISHED, &p);
                    if p.kind == Kind::Mute {
                        mutes.push(*p);
                    }
                }
                Effect::Lifted(p) => {
                    publish(&PUNISHMENT_REVOKED, &p);
                    if p.kind == Kind::Mute {
                        mutes.push(*p);
                    }
                }
            }
        }
        if !mutes.is_empty() {
            let reached: Vec<PlayerInfo> = all
                .iter()
                .filter(|o| {
                    let ip = parse_ip(&o.connection.address);
                    mutes.iter().any(|m| {
                        m.targets_account(uuid_of(o), &o.profile.name) || ip.is_some_and(|ip| m.covers_address(ip))
                    })
                })
                .cloned()
                .collect();
            self.refresh_muted(&reached);
        }
        admin
    }

    /// `%pumbobans_muted%` of online players.
    fn refresh_muted(&self, all: &[PlayerInfo]) {
        let t = now();
        let values: Vec<(PlayerId, bool)> = self
            .with(|s| {
                all.iter()
                    .map(|p| {
                        let subject = Subject::new(uuid_of(p), &p.profile.name, parse_ip(&p.connection.address));
                        (p.id, s.engine.mute_for(&subject, t).is_some())
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (id, muted) in values {
            let _ = placeholders::set(MUTED_KEY, Some(id), pumbo_sdk::text::plain(muted.to_string()), &Context::Global);
        }
    }

    /// Reload and import need the plugin's files.
    fn admin(&self, to: Option<PlayerId>, who: &Sender, req: AdminRequest) {
        let say = |tone: Tone, key: &str, args: Args| {
            let msg = self.with(|s| style::message(s.engine.lang_for(who.locale.as_deref()), tone, key, &args));
            if let Some(m) = msg {
                reply(to, &m);
            }
        };
        match req {
            AdminRequest::Reload => match self.reload() {
                Ok(warnings) => {
                    for w in warnings {
                        say(Tone::Warn, "admin-reload-warning", Args::new().with("warning", style::value(&w)));
                    }
                    say(Tone::Success, "command-reloaded", Args::new());
                }
                Err(e) => say(Tone::Error, "command-reload-failed", Args::new().arg(style::value(&e))),
            },
            AdminRequest::ImportVanilla => {
                let read = |file: &str| std::fs::read_to_string(format!("{}/{file}", self.data_dir)).ok();
                let mut players_list = Vec::new();
                let mut ips = Vec::new();
                if let Some(t) = read("banned-players.json") {
                    match import::parse_players(&t) {
                        Ok(l) => players_list = l,
                        Err(e) => say(Tone::Error, "admin-import-failed", Args::new().with("error", style::value(e))),
                    }
                }
                if let Some(t) = read("banned-ips.json") {
                    match import::parse_ips(&t) {
                        Ok(l) => ips = l,
                        Err(e) => say(Tone::Error, "admin-import-failed", Args::new().with("error", style::value(e))),
                    }
                }
                let counts =
                    Args::new().with("players", style::value(players_list.len())).with("ips", style::value(ips.len()));
                say(Tone::Info, "admin-import-source", counts);
                let t = now();
                let (mut entries, dropped_p) = import::player_entries(&players_list, t);
                let (ip_entries, dropped_i) = import::ip_entries(&ips, t);
                entries.extend(ip_entries);
                match self.with(|s| s.engine.import(entries, t)) {
                    Some(Ok(r)) => {
                        let skipped = r.skipped + dropped_p + dropped_i;
                        log::info(&format!("PumboBans: imported {} punishments, skipped {skipped}", r.added));
                        let args =
                            Args::new().with("added", style::value(r.added)).with("skipped", style::value(skipped));
                        say(Tone::Success, "admin-import-done", args);
                    }
                    Some(Err(e)) => {
                        say(Tone::Error, "admin-import-failed", Args::new().with("error", style::value(e.0)))
                    }
                    None => {}
                }
            }
        }
    }

    /// The line a muted player gets, if `id` is muted.
    fn muted_line(&self, id: PlayerId) -> Option<Rich> {
        let info = players::get(id)?;
        let subject = Subject::new(uuid_of(&info), &info.profile.name, parse_ip(&info.connection.address));
        let loc = locale(&info);
        let t = now();
        self.with(|s| {
            let p = s.engine.mute_for(&subject, t)?;
            Some(render::muted(s.engine.lang_for(loc.as_deref()), &s.engine.cfg, &p, t))
        })
        .flatten()
    }

    fn sweep(&self) {
        let t = now();
        let result = self.with(|s| {
            let forget = t.saturating_sub(s.last_forget) >= FORGET_EVERY_MS;
            if forget {
                s.last_forget = t;
            }
            s.engine.sweep(t, forget)
        });
        match result {
            Some(Ok(r)) if r.addresses_forgotten > 0 => {
                log::info(&format!("PumboBans: forgot {} addresses past the retention period", r.addresses_forgotten));
            }
            Some(Err(e)) => log::warn(&format!("PumboBans: maintenance failed: {e}")),
            _ => {}
        }
        // ponytail: every online player once a minute (expired mutes); cheap
        // while few mutes are active, per-player expiry timers if that changes.
        self.refresh_muted(&players::all());
    }
}

fn reply(to: Option<PlayerId>, t: &Rich) {
    if t.is_empty() {
        return;
    }
    match to {
        Some(id) => players::send_message(id, text(t)),
        None => log::info(&t.plain()),
    }
}

/// A staff message to everyone online with the notify permission.
fn notify(all: &[PlayerInfo], message: &Rich, silent: bool, except: Option<Uuid>, audience: Audience) {
    let node = match audience {
        Audience::Punishments => permission(ID, "notify"),
        Audience::Alts => permission(ID, "notify.alts"),
    };
    let silent_node = permission(ID, "notify.silent");
    for p in all {
        if Some(uuid_of(p)) == except {
            continue;
        }
        if has(p.id, &node) && (!silent || has(p.id, &silent_node)) {
            send(p.id, message);
        }
    }
}

impl pumbo_sdk::Plugin for PumboBans {
    async fn init(&self) -> Result<(), String> {
        let store = Store::open(format!("{}/{ID}.redb", self.data_dir)).and_then(BansStore::new);
        self.start(store)
    }

    async fn on_reload(&self) -> Result<(), String> {
        self.reload().map(|_| ())
    }

    async fn on_pre_login(&self, e: PreLoginEvent) -> PreLoginReply {
        let ip = parse_ip(&e.connection.address);
        match self.with(|s| s.engine.check_address(&e.name, ip, now())) {
            Some(LoginDecision::Deny { screen, punishment }) => {
                if let Some(p) = punishment {
                    log::info(&format!("PumboBans: {} refused, banned address (#{})", e.name, p.id));
                }
                PreLoginReply::Deny(text(&screen))
            }
            Some(LoginDecision::Allow { .. }) => PreLoginReply::Allow,
            None => PreLoginReply::Deny(Text::Legacy(BUSY.into())),
        }
    }

    async fn on_login(&self, p: PlayerInfo) -> Verdict {
        let (uuid, name, ip) = (uuid_of(&p), p.profile.name.clone(), parse_ip(&p.connection.address));
        match self.with(|s| s.engine.check_login(uuid, &name, ip, now())) {
            Some(LoginDecision::Deny { screen, punishment }) => {
                if let Some(pun) = punishment {
                    log::info(&format!("PumboBans: {name} refused, banned (#{})", pun.id));
                }
                Verdict::Deny(text(&screen))
            }
            Some(LoginDecision::Allow { alt_notice }) => {
                if let Some(n) = alt_notice {
                    notify(&players::all(), &n, false, None, Audience::Alts);
                }
                Verdict::Allow
            }
            None => Verdict::Deny(Text::Legacy(BUSY.into())),
        }
    }

    async fn on_server_connected(&self, p: PlayerId, _server: String, previous: Option<String>) {
        if previous.is_some() {
            return;
        }
        // First backend: permissions of the provider are loaded; remember the
        // exemptions for punishments placed while the player is offline.
        let Some(info) = players::get(p) else { return };
        let exempt = exemptions(p);
        let (uuid, name, loc) = (uuid_of(&info), info.profile.name.clone(), locale(&info));
        if let Some(Err(e)) = self.with(|s| s.engine.note_join(uuid, &name, exempt, loc, now())) {
            log::warn(&format!("PumboBans: cannot store exemptions of {name}: {e}"));
        }
        self.refresh_muted(std::slice::from_ref(&info));
    }

    async fn on_chat(&self, p: PlayerId, _message: String) -> ChatReply {
        match self.muted_line(p) {
            Some(line) => {
                send(p, &line);
                ChatReply::Cancel
            }
            None => ChatReply::Pass,
        }
    }

    async fn on_backend_command(&self, e: BackendCommandEvent) -> BackendCommandReply {
        let line = e.line.trim_start_matches('/');
        // `minecraft:ban` is meant for the backend: compare without namespaces removed.
        let root = line.split_whitespace().next().unwrap_or_default().to_lowercase();
        let (_, args) = split_command(line);
        // A PumboBans command the client signed because the backend has a
        // command of the same name (`/ban Steve reason` with a vanilla `/ban`):
        // the proxy only lets unsigned commands reach plugins, so it is taken
        // here and run on the next tick (this event has a short deadline).
        let taken = self.with(|s| {
            if !s.roots.contains(&root) {
                return false;
            }
            let t = scheduler::after(1);
            s.pending.insert(t, (e.player, root.clone(), args.clone()));
            true
        });
        if taken == Some(true) {
            return BackendCommandReply::Cancel;
        }
        let blocked = self.with(|s| s.engine.cfg.is_blocked_for_muted(line)).unwrap_or(false);
        if blocked && let Some(msg) = self.muted_line(e.player) {
            send(e.player, &msg);
            return BackendCommandReply::Cancel;
        }
        BackendCommandReply::Pass
    }

    async fn on_command(&self, e: CommandEvent) {
        let (sub, label) = subcommand(&e.name);
        let mut words = vec![sub];
        words.extend(e.args);
        self.run_command(e.player, label, words).await;
    }

    async fn on_timer(&self, timer: u64) {
        let pending = self.with(|s| s.pending.remove(&timer)).flatten();
        if let Some((player, name, args)) = pending {
            let (sub, label) = subcommand(&name);
            let mut words = vec![sub];
            words.extend(args);
            self.run_command(Some(player), label, words).await;
            return;
        }
        if self.with(|s| s.sweep_timer == timer) == Some(true) {
            self.sweep();
        }
    }

    async fn on_service_call(&self, c: ServiceCall) -> Result<Vec<u8>, CallReject> {
        use pumbo_common::bans::{METHOD, SERVICE, SERVICE_MAJOR};
        if c.service != SERVICE || c.method != METHOD {
            return Err(CallReject::UnknownMethod);
        }
        if c.major != SERVICE_MAJOR {
            return Err(CallReject::Rejected(format!("major version {} not provided", c.major)));
        }
        let on = online(&players::all(), &[]);
        let result = self.with(|s| ipc::handle(&mut s.engine, &c.caller, &c.payload, &on, now()));
        let Some((answer, effects)) = result else { return Err(CallReject::Rejected("PumboBans is busy".into())) };
        self.apply(None, effects.0);
        Ok(answer)
    }
}

pumbo_sdk::plugin!(PumboBans);
pumbo_sdk::embed!(
    manifest = "pumbo-bans.yml",
    config = "../pumbo-bans-pumpkin/assets/config.yml",
    lang = ["assets/lang/en.yml", "assets/lang/pl.yml"],
);

#[cfg(test)]
mod tests;
