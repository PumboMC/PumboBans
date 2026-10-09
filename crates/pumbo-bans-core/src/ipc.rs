//! Requests from other plugins (inter-plugin messages). The format is the
//! contract in `pumbo_common::bans`: on Pumpkin through the host's IPC call to
//! `pumbobans`, on PumboProx through the service `pumbobans:punish@1.0`.
//! Requests and answers are JSON objects:
//!
//! | request | answer |
//! | --- | --- |
//! | `{"op":"hello"}` | `{"ok":true,"plugin":"PumboBans","version":"0.1.0","protocol":1}` |
//! | `{"op":"check","uuid":"..","name":"..","ip":".."}` (any of the three) | `{"ok":true,"ban":{..}\|null,"mute":{..}\|null,"warnings":2}` |
//! | `{"op":"punish","kind":"ban","target":"Steve","duration":"1d","reason":"..","ip":false,"silent":false}` | `{"ok":true,"id":12}` |
//! | `{"op":"history","target":"Steve","limit":20}` | `{"ok":true,"punishments":[..]}` |
//!
//! Errors are `{"ok":false,"error":"..."}`. `check` needs `ipc.allow-queries`,
//! `punish` needs the sender in `ipc.allow-punish`.

use pumbo_common::bans::{Request, Summary};
use pumbo_common::id::{Uuid, parse_ip};
use serde_json::{Value, json};

use crate::commands::{self, Ctx, Online, Sender};
use crate::config::parse_length;
use crate::effects::Effects;
use crate::engine::{Engine, Subject};
use crate::model::{Actor, Kind, Punishment};
use crate::net;

/// Version of the request format.
pub const PROTOCOL: u32 = pumbo_common::bans::PROTOCOL;

fn error(msg: impl Into<String>) -> Value {
    json!({"ok": false, "error": msg.into()})
}

fn summary(p: &Punishment) -> Value {
    let s = Summary {
        id: p.id,
        kind: p.kind.as_str().into(),
        target: p.target_label(),
        reason: p.reason.clone(),
        operator: p.operator.name.clone(),
        created: p.created,
        expires: p.expires,
        revoked: p.revoked.is_some(),
    };
    serde_json::to_value(s).unwrap_or(Value::Null)
}

/// Handles one message from plugin `from`. Returns the answer and the effects to
/// carry out (kicks and notices of a placed punishment).
pub fn handle(engine: &mut Engine, from: &str, message: &[u8], online: &[Online], now: u64) -> (Vec<u8>, Effects) {
    let (answer, fx) = match Request::parse(message) {
        Ok(req) => answer(engine, from, req, online, now),
        Err(e) => (error(e), Effects::default()),
    };
    (answer.to_string().into_bytes(), fx)
}

