//! Commands: one tree reachable as `/pumbobans <sub>`, plus short commands such
//! as `/ban` and `/mute`.
//!
//! The platform gathers what the core cannot know (the sender's permissions, who
//! is online, their addresses and exemptions) into a [`Ctx`], calls [`run`], and
//! carries out the returned effects after the plugin state is released. Every
//! reply is built with the shared style (`pumbo_common::style`, `help`).

use std::collections::BTreeSet;
use std::net::IpAddr;

use pumbo_common::command::{Commands, Dispatch, Sub, permission};
use pumbo_common::help::Help;
use pumbo_common::id::{Uuid, is_java_name, name_key, parse_ip};
use pumbo_common::lang::Lang;
use pumbo_common::rich::{Click, Line, Segment, Text};
use pumbo_common::style::{self, Tone};
use pumbo_common::text::{Args, Style};
use pumbo_common::time::Term;

use crate::ID;
use crate::config::parse_length;
use crate::effects::{AdminRequest, Audience, Effect, Effects};
use crate::engine::{Engine, PunishError, PunishRequest, Punished, Resolved, RevokeError, Subject};
use crate::model::{Actor, Kind, Punishment};
use crate::net;
use crate::render::{self, CONSOLE};

/// What a subcommand does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Ban,
    TempBan,
    IpBan,
    Mute,
    TempMute,
    IpMute,
    Warn,
    Kick,
    Unban,
    UnbanIp,
    Unmute,
    Unwarn,
    History,
    Warns,
    BanList,
    MuteList,
    Check,
    Alts,
    StaffHistory,
    Info,
    Reload,
    Import,
    Purge,
    Version,
}

/// Main command name (`/pumbobans`).
/// Short command for [`main_command`], on PumboProx (`short-alias`) and Pumpkin.
pub const ALIAS: &str = "pb";

pub fn main_command() -> String {
    format!("pumbo{ID}")
}

/// The command tree of PumboBans, shown as `/pumbobans <sub>`.
pub fn tree() -> Commands<Action> {
    use Action::*;
    let s = |name, action, a, usage, min, desc| Sub::new(name, action, a).usage(usage, min).description(desc);
    Commands::new(ID)
        .label(format!("/{}", main_command()))
        .short_alias(format!("/{ALIAS}"))
        .with(
            s("ban", "ban", Ban, "<player> [time] [reason|#template] [-s] [-f]", 1, "help-ban")
                .details("help-ban-details"),
        )
        .with(
            s("tempban", "tempban", TempBan, "<player> <time> [reason] [-s] [-f]", 2, "help-tempban")
                .aliases(&["tban"]),
        )
        .with(
            s("ipban", "ipban", IpBan, "<player|ip> [time] [reason] [-s]", 1, "help-ipban")
                .aliases(&["banip"])
                .details("help-ipban-details"),
        )
        .with(s("mute", "mute", Mute, "<player> [time] [reason|#template] [-s] [-f]", 1, "help-mute"))
        .with(
            s("tempmute", "tempmute", TempMute, "<player> <time> [reason] [-s] [-f]", 2, "help-tempmute")
                .aliases(&["tmute"]),
        )
        .with(s("ipmute", "ipmute", IpMute, "<player|ip> [time] [reason] [-s]", 1, "help-ipmute").aliases(&["muteip"]))
        .with(
            s("warn", "warn", Warn, "<player> [reason|#template] [-s] [-f]", 1, "help-warn")
                .details("help-warn-details"),
        )
        .with(s("kick", "kick", Kick, "<player> [reason] [-s]", 1, "help-kick"))
        .with(s("unban", "unban", Unban, "<player|ip|#id> [-s]", 1, "help-unban").aliases(&["pardon"]))
        .with(s("unbanip", "unbanip", UnbanIp, "<player|ip> [-s]", 1, "help-unbanip").aliases(&["unipban"]))
        .with(s("unmute", "unmute", Unmute, "<player|ip|#id> [-s]", 1, "help-unmute"))
        .with(s("unwarn", "unwarn", Unwarn, "<player|#id> [-s]", 1, "help-unwarn").aliases(&["delwarn"]))
        .with(s("history", "history", History, "<player|ip> [page]", 1, "help-history").aliases(&["hist"]))
        .with(s("warns", "warns", Warns, "<player>", 1, "help-warns").aliases(&["warnings"]))
        .with(s("banlist", "banlist", BanList, "[page]", 0, "help-banlist").aliases(&["bans"]))
        .with(s("mutelist", "mutelist", MuteList, "[page]", 0, "help-mutelist").aliases(&["mutes"]))
        .with(s("check", "check", Check, "<player>", 1, "help-check").aliases(&["checkban", "checkmute"]))
        .with(s("alts", "alts", Alts, "<player>", 1, "help-alts").aliases(&["dupeip"]))
        .with(
            s("staffhistory", "staffhistory", StaffHistory, "<staff> [page]", 1, "help-staffhistory")
                .aliases(&["blame"]),
        )
        .with(s("info", "history", Info, "<#id>", 1, "help-info"))
        .with(s("reload", "admin.reload", Reload, "", 0, "help-reload"))
        .with(s("import", "admin.import", Import, "vanilla", 1, "help-import"))
        .with(s("purge", "admin.purge", Purge, "<player>", 1, "help-purge").details("help-purge-details"))
        .with(s("version", "admin.version", Version, "", 0, "help-version"))
}

/// Short commands registered next to `/pumbobans`, as `(command, subcommand)`.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("ban", "ban"),
    ("tempban", "tempban"),
    ("ipban", "ipban"),
    ("banip", "ipban"),
    ("mute", "mute"),
    ("tempmute", "tempmute"),
    ("ipmute", "ipmute"),
    ("warn", "warn"),
    ("kick", "kick"),
    ("unban", "unban"),
    ("unbanip", "unbanip"),
    ("unmute", "unmute"),
    ("unwarn", "unwarn"),
    ("history", "history"),
    ("warns", "warns"),
    ("banlist", "banlist"),
    ("mutelist", "mutelist"),
    ("checkban", "check"),
    ("alts", "alts"),
    ("staffhistory", "staffhistory"),
];

/// Short commands to register, without the ones disabled in the config.
pub fn shortcuts(disabled: &[String]) -> Vec<(&'static str, &'static str)> {
    SHORTCUTS.iter().copied().filter(|(c, _)| !disabled.iter().any(|d| d == c)).collect()
}

/// Every permission node a sender may need, for platforms that have to ask the
/// server about each one before running a command.
pub fn permission_nodes(engine: &Engine) -> Vec<String> {
    let mut nodes: BTreeSet<String> = tree().subs().iter().map(|s| permission(ID, s.action)).collect();
    for extra in ["notify", "notify.silent", "notify.alts", "exempt.bypass", "viewips"] {
        nodes.insert(permission(ID, extra));
    }
    for k in Kind::ALL {
        nodes.insert(exempt_node(k));
    }
    for l in &engine.cfg.punishments.limits {
        nodes.insert(permission(ID, &format!("limit.{}", l.group)));
    }
    nodes.into_iter().collect()
}

/// `pumbo.bans.exempt.<kind>`.
pub fn exempt_node(kind: Kind) -> String {
    permission(ID, &format!("exempt.{}", kind.as_str()))
}

