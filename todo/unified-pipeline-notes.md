# Should we remove yield_threshold
# Why “just a voltage converter” sprawls to 400-ish lines

| Slice of file                                                                  | Rough LOC | Purpose                                                              |
| ------------------------------------------------------------------------------ | --------- | -------------------------------------------------------------------- |
| **Core math** (`raw_to_voltage`, scaling pre-calc)                             | **≈ 10**  | The actual conversion                                                |
| Stage plumbing (trait impls, lazy I/O, param hot-reload, back-pressure, yield) | ≈ 120     | Glue every stage needs right now                                     |
| Factory + type-erased wrappers (`DataPlaneStageFactory`, `Erased*`)            | ≈ 60      | Let the runtime create the stage from JSON and store it behind `dyn` |
| `inventory::submit!` registration                                              | 5         | Auto-register with the graph                                         |
| Serde + schemars param struct                                                  | 25        | Runtime validation & schema export                                   |
| **Unit tests & mocks**                                                         | **≈ 180** | Three async tests, one schema test, plus `MockInput/Output` helpers  |
| Comments & doc-strings                                                         | ≈ 30      | Explanatory text                                                     |

So only \~10 lines are physics; the rest is *framework boilerplate* plus a healthy test suite. That’s why it feels bloated.

---

## Is that bad? Depends on your goals

| Goal                                        | Verdict                                                                 |
| ------------------------------------------- | ----------------------------------------------------------------------- |
| **High perf, hot path**                     | Boilerplate is mostly compile-time; in release build it optimises away. |
| **Code clarity / contributor friendliness** | 400 LOC for “x ⇢ y” is noisy. Worth trimming or hiding.                 |
| **Re-use across stages**                    | The repetitive chunks beg for macros or helper traits.                  |

---

## Tactics to shrink & DRY things up

1. **Macro-generate the common scaffolding**

   ```rust
   stage_def! {
       name = ToVoltageStage,
       inputs = RawEegPacket,
       outputs = VoltageEegPacket,
       params = {
           vref: f32 = 4.5,
           adc_bits: u8 = 24,
           yield_threshold: u32 = 64,
       },
       logic = |self, pkt, out| {
           let scale = self.scale.load(Ordering::Acquire);
           pkt.samples.iter()
               .map(|&r| r as f32 * scale)
               .collect_into(&mut out.samples);
       }
   }
   ```

   A single `stage_def!` could expand into the trait impls, factory, schema export, and inventory registration.

2. **Move unit tests into `tests/` directory**

   By convention Rust integration tests live in `tests/*.rs`, keeping library files lean. You can still mock the I/O helpers there.

3. **Offer a `BaseStage<TIn,TOut>` helper**

   Encapsulate:

   * cached `input_rx` / `output_tx`
   * the `process_packets` drain loop
   * yield logic

   Concrete stages override a single `fn transform(&self, in:&TIn, out:&mut TOut)`.

4. **Pre-compute the scale factor**

   Instead of storing `vref` & `adc_bits` separately and re-doing the divide every sample:

   ```rust
   fn set_vref_bits(&self, vref:f32, bits:u8) {
       let scale = vref / ((1 << (bits-1)) - 1) as f32;
       self.scale.store(scale.to_bits(), Ordering::Release);
   }
   ```

   Then `raw * scale` in the tight loop—no division.

5. **Consider dropping atomic for `vref` if hot-reload isn’t critical**

   A simple `RwLock<f32>` or even a plain field updated only on control messages is cheaper to read in the loop.

6. **Hide erased-trait dance behind the macro**

   Most contributors never need to spell out `DataPlaneStageErased`/`ErasedStageContext`.

---

### Quick metric after refactor

* Macro invocation + domain logic + a few comments: **\~40 LOC**
* Integration tests in `tests/to_voltage.rs`: as big as you like, but out of the main file.

That keeps the pipeline ergonomic without losing safety or test coverage.

