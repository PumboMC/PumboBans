# PumboBans for PumboProx

PumboBans on the PumboProx proxy: bans, temporary bans, IP bans, mutes, warnings with escalation and kicks for the whole network, with one history. The rules are the same as in PumboBans for Pumpkin (`pumbo-bans-core`); this plugin connects them to the proxy. One proxy means one database for every server behind it.

## What happens where

| When | What PumboBans does |
| --- | --- |
| Login, before encryption (`on-pre-login`) | refuses connections from banned addresses |
| Login, with the final UUID (`on-login`) | refuses banned accounts and banned names (a name ban banned with `-f` remembers the first account that uses the name), records the name and address for `/alts`, tells staff when someone joins from an address of a banned account |
| Chat (`on-chat`) and backend commands (`on-backend-command`) | a muted player's messages and the commands from `mutes.blocked-commands` are dropped; the player sees why, nobody gets kicked |
| A ban or kick while the player is online | the player is disconnected from the proxy with the ban screen, on any server |
| Commands | `/ban`, `/mute`, ... and `/pumbobans <command>` (also `/pb <command>` and `/pumbo bans <command>`), from players and from the proxy console |

The ban screen shows the reason, who placed it, the time left, the end date, the punishment ID and the appeal link (`notifications.appeal-url`).

## Installation

