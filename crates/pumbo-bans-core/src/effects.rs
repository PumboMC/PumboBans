//! What the platform has to do after a command or an IPC request. The core never
//! talks to the server itself; it returns these and the platform layer carries
//! them out once the plugin state is released. Texts are [`rich::Text`]: the
//! platform turns them into its components (players) or plain text (console).

use pumbo_common::id::Uuid;
use pumbo_common::rich::Text;

use crate::model::Punishment;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    /// A message to whoever ran the command (player or console).
    Reply(Text),
    /// Disconnect an online player with this screen.
    Kick { uuid: Uuid, screen: Text },
    /// A message to one online player.
    Tell { uuid: Uuid, message: Text },
    /// A message to staff: online players with the notify permission (and the
    /// silent one when `silent`), except `except`.
    Notify { message: Text, silent: bool, except: Option<Uuid>, audience: Audience },
    /// A line for the server log (plain text).
    Log(String),
    /// Something only the platform can do (files, the server's own lists).
    Admin(AdminRequest),
    /// A punishment was placed (platforms that announce it to other plugins).
    Placed(Box<Punishment>),
    /// A punishment was lifted.
    Lifted(Box<Punishment>),
}

/// Administrative work that needs the platform.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdminRequest {
    /// Read `config.yml` and the message files again.
    Reload,
    /// Import the server's vanilla ban lists.
    ImportVanilla,
}

/// Which notify permission a staff message needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Audience {
    /// `notify` (and `notify.silent` for silent punishments).
    Punishments,
    /// `notify.alts`.
    Alts,
}

/// Collects effects while a command runs.
#[derive(Default, Debug)]
pub struct Effects(pub Vec<Effect>);

impl Effects {
    pub fn reply(&mut self, text: Text) {
        if !text.is_empty() {
            self.0.push(Effect::Reply(text));
        }
    }

    pub fn push(&mut self, e: Effect) {
        self.0.push(e);
    }

    /// Every text the sender gets, as plain text, in order.
    pub fn replies(&self) -> Vec<String> {
        self.0
            .iter()
            .filter_map(|e| match e {
                Effect::Reply(t) => Some(t.plain()),
                _ => None,
            })
            .collect()
    }

    pub fn kicks(&self) -> Vec<Uuid> {
        self.0
            .iter()
            .filter_map(|e| match e {
                Effect::Kick { uuid, .. } => Some(*uuid),
                _ => None,
            })
            .collect()
    }
}
