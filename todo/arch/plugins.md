**Here’s a pragmatic way to bolt a plug-in system onto the current architecture, from build to GUI enablement, and where Python/other languages might slot in.**
(I’m *not* editing the doc yet—just outlining options so we can pick what to formalise.)

---

## 1 · Packaging & Build Flow

| Step                    | What happens                                                                                                                                                                   | Typical command                                              |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------ |
| **1.1 Scaffold**        | `cargo new --lib my_fft_stage --template stage_plugin` ─ template already pulls in `plugin_api` and exposes `simple_stage!`.                                                   | `cargo generate` or `cargo xtask new-plugin`                 |
| **1.2 Build for Pi 5**  | Cross-compile to a `.so`/`.dylib` that exports one C-ABI symbol: `register_plugin()`. Inside it you push your `StageFactory` into the global registry.                         | `cargo build --release --target aarch64-unknown-linux-gnu`   |
| **1.3 Ship + manifest** | Emit three artefacts into a single folder: ① `libmy_fft_stage.so`, ② `my_fft_stage.yaml` (defaults / GUI hints), ③ `manifest.toml` (name, version, ABI = 1).                   | Installer script or `cargo xtask install-plugin --path .`    |
| **1.4 Deploy**          | Copy folder into `/opt/elata/plugins/` (or `~/.elata/plugins/`).                                                                                                               | Could just be `git clone && make install`, no root required. |
| **1.5 Daemon boot**     | At startup the daemon **scans** that directory, `libloading::Library::new()`, calls `register_plugin`, verifies manifest version. Failures are logged but don’t kill the core. | No YAML change needed.                                       |

*Why a folder-per-plugin?* Lets you include docs, icons, extra shaders, etc., and keeps versions side-by-side.

---

## 2 · How the GUI “sees” the plug-in

1. **/stages endpoint** – Daemon now exposes `GET /stages` → JSON array of `StageDesc` + schema + default params collected from all factories.
2. **Enable / insert** – GUI presents those as a palette. Clicking *Add FFT Stage* sends a minimal **GraphPatch**:

   ```json
   { "action":"insert", "after":"notch", "stage":{"type":"MyFft", "size":1024} }
   ```

   Backend builds the patch, hot-links the new stage, returns success.
3. **Preset YAML** – If the plugin folder contained a `my_fft_stage.yaml`, the GUI can show “Load Example Pipeline” that merges that snippet into the current graph—useful for brand-new stages.

---

## 3 · Install Options

* **Git clone + build locally** – simplest; users just run one script that builds the `.so`.
* **Binary release** – You can pre-build aarch64 binaries, zip with manifest, and provide a one-liner:

  ```bash
  elata-plugin-install https://…/my_fft_stage-v0.2.zip
  ```

  The helper unpacks to the plugins folder.
* **Crates.io** – Nice but a bit heavier (requires `cargo install --path …` plus an install hook that copies the `.so`).

---

## 4 · Non-Rust Stages?

| Route                       | Status                                       | Latency hit                             | Notes                                                                                          |
| --------------------------- | -------------------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------------------- |
| **PyO3** (Rust ↔ CPython)   | Works today                                  | +40–100 µs call overhead                | Fine for ≥ 25 ms windows (e.g., 256-pt FFT). Template could generate the `pyo3::prelude` glue. |
| **Wasm guest (`wasmtime`)** | Clean sandbox, any language → Wasm           | +10-30 µs JIT, zero copy via shared mem | Good blend of safety & speed; aligns with future “stage marketplace”.                          |
| **gRPC/micro-service**      | Runs completely external binary (Python, JS) | network or loopback (\~200 µs)          | Only for slow, batchy analytics; not for per-packet DSP.                                       |

If we need Python, I’d start with **PyO3 inside a Rust plugin**: the `.so` still exports a `register_plugin()`, but the `StageImpl` body calls into Python for the heavy math. That keeps the install story identical.

---

## 5 · Why a manifest matters

```toml
# /opt/elata/plugins/my_fft_stage/manifest.toml
name      = "my_fft_stage"
version   = "0.2.1"
abi       = 1
requires  = ["plugin_api >= 0.3"]
stages    = ["MyFft", "Spectrogram"]
yaml_file = "my_fft_stage.yaml"
```

