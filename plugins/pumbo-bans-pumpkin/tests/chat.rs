//! What PumboBans shows on Pumpkin, kept in `tests/snapshots/chat.txt`: the
//! replies to `/pumbobans` and the short commands, staff notifications, what a
//! punished player sees, the ban screen and the console. A screen starts with
//! `== <screen> <who>: <command or event>`; every chat message is one line of
//! its JSON text component, other places say what they are (`notify:`,
//! `tell <player>:`, `kick:`, `screen:`), and the console gets its plain text
//! indented. The screens carry the numbers of the chat UI mockups (B1-B10,
//! S2, S3). `PUMBO_SNAPSHOT_WRITE=1` writes the file anew.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::fmt::Write as _;
use std::path::PathBuf;

use pumbo_bans_core::commands::{self, Ctx, Online, Sender};
use pumbo_bans_core::effects::{Effect, Effects};
use pumbo_bans_core::engine::{LoginDecision, Subject};
use pumbo_bans_core::render::{self, Langs};
use pumbo_bans_core::store::BansStore;
use pumbo_bans_core::{BansCfg, Engine};
use pumbo_bans_pumpkin::setup::BUNDLES;
use pumbo_common::config::load;
use pumbo_common::id::{Uuid, parse_ip};
use pumbo_common::lang::Lang;
use pumbo_common::rich::{self, Text};

/// 2026-11-12 14:00 UTC.
const T0: u64 = 1_794_492_000_000;
const HOUR: u64 = 3_600_000;
const DAY: u64 = 24 * HOUR;
const ADMIN: Uuid = Uuid(1000);
const STEVE: Uuid = Uuid(1);
const ALEX: Uuid = Uuid(2);
const BOB: Uuid = Uuid(3);
const CARL: Uuid = Uuid(4);

#[derive(Default)]
struct Snapshot(String);

impl Snapshot {
    fn screen(&mut self, screen: &str, who: &str, what: &str) {
        writeln!(self.0, "\n== {screen} {who}: {what}").unwrap();
    }

    fn chat(&mut self, t: &Text) {
        writeln!(self.0, "{}", rich::json(t)).unwrap();
    }

    fn other(&mut self, place: &str, t: &Text) {
        writeln!(self.0, "{place}: {}", rich::json(t)).unwrap();
    }

    fn console(&mut self, t: &Text) {
        for line in t.plain().lines() {
            writeln!(self.0, "  {line}").unwrap();
        }
    }

    /// Everything a command does that someone sees, in order.
    fn effects(&mut self, fx: &Effects, console: bool) {
        for e in &fx.0 {
            match e {
                Effect::Reply(t) if console => self.console(t),
                Effect::Reply(t) => self.chat(t),
                Effect::Notify { message, .. } => self.other("notify", message),
                Effect::Tell { uuid, message } => self.other(&format!("tell {}", name(*uuid)), message),
                Effect::Kick { uuid, screen } => self.other(&format!("kick {}", name(*uuid)), screen),
                _ => {}
            }
        }
    }
}

fn name(uuid: Uuid) -> &'static str {
    match uuid {
        STEVE => "Steve",
        ALEX => "Alex",
        BOB => "Bob",
        CARL => "Carl",
        _ => "?",
    }
}

fn engine(code: &str) -> Engine {
    let (cfg, w) = load::<BansCfg>("");
    assert!(w.is_empty(), "{w:?}");
    let (lang, w) = Lang::load(&BUNDLES, code, None);
    assert!(w.is_empty(), "{w:?}");
    Engine::new(cfg, Langs::single(lang), Ok(BansStore::in_memory()), "Pumpkin 0.2.2 (MC 26.3)").0
}

fn admin(e: &Engine) -> Sender {
    Sender::player(ADMIN, "Admin", commands::permission_nodes(e).into_iter().collect(), None)
}

