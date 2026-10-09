//! Punishments as text, in the shared Pumbo style: the ban screen, replies and
//! notifications with a tone, list lines; and the choice of language per player.

use std::collections::HashMap;
use std::time::Duration;

use pumbo_common::lang::Lang;
use pumbo_common::rich::{Click, Line, Text};
use pumbo_common::style::{self, Tone};
use pumbo_common::text::{Args, Style, escape, strip};

use crate::commands::ALIAS;
use crate::config::BansCfg;
use crate::dates::format_date_zoned as format_date;
use crate::model::{Actor, Kind, Punishment, Target};
use crate::net;

/// Name the console is stored under; shown translated.
pub const CONSOLE: &str = "Console";

/// The loaded languages: the configured default and the others.
#[derive(Debug, Clone)]
pub struct Langs {
    pub default: Lang,
    pub others: HashMap<String, Lang>,
    pub per_player: bool,
}

impl Langs {
    pub fn single(lang: Lang) -> Self {
        Self { default: lang, others: HashMap::new(), per_player: false }
    }

    /// The language for a client locale such as `pl_pl` or `en_US`.
    pub fn for_locale(&self, locale: Option<&str>) -> &Lang {
        if !self.per_player {
            return &self.default;
        }
        let Some(loc) = locale else { return &self.default };
        let code = loc.split(['_', '-']).next().unwrap_or("").to_lowercase();
        if code == self.default.code() {
            return &self.default;
        }
        self.others.get(&code).unwrap_or(&self.default)
    }
}

/// A span in words (`2 days 3 hours`), rounded up to whole seconds so that an
/// active punishment never shows zero.
pub fn duration(lang: &Lang, ms: u64) -> String {
    style::duration(lang, Duration::from_secs(ms.div_ceil(1000)))
}

/// Name of whoever acted, unescaped (`Console` translated).
pub fn actor_name(lang: &Lang, a: &Actor) -> String {
    if a.uuid.is_none() && a.name == CONSOLE { lang.get("console-name") } else { a.name.clone() }
}

/// `ban`, `temporary ban`, `IP ban`, ...
pub fn kind_label(lang: &Lang, p: &Punishment) -> String {
    let key = match (p.kind, &p.target, p.expires.is_some()) {
        (Kind::Ban, Target::Address { .. }, _) => "kind-ipban",
        (Kind::Ban, _, true) => "kind-tempban",
        (Kind::Ban, _, false) => "kind-ban",
        (Kind::Mute, Target::Address { .. }, _) => "kind-ipmute",
        (Kind::Mute, _, true) => "kind-tempmute",
        (Kind::Mute, _, false) => "kind-mute",
        (Kind::Warn, _, _) => "kind-warn",
        (Kind::Kick, _, _) => "kind-kick",
    };
    lang.get(key)
}

/// Label of a kind without a punishment (`has no active ban`).
pub fn plain_kind(lang: &Lang, kind: Kind) -> String {
    lang.get(match kind {
        Kind::Ban => "kind-ban",
        Kind::Mute => "kind-mute",
        Kind::Warn => "kind-warn",
        Kind::Kick => "kind-kick",
    })
}

/// `banned`, `IP-banned`, `muted`, ... for `{action}`.
fn action_label(lang: &Lang, p: &Punishment) -> String {
    let ip = matches!(p.target, Target::Address { .. });
    lang.get(match (p.kind, ip) {
        (Kind::Ban, true) => "action-ipban",
        (Kind::Ban, false) => "action-ban",
        (Kind::Mute, true) => "action-ipmute",
        (Kind::Mute, false) => "action-mute",
        (Kind::Warn, _) => "action-warn",
        (Kind::Kick, _) => "action-kick",
    })
}

/// ` for 7 days` / ` permanently` for bans and mutes; nothing for warnings and kicks.
fn length_phrase(lang: &Lang, p: &Punishment) -> String {
    match (p.kind, p.length()) {
        (Kind::Ban | Kind::Mute, Some(ms)) => {
            lang.format("length-for", &Args::new().with("length", duration(lang, ms)))
        }
        (Kind::Ban | Kind::Mute, None) => lang.get("length-permanent"),
        _ => String::new(),
    }
}

pub fn remaining_text(lang: &Lang, p: &Punishment, now: u64) -> String {
    match p.remaining(now) {
        Some(ms) => duration(lang, ms),
        None => lang.get("time-permanent"),
    }
}

pub fn expires_text(lang: &Lang, cfg: &BansCfg, p: &Punishment) -> String {
    match p.expires {
        Some(e) => format_date(e, cfg.notifications.timezone_offset_minutes),
        None => lang.get("date-never"),
    }
}

