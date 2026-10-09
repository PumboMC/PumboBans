//! `config.yml` of PumboBans. The commented template lives in the platform
//! layer (`assets/config.yml`) and must load to exactly [`BansCfg::default`].

use std::time::Duration;

use pumbo_common::config::{Check, Settings};
use pumbo_common::time::{Term, parse_term};
use serde::{Deserialize, Serialize};

use crate::model::Kind;

/// How IP punishments reach accounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Strictness {
    /// Only connections from the punished address.
    Lenient,
    /// Also every account that has used a punished address.
    #[default]
    Normal,
    /// Also every account that shares any address with an account that used a
    /// punished address (one step through alt accounts).
    Stern,
    /// Like `stern`, and a ban or mute of an account also covers every account
    /// that shares an address with it.
    Strict,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct BansCfg {
    /// Default language (`en`, `pl` or any file in `lang/`).
    pub language: String,
    /// Messages to a player in the player's client language when available.
    pub per_player_language: bool,
    pub enforcement: EnforcementCfg,
    pub punishments: PunishmentsCfg,
    pub mutes: MutesCfg,
    pub warnings: WarningsCfg,
    pub templates: Vec<TemplateCfg>,
    pub notifications: NotificationsCfg,
    pub privacy: PrivacyCfg,
    pub commands: CommandsCfg,
    pub ipc: IpcCfg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct EnforcementCfg {
    pub address_strictness: Strictness,
    /// Prefix an IPv4 punishment covers (32 = one address).
    pub ipv4_prefix: u8,
    /// Prefix an IPv6 punishment covers (64 = one customer network).
    pub ipv6_prefix: u8,
    /// Without a working database nobody may join.
    pub fail_closed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PunishmentsCfg {
    /// Refuse punishments without a reason (a template counts as a reason).
    pub require_reason: bool,
    /// Allow punishing a name that never joined, with `-f`.
    pub allow_unknown_names: bool,
    /// A ban or mute of someone already banned or muted replaces the old one.
    pub allow_override: bool,
    /// Longest punishments per staff group (`pumbo.bans.limit.<group>`).
    pub limits: Vec<LimitCfg>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
#[derive(Default)]
pub struct LimitCfg {
    pub group: String,
    /// Longest ban; empty: unlimited.
    pub ban: String,
    /// Longest mute; empty: unlimited.
    pub mute: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct MutesCfg {
    /// Commands a muted player may not use (no slash, namespaces ignored).
    pub blocked_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct WarningsCfg {
    /// How long a warning counts (`perm`: forever).
    pub duration: String,
    /// Above the highest step, repeat it on every further warning.
    pub repeat_highest: bool,
    pub escalation: Vec<EscalationCfg>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
#[derive(Default)]
pub struct EscalationCfg {
    /// Active warnings that trigger this step.
    pub count: u32,
    /// `ban`, `mute` or `kick`.
    pub action: String,
    /// Length for `ban` and `mute` (`perm` allowed; empty: permanent).
    pub duration: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
#[derive(Default)]
pub struct TemplateCfg {
    /// Used as `#name` in commands.
    pub name: String,
    pub reason: String,
    /// Lengths of the 1st, 2nd, ... punishment of a player with this template;
    /// the last one repeats. Empty: the length typed in the command.
    pub durations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct NotificationsCfg {
    /// Log every punishment and revocation.
    pub console: bool,
    /// Tell staff when someone joins from an address a banned account used.
    pub alt_join: bool,
    /// Link on the ban screen (empty: no line).
    pub appeal_url: String,
    /// Dates in messages: UTC shifted by this many minutes.
    pub timezone_offset_minutes: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PrivacyCfg {
    /// Days an address stays linked to an account after its last use (0: keep).
    pub address_retention_days: u32,
    /// Also remember the address of a login refused because of a ban.
    pub record_refused_logins: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct CommandsCfg {
    /// Short commands not to register (e.g. `kick` to keep the server's own).
    pub disabled: Vec<String>,
    /// Entries per page in lists.
    pub page_size: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct IpcCfg {
    /// Other plugins may ask whether a player is banned or muted.
    pub allow_queries: bool,
    /// Plugins (by name) allowed to place and lift punishments.
    pub allow_punish: Vec<String>,
}

impl Default for BansCfg {
    fn default() -> Self {
        Self {
            language: "en".into(),
            per_player_language: true,
            enforcement: EnforcementCfg::default(),
            punishments: PunishmentsCfg::default(),
            mutes: MutesCfg::default(),
            warnings: WarningsCfg::default(),
            templates: vec![
                TemplateCfg::new("cheating", "Cheating", &["7d", "30d", "perm"]),
                TemplateCfg::new("spam", "Spamming", &["30m", "2h", "1d"]),
                TemplateCfg::new("griefing", "Griefing", &["3d", "14d", "perm"]),
            ],
            notifications: NotificationsCfg::default(),
            privacy: PrivacyCfg::default(),
            commands: CommandsCfg::default(),
            ipc: IpcCfg::default(),
        }
    }
}

impl TemplateCfg {
    fn new(name: &str, reason: &str, durations: &[&str]) -> Self {
        Self {
            name: name.into(),
            reason: reason.into(),
            durations: durations.iter().map(|d| (*d).to_string()).collect(),
        }
    }
}

impl Default for EnforcementCfg {
    fn default() -> Self {
        Self { address_strictness: Strictness::Normal, ipv4_prefix: 32, ipv6_prefix: 64, fail_closed: true }
    }
}

impl Default for PunishmentsCfg {
    fn default() -> Self {
        Self { require_reason: false, allow_unknown_names: true, allow_override: false, limits: Vec::new() }
    }
}

impl Default for MutesCfg {
    fn default() -> Self {
        let list = ["msg", "tell", "w", "whisper", "me", "say", "r", "reply", "teammsg", "tm", "mail", "m", "pm", "dm"];
        Self { blocked_commands: list.iter().map(|s| (*s).to_string()).collect() }
    }
}

impl Default for WarningsCfg {
    fn default() -> Self {
        Self {
            duration: "30d".into(),
            repeat_highest: true,
            escalation: vec![
                EscalationCfg {
                    count: 3,
                    action: "mute".into(),
                    duration: "1h".into(),
                    reason: "Too many warnings".into(),
                },
                EscalationCfg {
                    count: 5,
                    action: "ban".into(),
                    duration: "1d".into(),
                    reason: "Too many warnings".into(),
                },
            ],
        }
    }
}

impl Default for NotificationsCfg {
    fn default() -> Self {
        Self { console: true, alt_join: true, appeal_url: String::new(), timezone_offset_minutes: 0 }
    }
}

impl Default for PrivacyCfg {
    fn default() -> Self {
        Self { address_retention_days: 180, record_refused_logins: true }
    }
}

impl Default for CommandsCfg {
    fn default() -> Self {
        Self { disabled: Vec::new(), page_size: 8 }
    }
}

impl Default for IpcCfg {
    fn default() -> Self {
        Self { allow_queries: true, allow_punish: Vec::new() }
    }
}

/// A length for punishments: a duration above zero or `perm`.
pub fn parse_length(text: &str) -> Option<Term> {
    match parse_term(text).ok()? {
        Term::Limited(d) if d.is_zero() => None,
        t => Some(t),
    }
}

/// Whether `t` is no longer than `limit`.
pub fn within(t: Term, limit: Term) -> bool {
    match (t, limit) {
        (_, Term::Permanent) => true,
        (Term::Permanent, Term::Limited(_)) => false,
        (Term::Limited(a), Term::Limited(b)) => a <= b,
    }
}

impl Settings for BansCfg {
    fn validate(&mut self, check: &mut Check<'_>) {
        check.clamp("enforcement.ipv4-prefix", &mut self.enforcement.ipv4_prefix, 8, 32);
        check.clamp("enforcement.ipv6-prefix", &mut self.enforcement.ipv6_prefix, 32, 128);
        check
            .ensure("warnings.duration", &mut self.warnings.duration, "30d".to_string(), |d| parse_length(d).is_some());
        self.warnings.escalation.retain(|s| {
            let ok = s.count > 0
                && matches!(s.action.as_str(), "ban" | "mute" | "kick")
                && (s.duration.is_empty() || parse_length(&s.duration).is_some());
            if !ok {
                check.warn("warnings.escalation", format!("step for {} warnings is invalid and ignored", s.count));
            }
            ok
        });
        self.warnings.escalation.sort_by_key(|s| s.count);
        self.templates.retain_mut(|t| {
            let before = t.durations.len();
            t.durations.retain(|d| parse_length(d).is_some());
            if t.durations.len() != before {
                check.warn("templates", format!("template '{}': lengths that are not lengths were dropped", t.name));
            }
            t.name = t.name.trim().trim_start_matches('#').to_lowercase();
            if t.name.is_empty() {
                check.warn("templates", "a template without a name is ignored");
            }
            !t.name.is_empty()
        });
        self.punishments.limits.retain(|l| {
            let ok = pumbo_common::command::is_valid_node(&l.group)
                && [&l.ban, &l.mute].iter().all(|v| v.is_empty() || parse_length(v).is_some());
            if !ok {
                check.warn("punishments.limits", format!("limit for group '{}' is invalid and ignored", l.group));
            }
            ok
        });
        check.clamp("commands.page-size", &mut self.commands.page_size, 1, 50);
        check.clamp(
            "notifications.timezone-offset-minutes",
            &mut self.notifications.timezone_offset_minutes,
            -720,
            840,
        );
        for c in &mut self.mutes.blocked_commands {
            *c = c.trim().trim_start_matches('/').to_lowercase();
        }
        for c in &mut self.commands.disabled {
            *c = c.trim().trim_start_matches('/').to_lowercase();
        }
        self.ipc.allow_punish.iter_mut().for_each(|p| *p = p.trim().to_lowercase());
    }
}

impl BansCfg {
    /// Length warnings count for.
    pub fn warning_length(&self) -> Term {
        parse_length(&self.warnings.duration).unwrap_or(Term::Limited(Duration::from_secs(30 * 86_400)))
    }

    pub fn template(&self, name: &str) -> Option<&TemplateCfg> {
        let name = name.trim_start_matches('#').to_lowercase();
        self.templates.iter().find(|t| t.name == name)
    }

    /// The limit for `kind` of someone in the given groups; the most generous
    /// group wins, a group without a limit for the kind means unlimited.
    pub fn length_limit(&self, kind: Kind, groups: &[String]) -> Option<Term> {
        let mut best: Option<Term> = None;
        for g in groups {
            let Some(l) = self.punishments.limits.iter().find(|l| &l.group == g) else { continue };
            let raw = match kind {
                Kind::Ban => &l.ban,
                Kind::Mute => &l.mute,
                Kind::Warn | Kind::Kick => return None,
            };
            let len = parse_length(raw)?;
            best = Some(match best {
                Some(b) if within(len, b) => b,
                _ => len,
            });
        }
        best
    }

    /// Whether a command line (without `/`) is blocked for muted players.
    pub fn is_blocked_for_muted(&self, line: &str) -> bool {
        let (name, _) = pumbo_common::command::split_command(line);
        !name.is_empty() && self.mutes.blocked_commands.contains(&name)
    }
}

#[cfg(test)]
mod tests {
    use pumbo_common::config::load;

    use super::*;

    #[test]
    fn empty_file_is_default() {
        let (cfg, w) = load::<BansCfg>("");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(cfg, BansCfg::default());
    }

    #[test]
    fn corrections() {
        let (cfg, w) = load::<BansCfg>(
            "enforcement:\n  ipv6-prefix: 200\n  address-strictness: stern\n\
             warnings:\n  duration: soon\n  escalation: [{ count: 2, action: explode }]\n\
             templates:\n  - { name: \"#Spam\", reason: x, durations: [1h, bad] }\n",
        );
        assert_eq!(cfg.enforcement.ipv6_prefix, 128);
        assert_eq!(cfg.enforcement.address_strictness, Strictness::Stern);
        assert_eq!(cfg.warnings.duration, "30d");
        assert!(cfg.warnings.escalation.is_empty());
        assert_eq!(cfg.templates.len(), 1);
        assert_eq!(cfg.template("#SPAM").map(|t| t.durations.len()), Some(1));
        assert_eq!(w.len(), 4, "{w:?}");
    }

    #[test]
    fn muted_commands() {
        let cfg = BansCfg::default();
        assert!(cfg.is_blocked_for_muted("msg Steve hi"));
        assert!(cfg.is_blocked_for_muted("/minecraft:tell Steve hi"));
        assert!(cfg.is_blocked_for_muted("ME waves"));
        assert!(!cfg.is_blocked_for_muted("spawn"));
        assert!(!cfg.is_blocked_for_muted(""));
    }

    #[test]
    fn limits() {
        let (cfg, w) = load::<BansCfg>(
            "punishments:\n  limits:\n    - { group: helper, ban: 1d, mute: 1h }\n    - { group: mod, ban: 30d }\n",
        );
        assert!(w.is_empty(), "{w:?}");
        let helper = vec!["helper".to_string()];
        let both = vec!["helper".to_string(), "mod".to_string()];
        assert_eq!(cfg.length_limit(Kind::Ban, &helper), parse_length("1d"));
        assert_eq!(cfg.length_limit(Kind::Ban, &both), parse_length("30d"));
        assert_eq!(cfg.length_limit(Kind::Mute, &both), None);
        assert_eq!(cfg.length_limit(Kind::Ban, &[]), None);
        assert!(within(parse_length("1h").unwrap(), Term::Permanent));
        assert!(!within(Term::Permanent, parse_length("1h").unwrap()));
        assert!(parse_length("0s").is_none());
    }
}
