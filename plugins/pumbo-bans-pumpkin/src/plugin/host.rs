//! Thin helpers over the Pumpkin plugin API (the same calls exist in Pumpkin
//! 0.2.0, 0.1.0-dev and the source build with the Pumbo hooks), and carrying
//! out the core's effects.

use pumbo_bans_core::commands::Online;
use pumbo_bans_core::effects::{AdminRequest, Audience, Effect};
use pumbo_common::command::permission;
use pumbo_common::id::{Uuid, parse_ip};
use pumbo_common::rich::{Click, Segment, Text};
use pumbo_common::text::{Color, Named};

pub use crate::papi::command::{
    Arg, ArgumentType, Command, CommandError, CommandNode, CommandSender, CommandSuggestion, CommandSuggestions,
    ConsumedArgs, StringType, SuggestionRequest,
};
pub use crate::papi::commands::{CommandHandler, CommandSuggestionHandler};
pub use crate::papi::common::{NamedColor, RgbColor};
pub use crate::papi::events::{
    AsyncPlayerPreLoginEvent, EventData, EventHandler, EventPriority, PlayerChatEvent, PlayerCommandSendEvent,
    PlayerJoinEvent,
};
pub use crate::papi::logging::{LogLevel, log};
pub use crate::papi::permission::{Permission, PermissionDefault, PermissionLevel};
pub use crate::papi::player::{BedrockDisconnectReason, BedrockKickOptions, JavaKickOptions, SocketTeardownPolicy};
pub use crate::papi::text::TextComponent;
pub use crate::papi::{Context, Player, Plugin, PluginMetadata, Server, permissions, register_plugin};

use crate::setup::pumpkin_node;

#[cfg(feature = "mc263")]
pub const API_LABEL: &str = "Pumpkin 0.2.0 / MC 26.3";
#[cfg(feature = "mc262")]
pub const API_LABEL: &str = "Pumpkin 0.1.0-dev / MC 26.2";
#[cfg(feature = "mcgit")]
pub const API_LABEL: &str = "Pumpkin source build with the Pumbo hooks (pumbo/all) / MC 26.3";

thread_local! {
    /// The server handle from `on_load`, for calls that do not get one
    /// (inter-plugin messages).
    static SERVER: std::cell::RefCell<Option<Server>> = const { std::cell::RefCell::new(None) };
}

pub fn remember_server(server: Server) {
    SERVER.with(|s| {
        if let Ok(mut g) = s.try_borrow_mut() {
            *g = Some(server);
        }
    });
}

/// Runs `f` with the remembered server handle (shared borrow, so nested calls work).
pub fn with_server<R>(f: impl FnOnce(&Server) -> R) -> Option<R> {
    SERVER.with(|s| s.try_borrow().ok().and_then(|g| g.as_ref().map(f)))
}

pub fn info(msg: &str) {
    log(LogLevel::Info, msg);
}

pub fn warn(msg: &str) {
    log(LogLevel::Warn, msg);
}

pub fn error(msg: &str) {
    log(LogLevel::Error, msg);
}

/// A component from text with `&` codes (including `&#rrggbb`).
pub fn text(s: &str) -> TextComponent {
    component(&Text::parse(s))
}

/// A component from rich text: one child per segment, lines joined with line
/// breaks, click and hover actions kept.
pub fn component(t: &Text) -> TextComponent {
    let mut root = TextComponent::text("");
    for (i, line) in t.lines.iter().enumerate() {
        if i > 0 {
            root = root.add_child(TextComponent::text("\n"));
        }
        for seg in &line.segments {
            root = root.add_child(segment(seg));
        }
    }
    root
}

fn named(n: Named) -> NamedColor {
    match n {
        Named::Black => NamedColor::Black,
        Named::DarkBlue => NamedColor::DarkBlue,
        Named::DarkGreen => NamedColor::DarkGreen,
        Named::DarkAqua => NamedColor::DarkAqua,
        Named::DarkRed => NamedColor::DarkRed,
        Named::DarkPurple => NamedColor::DarkPurple,
        Named::Gold => NamedColor::Gold,
        Named::Gray => NamedColor::Gray,
        Named::DarkGray => NamedColor::DarkGray,
        Named::Blue => NamedColor::Blue,
        Named::Green => NamedColor::Green,
        Named::Aqua => NamedColor::Aqua,
        Named::Red => NamedColor::Red,
        Named::LightPurple => NamedColor::LightPurple,
        Named::Yellow => NamedColor::Yellow,
        Named::White => NamedColor::White,
    }
}