/// The reason as typed (unescaped), or the "no reason" text.
pub fn reason_raw(lang: &Lang, p: &Punishment) -> String {
    if p.reason.trim().is_empty() { lang.get("no-reason") } else { p.reason.clone() }
}

pub fn status(lang: &Lang, p: &Punishment, now: u64) -> String {
    if let Some(r) = &p.revoked {
        return lang.format("status-revoked", &Args::new().with("by", escape(&actor_name(lang, &r.by))));
    }
    if !p.kind.is_lasting() {
        return lang.get("status-done");
    }
    if !p.is_active(now) {
        return lang.get("status-expired");
    }
    match p.remaining(now) {
        Some(ms) => lang.format("status-active", &Args::new().with("remaining", duration(lang, ms))),
        None => lang.get("status-active-permanent"),
    }
}

/// Placeholders as escaped plain text, for screens and list lines that carry
/// their own colours.
pub fn plain_args(lang: &Lang, cfg: &BansCfg, p: &Punishment, now: u64) -> Args {
    Args::new()
        .with("id", p.id)
        .with("kind", kind_label(lang, p))
        .with("target", escape(&p.target_label()))
        .with("operator", escape(&actor_name(lang, &p.operator)))
        .with("reason", escape(&reason_raw(lang, p)))
        .with("length", length_phrase(lang, p))
        .with("remaining", remaining_text(lang, p, now))
        .with("expires", expires_text(lang, cfg, p))
        .with("date", format_date(p.created, cfg.notifications.timezone_offset_minutes))
        .with("status", status(lang, p, now))
}

/// Placeholders for one-line messages, which colour the values themselves.
pub fn value_args(lang: &Lang, cfg: &BansCfg, p: &Punishment, now: u64) -> Args {
    let silent = if p.silent { lang.get("notify-silent-tag") } else { String::new() };
    plain_args(lang, cfg, p, now).with("action", action_label(lang, p)).with("silent", silent)
}

/// Every line of `text` gets the details of `p` as a tooltip and a click that
/// types `/pb history <target>`.
pub fn with_details(lang: &Lang, cfg: &BansCfg, p: &Punishment, now: u64, text: Text) -> Text {
    let term = if p.kind.is_lasting() {
        let length = p.length().map(|ms| duration(lang, ms)).unwrap_or_else(|| lang.get("time-permanent"));
        lang.format("hover-term", &Args::new().with("duration", length).with("expires", expires_text(lang, cfg, p)))
    } else {
        String::new()
    };
    let scope = if p.scope == "*" { lang.get("scope-global") } else { escape(&p.scope) };
    let args = plain_args(lang, cfg, p, now).with("term", term).with("scope", scope);
    let hover = Text::parse_with(&lang.format("notify-hover", &args), Style::colored(style::INFO));
    let token = match &p.target {
        Target::Address { net } => net::label(net),
        _ => p.victim_name.clone().unwrap_or_else(|| p.target_label()),
    };
    let click = Click::Suggest(format!("/{ALIAS} history {token}"));
    let lines = text.lines.into_iter().map(|l| l.on_click(click.clone()).on_hover(hover.clone())).collect();
    Text { lines }
}

/// The disconnect screen of a banned player, several lines.
pub fn ban_screen(lang: &Lang, cfg: &BansCfg, p: &Punishment, now: u64) -> Text {
    let url = cfg.notifications.appeal_url.trim();
    let appeal = if url.is_empty() {
        String::new()
    } else {
        lang.format("screen-appeal", &Args::new().with("url", escape(url)))
    };
    let key = if p.is_permanent() { "screen-ban-permanent" } else { "screen-ban" };
    Text::parse(&lang.format(key, &plain_args(lang, cfg, p, now).with("appeal", appeal)))
}

pub fn kick_screen(lang: &Lang, cfg: &BansCfg, p: &Punishment, now: u64) -> Text {
    Text::parse(&lang.format("screen-kick", &plain_args(lang, cfg, p, now)))
}

pub fn unavailable_screen(lang: &Lang) -> Text {
    Text::parse(&lang.get("screen-unavailable"))
}

/// A one-line message about a punishment with a tone (`player-muted`, `notify-punish`).
pub fn about(lang: &Lang, cfg: &BansCfg, tone: Tone, key: &str, p: &Punishment, now: u64) -> Text {
    style::message(lang, tone, key, &value_args(lang, cfg, p, now))
}

/// A list line (`history-line`, `list-line`, `info-line`) with its own colours.
pub fn line(lang: &Lang, cfg: &BansCfg, key: &str, p: &Punishment, now: u64) -> Line {
    Line::parse(&lang.format(key, &plain_args(lang, cfg, p, now)))
}

