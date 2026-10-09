# PumboBans

PumboBans is a punishment system for the [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) Minecraft server: bans, temporary bans, IP bans, mutes, warnings with escalation and kicks, with a full history. It is written in Rust and runs as a WebAssembly plugin. The rules live in `pumbo-bans-core`; the same punishments run on the PumboProx proxy too ([`pumbo-bans-prox`](../pumbo-bans-prox/README.md)).

## Features

- **Bans**: permanent and temporary, by account, by name (players who never joined, with `-f`) and by IP address or range. IPv6 addresses are banned by their /64 network, IPv4-mapped IPv6 counts as IPv4.
- **Checked before the world**: bans are checked when the player logs in, before they enter the world. The disconnect screen shows the reason, who banned them, the time left, the end date, the punishment ID and an optional appeal link, in the player's language.
- **Alt accounts**: four levels of address enforcement (`lenient`, `normal`, `stern`, `strict`), `/alts`, and a staff notice when someone joins from an address a banned account has used.
- **Mutes**: chat and a configurable list of message commands are blocked; blocked messages are neither sent nor written to the server log.
- **Warnings with escalation**: for example 3 active warnings bring a 1 hour mute, 5 bring a 1 day ban.
- **Reason templates**: `/ban Steve #cheating` picks the reason and a length that grows with every repeat (7 days, 30 days, permanent).
- **History**: nothing is deleted; lifted punishments keep who lifted them and when. Clickable pages for `/history`, `/banlist`, `/mutelist`, `/staffhistory`.
- **Staff limits and exemptions**: length limits per staff group, protection of chosen players (also while they are offline).
- **Fail closed**: if the database cannot be opened, nobody joins (configurable).
- **Privacy**: IP addresses are forgotten after a retention period, `/pumbobans purge <player>` removes a player's addresses on request.
- **Import**: the server's own ban lists and vanilla `banned-players.json` / `banned-ips.json` files.
- **Other plugins** can ask PumboBans whether a player is banned or muted, and allowed ones can place punishments (inter-plugin messages).
- English and Polish messages, the client's language per player, the shared Pumbo look (prefix, colours, help pages with clicks and tooltips).

## Compatibility

| File | Pumpkin | Minecraft |
| --- | --- | --- |
| `PumboBans-26.3.wasm` | release `0.2.0+26.3-26.51` | 26.3 |
| `PumboBans-26.2.wasm` | release `0.1.0-dev+26.2-26.45` | 26.2 |

Pumpkin checks the plugin API strictly: a file only loads on the server version it was built for.

## Installation

1. Put the matching `.wasm` file into the `plugins/` folder of the server.
2. Start the server. PumboBans writes `plugins/data/pumbobans/config.yml`, `lang/en.yml` and `lang/pl.yml`, and keeps its data in `plugins/data/pumbobans/bans.redb`.
3. Recommended in `pumpkin.toml`: `allow_chat_reports = false`, so that cancelled chat messages of muted players do not disturb signed chat.
4. Give your staff permissions (operators get the moderation commands by default, see below).
5. Optional: `/pumbobans import vanilla` takes over the bans the server already has.

The plugin needs the `fs.read.data` and `fs.write.data` permissions of Pumpkin's plugin sandbox and nothing else (no network).

## Building

```sh
./build.sh          # dist/PumboBans-26.3.wasm and dist/PumboBans-26.2.wasm
cargo test -p pumbo-bans-core -p pumbo-bans-pumpkin
```

Rust stable with the `wasm32-wasip2` target is required.

## Commands

Everything is available as `/pumbobans <command>` and `/pb <command>`; the short commands are aliases and each can be turned off in `[commands] disabled`. `/pumbobans` or `/pumbobans help [page]` lists the commands you may use.

| Command | Short | Arguments | Permission |
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
| `reload` | | | `admin.reload` |
| `import vanilla` | | | `admin.import` |
| `purge` | | `<player>` | `admin.purge` |
| `version` | | | `admin.version` |

- Times: `30s`, `30m`, `2h`, `7d`, `2w`, `1mo`, `1y`, combined like `1d12h`, or `perm`. Without a time, bans and mutes are permanent.
- `-s` places the punishment silently (only staff with `notify.silent` see it), `-f` punishes a name that has never joined.
- `#template` uses a reason template from the config; text after it is added to the reason.
- `/ban`, `/kick` and `/banlist` replace the server's own commands of the same name, also in the console.

## Permissions

The shared Pumbo name of every permission is `pumbo.bans.<permission>`. Pumpkin only accepts permissions in the plugin's own namespace, so on Pumpkin they are `pumbobans:<permission>`, for example `pumbobans:ban`.

| Permission | Default | Gives |
| --- | --- | --- |
| the command permissions above | operators (level 3) | the command |
| `admin.reload`, `admin.import`, `admin.purge`, `admin.version` | operators (level 4) | administration |
| `command` | operators (level 3) | `/pumbobans` itself (each command still checks its own permission) |
| `notify` | operators (level 3) | notices about punishments |
| `notify.silent` | operators (level 3) | notices about silent punishments too |
| `notify.alts` | operators (level 3) | notice when someone joins from a banned account's address |
| `viewips` | operators (level 4) | full IP addresses in `/alts` (otherwise `1.2.x.x`) |
| `exempt.ban`, `exempt.mute`, `exempt.warn`, `exempt.kick` | nobody | cannot be punished that way |
| `exempt.bypass` | operators (level 4) | punish exempt players anyway |
| `limit.<group>` | nobody | length limits of a staff group from the config |