fn segment(seg: &Segment) -> TextComponent {
    let mut c = TextComponent::text(&seg.text);
    match seg.style.color {
        Some(Color::Named(n)) => c = c.color_named(named(n)),
        Some(Color::Rgb(v)) => c = c.color_rgb(RgbColor { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8 }),
        None => {}
    }
    // Segments are siblings under an unstyled root, so only set what is on.
    let st = seg.style;
    if st.bold {
        c = c.bold(true);
    }
    if st.italic {
        c = c.italic(true);
    }
    if st.underlined {
        c = c.underlined(true);
    }
    if st.strikethrough {
        c = c.strikethrough(true);
    }
    if st.obfuscated {
        c = c.obfuscated(true);
    }
    match &seg.click {
        Some(Click::Suggest(cmd)) => c = c.click_suggest_command(cmd),
        Some(Click::Run(cmd)) => c = c.click_run_command(cmd),
        Some(Click::Copy(t)) => c = c.click_copy_to_clipboard(t),
        Some(Click::Url(u)) => c = c.click_open_url(u),
        None => {}
    }
    if let Some(hover) = &seg.hover {
        c = c.hover_show_text(component(hover));
    }
    c
}

pub fn uuid_of(player: &Player) -> Uuid {
    let id = player.get_id();
    Uuid::from_high_low(id.high, id.low)
}

pub fn api_uuid(u: Uuid) -> crate::papi::uuid::Uuid {
    let (high, low) = u.high_low();
    crate::papi::uuid::Uuid { high, low }
}

pub fn send(player: &Player, t: &Text) {
    if !t.is_empty() {
        player.send_system_message(component(t), false);
    }
}

pub fn kick(player: &Player, screen: &Text) {
    if let Some(java) = player.as_java() {
        java.kick(JavaKickOptions {
            reason: component(screen),
            log_to_console: false,
            teardown_policy: SocketTeardownPolicy::Graceful,
        });
    } else if let Some(bedrock) = player.as_bedrock() {
        let plain = screen.plain();
        bedrock.kick(&BedrockKickOptions {
            reason: BedrockDisconnectReason::Kicked,
            message: plain.clone(),
            skip_message: false,
            filtered_message: plain,
            log_to_console: false,
            teardown_policy: SocketTeardownPolicy::Graceful,
        });
    }
}

/// Snapshot of the online players. `exempt_for` names players whose
/// exemption permissions are checked (the command's target).
pub fn online(server: &Server, exempt_for: &[String]) -> Vec<Online> {
    server
        .get_all_players()
        .into_iter()
        .map(|p| {
            let name = p.get_name();
            let exempt =
                if exempt_for.iter().any(|t| t.eq_ignore_ascii_case(&name)) { exemptions(&p) } else { Vec::new() };
            Online { uuid: uuid_of(&p), ip: parse_ip(&p.get_ip()), locale: Some(p.get_locale()), name, exempt }
        })
        .collect()
}

/// Exemption permissions a player has right now.
pub fn exemptions(player: &Player) -> Vec<pumbo_bans_core::Kind> {
    pumbo_bans_core::Kind::ALL
        .into_iter()
        .filter(|k| player.has_permission(&pumpkin_node(&pumbo_bans_core::commands::exempt_node(*k))))
        .collect()
}

/// Where replies of a command go.
pub enum Sink<'a> {
    Sender(&'a CommandSender),
    /// Nobody (inter-plugin requests).
    None,
}

impl Sink<'_> {
    /// Players get components, the console plain text.
    pub fn send(&self, t: &Text) {
        if t.is_empty() {
            return;
        }
        if let Sink::Sender(s) = self {
            match s.as_player() {
                Some(p) => send(&p, t),
                None => s.send_message(TextComponent::text(&t.plain())),
            }
        }
    }
}

/// Carries out effects. Admin requests are returned for the caller.
pub fn apply(server: &Server, sink: &Sink<'_>, effects: Vec<Effect>) -> Vec<AdminRequest> {
    let mut admin = Vec::new();
    for e in effects {
        match e {
            Effect::Reply(t) => sink.send(&t),
            Effect::Kick { uuid, screen } => {
                if let Some(p) = server.get_player_by_uuid(api_uuid(uuid)) {
                    kick(&p, &screen);
                }
            }
            Effect::Tell { uuid, message } => {
                if let Some(p) = server.get_player_by_uuid(api_uuid(uuid)) {
                    send(&p, &message);
                }
            }
            Effect::Notify { message, silent, except, audience } => notify(server, &message, silent, except, audience),
            Effect::Log(line) => info(&format!("PumboBans: {line}")),
            Effect::Admin(req) => admin.push(req),
            // Pumpkin has no events between plugins yet (spec §18).
            Effect::Placed(_) | Effect::Lifted(_) => {}
        }
    }
    admin
}

/// Sends a staff message to everyone online with the notify permission.
pub fn notify(server: &Server, message: &Text, silent: bool, except: Option<Uuid>, audience: Audience) {
    let id = pumbo_bans_core::ID;
    let node = match audience {
        Audience::Punishments => pumpkin_node(&permission(id, "notify")),
        Audience::Alts => pumpkin_node(&permission(id, "notify.alts")),
    };
    let silent_node = pumpkin_node(&permission(id, "notify.silent"));
    for p in server.get_all_players() {
        if Some(uuid_of(&p)) == except {
            continue;
        }
        if p.has_permission(&node) && (!silent || p.has_permission(&silent_node)) {
            send(&p, message);
        }
    }
}