/// Who runs a command.
#[derive(Debug, Clone)]
pub struct Sender {
    pub actor: Actor,
    pub console: bool,
    /// Granted permission nodes (`pumbo.bans.ban`, ...). The console has all.
    pub granted: BTreeSet<String>,
    pub locale: Option<String>,
}

impl Sender {
    pub fn console() -> Self {
        Self { actor: Actor::console(CONSOLE), console: true, granted: BTreeSet::new(), locale: None }
    }

    pub fn player(uuid: Uuid, name: &str, granted: BTreeSet<String>, locale: Option<String>) -> Self {
        Self { actor: Actor::player(uuid, name), console: false, granted, locale }
    }

    pub fn has(&self, node: &str) -> bool {
        self.console || self.granted.contains(node)
    }

    fn has_action(&self, action: &str) -> bool {
        self.has(&permission(ID, action))
    }
}

/// A player who is online right now.
#[derive(Debug, Clone)]
pub struct Online {
    pub uuid: Uuid,
    pub name: String,
    pub ip: Option<IpAddr>,
    pub locale: Option<String>,
    /// Exemption permissions; platforms may fill this only for the player a
    /// command names.
    pub exempt: Vec<Kind>,
}

/// Everything a command needs to know about the world.
#[derive(Debug, Clone)]
pub struct Ctx<'a> {
    pub sender: &'a Sender,
    pub online: &'a [Online],
    pub now: u64,
    /// The short command typed (`ban`), for usage lines; `None` for `/pumbobans`.
    pub label: Option<&'a str>,
}

impl Ctx<'_> {
    fn online_by_name(&self, name: &str) -> Option<&Online> {
        let key = name_key(name);
        self.online.iter().find(|o| name_key(&o.name) == key)
    }

    fn online_by_uuid(&self, uuid: Uuid) -> Option<&Online> {
        self.online.iter().find(|o| o.uuid == uuid)
    }
}

/// The help of PumboBans for the sender.
pub fn help(lang: &Lang, sender: &Sender, page: usize) -> Text {
    let help = Help::from_commands("PumboBans", &tree(), lang)
        .version(env!("CARGO_PKG_VERSION"))
        .section(lang.get("help-section"));
    if sender.console { help.console(lang, |n| sender.has(n)) } else { help.chat(lang, page, |n| sender.has(n)) }
}

/// Runs `args` (the subcommand and its arguments) and returns what to do.
pub fn run(engine: &mut Engine, args: &[String], ctx: &Ctx<'_>) -> Effects {
    let mut fx = Effects::default();
    let lang = engine.lang_for(ctx.sender.locale.as_deref()).clone();
    let t = tree();
    match t.dispatch(args, |node| ctx.sender.has(node)) {
        Dispatch::Help => fx.reply(help(&lang, ctx.sender, pumbo_common::help::page_arg(args.get(1..).unwrap_or(&[])))),
        Dispatch::Unknown { name } => {
            // `/pumbobans 2` pages through the help.
            match name.parse::<usize>() {
                Ok(page) => fx.reply(help(&lang, ctx.sender, page)),
                Err(_) => fx.reply(style::unknown_subcommand(&lang, &name, &t.help_line())),
            }
        }
        Dispatch::NoPermission { .. } => fx.reply(style::error(&lang, "command-no-permission", &Args::new())),
        Dispatch::Usage { .. } => {
            let first = args.first().map(|a| a.to_lowercase()).unwrap_or_default();
            if let Some(sub) = t.subs().iter().find(|s| s.name == first || s.aliases.contains(&first.as_str())) {
                fx.reply(style::usage(&lang, &usage_line(&t, sub, ctx)));
            }
        }
        Dispatch::Run { sub, args: rest } => {
            let usage = style::usage(&lang, &usage_line(&t, sub, ctx));
            let mut c = Cmd { engine, lang: &lang, ctx, fx: &mut fx, usage };
            c.run(sub.handler, rest);
        }
    }
    fx
}

/// `/ban <player> ...` when typed as a short command, `/pumbobans ban ...` otherwise.
fn usage_line(t: &Commands<Action>, sub: &Sub<Action>, ctx: &Ctx<'_>) -> String {
    match ctx.label {
        Some(label) => format!("/{label} {}", sub.usage).trim_end().to_string(),
        None => t.usage(sub),
    }
}

/// Command arguments with `-s` / `-f` taken out.
struct Parsed {
    words: Vec<String>,
    silent: bool,
    force: bool,
}