fn answer(engine: &mut Engine, from: &str, req: Request, online: &[Online], now: u64) -> (Value, Effects) {
    let none = Effects::default();
    match req {
        Request::Hello => (
            json!({"ok": true, "plugin": "PumboBans", "version": env!("CARGO_PKG_VERSION"), "protocol": PROTOCOL}),
            none,
        ),
        Request::Check { uuid, name, ip } => {
            if !engine.cfg.ipc.allow_queries {
                return (error("queries are disabled (ipc.allow-queries)"), none);
            }
            let uuid = match uuid.as_deref().and_then(Uuid::parse) {
                Some(u) => Some(u),
                None => name.as_deref().and_then(|n| engine.uuid_by_name(n).ok().flatten()),
            };
            let s = Subject { uuid, name, ip: ip.as_deref().and_then(parse_ip) };
            let ban = engine.find_active(Kind::Ban, &s, now);
            let mute = engine.find_active(Kind::Mute, &s, now);
            let (Ok(ban), Ok(mute)) = (ban, mute) else { return (error("the database is unavailable"), none) };
            let warnings = s
                .uuid
                .map(|u| {
                    let name = s.name.clone().unwrap_or_default();
                    engine
                        .active_warns_for(&crate::engine::Resolved::Player { uuid: u, name, exempt: Vec::new() }, now)
                        .len()
                })
                .unwrap_or(0);
            (
                json!({"ok": true, "ban": ban.as_ref().map(summary), "mute": mute.as_ref().map(summary), "warnings": warnings}),
                none,
            )
        }
        Request::Punish { kind, target, duration, reason, ip, silent } => {
            if !engine.cfg.ipc.allow_punish.iter().any(|p| p.eq_ignore_ascii_case(from)) {
                return (error(format!("plugin {from} may not punish (ipc.allow-punish)")), none);
            }
            let Some(kind) = Kind::parse(&kind) else { return (error(format!("unknown kind {kind}")), none) };
            let length = match duration.as_deref() {
                None | Some("") => None,
                Some(d) => match parse_length(d) {
                    Some(t) => Some(t),
                    None => return (error(format!("bad duration {d}")), none),
                },
            };
            let sender = Sender { actor: Actor::console(from), ..Sender::console() };
            let ctx = Ctx { sender: &sender, online, now, label: None };
            let order = commands::PluginOrder { kind, by_address: ip, target, length, reason, silent };
            match commands::punish_for_plugin(engine, &ctx, order) {
                Ok((done, fx)) => (json!({"ok": true, "id": done.punishment.id}), fx),
                Err(e) => (error(e), none),
            }
        }
        Request::History { target, limit } => {
            if !engine.cfg.ipc.allow_queries {
                return (error("queries are disabled (ipc.allow-queries)"), none);
            }
            let cfg = &engine.cfg.enforcement;
            let list = if let Some(range) = net::parse_range(&target, cfg.ipv4_prefix, cfg.ipv6_prefix) {
                engine.history_of_address(&range)
            } else {
                let uuid = Uuid::parse(&target).or_else(|| engine.uuid_by_name(&target).ok().flatten());
                match uuid {
                    Some(u) => engine.history_of_player(u),
                    None => engine.history_of_name(&target),
                }
            };
            match list {
                Ok(list) => {
                    let items: Vec<Value> = list.iter().take(limit.unwrap_or(20).min(200)).map(summary).collect();
                    (json!({"ok": true, "punishments": items}), none)
                }
                Err(_) => (error("the database is unavailable"), none),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pumbo_common::config::load;
    use pumbo_common::lang::{COMMON, Lang};

    use super::*;
    use crate::BansCfg;
    use crate::render::Langs;
    use crate::store::BansStore;

    const T0: u64 = 1_800_000_000_000;

    fn engine(cfg: &str) -> Engine {
        let (cfg, _) = load::<BansCfg>(cfg);
        let lang = Lang::load(&[COMMON, crate::LANG], "en", None).0;
        Engine::new(cfg, Langs::single(lang), Ok(BansStore::in_memory()), "test").0
    }

    fn call(e: &mut Engine, from: &str, req: &str, online: &[Online]) -> (Value, Effects) {
        let (bytes, fx) = handle(e, from, req.as_bytes(), online, T0);
        (serde_json::from_slice(&bytes).unwrap(), fx)
    }

    #[test]
    fn hello_check_punish_history() {
        let mut e = engine("ipc:\n  allow-punish: [PumboFilter]\n");
        let (v, _) = call(&mut e, "pumbofilter", r#"{"op":"hello"}"#, &[]);
        assert_eq!(v["protocol"], 1);
        let (v, _) = call(&mut e, "pumbofilter", r#"{"op":"check","name":"Steve"}"#, &[]);
        assert_eq!(v["ban"], Value::Null);
        let online =
            [Online { uuid: Uuid(1), name: "Bot1".into(), ip: parse_ip("9.9.9.9"), locale: None, exempt: vec![] }];
        let (v, fx) = call(
            &mut e,
            "pumbofilter",
            r#"{"op":"punish","kind":"ban","target":"9.9.9.9","ip":true,"duration":"1h","reason":"bot","silent":true}"#,
            &online,
        );
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["id"], 1);
        assert_eq!(fx.kicks(), vec![Uuid(1)]);
        assert!(fx.replies().is_empty());
        let (v, _) = call(&mut e, "pumbofilter", r#"{"op":"check","ip":"9.9.9.9"}"#, &[]);
        assert_eq!(v["ban"]["id"], 1);
        assert_eq!(v["ban"]["operator"], "pumbofilter");
        let (v, _) = call(&mut e, "x", r#"{"op":"history","target":"9.9.9.9"}"#, &[]);
        assert_eq!(v["punishments"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn refusals() {
        let mut e = engine("ipc:\n  allow-queries: false\n");
        let (v, _) = call(&mut e, "other", r#"{"op":"punish","kind":"ban","target":"Steve"}"#, &[]);
        assert_eq!(v["ok"], false);
        let (v, _) = call(&mut e, "other", r#"{"op":"check","name":"Steve"}"#, &[]);
        assert_eq!(v["ok"], false);
        let (v, _) = call(&mut e, "other", "not json", &[]);
        assert!(v["error"].as_str().unwrap().starts_with("bad request"));
        let (v, _) = call(&mut e, "other", r#"{"op":"fly"}"#, &[]);
        assert_eq!(v["ok"], false);
    }
}