*ABI bump* → daemon refuses load, logs “incompatible ABI 2 (core=1)”.

---

### Recommendations

1. **Folder-per-plugin with manifest + optional YAML preset** keeps distribution simple.
2. Expose **`GET /stages`** so GUI stays decoupled.
3. Start with **Rust dynamic libs**; add PyO3/Wasm when you actually need cross-language code.

If that flow looks reasonable, we can formalise a short “Plugin Packaging & Discovery” sub-section in the architecture doc next.

**Short answer:** your core data-path, scheduler, and stage API already cover ≈ 90 % of what plugins need.  Adding a plug-in system later is **mostly additive**—you won’t have to rethink the allocator, the graph, or the thread model.  The only architectural tweaks worth pencilling in now are lightweight seams for **discovery, versioning, and runtime control** so you don’t end up bolting them on awkwardly.

---

## 1 · What can stay exactly as-is

| Layer                                     | Why it’s unaffected by plugins                                                       |
| ----------------------------------------- | ------------------------------------------------------------------------------------ |
| **`Arc<RtPacket>` buffers**               | Plugins just receive/emit the same packets. No ABI change.                           |
| **Thread-per-group scheduler**            | Loader drops a new stage into an existing group; execution model identical.          |
| **Stage trait (`process → StageResult`)** | Works unchanged for dynamically loaded code as long as the ABI matches `plugin_api`. |
| **YAML topology**                         | Graph file still lists stages in order; only the *set of stage types* grows.         |

---

## 2 · Small seams to add now (so you never refactor later)

| Seam                               | One-liner to add to doc                                                                           | What it buys you                                                                    |
| ---------------------------------- | ------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| **ABI constant**                   | `pub const PLUGIN_ABI: u32 = 1;` in `plugin_api`                                                  | Future core bumps can refuse stale `.so`s gracefully.                               |
| **`StageFactory` registry**        | “At daemon boot each compiled-in or dynamic library calls `register_plugin(&mut StageRegistry)`.” | Unified path for built-ins *and* plugins.                                           |
| **Manifest format**                | `manifest.toml { name, version, abi, stages[] }`                                                  | Lets the loader sanity-check *before* `dlopen`, keeps GUI metadata out of the code. |
| **`StageId` new-type**             | `#[derive(Copy,Clone,Hash,Debug)] struct StageId(u64);`                                           | Stable key for REST/GUI calls even if names collide after hot-insert.               |
| **Optional `GraphPatch` endpoint** | skeleton JSON schema: `{action, target, stage}`                                                   | Gives you a place to send “insert FFT after X” without touching YAML internals.     |
| **Metrics/telemetry channel**      | reserve an enum variant (`RuntimeMsg::Telemetry`)                                                 | Plugins emit counters the same way built-ins will.                                  |

These are thin shims; implementing them later is easy, but **documenting them now keeps the interfaces stable**.

---

## 3 · What *might* change but can wait

* **Plugin sandbox choice** (raw `libloading`, Wasmtime, PyO3): you can prototype with `libloading` today and wrap or replace it later—no graph changes required.
* **Cross-language stages**: if/when you expose Python, it lives behind a Rust `StageImpl`; the outer trait still matches, so the architecture doc doesn’t need new primitives for that.

---

## 4 · Impact on existing sections of the Blueprint

| Section                    | Add a bullet that…                                                      |
| -------------------------- | ----------------------------------------------------------------------- |
| **“Plugin-friendly APIs”** | notes the `StageFactory` register-on-load pattern and ABI constant.     |
| **“Runtime Control API”**  | reserves `/stages` (list available) and `/graph/patch` (apply changes). |
| **“Key Types”**            | lists `StageId` and mentions manifest ABI.                              |

That’s it—no diagrams or allocator specs need to change.

---

### Bottom line

*Architecturally, you’re safe:* plug-ins slot in without disturbing the hot loop.
Add a **factory registry, manifest, and ABI constant** now (tiny edits), and the rest can be layered on whenever you decide to ship third-party stages.