fn parse_flags(args: &[String]) -> Parsed {
    let mut p = Parsed { words: Vec::new(), silent: false, force: false };
    for a in args {
        match a.to_lowercase().as_str() {
            "-s" | "--silent" => p.silent = true,
            "-f" | "--force" => p.force = true,
            _ => p.words.push(a.clone()),
        }
    }
    p
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LengthArg {
    None,
    Optional,
    Required,
}

struct Cmd<'a, 'b> {
    engine: &'a mut Engine,
    lang: &'a Lang,
    ctx: &'a Ctx<'b>,
    fx: &'a mut Effects,
    usage: Text,
}

impl Cmd<'_, '_> {
    fn say(&mut self, tone: Tone, key: &str, args: Args) {
        self.fx.reply(style::message(self.lang, tone, key, &args));
    }

    fn error(&mut self, key: &str, args: Args) {
        self.say(Tone::Error, key, args);
    }

    fn show_usage(&mut self) {
        let u = self.usage.clone();
        self.fx.reply(u);
    }

    fn page_size(&self) -> usize {
        self.engine.cfg.commands.page_size as usize
    }

    fn run(&mut self, action: Action, args: &[String]) {
        use Action::*;
        match action {
            Ban => self.punish(Kind::Ban, args, LengthArg::Optional, false),
            TempBan => self.punish(Kind::Ban, args, LengthArg::Required, false),
            IpBan => self.punish(Kind::Ban, args, LengthArg::Optional, true),
            Mute => self.punish(Kind::Mute, args, LengthArg::Optional, false),
            TempMute => self.punish(Kind::Mute, args, LengthArg::Required, false),
            IpMute => self.punish(Kind::Mute, args, LengthArg::Optional, true),
            Warn => self.punish(Kind::Warn, args, LengthArg::None, false),
            Kick => self.punish(Kind::Kick, args, LengthArg::None, false),
            Unban => self.revoke(Kind::Ban, args, false),
            UnbanIp => self.revoke(Kind::Ban, args, true),
            Unmute => self.revoke(Kind::Mute, args, false),
            Unwarn => self.revoke(Kind::Warn, args, false),
            History => self.history(args),
            Warns => self.warns(args),
            BanList => self.list(Kind::Ban, args),
            MuteList => self.list(Kind::Mute, args),
            Check => self.check(args),
            Alts => self.alts(args),
            StaffHistory => self.staff_history(args),
            Info => self.info(args),
            Reload => self.fx.push(Effect::Admin(AdminRequest::Reload)),
            Import => {
                if args.first().is_some_and(|a| a.eq_ignore_ascii_case("vanilla")) {
                    self.fx.push(Effect::Admin(AdminRequest::ImportVanilla));
                } else {
                    self.show_usage();
                }
            }
            Purge => self.purge(args),
            Version => {
                let e = &*self.engine;
                let db = if e.storage_ok() {
                    self.lang.get("admin-database-ok")
                } else {
                    self.lang.get("admin-database-down")
                };
                let rows = [
                    (self.lang.get("version-platform"), e.platform.clone()),
                    (self.lang.get("admin-database"), db),
                    (self.lang.get("admin-active"), style::number(self.lang, e.active_count(self.ctx.now) as u64)),
                ];
                self.fx.reply(style::version("PumboBans", env!("CARGO_PKG_VERSION"), rows));
            }
        }
    }

    // --------------------------------------------------------------
    // Targets

    /// An account by online name, UUID or remembered name; a never-seen name
    /// with `-f`.
    fn resolve_player(&mut self, token: &str, force: bool) -> Option<Resolved> {
        if let Some(o) = self.ctx.online_by_name(token) {
            return Some(Resolved::Player { uuid: o.uuid, name: o.name.clone(), exempt: o.exempt.clone() });
        }
        let uuid = match Uuid::parse(token) {
            Some(u) => Some(u),
            None => match self.engine.uuid_by_name(token) {
                Ok(u) => u,
                Err(_) => {
                    self.error("error-db", Args::new());
                    return None;
                }
            },
        };
        if let Some(uuid) = uuid {
            if let Some(o) = self.ctx.online_by_uuid(uuid) {
                return Some(Resolved::Player { uuid, name: o.name.clone(), exempt: o.exempt.clone() });
            }
            let rec = self.engine.player(uuid).ok().flatten();
            let name = rec.as_ref().map(|r| r.name.clone()).unwrap_or_else(|| token.to_string());
            let exempt = rec.map(|r| r.exempt).unwrap_or_default();
            return Some(Resolved::Player { uuid, name, exempt });
        }
        let target = style::value(token);
        if !self.engine.cfg.punishments.allow_unknown_names {
            self.error("error-unknown-names-disabled", Args::new().with("target", target));
            return None;
        }
        if force && is_java_name(token) {
            return Some(Resolved::Name { name: token.to_string() });
        }
        self.error("error-player-unknown", Args::new().with("target", target));
        None
    }

    /// An address or range typed directly, or the current / last address of a player.
    fn resolve_address(&mut self, token: &str) -> Option<Resolved> {
        let (v4, v6) = (self.engine.cfg.enforcement.ipv4_prefix, self.engine.cfg.enforcement.ipv6_prefix);
        if let Some(range) = net::parse_range(token, v4, v6) {
            return Some(Resolved::Address { net: range, via: None });
        }
        let Resolved::Player { uuid, name, .. } = self.resolve_player(token, false)? else { return None };
        let ip = match self.ctx.online_by_uuid(uuid).and_then(|o| o.ip) {
            Some(ip) => Some(ip),
            None => self
                .engine
                .player(uuid)
                .ok()
                .flatten()
                .and_then(|r| r.addresses.iter().max_by_key(|a| a.last).map(|a| a.value)),
        };
        match ip {
            Some(ip) => Some(Resolved::Address { net: net::punish_range(ip, v4, v6), via: Some((uuid, name)) }),
            None => {
                self.error("error-no-address", Args::new().with("target", style::value(&name)));
                None
            }
        }
    }

    // --------------------------------------------------------------
    // Punishing

    fn punish(&mut self, kind: Kind, args: &[String], length_arg: LengthArg, by_address: bool) {
        let p = parse_flags(args);
        let Some((token, rest)) = p.words.split_first() else {
            self.show_usage();
            return;
        };
        let mut rest: Vec<String> = rest.to_vec();
        let mut length: Option<Term> = None;
        if length_arg != LengthArg::None {
            match rest.first().map(|w| (w.clone(), parse_length(w))) {
                Some((_, Some(t))) => {
                    length = Some(t);
                    rest.remove(0);
                }
                Some((w, None)) if length_arg == LengthArg::Required => {
                    self.error("command-invalid-duration", Args::new().arg(style::value(&w)));
                    return;
                }
                None if length_arg == LengthArg::Required => {
                    self.show_usage();
                    return;
                }
                _ => {}
            }
        }
        let mut template = None;
        if let Some(first) = rest.first()
            && let Some(name) = first.strip_prefix('#')
            && !name.is_empty()
        {
            template = Some(name.to_lowercase());
            rest.remove(0);
        }
        let reason = Some(rest.join(" ")).filter(|r| !r.trim().is_empty());

        let target = if by_address { self.resolve_address(token) } else { self.resolve_player(token, p.force) };
        let Some(target) = target else { return };
        if !self.ctx.sender.console && target.uuid().is_some() && target.uuid() == self.ctx.sender.actor.uuid {
            self.error("error-self", Args::new());
            return;
        }
        if kind == Kind::Kick && target.uuid().and_then(|u| self.ctx.online_by_uuid(u)).is_none() {
            self.error("error-not-online", Args::new().with("target", style::value(target.label())));
            return;
        }
        let groups: Vec<String> = self
            .engine
            .cfg
            .punishments
            .limits
            .iter()
            .filter(|l| !self.ctx.sender.console && self.ctx.sender.has_action(&format!("limit.{}", l.group)))
            .map(|l| l.group.clone())
            .collect();
        let req = PunishRequest {
            kind,
            target: target.clone(),
            length,
            reason,
            template,
            silent: p.silent,
            operator: self.ctx.sender.actor.clone(),
            bypass_exempt: self.ctx.sender.has_action("exempt.bypass"),
            length_limit: self.engine.cfg.length_limit(kind, &groups),
            source: None,
        };
        match self.engine.punish(req, self.ctx.now) {
            Ok(done) => {
                let out = after_punish(self.engine, self.ctx, &done, true);
                self.fx.0.extend(out.0);
            }
            Err(e) => {
                let text = punish_error(self.engine, self.lang, e, kind, &target);
                self.fx.reply(text);
            }
        }
    }

    // --------------------------------------------------------------
    // Lifting

    fn revoke(&mut self, kind: Kind, args: &[String], by_address: bool) {
        let p = parse_flags(args);
        let Some(token) = p.words.first() else {
            self.show_usage();
            return;
        };
        let silent = p.silent;
        let by = self.ctx.sender.actor.clone();
        let now = self.ctx.now;
        let result: Result<Vec<Punishment>, (RevokeError, String)> = if let Some(id) = parse_id(token) {
            self.engine.revoke_id(id, Some(kind), &by, None, now).map(|p| vec![p]).map_err(|e| (e, format!("#{id}")))
        } else if kind == Kind::Warn {
            // A player's newest active warning.
            let Some(target) = self.resolve_player(token, true) else { return };
            match self.engine.active_warns_for(&target, now).first() {
                Some(w) => self
                    .engine
                    .revoke_id(w.id, Some(Kind::Warn), &by, None, now)
                    .map(|p| vec![p])
                    .map_err(|e| (e, target.label())),
                None => Err((RevokeError::NotPunished, target.label())),
            }
        } else {
            let cfg = &self.engine.cfg.enforcement;
            let target = match net::parse_range(token, cfg.ipv4_prefix, cfg.ipv6_prefix) {
                Some(range) => Some(Resolved::Address { net: range, via: None }),
                None if by_address => self.resolve_address(token),
                None => self.resolve_player(token, true),
            };
            let Some(target) = target else { return };
            let mut r = self.engine.revoke(kind, &target, &by, None, now);
            // A known player can also have a name punishment from before the first login.
            if matches!(r, Err(RevokeError::NotPunished))
                && let Resolved::Player { name, .. } = &target
            {
                r = self.engine.revoke(kind, &Resolved::Name { name: name.clone() }, &by, None, now);
            }
            r.map_err(|e| (e, target.label()))
        };
        match result {
            Ok(lifted) => {
                for p in &lifted {
                    self.fx.push(Effect::Lifted(Box::new(p.clone())));
                    let mine = render::notify_revoke(self.lang, &self.engine.cfg, p, &by, silent, now);
                    self.fx.reply(mine);
                    let staff = render::notify_revoke(self.engine.lang(), &self.engine.cfg, p, &by, silent, now);
                    self.notify(staff, silent);
                    if kind == Kind::Mute {
                        for o in self.affected(p) {
                            let lang = self.engine.lang_for(o.locale.as_deref());
                            let message = style::success(lang, "player-unmuted", &Args::new());
                            self.fx.push(Effect::Tell { uuid: o.uuid, message });
                        }
                    }
                }
            }
            Err((e, label)) => {
                let kind_label = render::plain_kind(self.lang, kind);
                match e {
                    RevokeError::NotPunished => self.error(
                        "error-not-punished",
                        Args::new().with("target", style::value(&label)).with("kind", kind_label),
                    ),
                    RevokeError::NoSuchId => self
                        .error("error-no-such-id", Args::new().with("id", style::value(label.trim_start_matches('#')))),
                    RevokeError::WrongKind(p) => self.error(
                        "error-not-punished",
                        Args::new().with("target", style::value(format!("#{}", p.id))).with("kind", kind_label),
                    ),
                    RevokeError::Storage(_) => self.error("error-db", Args::new()),
                }
            }
        }
    }

    /// Online players a punishment reaches through their account or address.
    fn affected(&self, p: &Punishment) -> Vec<Online> {
        self.ctx
            .online
            .iter()
            .filter(|o| p.targets_account(o.uuid, &o.name) || o.ip.is_some_and(|ip| p.covers_address(ip)))
            .cloned()
            .collect()
    }

    fn notify(&mut self, message: Text, silent: bool) {
        if self.engine.cfg.notifications.console {
            self.fx.push(Effect::Log(render::log_line(self.engine.lang(), &message)));
        }
        self.fx.push(Effect::Notify {
            message,
            silent,
            except: self.ctx.sender.actor.uuid,
            audience: Audience::Punishments,
        });
    }

    // --------------------------------------------------------------
    // Lookups

    fn history(&mut self, args: &[String]) {
        let Some(token) = args.first() else { return };
        let cfg = &self.engine.cfg.enforcement;
        let (list, label) = if let Some(range) = net::parse_range(token, cfg.ipv4_prefix, cfg.ipv6_prefix) {
            (self.engine.history_of_address(&range), net::label(&range))
        } else {
            match self.resolve_player(token, true) {
                Some(Resolved::Player { uuid, name, .. }) => (self.engine.history_of_player(uuid), name),
                Some(Resolved::Name { name }) => (self.engine.history_of_name(&name), name),
                _ => return,
            }
        };
        let Ok(list) = list else {
            self.error("error-db", Args::new());
            return;
        };
        let page_cmd = format!("/{} history {token}", main_command());
        let keys = ListKeys { header: "history-header", line: "history-line", empty: "history-empty" };
        self.show_list(&list, args.get(1), keys, &label, &page_cmd);
    }

    /// A header, one line per punishment (click: details) and page arrows.
    fn show_list(&mut self, list: &[Punishment], page: Option<&String>, keys: ListKeys, label: &str, page_cmd: &str) {
        if list.is_empty() {
            self.say(Tone::Info, keys.empty, Args::new().with("target", style::value(label)));
            return;
        }
        let size = self.page_size();
        let pages = list.len().div_ceil(size).max(1);
        let page = page.and_then(|p| p.parse::<usize>().ok()).unwrap_or(1).clamp(1, pages);
        let args = Args::new()
            .with("target", style::value(label))
            .with("count", style::value(style::number(self.lang, list.len() as u64)))
            .with("page", page)
            .with("pages", pages);
        let mut text = style::info(self.lang, keys.header, &args);
        let main = main_command();
        for p in list.iter().skip((page - 1) * size).take(size) {
            let info = render::line(self.lang, &self.engine.cfg, "info-line", p, self.ctx.now);
            let line = render::line(self.lang, &self.engine.cfg, keys.line, p, self.ctx.now)
                .on_click(Click::Suggest(format!("/{main} info #{}", p.id)))
                .on_hover(Text::from(info));
            text.push(line);
        }
        if pages > 1 {
            text.push(self.arrows(page, pages, page_cmd));
        }
        self.fx.reply(text);
    }

    /// `◀ 2/5 ▶`; the arrows run the previous and next page.
    fn arrows(&self, page: usize, pages: usize, page_cmd: &str) -> Line {
        let arrow = |symbol: &str, enabled: bool, target: usize, hover: &str| {
            let seg = if enabled {
                Segment::new(symbol, Style::colored(style::BRAND).bolded())
            } else {
                Segment::colored(symbol, style::MUTED)
            };
            if enabled {
                seg.on_click(Click::Run(format!("{page_cmd} {target}")))
                    .on_hover(Text::parse_with(&self.lang.get(hover), Style::colored(style::INFO)))
            } else {
                seg
            }
        };
        Line::new()
            .with(arrow("◀", page > 1, page.saturating_sub(1), "list-previous"))
            .with(Segment::colored(format!(" {page}/{pages} "), style::INFO))
            .with(arrow("▶", page < pages, page + 1, "list-next"))
    }

    fn warns(&mut self, args: &[String]) {
        let Some(token) = args.first() else { return };
        let Some(target) = self.resolve_player(token, true) else { return };
        let list = self.engine.active_warns_for(&target, self.ctx.now);
        let args = Args::new().with("target", style::value(target.label())).with("count", style::value(list.len()));
        let mut text = style::info(self.lang, "warns-header", &args);
        for p in &list {
            text.push(render::line(self.lang, &self.engine.cfg, "history-line", p, self.ctx.now));
        }
        self.fx.reply(text);
    }

    fn list(&mut self, kind: Kind, args: &[String]) {
        let list = self.engine.active_list(kind, self.ctx.now);
        let (header, sub) =
            if kind == Kind::Ban { ("banlist-header", "banlist") } else { ("mutelist-header", "mutelist") };
        let page_cmd = format!("/{} {sub}", main_command());
        let keys = ListKeys { header, line: "list-line", empty: "list-empty" };
        self.show_list(&list, args.first(), keys, "", &page_cmd);
    }

    fn check(&mut self, args: &[String]) {
        let Some(token) = args.first() else { return };
        let now = self.ctx.now;
        let (subject, label) = if let Some(ip) = parse_ip(token) {
            (Subject { uuid: None, name: None, ip: Some(ip) }, ip.to_string())
        } else {
            match self.resolve_player(token, true) {
                Some(Resolved::Player { uuid, name, .. }) => {
                    let ip = self.ctx.online_by_uuid(uuid).and_then(|o| o.ip);
                    (Subject::new(uuid, &name, ip), name)
                }
                Some(Resolved::Name { name }) => (Subject { uuid: None, name: Some(name.clone()), ip: None }, name),
                _ => return,
            }
        };
        let ban = self.engine.find_active(Kind::Ban, &subject, now);
        let mute = self.engine.find_active(Kind::Mute, &subject, now);
        let (Ok(ban), Ok(mute)) = (ban, mute) else {
            self.error("error-db", Args::new());
            return;
        };
        let mut any = false;
        for (p, key) in [(ban, "check-ban"), (mute, "check-mute")] {
            if let Some(p) = p {
                any = true;
                let args = render::value_args(self.lang, &self.engine.cfg, &p, now)
                    .with("target", pumbo_common::text::escape(&label));
                self.say(Tone::Info, key, args);
            }
        }
        if let Some(uuid) = subject.uuid {
            let target = Resolved::Player { uuid, name: label.clone(), exempt: Vec::new() };
            let warns = self.engine.active_warns_for(&target, now).len();
            if warns > 0 {
                any = true;
                let args = Args::new().with("target", style::value(&label)).with("count", style::value(warns));
                self.say(Tone::Info, "check-warns", args);
            }
        }
        if !any {
            self.say(Tone::Success, "check-clean", Args::new().with("target", style::value(&label)));
        }
    }

    fn alts(&mut self, args: &[String]) {
        let Some(token) = args.first() else { return };
        let Some(Resolved::Player { uuid, name, .. }) = self.resolve_player(token, false) else { return };
        let Ok(alts) = self.engine.alts(uuid, self.ctx.now) else {
            self.error("error-db", Args::new());
            return;
        };
        if alts.is_empty() {
            self.say(Tone::Info, "alts-none", Args::new().with("target", style::value(&name)));
            return;
        }
        let view_ips = self.ctx.sender.has_action("viewips");
        let mut text = style::info(self.lang, "alts-header", &Args::new().with("target", style::value(&name)));
        let main = main_command();
        for a in alts {
            let mut status = Vec::new();
            if a.banned {
                status.push(self.lang.get("alts-banned"));
            }
            if a.muted {
                status.push(self.lang.get("alts-muted"));
            }
            let address = if view_ips { a.address.to_string() } else { mask_ip(a.address) };
            let args = Args::new()
                .with("name", pumbo_common::text::escape(&a.name))
                .with("address", address)
                .with("status", status.join(" "));
            let line = Line::parse(&self.lang.format("alts-line", &args))
                .on_click(Click::Suggest(format!("/{main} history {}", a.name)));
            text.push(line);
        }
        self.fx.reply(text);
    }

    fn staff_history(&mut self, args: &[String]) {
        let Some(token) = args.first() else { return };
        let actor = if token.eq_ignore_ascii_case("console") || token.eq_ignore_ascii_case(CONSOLE) {
            Actor::console(CONSOLE)
        } else if name_key(token) == name_key(&self.ctx.sender.actor.name) {
            self.ctx.sender.actor.clone()
        } else {
            match self.resolve_player(token, false) {
                Some(Resolved::Player { uuid, name, .. }) => Actor::player(uuid, &name),
                _ => return,
            }
        };
        let Ok(list) = self.engine.history_of_operator(&actor) else {
            self.error("error-db", Args::new());
            return;
        };
        let label = render::actor_name(self.lang, &actor);
        let page_cmd = format!("/{} staffhistory {token}", main_command());
        let keys = ListKeys { header: "staffhistory-header", line: "history-line", empty: "history-empty" };
        self.show_list(&list, args.get(1), keys, &label, &page_cmd);
    }

    fn info(&mut self, args: &[String]) {
        let Some(id) = args.first().and_then(|a| parse_id(a)) else {
            self.show_usage();
            return;
        };
        match self.engine.get(id) {
            Ok(Some(p)) => {
                let line = render::line(self.lang, &self.engine.cfg, "info-line", &p, self.ctx.now);
                self.fx.reply(Text::from(style::prefix(self.lang).append(line)));
            }
            Ok(None) => self.error("error-no-such-id", Args::new().with("id", style::value(id))),
            Err(_) => self.error("error-db", Args::new()),
        }
    }

    fn purge(&mut self, args: &[String]) {
        let Some(token) = args.first() else { return };
        let Some(Resolved::Player { uuid, name, .. }) = self.resolve_player(token, false) else { return };
        match self.engine.purge_addresses(uuid) {
            Ok(n) => {
                let args = Args::new().with("target", style::value(&name)).with("count", style::value(n));
                self.say(Tone::Success, "admin-purge-done", args);
                if self.engine.cfg.notifications.console {
                    self.fx.push(Effect::Log(format!("addresses of {name} ({uuid}) forgotten: {n}")));
                }
            }
            Err(_) => self.error("error-db", Args::new()),
        }
    }
}

/// Message keys of one list.
struct ListKeys {
    header: &'static str,
    line: &'static str,
    empty: &'static str,
}

/// The reply to a refused punishment.
fn punish_error(engine: &Engine, lang: &Lang, e: PunishError, kind: Kind, target: &Resolved) -> Text {
    let label = style::value(target.label());
    match e {
        PunishError::Exempt => style::error(lang, "error-exempt", &Args::new().with("target", label)),
        PunishError::AlreadyPunished(p) => {
            let args = Args::new()
                .with("target", label)
                .with("kind", render::kind_label(lang, &p))
                .with("id", style::value(p.id));
            style::error(lang, "error-already", &args)
        }
        PunishError::UnknownTemplate(name) => {
            let list: Vec<String> = engine.cfg.templates.iter().map(|t| format!("#{}", t.name)).collect();
            let args =
                Args::new().with("name", style::value(format!("#{name}"))).with("list", style::value(list.join(", ")));
            style::error(lang, "error-unknown-template", &args)
        }
        PunishError::ReasonRequired => style::error(lang, "error-reason-required", &Args::new()),
        PunishError::LimitExceeded(limit) => {
            let args = Args::new()
                .with("kind", render::plain_kind(lang, kind))
                .with("limit", style::value(style::term(lang, limit)));
            style::error(lang, "error-limit", &args)
        }
        PunishError::Storage(_) => style::error(lang, "error-db", &Args::new()),
    }
}

/// `#12`.
fn parse_id(token: &str) -> Option<u64> {
    token.strip_prefix('#').and_then(|s| s.parse().ok())
}

/// `1.2.x.x` / `2001:db8:x:x::` for staff without `viewips`.
pub fn mask_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, _, _] = v4.octets();
            format!("{a}.{b}.x.x")
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{:x}:{:x}:x:x::", s.first().copied().unwrap_or(0), s.get(1).copied().unwrap_or(0))
        }
    }
}

