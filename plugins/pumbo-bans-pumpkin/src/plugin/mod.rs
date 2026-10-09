//! WebAssembly glue: registration of permissions, commands, events, the
//! maintenance task and inter-plugin messages.

mod commands;
mod events;
mod host;
mod state;

use pumbo_bans_core::commands::{main_command, permission_nodes, shortcuts};
use pumbo_bans_core::store::BansStore;
use pumbo_bans_core::{Engine, ipc};
use pumbo_common::clock::now_ms;
use pumbo_common::command::permission;
use pumbo_common::store::Store;

use crate::papi::scheduler::SchedulerExt;
use crate::setup::{self, PLUGIN_NAME, pumpkin_node};
use host::{
    ArgumentType, Command, CommandNode, Context, EventPriority, Permission, PermissionDefault, PermissionLevel, Plugin,
    PluginMetadata, StringType, register_plugin,
};

/// Forget addresses past retention at most this often.
const FORGET_EVERY_MS: u64 = 3_600_000;

struct PumboBans;

/// Node that lets a player use `/pumbobans` at all (each subcommand still
/// checks its own permission).
fn command_node() -> String {
    permission(pumbo_bans_core::ID, "command")
}

fn level(op: u8) -> PermissionLevel {
    match op {
        0 => PermissionLevel::Zero,
        1 => PermissionLevel::One,
        2 => PermissionLevel::Two,
        3 => PermissionLevel::Three,
        _ => PermissionLevel::Four,
    }
}

fn register_permissions(context: &Context, nodes: &[String]) {
    for node in nodes {
        let (default, description) = setup::node_info(node);
        let default = match default {
            setup::Default::Op(n) => PermissionDefault::Op(level(n)),
            setup::Default::Nobody => PermissionDefault::Deny,
        };
        // Already registered (after a reload) is fine.
        let _ = context.register_permission(&Permission {
            node: pumpkin_node(node),
            description,
            default,
            children: vec![],
        });
    }
}

impl Plugin for PumboBans {
    fn new() -> Self {
        PumboBans
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: PLUGIN_NAME.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["Patryk Skoczylas".into()],
            description: "Bans, mutes, warnings and kicks with history".into(),
            dependencies: vec![],
            permissions: vec![
                host::permissions::FS_READ_DATA.to_string(),
                host::permissions::FS_WRITE_DATA.to_string(),
            ],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        host::remember_server(context.get_server());
        let dir = context.get_data_folder();
        let _ = std::fs::create_dir_all(&dir);
        let (cfg, mut warnings) = setup::load_config(&dir);
        let (langs, w) = setup::load_langs(&dir, &cfg);
        warnings.extend(w);
        for w in &warnings {
            host::warn(&format!("PumboBans: config: {w}"));
        }
        let store = Store::open(format!("{dir}/{}.redb", pumbo_bans_core::ID)).and_then(BansStore::new);
        let (engine, problems) = Engine::new(cfg, langs, store, host::API_LABEL);
        for p in &problems {
            // Fail closed: without the database nobody gets in (unless configured otherwise).
            host::error(&format!("PumboBans: {p}"));
        }
        let disabled = engine.cfg.commands.disabled.clone();
        let mut nodes = permission_nodes(&engine);
        nodes.push(command_node());
        let active = engine.active_count(now_ms());
        state::install(state::State { engine, data_dir: dir, last_forget_ms: 0 });

        register_permissions(&context, &nodes);

        let main = main_command();
        let cmd = Command::new(&[main.clone(), pumbo_bans_core::commands::ALIAS.into()], "PumboBans")
            .then(
                CommandNode::argument("args", &ArgumentType::String(StringType::Greedy))
                    .execute(commands::Handler { label: None, sub: None })
                    .suggest(commands::Suggest { label: None }),
            )
            .execute(commands::Handler { label: None, sub: None });
        context.register_command(cmd, &pumpkin_node(&command_node()));
        for (name, sub) in shortcuts(&disabled) {
            let cmd = Command::new(&[name.to_string()], "PumboBans")
                .then(
                    CommandNode::argument("args", &ArgumentType::String(StringType::Greedy))
                        .execute(commands::Handler { label: Some(name), sub: Some(sub) })
                        .suggest(commands::Suggest { label: Some(name) }),
                )
                .execute(commands::Handler { label: Some(name), sub: Some(sub) });
            let node = pumbo_bans_core::commands::tree()
                .subs()
                .iter()
                .find(|s| s.name == sub)
                .map(|s| permission(pumbo_bans_core::ID, s.action))
                .unwrap_or_else(command_node);
            context.register_command(cmd, &pumpkin_node(&node));
        }

        context.register_event_handler::<host::AsyncPlayerPreLoginEvent, _>(
            events::PreLogin,
            EventPriority::Highest,
            true,
        )?;
        context.register_event_handler::<host::PlayerJoinEvent, _>(events::Join, EventPriority::Normal, false)?;
        context.register_event_handler::<host::PlayerChatEvent, _>(events::Chat, EventPriority::Highest, true)?;
        context.register_event_handler::<host::PlayerCommandSendEvent, _>(
            events::CommandSend,
            EventPriority::Highest,
            true,
        )?;

        // One repeating task (closures are never freed by the API): retire
        // expired punishments every minute, forget old addresses every hour.
        context.schedule_repeating_task(1200, 1200, |_server| {
            let now = now_ms();
            let result = state::with(|s| {
                let forget = now.saturating_sub(s.last_forget_ms) >= FORGET_EVERY_MS;
                if forget {
                    s.last_forget_ms = now;
                }
                s.engine.sweep(now, forget)
            });
            match result {
                Some(Ok(r)) if r.addresses_forgotten > 0 => {
                    host::info(&format!(
                        "PumboBans: forgot {} addresses past the retention period",
                        r.addresses_forgotten
                    ));
                }
                Some(Err(e)) => host::warn(&format!("PumboBans: maintenance failed: {e}")),
                _ => {}
            }
        });

        host::info(&format!(
            "PumboBans {} loaded for {} ({active} active punishments, commands /{main} and {} short ones)",
            env!("CARGO_PKG_VERSION"),
            host::API_LABEL,
            shortcuts(&disabled).len()
        ));
        Ok(())
    }

    fn handle_ipc_message(&self, sender: String, message: Vec<u8>) -> Result<Vec<u8>, String> {
        let online = host::with_server(|s| host::online(s, &[])).unwrap_or_default();
        let now = now_ms();
        let result = state::with(|s| ipc::handle(&mut s.engine, &sender, &message, &online, now));
        let Some((answer, effects)) = result else {
            return Err("PumboBans is busy".into());
        };
        host::with_server(|server| host::apply(server, &host::Sink::None, effects.0));
        Ok(answer)
    }
}

register_plugin!(PumboBans);
