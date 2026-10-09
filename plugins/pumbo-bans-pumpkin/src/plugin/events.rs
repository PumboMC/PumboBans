//! Event handlers: ban check before the player enters the world, mutes in chat
//! and commands, exemption snapshot on join.

use pumbo_bans_core::effects::Audience;
use pumbo_bans_core::engine::{LoginDecision, Subject};
use pumbo_bans_core::render;
use pumbo_common::clock::now_ms;
use pumbo_common::id::{Uuid, parse_ip};
use pumbo_common::rich::Text;

use super::host::{
    self, AsyncPlayerPreLoginEvent, EventData, EventHandler, PlayerChatEvent, PlayerCommandSendEvent, PlayerJoinEvent,
    Server, component, text,
};
use super::state;

/// Shown when the plugin cannot look at its state (it is busy in a nested call):
/// refusing is the safe answer.
const BUSY: &str = "&cPumboBans is busy, please join again.";

pub struct PreLogin;

impl EventHandler<AsyncPlayerPreLoginEvent> for PreLogin {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<AsyncPlayerPreLoginEvent>,
    ) -> EventData<AsyncPlayerPreLoginEvent> {
        let name = e.player_name.clone();
        let ip = parse_ip(&e.ip_address);
        let now = now_ms();
        let decision = match Uuid::parse(&e.player_uuid) {
            Some(uuid) => state::with(|s| s.engine.check_login(uuid, &name, ip, now)),
            // Without a UUID nothing can be checked properly; fall back to the name.
            None => state::with(|s| {
                let found = s.engine.find_active(
                    pumbo_bans_core::Kind::Ban,
                    &Subject { uuid: None, name: Some(name.clone()), ip },
                    now,
                );
                match found {
                    Ok(Some(p)) => LoginDecision::Deny {
                        screen: render::ban_screen(s.engine.lang(), &s.engine.cfg, &p, now),
                        punishment: Some(Box::new(p)),
                    },
                    _ => LoginDecision::Allow { alt_notice: None },
                }
            }),
        };
        match decision {
            Some(LoginDecision::Deny { screen, punishment }) => {
                e.cancelled = true;
                e.kick_message = component(&screen);
                if let Some(p) = punishment {
                    host::info(&format!("PumboBans: {name} refused, banned (#{})", p.id));
                }
            }
            Some(LoginDecision::Allow { alt_notice: Some(notice) }) => {
                host::notify(&server, &notice, false, None, Audience::Alts);
            }
            Some(LoginDecision::Allow { alt_notice: None }) => {}
            None => {
                e.cancelled = true;
                e.kick_message = text(BUSY);
            }
        }
        e
    }
}

pub struct Join;

impl EventHandler<PlayerJoinEvent> for Join {
    fn handle(&self, _server: Server, e: EventData<PlayerJoinEvent>) -> EventData<PlayerJoinEvent> {
        let player = &e.player;
        let exempt = host::exemptions(player);
        let uuid = host::uuid_of(player);
        let name = player.get_name();
        let locale = Some(player.get_locale());
        let result = state::with(|s| s.engine.note_join(uuid, &name, exempt, locale, now_ms()));
        if let Some(Err(err)) = result {
            host::warn(&format!("PumboBans: cannot store exemptions of {name}: {err}"));
        }
        e
    }
}

/// The active mute of a player and the line to show, if muted.
fn muted_line(player: &host::Player) -> Option<Text> {
    let uuid = host::uuid_of(player);
    let name = player.get_name();
    let ip = parse_ip(&player.get_ip());
    let locale = player.get_locale();
    let now = now_ms();
    state::with(|s| {
        let p = s.engine.mute_for(&Subject::new(uuid, &name, ip), now)?;
        let lang = s.engine.lang_for(Some(&locale));
        Some(render::muted(lang, &s.engine.cfg, &p, now))
    })
    .flatten()
}

pub struct Chat;

impl EventHandler<PlayerChatEvent> for Chat {
    fn handle(&self, _server: Server, mut e: EventData<PlayerChatEvent>) -> EventData<PlayerChatEvent> {
        if let Some(line) = muted_line(&e.player) {
            // Cancelled chat is neither sent nor written to the server log.
            e.cancelled = true;
            host::send(&e.player, &line);
        }
        e
    }
}

pub struct CommandSend;

impl EventHandler<PlayerCommandSendEvent> for CommandSend {
    fn handle(&self, _server: Server, mut e: EventData<PlayerCommandSendEvent>) -> EventData<PlayerCommandSendEvent> {
        let blocked = state::with(|s| s.engine.cfg.is_blocked_for_muted(&e.command)).unwrap_or(false);
        if blocked && let Some(line) = muted_line(&e.player) {
            e.cancelled = true;
            host::send(&e.player, &line);
        }
        e
    }
}