/// Effects of a placed punishment: replies, staff notices, log lines, kicks of
/// banned players who are online, notices to muted and warned players. Also used
/// for punishments placed by other plugins (`reply` false).
pub fn after_punish(engine: &Engine, ctx: &Ctx<'_>, done: &Punished, reply: bool) -> Effects {
    let mut fx = Effects::default();
    let now = ctx.now;
    let cfg = &engine.cfg;
    let sender_lang = engine.lang_for(ctx.sender.locale.as_deref());
    let one = |p: &Punishment, fx: &mut Effects, escalation: bool| {
        fx.push(Effect::Placed(Box::new(p.clone())));
        let staff = if escalation {
            render::notify_escalation(engine.lang(), cfg, p, done.warn_count, now)
        } else {
            render::notify_punish(engine.lang(), cfg, p, now)
        };
        if reply {
            let mine = if escalation {
                render::notify_escalation(sender_lang, cfg, p, done.warn_count, now)
            } else {
                render::punished(sender_lang, cfg, p, now)
            };
            fx.reply(mine);
        }
        if cfg.notifications.console {
            fx.push(Effect::Log(render::log_line(engine.lang(), &staff)));
        }
        fx.push(Effect::Notify {
            message: staff,
            silent: p.silent,
            except: ctx.sender.actor.uuid,
            audience: Audience::Punishments,
        });
        for o in ctx.online {
            let lang = engine.lang_for(o.locale.as_deref());
            let reached = match p.kind {
                // Bans follow the enforcement rules, so a ban of an address a player
                // used before also removes them (normal strictness and above).
                Kind::Ban => engine
                    .find_active(Kind::Ban, &Subject::new(o.uuid, &o.name, o.ip), now)
                    .ok()
                    .flatten()
                    .is_some_and(|f| f.id == p.id),
                _ => p.targets_account(o.uuid, &o.name) || o.ip.is_some_and(|ip| p.covers_address(ip)),
            };
            if !reached {
                continue;
            }
            match p.kind {
                Kind::Ban => fx.push(Effect::Kick { uuid: o.uuid, screen: render::ban_screen(lang, cfg, p, now) }),
                Kind::Kick => fx.push(Effect::Kick { uuid: o.uuid, screen: render::kick_screen(lang, cfg, p, now) }),
                Kind::Mute => fx.push(Effect::Tell {
                    uuid: o.uuid,
                    message: render::about(lang, cfg, Tone::Info, "player-mute-notice", p, now),
                }),
                Kind::Warn => {
                    fx.push(Effect::Tell { uuid: o.uuid, message: render::warned(lang, cfg, p, done.warn_count, now) })
                }
            }
        }
    };
    one(&done.punishment, &mut fx, false);
    if let Some(e) = &done.escalated {
        one(e, &mut fx, true);
    }
    fx
}

