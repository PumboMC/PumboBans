<p align="center">
  <img src="assets/logo.png" alt="Pumbo logo" width="160">
</p>

<h1 align="center">PumboBans</h1>

<p align="center">Bans, mutes and warnings for Pumpkin servers and PumboProx networks.</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0-blue" alt="License: GPL-3.0"></a>
  <img src="https://img.shields.io/badge/built%20with-Rust-orange?logo=rust" alt="Built with Rust">
  <img src="https://img.shields.io/badge/plugin-WebAssembly-654FF0?logo=webassembly&logoColor=white" alt="WebAssembly plugin">
  <a href="https://github.com/Pumpkin-MC/Pumpkin"><img src="https://img.shields.io/badge/Pumpkin-0.2.0%20%2826.3%29-F28C28" alt="Pumpkin 0.2.0 (26.3)"></a>
  <a href="https://github.com/Pumpkin-MC/Pumpkin"><img src="https://img.shields.io/badge/Pumpkin-0.1.0--dev%20%2826.2%29-F28C28" alt="Pumpkin 0.1.0-dev (26.2)"></a>
  <a href="https://github.com/PumboMC/PumboProx"><img src="https://img.shields.io/badge/PumboProx-supported-62B47A" alt="PumboProx: supported"></a>
  <img src="https://img.shields.io/badge/status-beta-yellow" alt="Status: beta">
</p>

<p align="center">
  <a href="#features">Features</a> ·
  <a href="#two-builds">Two builds</a> ·
  <a href="#installation">Installation</a> ·
  <a href="#configuration">Configuration</a> ·
  <a href="#commands-and-permissions">Commands</a> ·
  <a href="#building">Building</a>
</p>

---

<p align="center">
  <a href="https://github.com/PumboMC/PumboProx"><img src="assets/pumboprox.webp" alt="PumboProx: everything you need to run a network on Pumpkin" width="100%"></a>
</p>

<p align="center"><b>Running more than one server?</b> <a href="https://github.com/PumboMC/PumboProx">PumboProx</a> is the proxy for Pumpkin networks, with plugins in WebAssembly.<br>PumboBans runs on it too: one ban covers the whole network, or just the server you name.</p>

> [!NOTE]
> PumboBans is in **beta** (0.1.0-beta.1). Try it on a test server before you put players on it.

## What it does

PumboBans gives your staff bans, mutes, warnings and kicks, with a full history of every punishment. Bans are checked when the player logs in, before they enter the world. On PumboProx one ban covers every server of the network.

## Features

| | Feature | |
| --- | --- | --- |
| 🔨 | **Bans** | Permanent bans by account, or by name for players who never joined (`-f`). |
| ⏱️ | **Temporary bans** | Any length: `30m`, `7d`, `1mo`, or combined like `1d12h`. |
| 🌐 | **IP bans** | Ban an address or a range. IPv6 addresses are banned by their /64 network, so the next address does not help. |
| 🔇 | **Mutes** | Chat and message commands (`/msg`, `/tell`, `/me`, ...) are blocked. The player sees why and stays online. |
| ⚠️ | **Warnings** | Warnings count for 30 days (by default) and lead to the next punishment. |
| 👢 | **Kicks** | Disconnect a player with a reason. |
| 📑 | **Templates** | `/ban Steve #cheating` picks the reason and a length that grows with every repeat (7 days, 30 days, permanent). |
| 📈 | **Escalation** | For example 3 active warnings bring a 1 hour mute, 5 bring a 1 day ban. |
| ✉️ | **Appeals** | The ban screen shows the reason, who banned, the time left, the punishment ID and your appeal link. |
| 📜 | **History** | Nothing is deleted. Lifted punishments keep who lifted them and when. Clickable pages in `/history`, `/banlist`, `/mutelist`, `/staffhistory`. |
| 👥 | **Alt accounts** | `/alts` shows accounts that share an address. Four levels decide how far a ban reaches alt accounts. |
| 🛡️ | **Exemptions** | Protect chosen players from a kind of punishment, also while they are offline. |
| 📏 | **Staff limits** | The longest ban or mute each staff group may give. |
| 📥 | **Import** | Takes over vanilla `banned-players.json` and `banned-ips.json` (also from Paper and Pumpkin). |
| 🔔 | **Notifications** | Staff see every punishment, and a notice when someone joins from an address a banned account has used. |
| 🔐 | **Fail closed** | If the database cannot be opened, nobody joins (you can turn this off). |
| 🧹 | **Privacy** | IP addresses are forgotten after 180 days (by default). `/pumbobans purge <player>` removes them on request. |