1. Copy `pumbo-bans.wasm` into the proxy's `plugins/` folder (under any name, e.g. with the version): one file, the manifest is built in. The first start creates `plugins/pumbo-bans/config.yml` (the defaults with comments), `plugins/pumbo-bans/lang/en.yml` and `lang/pl.yml` (every message) and `plugins/data/pumbo-bans/`; a file you changed is never overwritten (one you did not change gets the new texts of an update), and options or messages a newer version adds or changes are named in the log. A `pumbo-bans.yml` left from an older version overrides the built-in manifest (the log says so): delete it.
2. Optional: edit `plugins/pumbo-bans/config.yml`. The options are the same as on Pumpkin, explained in [the Configuration section of PumboBans](../pumbo-bans-pumpkin/README.md#configuration). The messages are in `plugins/pumbo-bans/lang/<code>.yml`; another language is one more file there. Reload both with `/pumbo bans reload`; a file that is not valid YAML is reported with the line and column and the current settings stay.
3. Recommended in `pumboprox.yml`, so that nobody joins while PumboBans is not running:

   ```yaml
   plugins:
     required-plugins: [pumbo-bans]
   ```

4. Give staff permissions in `permissions.yml` of the proxy, for example:

   ```yaml
   groups:
     staff:
       permissions: ["pumbo.bans.*"]

   players:
     069a79f4-44e9-4726-a5be-fca90e38aaf5:
       name: Notch
       groups: [staff]
   ```

5. Backends and signed chat: dropping a signed chat message leaves a gap in the player's chain of signatures. Pumpkin does not check the chain (`allow_chat_reports = false`, its default), so nothing else is needed there. Paper and vanilla check it and refuse the player's later messages until they reconnect, so give those servers `chat-session-forwarding: false` in `pumboprox.yml` (chat becomes unsigned on that server and mutes are lossless):

   ```yaml
   servers:
     survival: { address: "127.0.0.1:25566", chat-session-forwarding: false }
   ```

The database is `plugins/data/pumbo-bans/bans.redb`. If it cannot be opened, nobody joins (`enforcement.fail-closed: true`); the proxy log says why.

## Configuration

`plugins/pumbo-bans/config.yml`, created at the first start. The options and their meaning are the same as on Pumpkin: see [Configuration of PumboBans](../pumbo-bans-pumpkin/README.md#configuration). Reload with `/pumbo bans reload`; the proxy checks the file first, and a file that is not valid YAML is reported with the line and column while the current settings stay. On the proxy, `ipc.allow-punish: [pumbo-filter]` lets PumboFilter's automatic IP bans through.

## Commands

Everything is available as `/pumbobans <command>`, `/pb <command>` and `/pumbo bans <command>`; the short commands are aliases and each can be turned off in `[commands] disabled`. `/pumbobans help [page]` lists the commands you may use; `/pumbobans reload`, `version` and `debug` are run by the proxy for every plugin (permissions `pumbo.bans.reload`, `pumbo.bans.version`, `pumbo.bans.debug`).

| Command | Short | Arguments | Permission `pumbo.bans.` |
| --- | --- | --- | --- |
| `ban` | `/ban` | `<player> [time] [reason\|#template] [-s] [-f]` | `ban` |
| `tempban` | `/tempban` | `<player> <time> [reason] [-s] [-f]` | `tempban` |
| `ipban` | `/ipban`, `/banip` | `<player\|ip> [time] [reason] [-s]` | `ipban` |
| `mute` | `/mute` | `<player> [time] [reason\|#template] [-s] [-f]` | `mute` |
| `tempmute` | `/tempmute` | `<player> <time> [reason] [-s] [-f]` | `tempmute` |
| `ipmute` | `/ipmute` | `<player\|ip> [time] [reason] [-s]` | `ipmute` |
| `warn` | `/warn` | `<player> [reason\|#template] [-s] [-f]` | `warn` |
| `kick` | `/kick` | `<player> [reason] [-s]` | `kick` |
| `unban` | `/unban` | `<player\|ip\|#id> [-s]` | `unban` |
| `unbanip` | `/unbanip` | `<player\|ip> [-s]` | `unbanip` |
| `unmute` | `/unmute` | `<player\|ip\|#id> [-s]` | `unmute` |
| `unwarn` | `/unwarn` | `<player\|#id> [-s]` | `unwarn` |
| `history` | `/history` | `<player\|ip> [page]` | `history` |
| `warns` | `/warns` | `<player>` | `warns` |
| `banlist` | `/banlist` | `[page]` | `banlist` |
| `mutelist` | `/mutelist` | `[page]` | `mutelist` |
| `check` | `/checkban` | `<player\|ip>` | `check` |
| `alts` | `/alts` | `<player>` | `alts` |
| `staffhistory` | `/staffhistory` | `<staff\|console> [page]` | `staffhistory` |
| `info` | | `<#id>` | `history` |
| `import vanilla` | | | `admin.import` |
| `purge` | | `<player>` | `admin.purge` |

Other nodes: `notify`, `notify.silent`, `notify.alts`, `viewips`, `exempt.<ban|mute|warn|kick>`, `exempt.bypass`, `limit.<group>`; all are listed with descriptions in `pumbo-bans.yml`, so `/pumbo proxy perms list` shows them. Times: `30s`, `30m`, `2h`, `7d`, `2w`, `1mo`, `1y`, combined (`1d12h`), `perm`. Flags: `-s` silent, `-f` a name that never joined.

Commands from the proxy console answer in the proxy log.

**Commands with the same name on a backend.** The proxy leaves a command name to the backend when the backend has it too (vanilla and Pumpkin have `/ban`, `/kick`, `/banlist`). An unsigned command still reaches PumboBans first. When the client signs it (because the backend's command takes a message, as in `/ban Steve griefing`), the proxy would pass it to the backend; PumboBans takes such a command back in `on-backend-command`, cancels it and runs it itself. `minecraft:ban` and the other namespaced forms still reach the backend. To leave a short command to the backends altogether, put it in `[commands] disabled`.

**Exemptions.** `pumbo.bans.exempt.<kind>` protects a player from that punishment (unless the staff member has `exempt.bypass`). For an online player PumboBans asks the proxy right away; for an offline player it asks `permissions.has-offline` (the permission file and the permission provider). If that fails, the player is not treated as exempt and the log says so.

**Import.** `/pumbobans import vanilla` reads `banned-players.json` and `banned-ips.json` (format of vanilla, Paper and Pumpkin) from `plugins/data/pumbo-bans/`.

## For other plugins

- Service `pumbobans:punish@1.0`, method `request`: the JSON requests `hello`, `check` (is a player banned or muted), `punish` (only for plugin IDs in `ipc.allow-punish`, e.g. `"pumbo-filter"`) and `history`. The contract with types is `pumbo_common::bans` (MIT OR Apache-2.0); it is the same format as the inter-plugin messages of PumboBans on Pumpkin. Declare it with `uses = [{ service = "pumbobans:punish", version = "1.0" }]`.
- Topics `pumbo:player-punished@1.0` and `pumbo:punishment-revoked@1.0` (types in `pumbo-contracts`): kind (`ban`, `ip-ban`, `mute`, `warn`, `kick`), target (UUID without dashes or an address range), end time, author, reason, ID.
- Placeholder `%pumbobans_muted%` (`true` / `false`, per player).

## Building

```sh
./build.sh          # dist/pumbo-bans.wasm
cargo test -p pumbo-bans-prox
```

Rust stable with the `wasm32-wasip2` target. The SDK `pumbo-sdk` (MIT OR Apache-2.0) comes from [PumboProx](https://github.com/PumboMC/PumboProx) as a git dependency.

## License

GPL-3.0-only, like the other Pumbo plugins.
