# Changelog

All notable changes to PumboBans, one section per release. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.4-beta] - 2026-11-11

### Fixed

- Pumpkin 0.2.2 crashing when a player types an argument of a PumboBans command (on 0.2.2 Tab no longer completes player names).

### Changed

- Release files are named after the Pumpkin releases they load on, for example `PumboBans-v0.1.4-beta-for-Pumpkin-0.2.1-and-0.2.2.wasm`; one file serves Pumpkin 0.2.1 and 0.2.2.

## [0.1.3-beta] - 2026-10-30

### Changed

- Builds for Pumpkin 0.2.1 (Minecraft 26.3).
- Pumpkin 0.2.0 has its own file, `PumboBans-Pumpkin-0.2.0-0.1.3-beta.wasm`.
- `/pumbobans version` and the load line name the Pumpkin release the file is for.
- Faster first start after a crash.
- The database cache takes at most 16 MiB instead of 1 GiB.

## [0.1.2-beta] - 2026-10-09

No changes in PumboBans; released with PumboPerms 0.1.2-beta.

## [0.1.1-beta] - 2026-10-09

### Fixed

- Help pages: the page number works again (`/pb help 2`, the ▶ arrow).

## [0.1.0-beta.1] - 2026-10-09

First public beta.

[0.1.4-beta]: https://github.com/PumboMC/PumboBans/compare/v0.1.3-beta...v0.1.4-beta
[0.1.3-beta]: https://github.com/PumboMC/PumboBans/compare/v0.1.2-beta...v0.1.3-beta
[0.1.2-beta]: https://github.com/PumboMC/PumboBans/compare/v0.1.1-beta...v0.1.2-beta
[0.1.1-beta]: https://github.com/PumboMC/PumboBans/compare/v0.1.0-beta.1...v0.1.1-beta
[0.1.0-beta.1]: https://github.com/PumboMC/PumboBans/releases/tag/v0.1.0-beta.1