/// A punishment requested by another plugin.
#[derive(Debug, Clone)]
pub struct PluginOrder {
    pub kind: Kind,
    pub by_address: bool,
    pub target: String,
    pub length: Option<Term>,
    pub reason: Option<String>,
    pub silent: bool,
}

/// Places a punishment for another plugin (IPC). Returns the punishment and its
/// effects (without replies), or the error as plain text.
pub fn punish_for_plugin(
    engine: &mut Engine,
    ctx: &Ctx<'_>,
    order: PluginOrder,
) -> Result<(Punished, Effects), String> {
    let PluginOrder { kind, by_address, target, length, reason, silent } = order;
    let lang = engine.lang().clone();
    let mut scratch = Effects::default();
    let resolved = {
        let mut c = Cmd { engine, lang: &lang, ctx, fx: &mut scratch, usage: Text::new() };
        if by_address { c.resolve_address(&target) } else { c.resolve_player(&target, true) }
    };
    let Some(resolved) = resolved else {
        let errors: Vec<String> = scratch.replies();
        let prefix = pumbo_common::text::strip(&lang.get("prefix"));
        return Err(errors
            .iter()
            .map(|e| e.strip_prefix(prefix.as_str()).unwrap_or(e).to_string())
            .collect::<Vec<_>>()
            .join(" "));
    };
    let req = PunishRequest {
        kind,
        target: resolved.clone(),
        length,
        reason,
        template: None,
        silent,
        operator: ctx.sender.actor.clone(),
        bypass_exempt: false,
        length_limit: None,
        source: Some(format!("ipc:{}", ctx.sender.actor.name)),
    };
    match engine.punish(req, ctx.now) {
        Ok(done) => {
            let fx = after_punish(engine, ctx, &done, false);
            Ok((done, fx))
        }
        Err(e) => Err(render::log_line(&lang, &punish_error(engine, &lang, e, kind, &resolved))),
    }
}

