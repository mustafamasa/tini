# Contributing to Tını

Thanks for helping. This file explains how the project is laid out and the conventions that keep it easy to maintain.

## Ground rules

- **Code is English, UI is Turkish.** Identifiers, file names, JSON keys, database columns, log messages and comments
  are English. Text shown to users (labels, messages, issue descriptions) is Turkish.
- **One target:** macOS on Apple Silicon. Do not add platform branches without a plan to test them.
- **Measure audio changes.** Anything that touches generation, post-processing or the engine must be compared on the
  same voice, text and seed. Report at least the 7–9 kHz band energy under vowels (relative to 1–4 kHz) and the
  click count.
- Keep pull requests focused. Run the checks below before opening one.
- **UI changes** keep the design rules: one warm neutral ramp, ink for primary actions, coral only for sound (playback, the spoken word, generation progress), sentence-case labels,
  no trailing periods on short strings and no decorative motion.

## Layout and boundaries

| Layer | Path | Owns |
|---|---|---|
| UI | `app/src` | Pages, components, zustand store (`store.ts`), typed command wrappers (`api.ts`) |
| Shell | `app/src-tauri/src/lib.rs` | Thin Tauri commands that forward to `tini_core::studio::Studio`; event bridge `tini-event` |
| Core | `crates/tini-core/src` | `studio` (entry point), `queue`, `jobs`, `store` (SQLite), `voices`, `engine` (sidecar client), `engine_env` (uv setup), `model` (weights download), `text`/`text_fix` (Turkish text), `word_timing`, `subtitles`, `transcribe`, `tone`, `audio`, `export`, `estimate` |
| Engine | `engine/voxcpm/tini_engine.py` | Long-lived VoxCPM process; JSON-lines protocol documented at the top of the file |

Contracts that must stay in sync when you change them:

- **Tauri commands and serialized types.** Rust structs (serde) ⇄ `app/src/api.ts` interfaces.
- **Sidecar protocol.** `crates/tini-core/src/engine.rs` ⇄ `engine/voxcpm/tini_engine.py`.
- **On-disk formats** (`voice.json`, `summary.json`, `settings.json`, SQLite schema). Changing one breaks existing
  libraries: keep old fields readable or ship a migration with the change.

## Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir app install && pnpm --dir app typecheck && pnpm --dir app build
```

`tini-cli` runs every job type headlessly against a data dir. Point it at a scratch directory with
`TINI_DATA_DIR=/tmp/tini-dev` so you never touch your real library.

`tini-cli serve` runs the local HTTP API (`crates/tini-core/src/api.rs`) in the foreground with the data dir's
settings port and token, and prints both; open `http://127.0.0.1:<port>/docs` for the endpoint list. The spec is the
hand-written `crates/tini-core/src/api_spec.json`: when you add or change a route, update it too (a unit test checks
that every route is in the spec). Example:

```bash
TINI_DATA_DIR=/tmp/tini-dev cargo run -p tini-core --bin tini-cli -- serve
```

## Python environment

Pinned dependencies live in `engine/voxcpm/requirements.txt`. Changing that file makes existing installations report
the engine as "outdated" and ask for a reinstall, so bump versions deliberately and test a fresh install
(`tini-cli install-env`).

## Releasing

Bump the version in `app/src-tauri/tauri.conf.json`, `app/package.json` and the Cargo manifests, then push a tag:

```bash
git tag v0.2.0
git push origin v0.2.0
```

The `release` workflow builds the app and publishes `Tini_<version>_aarch64.dmg` on the Releases page.

## License

By contributing you agree that your contributions are licensed under Apache-2.0.
