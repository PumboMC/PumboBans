//! `/pumbobans` and the short commands (`/ban`, `/mute`, ...): gather what the
//! core needs from the server, run the core, carry out the effects.

use std::collections::BTreeSet;

use pumbo_bans_core::commands::{self, Ctx, Online, Sender};
use pumbo_bans_core::effects::AdminRequest;
use pumbo_bans_core::import;
use pumbo_common::clock::now_ms;
use pumbo_common::style::{self, Tone};
use pumbo_common::text::Args;

use super::host::{
    self, Arg, CommandError, CommandHandler, CommandSender, CommandSuggestion, CommandSuggestionHandler,
    CommandSuggestions, ConsumedArgs, Player, Server, Sink, SuggestionRequest,
};
use super::state;
use crate::setup::{self, pumpkin_node};

/// `/pumbobans ...` (`label` None) or a short command (`label` = its name,
/// `sub` = the subcommand it stands for).
#[derive(Clone, Copy)]
pub struct Handler {
    pub label: Option<&'static str>,
    pub sub: Option<&'static str>,
}

impl CommandHandler for Handler {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let raw = match args.get_value("args") {
            Arg::Simple(s) | Arg::Msg(s) => s,
            _ => String::new(),
        };
        let mut words: Vec<String> = raw.split_whitespace().map(str::to_string).collect();
        if let Some(sub) = self.sub {
            words.insert(0, sub.to_string());
        }
        run(&sender, &server, self.label, &words);
        Ok(1)
    }
}

/// Permission nodes the sender has (all of them for the console).
fn sender_info(sender: &CommandSender) -> Sender {
    let Some(player) = sender.as_player() else { return Sender::console() };
    sender_of(&player)
}

fn sender_of(player: &Player) -> Sender {
    let nodes = state::with(|s| commands::permission_nodes(&s.engine)).unwrap_or_default();
    let granted: BTreeSet<String> = nodes.into_iter().filter(|n| player.has_permission(&pumpkin_node(n))).collect();
    Sender::player(host::uuid_of(player), &player.get_name(), granted, Some(player.get_locale()))
}

fn online_for(server: &Server, words: &[String]) -> Vec<Online> {
    let targets: Vec<String> = words.iter().take(2).cloned().collect();
    host::online(server, &targets)
}

pub fn run(sender: &CommandSender, server: &Server, label: Option<&'static str>, words: &[String]) {
    let who = sender_info(sender);
    let online = online_for(server, words);
    let now = now_ms();
    let effects = state::with(|s| {
        let ctx = Ctx { sender: &who, online: &online, now, label };
        commands::run(&mut s.engine, words, &ctx)
    });
    let sink = Sink::Sender(sender);
    let Some(effects) = effects else {
        sink_busy(&sink);
        return;
    };
    for req in host::apply(server, &sink, effects.0) {
        admin(server, &sink, &who, req);
    }
}

fn sink_busy(sink: &Sink<'_>) {
    sink.send(&pumbo_common::rich::Text::parse("&cPumboBans is busy, please try again."));
}

