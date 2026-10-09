# PumboBans

Bans, mutes, warnings and kicks for Pumpkin servers, with a full history of every punishment. Bans are checked at login, before the player enters the world.

## Features

- **Bans**, permanent or for any length (`30m`, `7d`, `1d12h`), also for players who never joined.
- **IP bans** of an address or a range. IPv6 addresses are banned by their /64 network.
- **Mutes** block chat and message commands. The player sees why and stays online.
- **Warnings** count for 30 days and lead to the next punishment.
- **Templates and escalation.** `/ban Steve #cheating` picks the reason and a length that grows with every repeat. 3 warnings can bring a mute.
- **Ban screen** with the reason, the time left, the punishment ID and your appeal link.
- **Full history** with clickable pages. Nothing is deleted.
- **Alt accounts** sharing an address, staff limits and exemptions.
- **Vanilla import** of `banned-players.json` and `banned-ips.json`.

## Commands

| Command | What it does |
| --- | --- |
| `/ban <player> [time] [reason]` | Ban a player |
| `/ipban <player\|ip> [time] [reason]` | Ban an address |
| `/mute <player> [time] [reason]` | Mute a player |
| `/warn <player> [reason]` | Warn a player |
| `/kick <player> [reason]` | Kick a player |
| `/unban`, `/unmute`, `/unwarn <player\|#id>` | Lift a punishment |
| `/history <player\|ip>` | Punishments of a player or an address |
| `/banlist` / `/mutelist` | Active bans and mutes |
| `/alts <player>` | Accounts sharing an address |
| `/pb import vanilla` | Import the server's ban lists |

Add `-s` to punish silently and `-f` for players who never joined. `/pb help` shows every command you may use. `/pb` is short for `/pumbobans`. Permissions are named `pumbobans:<name>` and default to operators.

## Installation

Drop the file into `plugins/` and start the server. The config is created in `plugins/data/pumbobans/`. To take over the server's existing bans, run `/pb import vanilla`. Works with Pumpkin 0.2.0 (Minecraft 26.3).

## Running a network?

PumboBans also runs on [PumboProx](https://github.com/PumboMC/PumboProx): one ban covers the whole network, or only the server you name.

---

PumboBans is in beta. Try it on a test server before you put players on it.
Source, documentation and issues: https://github.com/PumboMC/PumboBans (GPL-3.0)