---

## Bottom line

The file “looks right” in the sense that every section has a reason to exist, but you can:

* **Consolidate boilerplate** with a macro or base-class pattern.
* **Relocate tests** to trim the production file.
* **Micro-optimise** the math (pre-computed scale) and data handling (reuse buffers).

Do that and the voltage converter will shrink to something closer to what the domain complexity actually demands.

..........
Not crazy — just a trade-off.
Below is a quick “sanity grid” so you can see where the macro / base-stage idea shines and where it bites.

| Dimension                          | Status with **plain hand-written stages** | Status with **`stage_def!{…}` macro (or a BaseStage helper)**   |
| ---------------------------------- | ----------------------------------------- | --------------------------------------------------------------- |
| **Boilerplate**                    | High (5-7 impl blocks + mocks per file)   | Near-zero per new stage                                         |
| **Contributor ramp-up**            | Easy to read, copy/paste; no macro magic  | One-time “how does this macro expand?” hurdle                   |
| **Debugging / IDE jump-to-def**    | Straightforward                           | Harder; need `cargo expand` or RLS to see generated code        |
| **Compile-time errors**            | Point at the offending line in the stage  | Often point inside macro expansion → noisier                    |
| **Flexibility for oddball stages** | Total (you write whatever you want)       | You have to extend the macro every time you hit a new edge-case |
| **Binary size / perf**             | Identical after inlining/O2               | Identical (macro disappears at compile time)                    |
| **Long-term maintenance**          | N copies of similar code to patch         | One macro impl to patch, N copies just re-compile               |

### When the macro pattern is worth it

* **Many small stages** that all look the same apart from the math (exactly your case: gain, to-voltage, bias-remove, DC-offset, etc.).
* **External contributors**: they write 20 lines of math, the pipeline “just works”.
* **Hot-reload isn’t a goal** (because the macro approach assumes in-workspace compilation anyway).

### When to skip the macro

* You foresee **exotic stages** (multi-input joins, GPU kernels, variable output counts) that won’t fit the template.
* Team members dislike macro indirection and prefer explicit code, even if verbose.
* You plan to move back to a **runtime-loaded `.so` ABI** later; the macro would have to change again.

### Practical middle-ground

1. **Start with a small helper trait (`BaseStage`)** that hides the drain-loop / cached I/O handles. No macro yet.
   *New stage skeleton*: 25-30 LOC.
2. If you notice you’re still copy/pasting the same pattern after the third or fourth stage, graduate to a real macro or a code-gen build-script.
3. Keep **unit tests in `tests/`** so production files stay lean no matter what.

---

So, no, it isn’t crazy; it’s a perfectly common Rust move for DSL-ish frameworks (think `actix_web::get!`, `serde::Serialize`, `tokio::main`). Just weigh the discoverability cost against the productivity boost, and introduce it only when you’re confident most stages are “cookie-cutter”.

# Yaml style?  .. Medium Priority
2.1 Strip it to the essentials, push the rest behind presets

version: 1
run_id: "exp-2025-07-09-ads1299-beta1"
seed: 42         # reproducibility

pipeline:
  - id: ads1299_in
    type: Ads1299Source          # driver-backed stage
    params:
      batch_size: 16
      gain: 24
  - id: notch60
    type: IIRNotchFilter
    preset: "60hz_default"       # loads coeffs from built-ins
  - id: ds_250
    type: DownSampler
    params: { factor: 16 }

connections:
  - [ads1299_in, notch60]
  - [notch60, ds_250]

resources:
  mempools:
    default_16x32: { elements: 32 }   # 32 packets of 16×32-bit samples

    Presets hide gnarly arrays (filter taps, covariance matrices) but stay override-able.

    Explicit run_id + seed give you perfect reproducibility without digging into filenames.

    YAML keeps it copy-pasteable in notebooks; the runtime can accept JSON too.