/// Staff notification of a new punishment (`notify-punish`), with details.
pub fn notify_punish(lang: &Lang, cfg: &BansCfg, p: &Punishment, now: u64) -> Text {
    with_details(lang, cfg, p, now, about(lang, cfg, Tone::Info, "notify-punish", p, now))
}

/// Short confirmation for whoever placed the punishment.
pub fn punished(lang: &Lang, cfg: &BansCfg, p: &Punishment, now: u64) -> Text {
    with_details(lang, cfg, p, now, about(lang, cfg, Tone::Info, "reply-punish", p, now))
}

/// Staff notification that a punishment was lifted.
pub fn notify_revoke(lang: &Lang, cfg: &BansCfg, p: &Punishment, by: &Actor, silent: bool, now: u64) -> Text {
    let tag = if silent { lang.get("notify-silent-tag") } else { String::new() };
    let args = value_args(lang, cfg, p, now).with("operator", escape(&actor_name(lang, by))).with("silent", tag);
    with_details(lang, cfg, p, now, style::message(lang, Tone::Info, "notify-revoke", &args))
}

pub fn notify_escalation(lang: &Lang, cfg: &BansCfg, p: &Punishment, count: usize, now: u64) -> Text {
    let args = value_args(lang, cfg, p, now).with("count", count);
    with_details(lang, cfg, p, now, style::message(lang, Tone::Info, "notify-escalation", &args))
}

pub fn warned(lang: &Lang, cfg: &BansCfg, p: &Punishment, count: usize, now: u64) -> Text {
    let args = value_args(lang, cfg, p, now).with("count", count);
    style::message(lang, Tone::Info, "player-warned", &args)
}

/// What a muted player sees when they try to talk.
pub fn muted(lang: &Lang, cfg: &BansCfg, p: &Punishment, now: u64) -> Text {
    let key = if p.is_permanent() { "player-muted-permanent" } else { "player-muted" };
    about(lang, cfg, Tone::Info, key, p, now)
}

/// A message for the server log: plain text without the chat prefix.
pub fn log_line(lang: &Lang, text: &Text) -> String {
    let prefix = strip(&lang.get("prefix"));
    let plain = text.plain();
    plain.strip_prefix(prefix.as_str()).map(str::to_string).unwrap_or(plain)
}

#[cfg(test)]
mod tests {
    use pumbo_common::id::{Cidr, Uuid};
    use pumbo_common::lang::{Bundle, COMMON};
    use pumbo_common::text::{Color, Named};

    use super::*;
    use crate::LANG;

    const DAY: u64 = 86_400_000;
    const PREFIX: Bundle = Bundle { name: "test", files: &[("en", "prefix: \"P » \""), ("pl", "prefix: \"P » \"")] };

    fn en() -> Lang {
        Lang::load(&[COMMON, LANG, PREFIX], "en", None).0
    }

    fn pl() -> Lang {
        Lang::load(&[COMMON, LANG, PREFIX], "pl", None).0
    }

    fn ban(expires: Option<u64>) -> Punishment {
        Punishment {
            id: 42,
            kind: Kind::Ban,
            target: Target::player(Uuid(1)),
            victim_name: Some("Steve".into()),
            victim_uuid: None,
            operator: Actor::console(CONSOLE),
            reason: "Using &4x-ray".into(),
            created: 0,
            expires,
            silent: false,
            template: None,
            scope: "*".into(),
            revoked: None,
            source: None,
        }
    }

    #[test]
    fn ban_screen_has_everything() {
        let lang = en();
        let mut cfg = BansCfg::default();
        cfg.notifications.appeal_url = "https://example.org/appeal".into();
        let screen = ban_screen(&lang, &cfg, &ban(Some(7 * DAY)), DAY);
        let s = screen.plain();
        assert!(s.contains("Reason: Using &4x-ray"), "{s}");
        assert!(s.contains("Banned by: Console"));
        assert!(s.contains("Time left: 6 days"));
        assert!(s.contains("Until: 1970-01-08 00:00 UTC"));
        assert!(s.contains("#42"));
        assert!(s.contains("Appeal: https://example.org/appeal"));
        assert!(screen.lines.len() >= 7, "{screen:?}");
        let perm = ban_screen(&lang, &BansCfg::default(), &ban(None), 0).plain();
        assert!(perm.contains("permanently"));
        assert!(!perm.contains("Appeal"));
        let pl = ban_screen(&pl(), &BansCfg::default(), &ban(Some(2 * DAY + 3 * 3_600_000)), 0).plain();
        assert!(pl.contains("Pozostało: 2 dni 3 godziny"), "{pl}");
    }