fn players() -> Vec<Online> {
    [
        (STEVE, "Steve", "203.0.113.7"),
        (ALEX, "Alex", "203.0.113.7"),
        (BOB, "Bob", "198.51.100.20"),
        (CARL, "Carl", "198.51.100.21"),
    ]
    .into_iter()
    .map(|(uuid, name, ip)| Online { uuid, name: name.into(), ip: parse_ip(ip), locale: None, exempt: Vec::new() })
    .collect()
}

/// Runs a command line; `label` is the short command typed (`ban`), `None` for `/pumbobans`.
fn exec(e: &mut Engine, sender: &Sender, label: Option<&str>, line: &str, now: u64) -> Effects {
    let args: Vec<String> = line.split_whitespace().map(String::from).collect();
    let online = players();
    let ctx = Ctx { sender, online: &online, now, label };
    commands::run(e, &args, &ctx)
}

/// `/<label> <rest>`: the short command, or `/pumbobans` with `label` `None`.
fn shown(label: Option<&str>, line: &str) -> String {
    match label {
        Some(l) => format!("/{l} {}", line.split_once(' ').map_or("", |(_, r)| r)),
        None => format!("/pumbobans {line}"),
    }
}

fn step(s: &mut Snapshot, e: &mut Engine, screen: &str, label: Option<&str>, line: &str, now: u64) {
    let a = admin(e);
    s.screen(screen, "admin", shown(label, line).trim_end());
    let fx = exec(e, &a, label, line, now);
    s.effects(&fx, false);
}

/// Every player has joined once (names and addresses are known).
fn joined(e: &mut Engine) {
    for o in players() {
        e.record_login(o.uuid, &o.name, o.ip, T0 - 30 * DAY).unwrap();
    }
}

fn help_and_tones(s: &mut Snapshot) {
    let mut e = engine("en");
    let a = admin(&e);
    for page in ["help", "help 2", "help 3", "2"] {
        s.screen("B1", "admin", &format!("/pumbobans {page}"));
        s.effects(&exec(&mut e, &a, None, page, T0), false);
    }
    s.screen("B1", "console", "pumbobans help");
    s.effects(&exec(&mut e, &Sender::console(), None, "help", T0), true);
    let nobody = Sender::player(Uuid(77), "Guest", Default::default(), None);
    s.screen("S2", "player", "/pumbobans ban Steve");
    s.effects(&exec(&mut e, &nobody, None, "ban Steve", T0), false);
    s.screen("S2", "player", "/pumbobans help");
    s.effects(&exec(&mut e, &nobody, None, "help", T0), false);
    joined(&mut e);
    step(s, &mut e, "S2", None, "bna Steve", T0);
    step(s, &mut e, "S2", Some("tempban"), "tempban Steve", T0);
    step(s, &mut e, "S2", None, "tempban", T0);
    step(s, &mut e, "S2", None, "ban Nobody", T0);
    step(s, &mut e, "S2", None, "version", T0);
    step(s, &mut e, "S3", Some("banlist"), "banlist", T0);
}

/// Punishments and what staff and the punished players see (B3, B4, B10).
fn punish(s: &mut Snapshot, e: &mut Engine) {
    joined(e);
    step(s, e, "B3", Some("ban"), "ban Steve 7d griefing", T0);
    step(s, e, "B3", Some("mute"), "mute Alex 2h spam -s", T0 + 1_000);
    step(s, e, "B3", Some("warn"), "warn Bob bad language", T0 + 2_000);
    step(s, e, "B3", Some("kick"), "kick Carl afk in the arena", T0 + 3_000);
    step(s, e, "B3", Some("warn"), "warn Bob spam", T0 + 4_000);
    step(s, e, "B3", Some("warn"), "warn Bob caps", T0 + 5_000);
    step(s, e, "B3", Some("unmute"), "unmute Alex", T0 + 6_000);
    step(s, e, "B3", Some("ban"), "ban Steve", T0 + 7_000);
    step(s, e, "B3", Some("ipban"), "ipban 203.0.113.0/24 2h bot attack", T0 + 8_000);
    // a mute that lasts, for the line a muted player gets when chatting
    step(s, e, "B4", Some("mute"), "mute Alex 2h spam", T0 + 9_000);
    let mute = e.mute_for(&Subject::new(ALEX, "Alex", parse_ip("203.0.113.7")), T0 + 10_000).unwrap();
    s.screen("B4", "Alex", "chat while muted");
    s.chat(&render::muted(e.lang(), &e.cfg, &mute, T0 + 10_000));
    s.screen("B10", "Steve", "join while banned");
    match e.check_login(STEVE, "Steve", parse_ip("203.0.113.7"), T0 + DAY) {
        LoginDecision::Deny { screen, .. } => s.other("screen", &screen),
        LoginDecision::Allow { .. } => panic!("Steve is banned"),
    }
}