2.2 Treat the config as a first-class artifact

    Hash the entire file and store it next to the result files (<hash>.yaml, <hash>.h5).

    Add a CLI helper: pipeline run -c my.yaml --autoname that copies the exact config into the output folder.

    Expose a Python helper that can load/modify the YAML, then subprocess.run the Rust binary — gives data-sci users notebook control without Rust.


# More yaml stuff .. Medium Priority
6 Preset hashes & hobbyist usability

    Stick with YAML for readability; the runtime will accept JSON for programmatic callers.

    When a preset is loaded, hash its entire YAML block with SHA-256, write it to run-folder/preset-<hash>.yaml, and record the hash in the main results header.

    Omit RNG seeds unless you introduce stochastic ops (you’re deterministic today).

Hobbyists get:

    human-friendly YAML they can edit in VS Code,

    an “import preset” button in a web UI,

    automatic version stamping for reproducibility without thinking about hashes.


# Latency Calc .. Low Priority
Each stage knows it's latency? SPI data rate,etc = 20us
print(pipeline.latency())

# Performance Improvements .. Low Priority
1 Can we hit ~100 ns per stage?

    Budget check – 100 ns on a Pi 5 @ 2.4 GHz ≈ 240 CPU cycles. You only get that if each hop is:

        1 pointer read from an L1-resident ring-buffer,

        zero heap work,

        zero syscalls or await points,

        no trait-object indirection.

    Practical tweaks that usually drop a simple stage from ~500 ns → 120-180 ns:

        Busy-spin queue instead of Await: replace recv().await with a bounded SPSC ring that does a spin_loop_hint() for ≤ N cycles, then yields. (Crossbeam’s SegQueue + SpinWait helper is perfect.)

        Static dispatch: if you convert the hot path to a generic function (fn(process_fn: impl Fn(&mut [f32]))) you avoid the v-table lookup per call.

        #[inline(always)] on the tiny packet getters and memory_pool::release.

        No logging inside the loop; put tracing only on slow paths.

Hitting a hard 100 ns for every stage is unrealistic once you include filtering math, but pass-through / down-sample stages can reach it.

7 HashMap wiring – any worries?
Fine for config-driven graphs. If you want max speed:
    Replace HashMap<&'static str, _> with an index array generated at graph-build time.
    0 alloc at runtime; still zero-copy.
But premature unless profiling shows look-ups >1 % CPU.

# Removing Boilerplate .. Medium Priority
```rust
macro_rules! ctrl_loop {
    ($ctx:ident, $self:ident, { $($key:pat => $body:block),* }) => {{
        while let Ok(msg) = $ctx.control_rx.try_recv() {
            match msg { $($key => $body,)* _ => {} }
        }
    }}
}
```
Or write a derive-macro #[derive(Stage)] that injects the control-handling skeleton.
Next win: auto-gen the stage factory + schema from a #[stage_param] derive to erase 20 LOC of JSON glue each time.
macro_rules! ctrl_loop { /* ... */ }

rust


This is an excellent idea for improving the ergonomics of stage development.

Pros:

Reduces Boilerplate: It abstracts the repetitive try_recv loop and match statement, making stage code cleaner and more focused on its unique logic.
Enforces Consistency: Ensures all stages handle control messages in a uniform way.
Cons:

"Magic": Macros can sometimes hide complexity, making it slightly harder for new developers to understand the underlying control flow without finding the macro definition.
The suggestion to evolve this into a #[derive(Stage)] macro is the logical next step and a common pattern in mature Rust frameworks. It could auto-generate the entire run method, the handle_ctrl block, and the parameter schema boilerplate from simple #[stage_param] attributes on a struct's fields.

Recommendation: This is a powerful pattern for reducing code duplication. I would recommend creating a new task to implement ctrl_loop! and explore a derive macro once the core pipeline is stable. It's a great "next win" for the project.

