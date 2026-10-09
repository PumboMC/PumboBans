//! The plugin on the fake host of the SDK (`pumbo_sdk::testing`).

use std::time::Instant;

use pumbo_bans_core::Kind;
use pumbo_bans_core::engine::{PunishRequest, Resolved};
use pumbo_bans_core::model::Actor;
use pumbo_sdk::testing::{self, Sent, block_on, command};
use pumbo_sdk::{Connection, Plugin};
use serde_json::Value;

use super::*;

const MANIFEST: &str = include_str!("../pumbo-bans.yml");

fn start_with(config: Option<&str>, store: Result<BansStore, StoreError>) -> PumboBans {
    testing::reset();
    let dir = std::env::temp_dir().join(format!(
        "pumbo-bans-prox-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(dir.join("config.yml"));
    if let Some(c) = config {
        std::fs::write(dir.join("config.yml"), c).unwrap();
    }
    let d = dir.to_string_lossy().to_string();
    let p = PumboBans { state: RefCell::new(None), config_dir: d.clone(), data_dir: d };
    p.start(store).unwrap();
    p
}

fn plugin() -> PumboBans {
    start_with(None, Ok(BansStore::in_memory()))
}

/// A player with every PumboBans node.
fn admin(p: &PumboBans) -> PlayerId {
    let id = testing::add_player("Admin", Some("lobby"));
    for n in p.with(|s| permission_nodes(&s.engine)).unwrap() {
        testing::grant(id, &n);
    }
    id
}

fn set_address(id: PlayerId, address: &str) {
    testing::with(|h| h.players.get_mut(&id).unwrap().connection.address = address.into());
}

fn info(id: PlayerId) -> PlayerInfo {
    players::get(id).unwrap()
}

/// Plain text of a JSON component or any other text.
fn plain(t: &Text) -> String {
    fn walk(v: &Value, out: &mut String) {
        if let Some(s) = v["text"].as_str() {
            out.push_str(s);
        }
        if let Some(list) = v["extra"].as_array() {
            for e in list {
                walk(e, out);
            }
        }
    }
    match t {
        Text::Json(s) => {
            let mut out = String::new();
            walk(&serde_json::from_str(s).unwrap(), &mut out);
            out
        }
        other => testing::render(other),
    }
}

fn messages(id: PlayerId) -> Vec<String> {
    testing::sent_to(id)
        .into_iter()
        .filter_map(|s| match s {
            Sent::Message(t) => Some(plain(&t)),
            _ => None,
        })
        .collect()
}

fn kicks(id: PlayerId) -> Vec<String> {
    testing::sent_to(id)
        .into_iter()
        .filter_map(|s| match s {
            Sent::Kick(t) => Some(plain(&t)),
            _ => None,
        })
        .collect()
}

fn run(p: &PumboBans, by: Option<PlayerId>, name: &str, args: &[&str]) {
    let mut e = command(by.unwrap_or(0), name, args);
    e.player = by;
    block_on(p.on_command(e));
}

fn denied(v: &Verdict) -> Option<String> {
    match v {
        Verdict::Deny(t) => Some(plain(t)),
        Verdict::Allow => None,
    }
}

fn pre_login(p: &PumboBans, name: &str, address: &str) -> Option<String> {
    let e = PreLoginEvent {
        connection: Connection { address: address.into(), ..info_connection() },
        name: name.into(),
        claimed_uuid: None,
    };
    match block_on(p.on_pre_login(e)) {
        PreLoginReply::Deny(t) => Some(plain(&t)),
        _ => None,
    }
}

fn info_connection() -> Connection {
    let id = testing::add_player("Template", None);
    let c = info(id).connection;
    testing::with(|h| h.players.remove(&id));
    c
}

fn topics() -> Vec<(String, pumbo_sdk::contracts::Punishment)> {
    testing::with(|h| h.published.clone())
        .into_iter()
        .map(|(t, b)| (t, pumbo_sdk::service::from_cbor(&b).unwrap()))
        .collect()
}

#[test]
fn ban_online_kicks_and_the_next_login_is_refused() {
    let p = plugin();
    let a = admin(&p);
    let steve = testing::add_player("Steve", Some("survival"));
    run(&p, Some(a), "ban", &["Steve", "1d", "griefing", "the", "spawn"]);
    let kick = kicks(steve);
    assert_eq!(kick.len(), 1, "{:?}", testing::sent_to(steve));
    assert!(kick[0].contains("griefing the spawn") && kick[0].contains("#1"), "{}", kick[0]);
    assert!(messages(a).iter().any(|m| m.contains("Punished Steve for 1 day (temporary ban #1)")), "{:?}", messages(a));
    let (topic, payload) = topics().remove(0);
    assert_eq!(topic, "pumbo:player-punished@1.0");
    assert_eq!(payload.kind, pumbo_sdk::contracts::PunishmentKind::Ban);
    assert_eq!(payload.target, uuid_of(&info(steve)).simple());
    assert_eq!(payload.author, "Admin");

    let refused = denied(&block_on(p.on_login(info(steve)))).unwrap();
    assert!(refused.contains("griefing the spawn") && refused.contains("#1"), "{refused}");

    // Through /pumbo bans and the console as well.
    run(&p, None, "unban", &["Steve"]);
    assert!(testing::with(|h| h.logs.iter().any(|(_, l)| l.contains("lifted a punishment of Steve (temporary ban"))));
    assert!(topics().iter().any(|(t, _)| t == "pumbo:punishment-revoked@1.0"));
    assert!(denied(&block_on(p.on_login(info(steve)))).is_none());
}

#[test]
fn ip_bans_refuse_before_login_and_kick_everyone_on_the_address() {
    let p = plugin();
    let a = admin(&p);
    let steve = testing::add_player("Steve", Some("lobby"));
    let alt = testing::add_player("Alt", Some("lobby"));
    set_address(steve, "5.5.5.5");
    set_address(alt, "5.5.5.5");
    set_address(a, "9.9.9.9");
    run(&p, Some(a), "ipban", &["Steve", "2h", "evasion"]);
    assert_eq!(kicks(steve).len(), 1);
    assert_eq!(kicks(alt).len(), 1);
    assert!(kicks(a).is_empty());
    let screen = pre_login(&p, "Fresh", "5.5.5.5").unwrap();
    assert!(screen.contains("evasion"), "{screen}");
    assert!(pre_login(&p, "Fresh", "5.5.5.6").is_none());
    run(&p, Some(a), "unbanip", &["5.5.5.5"]);
    assert!(pre_login(&p, "Fresh", "5.5.5.5").is_none());
}

#[test]
fn name_ban_of_an_unknown_player_links_the_account() {
    let p = plugin();
    let a = admin(&p);
    run(&p, Some(a), "ban", &["Griefer", "-f", "griefing"]);
    assert!(messages(a).iter().any(|m| m.contains("Punished Griefer permanently")), "{:?}", messages(a));
    let g = testing::add_player("gRiEfEr", None);
    assert!(denied(&block_on(p.on_login(info(g)))).unwrap().contains("griefing"));
    let linked = p.with(|s| s.engine.active_list(Kind::Ban, now()).remove(0).victim_uuid).unwrap();
    assert_eq!(linked, Some(uuid_of(&info(g))));
}

#[test]
fn mutes_block_chat_and_message_commands_without_a_kick() {
    let p = plugin();
    let a = admin(&p);
    let steve = testing::add_player("Steve", Some("lobby"));
    run(&p, Some(a), "mute", &["Steve", "10m", "spam"]);
    assert!(kicks(steve).is_empty());
    assert!(messages(steve).iter().any(|m| m.contains("Admin muted you for 10 minutes")), "{:?}", messages(steve));
    assert_eq!(block_on(p.on_chat(steve, "hello".into())), ChatReply::Cancel);
    assert!(messages(steve).last().unwrap().contains("spam"));
    let cmd = |line: &str, signed: bool| {
        block_on(p.on_backend_command(BackendCommandEvent {
            player: steve,
            server: "lobby".into(),
            line: line.into(),
            signed,
        }))
    };
    assert_eq!(cmd("msg Alex hi", true), BackendCommandReply::Cancel);
    assert_eq!(cmd("minecraft:tell Alex hi", false), BackendCommandReply::Cancel);
    assert_eq!(cmd("spawn", false), BackendCommandReply::Pass);
    let muted =
        |id| testing::with(|h| h.placeholders.get(&("muted".to_string(), Some(id), "global".to_string())).cloned());
    assert_eq!(muted(steve), Some(pumbo_sdk::text::plain("true")));
    assert_eq!(block_on(p.on_chat(a, "hi".into())), ChatReply::Pass);
    run(&p, Some(a), "unmute", &["Steve"]);
    assert_eq!(block_on(p.on_chat(steve, "hello".into())), ChatReply::Pass);
    assert_eq!(cmd("msg Alex hi", true), BackendCommandReply::Pass);
    assert_eq!(muted(steve), Some(pumbo_sdk::text::plain("false")));
}

#[test]
fn three_warnings_bring_the_mute_of_the_first_step() {
    let p = plugin();
    let a = admin(&p);
    let steve = testing::add_player("Steve", Some("lobby"));
    for _ in 0..3 {
        run(&p, Some(a), "warn", &["Steve", "language"]);
    }
    assert!(messages(a).iter().any(|m| m.contains("Warning limit (3) reached by Steve: temporary mute for 1 hour")));
    assert_eq!(block_on(p.on_chat(steve, "x".into())), ChatReply::Cancel);
}

#[test]
fn a_signed_command_meant_for_pumbobans_is_taken_from_the_backend() {
    let p = plugin();
    let a = admin(&p);
    let steve = testing::add_player("Steve", Some("lobby"));
    let e = |line: &str| BackendCommandEvent { player: a, server: "lobby".into(), line: line.into(), signed: true };
    assert_eq!(block_on(p.on_backend_command(e("minecraft:ban Steve"))), BackendCommandReply::Pass);
    assert_eq!(block_on(p.on_backend_command(e("ban Steve 1h signed reason"))), BackendCommandReply::Cancel);
    let timer = p.with(|s| *s.pending.keys().next().unwrap()).unwrap();
    block_on(p.on_timer(timer));
    assert!(kicks(steve)[0].contains("signed reason"), "{:?}", kicks(steve));
    // Without the permission nothing happens.
    let nobody = testing::add_player("Nobody", Some("lobby"));
    let e2 = BackendCommandEvent { player: nobody, server: "lobby".into(), line: "kick Admin".into(), signed: true };
    assert_eq!(block_on(p.on_backend_command(e2)), BackendCommandReply::Cancel);
    let timer = p.with(|s| *s.pending.keys().next().unwrap()).unwrap();
    block_on(p.on_timer(timer));
    assert!(kicks(a).is_empty());
    assert!(messages(nobody).iter().any(|m| m.contains("permission")), "{:?}", messages(nobody));
}

#[test]
fn exemptions_of_online_and_offline_players() {
    let p = plugin();
    let helper = testing::add_player("Helper", Some("lobby"));
    testing::grant(helper, "pumbo.bans.ban");
    let owner = testing::add_player("Owner", Some("lobby"));
    testing::grant(owner, "pumbo.bans.exempt.ban");
    run(&p, Some(helper), "ban", &["Owner"]);
    assert!(messages(helper).iter().any(|m| m.contains("protected")), "{:?}", messages(helper));
    // Offline: the snapshot is refreshed from has-offline (the fake host knows
    // only online players, so Owner loses the exemption once gone).
    block_on(p.on_login(info(owner)));
    block_on(p.on_server_connected(owner, "lobby".into(), None));
    testing::with(|h| h.players.remove(&owner));
    run(&p, Some(helper), "ban", &["Owner"]);
    assert!(messages(helper).last().unwrap().contains("Punished Owner permanently"), "{:?}", messages(helper));
}

#[test]
fn a_reload_with_broken_yaml_keeps_the_settings() {
    let p = start_with(Some("ipc:\n  allow-punish: [pumbo-filter]\n"), Ok(BansStore::in_memory()));
    let allowed = || p.with(|s| s.engine.cfg.ipc.allow_punish.clone()).unwrap();
    let file = format!("{}/config.yml", p.config_dir);
    // `allow-punish` indented less than its section's first line
    std::fs::write(&file, "ipc:\n    allow-queries: true\n  allow-punish: []\n").unwrap();
    let err = block_on(p.on_reload()).unwrap_err();
    assert!(err.contains("config.yml") && err.contains("line 3"), "{err}");
    assert_eq!(allowed(), ["pumbo-filter"]);
    std::fs::write(&file, "ipc:\n  allow-punish: []\n").unwrap();
    block_on(p.on_reload()).unwrap();
    assert!(allowed().is_empty());
}

#[test]
fn the_service_answers_other_plugins() {
    let p = start_with(Some("ipc:\n  allow-punish: [pumbo-filter]\n"), Ok(BansStore::in_memory()));
    let steve = testing::add_player("Steve", Some("lobby"));
    block_on(p.on_login(info(steve)));
    let call = |caller: &str, req: pumbo_common::bans::Request| {
        let mut c = testing::service_call("pumbobans:punish", 1, "request", req.to_bytes());
        c.caller = caller.into();
        block_on(p.on_service_call(c)).unwrap()
    };
    use pumbo_common::bans::{Request, parse_check};
    let punish = Request::Punish {
        kind: "ban".into(),
        target: "Steve".into(),
        duration: Some("1h".into()),
        reason: Some("bot".into()),
        ip: false,
        silent: true,
    };
    let refused: Value = serde_json::from_slice(&call("pumbo-example", punish.clone())).unwrap();
    assert_eq!(refused["ok"], false);
    let placed: Value = serde_json::from_slice(&call("pumbo-filter", punish)).unwrap();
    assert_eq!(placed["id"], 1, "{placed}");
    assert_eq!(kicks(steve).len(), 1);
    let check = Request::Check { uuid: Some(uuid_of(&info(steve)).to_string()), name: None, ip: None };
    let answer = parse_check(&call("pumbo-auth", check)).unwrap();
    assert_eq!(answer.ban.map(|b| (b.id, b.operator)), Some((1, "pumbo-filter".to_string())));
    let wrong = testing::service_call("pumbobans:punish", 1, "check", Vec::new());
    assert_eq!(block_on(p.on_service_call(wrong)), Err(CallReject::UnknownMethod));
}

#[test]
fn without_a_database_nobody_joins() {
    let p = start_with(None, Err(StoreError::new("bans.redb is locked")));
    let steve = testing::add_player("Steve", None);
    assert!(pre_login(&p, "Steve", "1.2.3.4").is_some());
    assert!(denied(&block_on(p.on_login(info(steve)))).is_some());
    assert!(testing::with(|h| h.logs.iter().any(|(_, l)| l.contains("cannot open the database"))));
}

#[test]
fn history_of_500_punishments_is_fast() {
    let p = plugin();
    let a = admin(&p);
    let steve = testing::add_player("Steve", Some("lobby"));
    block_on(p.on_login(info(steve)));
    let target = Resolved::Player { uuid: uuid_of(&info(steve)), name: "Steve".into(), exempt: Vec::new() };
    p.with(|s| {
        for _ in 0..500 {
            let req = PunishRequest {
                kind: Kind::Kick,
                target: target.clone(),
                length: None,
                reason: Some("x".into()),
                template: None,
                silent: true,
                operator: Actor::console("Console"),
                bypass_exempt: true,
                length_limit: None,
                source: None,
            };
            s.engine.punish(req, now()).unwrap();
        }
    });
    let t = Instant::now();
    run(&p, Some(a), "history", &["Steve"]);
    let took = t.elapsed();
    assert!(messages(a).last().unwrap().contains("Punishments of Steve (500, page 1/63)"), "{:?}", messages(a));
    assert!(took.as_millis() < 50, "{took:?}");
    // The proxy refuses texts over 32 KiB ("(invalid text)").
    run(&p, Some(a), "help", &[]);
    for sent in testing::sent_to(a) {
        if let Sent::Message(Text::Json(j)) = sent {
            assert!(j.len() < 16 * 1024, "{} bytes", j.len());
        }
    }
}

#[test]
fn commands_are_registered_short_and_under_pumbo_bans() {
    let _p = plugin();
    let specs = testing::with(|h| h.commands.clone());
    let ban = specs.iter().find(|c| c.name == "ban" && !c.umbrella).unwrap();
    assert_eq!(ban.permission.as_deref(), Some("pumbo.bans.ban"));
    assert!(specs.iter().any(|c| c.name == "checkban" && c.permission.as_deref() == Some("pumbo.bans.check")));
    // The fake host keeps one table, so the umbrella twins of short commands
    // do not show up here; the umbrella-only ones do.
    for name in ["info", "purge", "import", "help"] {
        assert!(specs.iter().any(|c| c.name == name && c.umbrella), "{name}");
    }
    assert!(!specs.iter().any(|c| c.name == "reload" || c.name == "version"));
}

#[test]
fn manifest_lists_events_nodes_topics_and_the_service() {
    let m = pumbo_common::config::parse_yaml(MANIFEST).unwrap();
    assert_eq!(m["id"].as_str(), Some(setup::PLUGIN_ID));
    assert_eq!(m["short-name"].as_str(), Some(ID));
    assert_eq!(m["short-alias"].as_str(), Some(pumbo_bans_core::commands::ALIAS));
    let listed: Vec<&str> = m["permissions"].as_array().unwrap().iter().map(|p| p["node"].as_str().unwrap()).collect();
    let p = plugin();
    let mut needed = p.with(|s| permission_nodes(&s.engine)).unwrap();
    needed.extend(HOST_SUBCOMMANDS.iter().map(|s| permission(ID, s)));
    for n in &needed {
        assert!(listed.contains(&n.as_str()), "{n} missing in pumbo-bans.yml");
    }
    let publishes: Vec<&str> = m["publishes"].as_array().unwrap().iter().filter_map(|t| t.as_str()).collect();
    assert_eq!(publishes, [PLAYER_PUNISHED.versioned(), PUNISHMENT_REVOKED.versioned()]);
    assert_eq!(m["provides"][0]["service"].as_str(), Some(pumbo_common::bans::SERVICE));
    assert_eq!(m["command-filter"][0].as_str(), Some("*"));
    assert_eq!(m["placeholders"]["keys"][0]["name"].as_str(), Some(MUTED_KEY));
}