## Two builds

The punishment rules live in one shared core. You pick the build that fits your setup.

| Build | File | Where it goes | What is different |
| --- | --- | --- | --- |
| 🌐 **PumboProx** (whole network) | `PumboBans-Proxy-<version>.wasm` | `plugins/` of the proxy | One database for every server. Banned players are refused at the proxy, and a ban disconnects a player from any server. |
| 🎃 **Pumpkin** (one server) | `PumboBans-Pumpkin-26.3-<version>.wasm` or `PumboBans-Pumpkin-26.2-<version>.wasm` | `plugins/` of the server | Bans, mutes and history for that server. `/ban`, `/kick` and `/banlist` replace the server's own commands. |

## Installation

> [!TIP]
> Download the files from [Releases](https://github.com/PumboMC/PumboBans/releases/latest), or [build from source](#building).

**On PumboProx**

1. Put `PumboBans-Proxy-<version>.wasm` into the proxy's `plugins/` folder.
2. Start the proxy. The first start creates `plugins/pumbo-bans/config.yml` with comments.
3. Recommended in `pumboprox.yml`, so nobody joins while PumboBans is not running:

   ```yaml
   plugins:
     required-plugins: [pumbo-bans]
   ```

4. Give your staff `pumbo.bans.*` in the proxy's `permissions.yml`.

**On Pumpkin**

1. Put the file that matches your Pumpkin version into the server's `plugins/` folder.
2. Start the server. The first start creates `plugins/data/pumbobans/config.yml` and the language files.
3. Operators get the moderation commands by default. To take over the server's existing bans, run `/pumbobans import vanilla`.

The plugin only needs to read and write its own data folder. It has no network access.

## Configuration

The config file is written on the first start, with a short comment on every option. Reload it with `/pumbobans reload`. A wrong value is reported in the log with the option name and replaced by the default. The most used options:

| Option | Default | What it does |
| --- | --- | --- |
| `language` | `en` | Language of the messages: `en`, `pl` or any code with a `lang/<code>.yml` file. |
| `per-player-language` | `true` | Messages in the player's game language when a file for it exists. |
| `enforcement.address-strictness` | `normal` | How far bans reach alt accounts: `lenient`, `normal`, `stern` or `strict`. |
| `enforcement.fail-closed` | `true` | Nobody joins when the database cannot be opened. |
| `punishments.require-reason` | `false` | Refuse punishments without a reason. |
| `punishments.limits` | none | The longest punishment per staff group. |
| `mutes.blocked-commands` | `msg`, `tell`, `me`, ... | Commands a muted player cannot use. |
| `warnings.duration` | `30d` | How long a warning counts. |
| `warnings.escalation` | 3 → mute 1h, 5 → ban 1d | What follows a number of active warnings. |
| `templates` | `cheating`, `spam`, `griefing` | Reason templates with growing lengths. |
| `notifications.appeal-url` | empty | Your appeal link on the ban screen. |
| `privacy.address-retention-days` | `180` | When an unused address is forgotten (0 keeps it forever). |
| `commands.disabled` | none | Short commands not to register, for example `[kick]` to keep the server's own `/kick`. |
| `ipc.allow-punish` | none | Plugins allowed to place punishments, for example `[pumbo-filter]`. |

## Commands and permissions

Every command works as `/pumbobans <command>`, or `/pb <command>` for short. The top-level commands below (`/ban`, `/mute`, ...) are aliases you can turn off one by one in `commands.disabled`.

| Command | Arguments | Permission |
| --- | --- | --- |
| `/ban` | `<player> [time] [reason\|#template] [-s] [-f]` | `pumbo.bans.ban` |
| `/tempban` | `<player> <time> [reason] [-s] [-f]` | `pumbo.bans.tempban` |
| `/ipban`, `/banip` | `<player\|ip> [time] [reason] [-s]` | `pumbo.bans.ipban` |
| `/mute` | `<player> [time] [reason\|#template] [-s] [-f]` | `pumbo.bans.mute` |
| `/tempmute` | `<player> <time> [reason] [-s] [-f]` | `pumbo.bans.tempmute` |
| `/ipmute` | `<player\|ip> [time] [reason] [-s]` | `pumbo.bans.ipmute` |
| `/warn` | `<player> [reason\|#template] [-s] [-f]` | `pumbo.bans.warn` |
| `/kick` | `<player> [reason] [-s]` | `pumbo.bans.kick` |
| `/unban`, `/unmute` | `<player\|ip\|#id> [-s]` | `pumbo.bans.unban`, `pumbo.bans.unmute` |
| `/unbanip` | `<player\|ip> [-s]` | `pumbo.bans.unbanip` |
| `/unwarn` | `<player\|#id> [-s]` | `pumbo.bans.unwarn` |
| `/history` | `<player\|ip> [page]` | `pumbo.bans.history` |
| `/warns` | `<player>` | `pumbo.bans.warns` |
| `/banlist`, `/mutelist` | `[page]` | `pumbo.bans.banlist`, `pumbo.bans.mutelist` |
| `/checkban` | `<player\|ip>` | `pumbo.bans.check` |
| `/alts` | `<player>` | `pumbo.bans.alts` |
| `/staffhistory` | `<staff\|console> [page]` | `pumbo.bans.staffhistory` |
| `/pumbobans import vanilla` | | `pumbo.bans.admin.import` |
| `/pumbobans purge` | `<player>` | `pumbo.bans.admin.purge` |

Under `/pumbobans` and `/pb` some commands have other names too: `tban` (tempban), `banip` (ipban), `tmute` (tempmute), `muteip` (ipmute), `pardon` (unban), `unipban` (unbanip), `delwarn` (unwarn), `hist` (history), `warnings` (warns), `bans` (banlist), `mutes` (mutelist), `checkban` and `checkmute` (check), `dupeip` (alts), `blame` (staffhistory). `/pumbobans info <#id>` shows one punishment.

`-s` punishes silently (only staff with `notify.silent` see it), `-f` punishes a name that never joined. Without a time, bans and mutes are permanent.

Other permissions: `notify`, `notify.silent`, `notify.alts`, `viewips` (full addresses in `/alts`), `exempt.<ban|mute|warn|kick>`, `exempt.bypass` and `limit.<group>`, all under `pumbo.bans.`.

On PumboProx every command also works as `/pumbo bans <command>` (`/pumbo bans` alone shows the help) and from the proxy console. On Pumpkin the permissions are named `pumbobans:<name>` (for example `pumbobans:ban`) and default to operators.

## Works with other Pumbo plugins

PumboBans works on its own. When it finds other Pumbo plugins, it works with them:

| Plugin | Together |
| --- | --- |
| 🛡️ [PumboFilter](https://github.com/PumboMC/PumboFilter) | Places temporary IP bans for addresses that keep failing the bot check, when you allow it in `ipc.allow-punish`. |
| 🔒 [PumboAuth](https://github.com/PumboMC/PumboAuth) | Banned players are refused before the login. On PumboProx a ban also ends the player's sessions. |
| 🌐 [PumboProx](https://github.com/PumboMC/PumboProx) | One ban list for the whole network. Other plugins can ask whether a player is banned or muted, and the placeholder `%pumbobans_muted%` is available. |

## Building

You need Rust stable with the WebAssembly target:

```sh
rustup target add wasm32-wasip2
```

Cargo fetches the shared Pumbo libraries (`pumbo-common`, `pumbo-sdk`) from the [PumboProx](https://github.com/PumboMC/PumboProx) repository on the first build.

Pumpkin build, both versions (`plugins/pumbo-bans-pumpkin/dist/PumboBans-26.3.wasm` and `plugins/pumbo-bans-pumpkin/dist/PumboBans-26.2.wasm`):

```sh
plugins/pumbo-bans-pumpkin/build.sh
```

PumboProx build (`plugins/pumbo-bans-prox/dist/pumbo-bans.wasm`):

```sh
plugins/pumbo-bans-prox/build.sh
```

Tests of the core and both builds:

```sh
cargo test
```

## License

PumboBans (the core and both builds) is licensed under the [GNU General Public License v3.0](LICENSE). The shared library for Pumbo plugins (`pumbo-common`) is dual-licensed under MIT and Apache-2.0.

PumboBans is not affiliated with Mojang, Microsoft or the Pumpkin project.

---

<p align="center">
  Part of <a href="https://github.com/PumboMC/PumboProx"><b>PumboProx</b></a>. Everything you need to run a network on Pumpkin.
</p>