/// Reload and import need files and the server's own ban lists.
fn admin(server: &Server, sink: &Sink<'_>, who: &Sender, req: AdminRequest) {
    let reply = |tone: Tone, key: &str, args: Args| {
        let msg = state::with(|s| style::message(s.engine.lang_for(who.locale.as_deref()), tone, key, &args));
        if let Some(msg) = msg {
            sink.send(&msg);
        }
    };
    match req {
        AdminRequest::Reload => {
            let Some(dir) = state::with(|s| s.data_dir.clone()) else { return };
            let (cfg, mut warnings) = setup::load_config(&dir);
            let (langs, w) = setup::load_langs(&dir, &cfg);
            warnings.extend(w);
            if let Some(w) = warnings.iter().find(|w| w.fatal) {
                host::warn(&format!("PumboBans: reload refused, the current settings stay: {w}"));
                reply(Tone::Error, "command-reload-failed", Args::new().arg(style::value(&w.message)));
                return;
            }
            state::with(|s| s.engine.reconfigure(cfg, langs));
            for w in &warnings {
                host::warn(&format!("PumboBans: config: {w}"));
                reply(Tone::Warn, "admin-reload-warning", Args::new().with("warning", style::value(&w.message)));
            }
            reply(Tone::Success, "command-reloaded", Args::new());
        }
        AdminRequest::ImportVanilla => {
            let bans = server.get_ban_manager();
            let mut players: Vec<import::VanillaPlayerBan> = bans
                .list_player_bans()
                .into_iter()
                .map(|b| import::VanillaPlayerBan {
                    uuid: pumbo_common::id::Uuid::from_high_low(b.uuid.high, b.uuid.low).to_string(),
                    name: b.name,
                    created: b.created,
                    source: b.source,
                    expires: b.expires.unwrap_or_else(|| "forever".into()),
                    reason: b.reason,
                })
                .collect();
            let mut ips: Vec<import::VanillaIpBan> = bans
                .list_ip_bans()
                .into_iter()
                .map(|b| import::VanillaIpBan {
                    ip: b.ip,
                    created: b.created,
                    source: b.source,
                    expires: b.expires.unwrap_or_else(|| "forever".into()),
                    reason: b.reason,
                })
                .collect();
            // Lists from another server can be dropped into the data folder.
            let dir = state::with(|s| s.data_dir.clone()).unwrap_or_default();
            if let Ok(text) = std::fs::read_to_string(format!("{dir}/banned-players.json")) {
                match import::parse_players(&text) {
                    Ok(list) => players.extend(list),
                    Err(e) => reply(Tone::Error, "admin-import-failed", Args::new().with("error", style::value(e))),
                }
            }
            if let Ok(text) = std::fs::read_to_string(format!("{dir}/banned-ips.json")) {
                match import::parse_ips(&text) {
                    Ok(list) => ips.extend(list),
                    Err(e) => reply(Tone::Error, "admin-import-failed", Args::new().with("error", style::value(e))),
                }
            }
            let counts = Args::new().with("players", style::value(players.len())).with("ips", style::value(ips.len()));
            reply(Tone::Info, "admin-import-source", counts);
            let now = now_ms();
            let (mut entries, dropped_p) = import::player_entries(&players, now);
            let (ip_entries, dropped_i) = import::ip_entries(&ips, now);
            entries.extend(ip_entries);
            match state::with(|s| s.engine.import(entries, now)) {
                Some(Ok(r)) => {
                    let skipped = r.skipped + dropped_p + dropped_i;
                    host::info(&format!("PumboBans: imported {} punishments, skipped {skipped}", r.added));
                    let args = Args::new().with("added", style::value(r.added)).with("skipped", style::value(skipped));
                    reply(Tone::Success, "admin-import-done", args);
                }
                Some(Err(e)) => reply(Tone::Error, "admin-import-failed", Args::new().with("error", style::value(e.0))),
                None => {}
            }
        }
    }
}

/// Tab completion of the arguments.
#[derive(Clone, Copy)]
pub struct Suggest {
    pub label: Option<&'static str>,
}

impl CommandSuggestionHandler for Suggest {
    fn suggest(&self, sender: CommandSender, server: Server, request: SuggestionRequest) -> CommandSuggestions {
        let input = request.input.clone();
        let line = input.trim_start_matches('/');
        let after = line.split_once(' ').map(|(_, rest)| rest).unwrap_or("");
        let mut words: Vec<String> = after.split_whitespace().map(str::to_string).collect();
        if after.is_empty() || after.ends_with(' ') {
            words.push(String::new());
        }
        let typed = words.last().cloned().unwrap_or_default();
        let start = input.len().saturating_sub(typed.len());
        let who = sender_info(&sender);
        let online = host::online(&server, &[]);
        let values = state::with(|s| {
            let ctx = Ctx { sender: &who, online: &online, now: now_ms(), label: self.label };
            commands::complete(&s.engine, &words, &ctx)
        })
        .unwrap_or_default();
        CommandSuggestions {
            start: u32::try_from(start).unwrap_or(0),
            length: u32::try_from(typed.len()).unwrap_or(0),
            values: values.into_iter().map(|value| CommandSuggestion { value, tooltip: None }).collect(),
        }
    }
}
