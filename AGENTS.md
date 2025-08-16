# Repository Guidelines

## Project Structure & Module Organization
- Rust workspace: `crates/` contains core crates (`daemon`, `boards`, `sensors`, `pipeline`, `eeg_types`). The daemon produces binaries (`eeg_daemon`, `ws_test`).
- Plugins: `plugins/` contains optional DSP/data sinks (e.g., `basic_voltage_filter`, `brain_waves_fft`, `csv_recorder`).
- Kiosk UI: `kiosk/` is a Next.js app (`src/` with `app/`, `components/`, etc.).
- Pipelines & config: `pipelines/*.json`, `kiosk/default.yaml` hold runtime configuration.
- Ops scripts: `scripts/` (`install.sh`, `rebuild.sh`, `start.sh`, `stop.sh`, `uninstall.sh`).

## Build, Test, and Development Commands
- Build all Rust crates: `cargo build` (from repo root).
- Run daemon (dev): `RUST_BACKTRACE=1 RUST_LOG=debug cargo run --bin eeg_daemon`.
- Run kiosk (dev): `cd kiosk && npm install && npm run dev` (served via local proxy in `server.js`).
- Rebuild production (daemon + kiosk): `bash scripts/rebuild.sh`.
- Manage kiosk mode: `bash scripts/start.sh` | `bash scripts/stop.sh`.
- Full install on Pi: `bash scripts/install.sh` (enables services, kiosk, deps).

## Coding Style & Naming Conventions
- Rust: format with `cargo fmt`; lint with `cargo clippy -- -D warnings`. Use snake_case for modules/functions, UpperCamelCase for types, SCREAMING_SNAKE_CASE for consts.
- TypeScript/Next.js: run `npm run lint`. Prefer 2‑space indent, PascalCase React components in `kiosk/src/components`, and absolute imports via `@/` path alias.
- File naming: crates/plugins use snake_case directory and package names.

## Testing Guidelines
- Rust unit/integration tests: `cargo test` (workspace) or per crate (e.g., `cd crates/daemon && cargo test`). Place unit tests inline under `#[cfg(test)]` or as `tests/` with `*_test.rs` files.
- Frontend: no formal test setup; keep UI logic minimal and covered by lint. If adding tests, prefer Playwright or Vitest and wire scripts under `kiosk/package.json`.

## Commit & Pull Request Guidelines
- Commits: imperative mood, concise subject (≤72 chars). Optional scope prefix, e.g., `daemon: init ADS1299 setup` or `kiosk: add FFT plot`.
- PRs: include summary, rationale, screenshots/CLI output if UI/CLI changes, and link issues. Ensure `cargo fmt`, `cargo clippy`, `cargo test`, and `npm run lint` pass.

## Security & Configuration Tips
- Do not commit secrets; configs in `pipelines/` and `kiosk/default.yaml` should be sample‑safe.
- Services: `eeg_daemon` and kiosk run as systemd units on install; prefer least‑privilege and verify before enabling on production hardware.