/// Tab completion for `/pumbobans` arguments (after the command name) and for
/// short commands (`label` set, `args` without the subcommand).
pub fn complete(engine: &Engine, args: &[String], ctx: &Ctx<'_>) -> Vec<String> {
    let t = tree();
    let (sub, rest): (Option<&Sub<Action>>, &[String]) = match ctx.label {
        Some(label) => (
            SHORTCUTS.iter().find(|(c, _)| *c == label).and_then(|(_, s)| t.subs().iter().find(|x| x.name == *s)),
            args,
        ),
        None => {
            if args.len() <= 1 {
                return t.complete(args, |n| ctx.sender.has(n));
            }
            let first = args.first().map(|a| a.to_lowercase()).unwrap_or_default();
            (
                t.subs().iter().find(|s| s.name == first || s.aliases.contains(&first.as_str())),
                args.get(1..).unwrap_or(&[]),
            )
        }
    };
    let Some(sub) = sub else { return Vec::new() };
    if !ctx.sender.has(&t.permission(sub)) {
        return Vec::new();
    }
    let typed = rest.last().map(|s| s.to_lowercase()).unwrap_or_default();
    let position = rest.len().max(1);
    let mut out: Vec<String> = Vec::new();
    let takes_player = sub.usage.starts_with("<player") || sub.usage.starts_with("<staff");
    let takes_time = sub.usage.contains("[time]") || sub.usage.contains("<time>");
    if position == 1 && takes_player {
        out.extend(ctx.online.iter().map(|o| o.name.clone()));
    } else if position == 2 && takes_time {
        out.extend(["30m", "1h", "1d", "7d", "30d", "perm"].iter().map(|s| (*s).to_string()));
        out.extend(engine.cfg.templates.iter().map(|t| format!("#{}", t.name)));
    } else if position == 2 && sub.usage.contains("#template") {
        out.extend(engine.cfg.templates.iter().map(|t| format!("#{}", t.name)));
    }
    if position >= 2 && sub.usage.contains("[-s]") {
        out.push("-s".into());
    }
    out.retain(|s| s.to_lowercase().starts_with(&typed));
    out
}

#[cfg(test)]
mod tests {
    use pumbo_common::config::load;
    use pumbo_common::lang::{Bundle, COMMON};

    use super::*;
    use crate::BansCfg;
    use crate::render::Langs;
    use crate::store::BansStore;

    const T0: u64 = 1_800_000_000_000;
    const ADMIN: Uuid = Uuid(1000);
    const PREFIX: Bundle = Bundle { name: "test", files: &[("en", "prefix: \"[B] \"")] };

    fn engine_with(cfg: &str) -> Engine {
        let (cfg, w) = load::<BansCfg>(cfg);
        assert!(w.is_empty(), "{w:?}");
        let lang = Lang::load(&[COMMON, crate::LANG, PREFIX], "en", None).0;
        Engine::new(cfg, Langs::single(lang), Ok(BansStore::in_memory()), "test").0
    }

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    fn admin() -> Sender {
        let granted = permission_nodes(&engine_with("")).into_iter().collect();
        Sender::player(ADMIN, "Admin", granted, None)
    }

    fn online(n: u128, name: &str, ip: &str) -> Online {
        Online { uuid: Uuid(n), name: name.into(), ip: parse_ip(ip), locale: None, exempt: Vec::new() }
    }

    fn exec(e: &mut Engine, sender: &Sender, players: &[Online], line: &str, now: u64) -> Effects {
        let ctx = Ctx { sender, online: players, now, label: None };
        run(e, &args(line), &ctx)
    }

    fn text(fx: &Effects) -> String {
        fx.replies().join("\n")
    }

    /// The staff notification of the first placed punishment.
    fn notice(fx: &Effects) -> String {
        fx.0.iter()
            .find_map(|f| if let Effect::Notify { message, .. } = f { Some(message.plain()) } else { None })
            .unwrap()
    }