/// Lists, one punishment, a player check, accounts on one address (B5-B9).
fn views(s: &mut Snapshot, e: &mut Engine) {
    step(s, e, "B5", Some("history"), "history Steve", T0 + HOUR);
    step(s, e, "B5", Some("history"), "history Bob", T0 + HOUR);
    step(s, e, "B6", None, "info #1", T0 + HOUR);
    step(s, e, "B7", Some("banlist"), "banlist", T0 + HOUR);
    step(s, e, "B7", Some("mutelist"), "mutelist", T0 + HOUR);
    step(s, e, "B8", Some("checkban"), "check Steve", T0 + HOUR);
    step(s, e, "B8", Some("checkban"), "check Bob", T0 + HOUR);
    step(s, e, "B8", Some("warns"), "warns Bob", T0 + HOUR);
    step(s, e, "B9", Some("alts"), "alts Steve", T0 + HOUR);
    step(s, e, "B5", Some("staffhistory"), "staffhistory Admin", T0 + HOUR);
    step(s, e, "B3", Some("unban"), "unban Steve", T0 + HOUR);
    step(s, e, "B5", Some("history"), "history Steve", T0 + HOUR);
    s.screen("B5", "console", "history Steve");
    s.effects(&exec(e, &Sender::console(), None, "history Steve", T0 + HOUR), true);
}

/// Many bans, for the pages of a list (S3).
fn pages(s: &mut Snapshot) {
    let mut e = engine("en");
    let a = admin(&e);
    for n in 0..12u128 {
        let name = format!("Griefer{n}");
        e.record_login(Uuid(500 + n), &name, None, T0 - DAY).unwrap();
        exec(&mut e, &a, Some("ban"), &format!("ban {name} {}d griefing", n + 1), T0 + n as u64);
    }
    step(s, &mut e, "S3", Some("banlist"), "banlist", T0 + HOUR);
    step(s, &mut e, "S3", Some("banlist"), "banlist 2", T0 + HOUR);
}

#[test]
fn chat_screens() {
    let mut s = Snapshot::default();
    s.0.push_str("PumboBans on Pumpkin: chat screens. Written by tests/chat.rs (PUMBO_SNAPSHOT_WRITE=1).\n");
    help_and_tones(&mut s);
    let mut e = engine("en");
    punish(&mut s, &mut e);
    views(&mut s, &mut e);
    let mut pl = engine("pl");
    s.screen("B4", "pl", "the same punishments in Polish");
    joined(&mut pl);
    let a = admin(&pl);
    for (label, line, at) in
        [("mute", "mute Alex 2h spam", 0), ("warn", "warn Bob bad language", 1), ("unmute", "unmute Alex", 2)]
    {
        let fx = exec(&mut pl, &a, Some(label), line, T0 + at);
        s.effects(&fx, false);
    }
    pages(&mut s);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/chat.txt");
    if std::env::var_os("PUMBO_SNAPSHOT_WRITE").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &s.0).unwrap();
        return;
    }
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    if old != s.0 {
        let first = old.lines().zip(s.0.lines()).position(|(a, b)| a != b).unwrap_or(0);
        panic!(
            "tests/snapshots/chat.txt differs from line {} on (PUMBO_SNAPSHOT_WRITE=1 writes it anew):\n- {}\n+ {}",
            first + 1,
            old.lines().nth(first).unwrap_or(""),
            s.0.lines().nth(first).unwrap_or("")
        );
    }
}
