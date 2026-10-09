//! PumboBans core: the punishment system without platform code.
//!
//! - [`model`]: punishments, their targets and what is remembered about players
//! - [`engine`]: who may join and talk, placing and lifting punishments, warning
//!   escalation, reason templates, history, alt accounts, retention
//! - [`commands`]: the command tree (`/pumbobans ...`, `/ban`, `/mute`, ...)
//!   parsed and executed into [`effects::Effect`]s
//! - [`store`]: punishments and players on top of `pumbo_common::store`
//! - [`config`]: the `config.yml` options
//! - [`render`]: ban screen, notifications and list lines
//! - [`import`]: vanilla ban lists (`banned-players.json`, `banned-ips.json`)
//! - [`ipc`]: the request format other plugins use to ask PumboBans
//! - [`net`], [`dates`]: address ranges and calendar dates
//!
//! Time comes in as `now` (Unix milliseconds); nothing here talks to a host.

pub mod commands;
pub mod config;
pub mod dates;
pub mod effects;
pub mod engine;
pub mod import;
pub mod ipc;
pub mod model;
pub mod net;
pub mod render;
pub mod store;

pub use config::BansCfg;
pub use engine::Engine;
pub use model::{Actor, Kind, Punishment, Target};

use pumbo_common::lang::Bundle;

/// Plugin id: permissions `pumbo.bans.<action>`, `/pumbo bans`, `bans.redb`.
pub const ID: &str = "bans";

/// Player and staff messages of PumboBans.
pub const LANG: Bundle =
    Bundle { name: "bans", files: &[("en", include_str!("../lang/en.yml")), ("pl", include_str!("../lang/pl.yml"))] };

#[cfg(test)]
mod tests {
    use pumbo_common::lang::{COMMON, Lang, check_bundle};

    use super::*;

    #[test]
    fn messages_are_complete_in_every_language() {
        assert_eq!(check_bundle(&LANG), Vec::<String>::new());
        let (pl, w) = Lang::load(&[COMMON, LANG], "pl", None);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(pl.get("kind-ipban"), "ban na IP");
    }

    #[test]
    fn placeholders_match_between_languages() {
        let names = |text: &str| -> Vec<(String, Vec<String>)> {
            let table = pumbo_common::lang::parse(text).unwrap();
            let mut out: Vec<(String, Vec<String>)> = table
                .into_iter()
                .map(|(k, v)| {
                    let mut p: Vec<String> =
                        v.split('{').skip(1).filter_map(|s| s.split_once('}').map(|(n, _)| n.to_string())).collect();
                    p.sort();
                    p.dedup();
                    (k, p)
                })
                .collect();
            out.sort();
            out
        };
        let en = names(LANG.file("en").unwrap());
        let pl = names(LANG.file("pl").unwrap());
        assert_eq!(en, pl);
    }
}