    #[test]
    fn labels() {
        let lang = en();
        let mut p = ban(Some(2 * 3_600_000));
        assert_eq!(kind_label(&lang, &p), "temporary ban");
        assert_eq!(length_phrase(&lang, &p), " for &a&o2 hours&r");
        p.expires = None;
        assert_eq!(kind_label(&lang, &p), "ban");
        assert_eq!(length_phrase(&lang, &p), " &a&opermanently&r");
        p.kind = Kind::Warn;
        assert_eq!(length_phrase(&lang, &p), "");
        p.kind = Kind::Mute;
        p.target = Target::Address { net: Cidr::parse("1.2.3.4").unwrap() };
        p.victim_name = None;
        assert_eq!(kind_label(&lang, &p), "IP mute");
        assert_eq!(duration(&lang, 400), "1 second");
        assert_eq!(duration(&pl(), 22 * 60_000), "22 minuty");
    }

    #[test]
    fn notifications() {
        let lang = en();
        let cfg = BansCfg::default();
        let mut p = ban(None);
        p.silent = true;
        p.operator = Actor::player(Uuid(2), "Qhash");
        let n = notify_punish(&lang, &cfg, &p, 0);
        assert_eq!(n.plain(), "P » [silent] Qhash banned Steve permanently: Using &4x-ray (ban #42)");
        assert_eq!(log_line(&lang, &n), "[silent] Qhash banned Steve permanently: Using &4x-ray (ban #42)");
        // grey text; names red, time green, reason yellow, all in italics
        let seg = |t: &str| n.lines[0].segments.iter().find(|s| s.text == t).unwrap_or_else(|| panic!("{t}: {n:?}"));
        for (t, color) in [("Qhash", Named::Red), ("Steve", Named::Red), ("permanently", Named::Green)] {
            assert_eq!(seg(t).style.color, Some(Color::Named(color)), "{t}");
            assert!(seg(t).style.italic, "{t}");
        }
        assert_eq!(seg("Using &4x-ray").style.color, Some(Color::Named(Named::Yellow)));
        assert_eq!(seg(" (ban #42)").style.color, Some(style::INFO));
        assert!(!seg(" (ban #42)").style.italic);
        // every segment: details on hover, a click that types the history command
        for s in &n.lines[0].segments {
            assert_eq!(s.click, Some(Click::Suggest("/pb history Steve".into())));
            let hover = s.hover.as_ref().map(Text::plain).unwrap_or_default();
            assert!(
                hover.starts_with("ban #42\nPlayer: Steve\nBy: Qhash\nDuration: permanent\nUntil: never"),
                "{hover}"
            );
            assert!(hover.contains("Server: all servers") && hover.contains("Reason: Using &4x-ray"), "{hover}");
        }
        let mut t = ban(Some(7 * DAY));
        t.target = Target::Address { net: Cidr::parse("1.2.3.4").unwrap() };
        t.victim_name = None;
        let mine = punished(&lang, &cfg, &t, 0);
        assert_eq!(mine.plain(), "P » Punished 1.2.3.4 for 7 days (IP ban #42)");
        assert_eq!(mine.lines[0].segments[2].click, Some(Click::Suggest("/pb history 1.2.3.4".into())));
        let r = notify_revoke(&lang, &cfg, &p, &Actor::player(Uuid(3), "Mod"), false, 0);
        assert_eq!(r.plain(), "P » Mod lifted a punishment of Steve (ban #42)");
        let n = notify_punish(&pl(), &cfg, &ban(Some(3_600_000)), 0);
        assert_eq!(n.plain(), "P » Konsola banuje Steve, czas: 1 godzina, powód: Using &4x-ray (ban czasowy #42)");
        let mut kick = ban(None);
        kick.kind = Kind::Kick;
        let hover = notify_punish(&lang, &cfg, &kick, 0).lines[0].segments[2].hover.clone().unwrap().plain();
        assert!(!hover.contains("Duration"), "{hover}");
        let m = muted(
            &pl(),
            &cfg,
            &{
                let mut m = ban(Some(DAY));
                m.kind = Kind::Mute;
                m
            },
            0,
        );
        assert_eq!(
            m.plain(),
            "P » Masz wyciszenie do 1970-01-02 00:00 UTC (pozostało: 1 dzień). Powód: Using &4x-ray (#42)"
        );
    }

    #[test]
    fn locale_choice() {
        let mut langs = Langs::single(en());
        langs.per_player = true;
        langs.others.insert("pl".into(), pl());
        assert_eq!(langs.for_locale(Some("pl_pl")).get("kind-warn"), "ostrzeżenie");
        assert_eq!(langs.for_locale(Some("de_de")).get("kind-warn"), "warning");
        assert_eq!(langs.for_locale(None).get("kind-warn"), "warning");
        langs.per_player = false;
        assert_eq!(langs.for_locale(Some("pl_pl")).get("kind-warn"), "warning");
    }
}