Exemptions of offline players come from their last login, because Pumpkin cannot check permissions of players who are not online.

The defaults need no setup: Pumpkin grants an "operators (level N)" permission to every operator of level N or higher (`/op` gives level 4 unless `op_permission_level` in `pumpkin.toml` says otherwise). Players without the permission do not see the commands at all ("unknown command"). The console has every permission.

## Configuration

`plugins/data/pumbobans/config.yml` (reload with `/pumbobans reload`). The file is written with short comments on first start, the most used options at the top; the details are here. Lengths are written like `30m`, `12h`, `7d`, `2w`, `1mo`, `1y`, combined like `1d12h`, or `perm`.

| Option | Default | Meaning |
| --- | --- | --- |
| `language` | `en` | default language of messages (`lang/<code>.yml`; `en` and `pl` are bundled) |
| `per-player-language` | `true` | messages in the client's language when a file exists (`pl_pl` → `lang/pl.yml`) |
| `enforcement.address-strictness` | `normal` | `lenient`: only connections from a banned address; `normal`: also accounts that used it; `stern`: also accounts sharing any address with those; `strict`: also account bans reach alt accounts |
| `enforcement.ipv4-prefix` / `ipv6-prefix` | `32` / `64` | how much of an address an IP punishment covers; 32 is one IPv4 address; IPv6 users usually get a whole /64, so 64 stops them from picking the next address |
| `enforcement.fail-closed` | `true` | when the database cannot be opened nobody joins; `false` lets everyone in instead (an error is logged either way) |
| `punishments.require-reason` | `false` | refuse punishments without a reason (a template counts as a reason) |
| `punishments.allow-unknown-names` | `true` | allow `-f` for names that never joined; the punishment sticks to the first account that joins with that name |
| `punishments.allow-override` | `false` | a new ban or mute replaces the old one; `false`: it is refused with a hint to lift the old one first |
| `punishments.limits` | none | longest punishment per staff group, see below |
| `mutes.blocked-commands` | `msg`, `tell`, `w`, `whisper`, `me`, `say`, `r`, `reply`, ... | commands muted players cannot use, without the slash; namespaces such as `minecraft:` are ignored; chat is always blocked |
| `warnings.duration` | `30d` | how long a warning counts towards escalation (`perm`: forever) |
| `warnings.escalation` | 3 → mute 1h, 5 → ban 1d | the punishment that follows a number of active warnings, see below |
| `warnings.repeat-highest` | `true` | above the highest `count`, repeat its action on every further warning |
| `templates` | `cheating`, `spam`, `griefing` | reason templates, see below |
| `notifications.console` | `true` | write punishments to the server log |
| `notifications.alt-join` | `true` | tell staff (`pumbo.bans.notify.alts`) when someone joins from an address a banned account has used |
| `notifications.appeal-url` | empty | shown on the ban screen when set, e.g. `https://example.org/appeal` |
| `notifications.timezone-offset-minutes` | `0` | dates in messages are UTC shifted by this many minutes (120 = UTC+2) |
| `privacy.address-retention-days` | `180` | IP addresses are personal data: an address stays linked to an account for this many days after it was last used, then it is forgotten (0: keep forever); punishments of an address are not affected |
| `privacy.record-refused-logins` | `true` | also remember the address of a login refused because of a ban (helps to find ban evasion) |
| `commands.disabled` | none | short commands not to register, e.g. `[kick]` to keep the server's own `/kick`; everything stays available as `/pumbobans <command>` |
| `commands.page-size` | `8` | entries per page in `/history`, `/banlist`, `/mutelist` and `/staffhistory` |
| `ipc.allow-queries` | `true` | other plugins may ask about bans and mutes |
| `ipc.allow-punish` | none | plugins (by name) allowed to place punishments, e.g. `[pumbo-filter]` |

**Limits per staff group.** Staff with the permission `pumbo.bans.limit.<group>` (on Pumpkin: `pumbobans:limit.<group>`) cannot give longer punishments than their group allows; an empty value means unlimited, and with several groups the most generous one counts:

```yaml
punishments:
  limits:
    - group: helper
      ban: 7d
      mute: 1d
```

**Warning escalation.** When a player reaches `count` active warnings, the punishment follows. `action` is `ban`, `mute` or `kick`; an empty `duration` is permanent. `escalation: []` turns escalation off:

```yaml
warnings:
  escalation:
    - count: 3
      action: mute
      duration: 1h
      reason: Too many warnings
```

**Reason templates.** `/ban Steve #cheating` uses the template `cheating`; text after it is added to the reason. The first punishment of a player with a template gets the first length, the second the second, and so on; the last one repeats:

```yaml
templates:
  - name: cheating
    reason: Cheating
    durations: [7d, 30d, perm]
```

## For other plugins

Send JSON to the plugin `pumbobans` with Pumpkin's inter-plugin call (`ipc::send_ipc_message`):

```json
{"op": "check", "name": "Steve"}
{"op": "punish", "kind": "ban", "target": "1.2.3.4", "ip": true, "duration": "1h", "reason": "bot", "silent": true}
{"op": "history", "target": "Steve", "limit": 20}
{"op": "hello"}
```

Answers are JSON objects with `"ok": true` or `"ok": false, "error": "..."`. Placing punishments needs the sender's plugin name in `ipc.allow-punish`.

## License

See the `license` field in `Cargo.toml` and the license files in the repository root.