    #[test]
    fn ban_kicks_online_player_and_blocks_login() {
        let mut e = engine_with("");
        let players = [online(1, "Steve", "1.1.1.1")];
        let fx = exec(&mut e, &admin(), &players, "ban Steve 1d griefing the spawn", T0);
        assert_eq!(text(&fx), "[B] Punished Steve for 1 day (temporary ban #1)");
        assert_eq!(notice(&fx), "[B] Admin banned Steve for 1 day: griefing the spawn (temporary ban #1)");
        assert_eq!(fx.kicks(), vec![Uuid(1)]);
        assert!(fx.0.iter().any(|f| matches!(f, Effect::Notify { except: Some(ADMIN), silent: false, .. })));
        assert!(fx.0.iter().any(|f| matches!(f, Effect::Log(l) if l.starts_with("Admin banned Steve"))));
        let screen = fx.0.iter().find_map(|f| match f {
            Effect::Kick { screen, .. } => Some(screen.plain()),
            _ => None,
        });
        assert!(screen.unwrap_or_default().contains("Reason: griefing the spawn"));
        let d = e.check_login(Uuid(1), "Steve", parse_ip("1.1.1.1"), T0 + 1000);
        assert!(matches!(d, crate::engine::LoginDecision::Deny { .. }));
        let fx = exec(&mut e, &admin(), &players, "ban Steve", T0);
        assert!(text(&fx).contains("already has an active temporary ban (#1)"), "{}", text(&fx));
        let fx = exec(&mut e, &admin(), &[], "unban Steve", T0 + 5);
        assert_eq!(text(&fx), "[B] Admin lifted a punishment of Steve (temporary ban #1)");
        let fx = exec(&mut e, &admin(), &[], "history Steve", T0 + 6);
        let t = text(&fx);
        assert!(t.contains("Punishments of Steve (1, page 1/1)") && t.contains("lifted by Admin"), "{t}");
    }

    #[test]
    fn unknown_names_need_force() {
        let mut e = engine_with("");
        let fx = exec(&mut e, &admin(), &[], "ban Nobody", T0);
        assert!(text(&fx).contains("has never joined"), "{}", text(&fx));
        let fx = exec(&mut e, &admin(), &[], "ban Nobody -f hacking -s", T0);
        assert_eq!(notice(&fx), "[B] [silent] Admin banned Nobody permanently: hacking (ban #1)");
        assert!(fx.0.iter().any(|f| matches!(f, Effect::Notify { silent: true, .. })));
        assert!(matches!(e.check_login(Uuid(5), "nobody", None, T0), crate::engine::LoginDecision::Deny { .. }));
    }

    #[test]
    fn tempban_requires_a_time_and_usage_uses_the_short_name() {
        let mut e = engine_with("");
        e.record_login(Uuid(1), "Steve", None, T0).unwrap();
        let fx = exec(&mut e, &admin(), &[], "tempban Steve soon", T0);
        assert!(text(&fx).contains("Invalid time soon"), "{}", text(&fx));
        let sender = admin();
        let ctx = Ctx { sender: &sender, online: &[], now: T0, label: Some("tempban") };
        let fx = run(&mut e, &args("tempban Steve"), &ctx);
        assert!(text(&fx).contains("Usage: /tempban <player> <time>"), "{}", text(&fx));
        let fx = exec(&mut e, &admin(), &[], "tempban", T0);
        assert!(text(&fx).contains("Usage: /pumbobans tempban <player> <time>"), "{}", text(&fx));
    }

    #[test]
    fn ipban_hits_other_accounts_on_the_address() {
        let mut e = engine_with("");
        let players = [online(1, "Steve", "5.5.5.5"), online(2, "Alt", "5.5.5.5"), online(3, "Other", "6.6.6.6")];
        let fx = exec(&mut e, &admin(), &players, "ipban Steve 2h ban evasion", T0);
        let mut kicked = fx.kicks();
        kicked.sort();
        assert_eq!(kicked, vec![Uuid(1), Uuid(2)]);
        assert!(text(&fx).contains("Punished Steve (5.5.5.5) for 2 hours (IP ban"), "{}", text(&fx));
        let fx = exec(&mut e, &admin(), &[], "unbanip 5.5.5.5", T0);
        assert!(text(&fx).contains("lifted"), "{}", text(&fx));
    }

    #[test]
    fn mute_and_warn_with_escalation() {
        let mut e = engine_with("");
        let players = [online(1, "Steve", "1.1.1.1")];
        let fx = exec(&mut e, &admin(), &players, "mute Steve 10m spam", T0);
        assert!(fx.0.iter().any(|f| matches!(f, Effect::Tell { uuid, message } if *uuid == Uuid(1) && message.plain().contains("Admin muted you for 10 minutes"))));
        assert!(e.mute_for(&Subject::new(Uuid(1), "Steve", None), T0).is_some());
        let fx = exec(&mut e, &admin(), &players, "unmute Steve", T0);
        assert!(fx.0.iter().any(|f| matches!(f, Effect::Tell { message, .. } if message.plain().contains("lifted"))));
        for _ in 0..2 {
            exec(&mut e, &admin(), &players, "warn Steve language", T0);
        }
        let fx = exec(&mut e, &admin(), &players, "warn Steve language", T0);
        let t = text(&fx);
        assert!(t.contains("Warning limit (3) reached by Steve: temporary mute for 1 hour (#5)"), "{t}");
        assert!(
            fx.0.iter()
                .any(|f| matches!(f, Effect::Tell { message, .. } if message.plain().contains("active warnings: 3")))
        );
        let fx = exec(&mut e, &admin(), &players, "warns Steve", T0);
        assert!(text(&fx).contains("Active warnings of Steve: 3"));
        let fx = exec(&mut e, &admin(), &players, "unwarn Steve", T0);
        assert!(text(&fx).contains("lifted a punishment of Steve (warning"), "{}", text(&fx));
        let fx = exec(&mut e, &admin(), &players, "check Steve", T0);
        let t = text(&fx);
        assert!(t.contains("Steve is muted, time left: 1 hour") && t.contains("active warnings: 2"), "{t}");
    }

    #[test]
    fn kick_needs_an_online_player() {
        let mut e = engine_with("");
        e.record_login(Uuid(1), "Steve", None, T0).unwrap();
        let fx = exec(&mut e, &admin(), &[], "kick Steve bye", T0);
        assert!(text(&fx).contains("Steve is not online"));
        let players = [online(1, "Steve", "1.1.1.1")];
        let fx = exec(&mut e, &admin(), &players, "kick Steve bye", T0);
        assert_eq!(fx.kicks(), vec![Uuid(1)]);
        assert!(fx.0.iter().any(|f| matches!(f, Effect::Kick { screen, .. } if screen.plain().contains("bye"))));
    }

    #[test]
    fn permissions_exemption_and_self() {
        let mut e = engine_with("");
        let mut target = online(1, "Owner", "1.1.1.1");
        target.exempt = vec![Kind::Ban];
        let players = [target];
        let helper = Sender::player(Uuid(50), "Helper", [permission(ID, "ban")].into_iter().collect(), None);
        let fx = exec(&mut e, &helper, &players, "ban Owner", T0);
        assert!(text(&fx).contains("protected"), "{}", text(&fx));
        let fx = exec(&mut e, &helper, &players, "mute Owner", T0);
        assert!(text(&fx).contains("don't have permission"), "{}", text(&fx));
        let fx = exec(&mut e, &helper, &[online(50, "Helper", "2.2.2.2")], "ban Helper", T0);
        assert!(text(&fx).contains("cannot punish yourself"));
        let fx = exec(&mut e, &admin(), &players, "ban Owner", T0);
        assert_eq!(fx.kicks(), vec![Uuid(1)]);
    }

