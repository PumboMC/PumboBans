# Configuration

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
