# Installation

> [!TIP]
> Download the files from [Releases](https://github.com/PumboMC/PumboBans/releases/latest), or [build from source](#building).

**On PumboProx**

1. Put `PumboBans-v<version>-for-PumboProx.wasm` into the proxy's `plugins/` folder.
2. Start the proxy. The first start creates `plugins/pumbo-bans/config.yml` with comments.
3. Recommended in `pumboprox.yml`, so nobody joins while PumboBans is not running:

   ```yaml
   plugins:
     required-plugins: [pumbo-bans]
   ```

4. Give your staff `pumbo.bans.*` in the proxy's `permissions.yml`.

**On Pumpkin**

1. Put the file that matches your Pumpkin version into the server's `plugins/` folder.
2. **Pumpkin's plugin sandbox.** PumboBans needs `fs.read.data` and `fs.write.data`: it reads and writes only its own data folder and has no network access. Pumpkin asks about them in the console on the first start. On a hosting panel the server runs without a terminal, so Pumpkin cannot ask: it refuses them, keeps the refusal in `plugins/permission_cache.json`, and the plugin does not load, even after a restart. Approve them in `pumpkin.toml`:

   ```toml
   [plugins.overrides.pumbobans]
   allowed_permissions = ["fs.read.data", "fs.write.data"]
   ```

   This also overrides a refusal in the cache. Write the names with dots: the colon form from Pumpkin's examples never matches.
3. Start the server. The first start creates `plugins/data/pumbobans/config.yml` and the language files.
4. Operators get the moderation commands by default. To take over the server's existing bans, run `/pumbobans import vanilla`.

> [!WARNING]
> On Pumpkin 0.2.1, 0.2.0 and 0.1.0-dev typing a plugin command in the server console can crash the server. This is a bug in Pumpkin. Until it is fixed, set `use_tty = false` in `[commands]` of `pumpkin.toml`. The console still takes commands, without Tab completion.

## Two builds

The punishment rules live in one shared core. You pick the build that fits your setup.

| Build | File | Where it goes | What is different |
| --- | --- | --- | --- |
| 🌐 **PumboProx** (whole network) | `PumboBans-v<version>-for-PumboProx.wasm` | `plugins/` of the proxy | One database for every server. Banned players are refused at the proxy, and a ban disconnects a player from any server. |
| 🎃 **Pumpkin** (one server) | `PumboBans-v<version>-for-Pumpkin-….wasm`, one per [Pumpkin version](#pumpkin-versions) | `plugins/` of the server | Bans, mutes and history for that server. `/ban`, `/kick` and `/banlist` replace the server's own commands. |

## Pumpkin versions

Each Pumpkin release loads only its own file:

- Pumpkin 0.2.1+26.3-26.51 and 0.2.2+26.3-26.60: `PumboBans-v<version>-for-Pumpkin-0.2.1-and-0.2.2.wasm`, or the file on the Pumpkin market
- Pumpkin 0.2.0+26.3-26.51: `PumboBans-v<version>-for-Pumpkin-0.2.0.wasm`
- Pumpkin 0.1.0-dev+26.2-26.45: `PumboBans-v<version>-for-Pumpkin-0.1.0-dev.wasm`

The files are in the [latest release](https://github.com/PumboMC/PumboBans/releases/latest).

On Pumpkin 0.2.2 the commands have no Tab completion: Pumpkin 0.2.2 crashes when a player asks a plugin for completions. It comes back with a PumboBans update once Pumpkin fixes this.

## Building

You need Rust stable with the WebAssembly target:

```sh
rustup target add wasm32-wasip2
```

Cargo fetches the shared Pumbo libraries (`pumbo-common`, `pumbo-sdk`) from the [PumboProx](https://github.com/PumboMC/PumboProx) repository on the first build.

Pumpkin build, every version (`PumboBans-26.3.wasm`, `PumboBans-0.2.0.wasm` and `PumboBans-26.2.wasm` in `plugins/pumbo-bans-pumpkin/dist/`):

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