#?
I also want to acknowledge the excellent, forward-looking suggestions in the feedback:

#[derive(Stage)] Macro: A proc-macro to generate stage boilerplate is a fantastic idea for improving developer velocity and reducing errors.
Schema-driven Factories: Using schemars to derive the parameter_schema from the configuration struct would make stage creation more robust and less error-prone.
These are powerful architectural enhancements. I propose we treat them as separate, high-value tasks to be tackled after the FilterStage is hardened, as they will benefit the entire pipeline ecosystem.

# Performance: Eliminate Hot-Path Lookups .. High Priority
The feedback correctly identifies that looking up the input channel via ctx.inputs["in"] on every packet is an unnecessary overhead. I will eliminate this by caching the I/O handles within the stage itself.

What: I will add two fields, input_rx and output_tx, to the FilterStage struct to hold direct handles to the stage's input and output channels.
Why: This avoids the HashMap lookup (~35ns) in the process_packet hot loop, ensuring the fastest possible path for data.
How:
Modify the FilterStage struct to store Options for the input and output channels.
The process_packet method will, on its first invocation, retrieve the channels from the StageContext, move them into the struct's fields for future use, and then proceed. All subsequent calls will use the cached handles directly.
// Proposed change to FilterStage struct
pub struct FilterStage {
    coeffs: Vec<f32>,
    enabled: AtomicBool,
    // Cached handles to avoid HashMap lookups in the hot path
    input_rx: Option<Box<dyn Input<VoltageEegPacket>>>,
    output_tx: Option<Box<dyn Output<VoltageEegPacket>>>,
}

rust

ChatGPT said:

Totally reasonable—not “too much” complexity
Aspect	Assessment
Added code	~12 – 15 LOC (two Option<…> fields + a one-time “init‐on-first-use” branch). That’s minor compared with the ~150 LOC you already have.
Cognitive load	Contributors only need to know why the input_rx/output_tx exist (tiny doc comment). They never touch them again—the pattern is copy-pasteable.
Performance payoff	Saves a 35 ns hash-lookup per packet per stage. At 4 kSPS × 32 ch (125 k pkts / s) that’s ≈ 4.4 ms CPU per second. On a Pi 5 that’s ~4 % of one core—worth it in a tight real-time loop.
Alternatives	Builder pre-binding (StageContext gives you typed indices) is cleaner long-term but requires broader refactor. The lazy-cache approach is a quick win without touching the graph builder.
Minimal implementation sketch

pub struct FilterStage {
    coeffs: Vec<f32>,
    enabled: AtomicBool,
    input_rx:  Option<Box<dyn Input<VoltageEegPacket>>>,
    output_tx: Option<Box<dyn Output<VoltageEegPacket>>>,
}

impl FilterStage {
    #[inline(always)]
    fn io<'a>(
        &'a mut self,
        ctx: &'a mut StageContext,
    ) -> (&'a mut dyn Input<VoltageEegPacket>, &'a mut dyn Output<VoltageEegPacket>) {
        if self.input_rx.is_none() {
            self.input_rx  = Some(ctx.inputs.remove("in")
                                          .expect("input 'in' missing"));
            self.output_tx = Some(ctx.outputs.remove("out")
                                          .expect("output 'out' missing"));
        }
        ( self.input_rx.as_mut().unwrap().as_mut(),
          self.output_tx.as_mut().unwrap().as_mut() )
    }
}

    Run-time cost of the first call: one HashMap removal each.

    Subsequent calls: just two pointer derefs.

For open-source contributors

    Clear comment near the fields (“cached once for perf”) is enough.

    Pattern can be reused in other stages; copy-paste reduces boilerplate.

    If a contributor doesn’t care about the last 5 %, they can still write a simpler stage—nothing forces them to cache.

Bottom line: it’s a pragmatic micro-optimization that costs almost nothing in complexity but buys measurable CPU time on resource-constrained hardware. Go for it.