    #[test]
    fn limits_apply_to_groups() {
        let mut e = engine_with("punishments:\n  limits:\n    - group: helper\n      ban: \"1d\"\n");
        e.record_login(Uuid(1), "Steve", None, T0).unwrap();
        let granted = [permission(ID, "ban"), permission(ID, "limit.helper")].into_iter().collect();
        let helper = Sender::player(Uuid(50), "Helper", granted, None);
        let fx = exec(&mut e, &helper, &[], "ban Steve 2d", T0);
        assert!(text(&fx).contains("at most 1 day"), "{}", text(&fx));
        let fx = exec(&mut e, &helper, &[], "ban Steve", T0);
        assert!(text(&fx).contains("at most 1 day"), "{}", text(&fx));
        let fx = exec(&mut e, &helper, &[], "ban Steve 12h", T0);
        assert!(text(&fx).contains("for 12 hours (temporary ban"), "{}", text(&fx));
    }

    #[test]
    fn templates_from_commands() {
        let mut e = engine_with("");
        e.record_login(Uuid(1), "Steve", None, T0).unwrap();
        let fx = exec(&mut e, &admin(), &[], "ban Steve #cheating fly", T0);
        assert!(notice(&fx).contains("for 7 days: Cheating (fly)"), "{}", notice(&fx));
        let fx = exec(&mut e, &admin(), &[], "mute Steve #nope", T0);
        let t = text(&fx);
        assert!(t.contains("There is no template #nope. Templates: #cheating, #spam, #griefing"), "{t}");
    }

    #[test]
    fn lists_alts_info_purge_and_version() {
        let mut e = engine_with("");
        e.record_login(Uuid(1), "Steve", parse_ip("8.8.8.8"), T0).unwrap();
        e.record_login(Uuid(2), "Alex", parse_ip("8.8.8.8"), T0).unwrap();
        exec(&mut e, &admin(), &[], "ban Alex hacking", T0);
        let fx = exec(&mut e, &admin(), &[], "banlist", T0);
        assert!(text(&fx).contains("Active bans (1, page 1/1)"), "{}", text(&fx));
        let fx = exec(&mut e, &admin(), &[], "mutelist", T0);
        assert!(text(&fx).contains("Nothing to show"));
        let fx = exec(&mut e, &admin(), &[], "alts Steve", T0);
        assert!(text(&fx).contains("Alex (8.8.8.8) [banned]"), "{}", text(&fx));
        let limited = Sender::player(Uuid(50), "Helper", [permission(ID, "alts")].into_iter().collect(), None);
        let fx = exec(&mut e, &limited, &[], "alts Steve", T0);
        assert!(text(&fx).contains("Alex (8.8.x.x)"), "{}", text(&fx));
        let fx = exec(&mut e, &admin(), &[], "info #1", T0);
        assert!(text(&fx).contains("#1 ban of Alex"), "{}", text(&fx));
        let fx = exec(&mut e, &admin(), &[], "staffhistory Admin", T0);
        assert!(text(&fx).contains("Punishments given by Admin (1"), "{}", text(&fx));
        let fx = exec(&mut e, &admin(), &[], "purge Steve", T0);
        assert!(text(&fx).contains("Forgot 1 IP addresses of Steve"));
        let fx = exec(&mut e, &admin(), &[], "version", T0);
        let t = text(&fx);
        assert!(
            t.contains("PumboBans 0.1.0") && t.contains("Database: ok") && t.contains("Active punishments: 1"),
            "{t}"
        );
        let fx = exec(&mut e, &admin(), &[], "reload", T0);
        assert_eq!(fx.0, vec![Effect::Admin(AdminRequest::Reload)]);
        let fx = exec(&mut e, &Sender::console(), &[], "import vanilla", T0);
        assert_eq!(fx.0, vec![Effect::Admin(AdminRequest::ImportVanilla)]);
    }

    #[test]
    fn history_pages_have_arrows() {
        let mut e = engine_with("commands:\n  page-size: 2\n");
        e.record_login(Uuid(1), "Steve", None, T0).unwrap();
        for _ in 0..5 {
            exec(&mut e, &admin(), &[], "warn Steve x", T0);
        }
        let fx = exec(&mut e, &admin(), &[], "history Steve 2", T0);
        let Some(Effect::Reply(t)) = fx.0.first() else { panic!("{fx:?}") };
        let last = t.lines.last().unwrap();
        assert_eq!(last.plain(), "◀ 2/4 ▶");
        let next = last.segments.last().unwrap();
        assert_eq!(next.click, Some(Click::Run("/pumbobans history Steve 3".into())));
        let first_entry = t.lines.get(1).unwrap().segments.first().unwrap();
        assert_eq!(first_entry.click, Some(Click::Suggest("/pumbobans info #5".into())));
    }

    #[test]
    fn help_pages_and_console() {
        let mut e = engine_with("");
        let fx = exec(&mut e, &admin(), &[], "help", T0);
        let t = text(&fx);
        let header = format!("PumboBans {} · Admin   /pumbobans /pb", env!("CARGO_PKG_VERSION"));
        assert!(t.contains(&header) && t.contains("ban <player>"), "{t}");
        assert!(t.contains("1/3"), "{t}");
        let fx = exec(&mut e, &Sender::console(), &[], "", T0);
        let t = text(&fx);
        assert!(t.contains("staffhistory") && t.contains("version") && !t.contains("1/3"), "{t}");
        let nobody = Sender::player(Uuid(9), "Nobody", BTreeSet::new(), None);
        let fx = exec(&mut e, &nobody, &[], "help", T0);
        assert!(!text(&fx).contains("ban <player>"), "{}", text(&fx));
        let fx = exec(&mut e, &admin(), &[], "frobnicate", T0);
        assert!(text(&fx).contains("Unknown subcommand frobnicate"), "{}", text(&fx));
    }

    #[test]
    fn completion() {
        let e = engine_with("");
        let sender = admin();
        let players = [online(1, "Steve", "1.1.1.1"), online(2, "Sam", "1.1.1.2")];
        let ctx = Ctx { sender: &sender, online: &players, now: T0, label: None };
        assert!(complete(&e, &args("te"), &ctx).contains(&"tempban".to_string()));
        assert_eq!(complete(&e, &args("ban St"), &ctx), vec!["Steve".to_string()]);
        let ctx = Ctx { sender: &sender, online: &players, now: T0, label: Some("ban") };
        assert_eq!(complete(&e, &args("Steve #c"), &ctx), vec!["#cheating".to_string()]);
        let c = complete(&e, &args("Steve 1"), &ctx);
        assert!(c.contains(&"1h".to_string()) && c.contains(&"1d".to_string()));
    }

    #[test]
    fn shortcuts_can_be_disabled() {
        let s = shortcuts(&["kick".to_string()]);
        assert!(!s.iter().any(|(c, _)| *c == "kick"));
        assert!(s.iter().any(|(c, _)| *c == "ban"));
        for (_, sub) in SHORTCUTS {
            assert!(tree().subs().iter().any(|s| s.name == *sub), "{sub}");
        }
    }
}
