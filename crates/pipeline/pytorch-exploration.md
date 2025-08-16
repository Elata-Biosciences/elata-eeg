# Stateful DSP Stage: PyTorch → ONNX → onnx2c → **Generic Rust Plugin**

Minimal **“hello‑world”** example that *holds internal state* (a 1‑pole low‑pass filter) while travelling the entire tool‑chain:

> **PyTorch → ONNX → onnx2c‑generated C → Rust via a plugin ABI**

The key is to make the state an explicit tensor so every call receives the previous state and returns the updated state.

This README shows how to export the filter, compile it to pure C, build a **zero‑allocation shared object**, and load it from Rust *without any bespoke glue such as an ********`IirStage`******** wrapper*.

---

## Table of Contents

1. [Author the model in PyTorch & export to ONNX](#1-author-the-model-in-pytorch--export-to-onnx)
2. [Compile the ONNX file to C & build a shared library](#2-compile-the-onnx-file-to-c--build-a-shared-library)
3. [Describe the plugin with a YAML manifest](#3-describe-the-plugin-with-a-yaml-manifest)
4. [Load & run the plugin from generic Rust](#4-load--run-the-plugin-from-generic-rust)
5. [Pros, Trade‑offs, and Next Steps](#5-pros-trade-offs-and-next-steps)
6. [References](#references)

---

## 1  Author the model in PyTorch & export to ONNX

```python
# iir1_export.py
import torch
import torch.nn as nn

class IIR1(nn.Module):
    def __init__(self, a=0.1, b=0.9):
        super().__init__()
        self.register_buffer("a", torch.tensor([a], dtype=torch.float32))
        self.register_buffer("b", torch.tensor([b], dtype=torch.float32))

    def forward(self, x, y_prev):
        y = self.a * x + self.b * y_prev
        return y, y  # (output, next_state)

model = IIR1()
dummy_x     = torch.zeros(1, 1)
dummy_state = torch.zeros(1, 1)

torch.onnx.export(
    model,
    (dummy_x, dummy_state),
    "iir1.onnx",
    input_names=["x", "state_in"],
    output_names=["y", "state_out"],
    opset_version=17,
)
print("Exported iir1.onnx")
```

Run:

```bash
python iir1_export.py
```

---

## 2  Compile the ONNX file to C & build a shared library

```bash
onnx2c iir1.onnx -o iir1.c

gcc -O3 -ffast-math -fPIC -shared iir1.c -o liblp_filter.so
```

`onnx2c` emits a single allocator‑free C file exposing:

```c
void entry(const float *x,
           const float *state_in,
           float       *y,
           float       *state_out);
```

Building with `-shared -fPIC` turns it into a drop‑in plugin while keeping **zero dynamic memory**.

---

## 3  Describe the plugin with a YAML manifest

```yaml
# lp_filter.yml
name:    "lowpass_1st"
lib_path: "./liblp_filter.so"
entry:   "entry"          # symbol to call
inputs:
  - { name: "x",          elems: 1 }
  - { name: "state_in",   elems: 1 }
outputs:
  - { name: "y",          elems: 1 }
  - { name: "state_out",  elems: 1 }
frame: 1                   # samples per call
```

*Change ********`elems`******** or ********`frame`******** if your graph processes vectors instead of scalars.*

---

## 4  Load & run the plugin from generic Rust

`Cargo.toml` (only runtime deps shown):

```toml
[dependencies]
libloading = "0.8"
serde      = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
anyhow     = "1"
```

`src/plugin.rs` (condensed):

```rust
use libloading::{Library, Symbol};
use serde::Deserialize;

#[derive(Deserialize)]
struct TensorSpec { elems: usize }
#[derive(Deserialize)]
struct Manifest {
    lib_path: String,
    entry:    String,
    inputs:   Vec<TensorSpec>,
    outputs:  Vec<TensorSpec>,
}

type EntryFn = unsafe extern "C" fn(*const f32, *const f32, *mut f32, *mut f32);

pub struct Model {
    _lib:  Library,   // keeps the .so alive
    entry: EntryFn,
    in_buf:  Vec<f32>,
    st_in:   Vec<f32>,
    out_buf: Vec<f32>,
    st_out:  Vec<f32>,
}

impl Model {
    pub fn load_from_yaml(yaml: &str) -> anyhow::Result<Self> {
        let meta: Manifest = serde_yaml::from_str(yaml)?;
        let lib   = unsafe { Library::new(&meta.lib_path)? };
        let entry = unsafe { *lib.get::<Symbol<EntryFn>>(meta.entry.as_bytes())? };

        Ok(Self {
            _lib: lib,
            entry,
            in_buf:  vec![0.0; meta.inputs[0].elems],
            st_in:   vec![0.0; meta.inputs[1].elems],
            out_buf: vec![0.0; meta.outputs[0].elems],
            st_out:  vec![0.0; meta.outputs[1].elems],
        })
    }

    pub fn process_sample(&mut self, x: f32) -> f32 {
        self.in_buf[0] = x;
        unsafe {
            (self.entry)(
                self.in_buf.as_ptr(),
                self.st_in.as_ptr(),
                self.out_buf.as_mut_ptr(),
                self.st_out.as_mut_ptr(),
            );
        }
        std::mem::swap(&mut self.st_in, &mut self.st_out);
        self.out_buf[0]
    }
}
```

*No bespoke ********`IirStage`******** struct is needed—any future model just delivers a new ********`.so`******** + YAML.*

---

## 5  Pros, Trade‑offs, and Next Steps

| ✔️ Pros                                                               | ⚠️ Trade‑offs                                                  |
| --------------------------------------------------------------------- | -------------------------------------------------------------- |
| **One generic runner** covers every onnx2c plugin.                    | Shape/size mistakes become runtime errors—unit‑test manifests. |
| **Hot‑swap** models by loading new `.so` + YAML.                      | No cross‑unit LTO; minor perf hit on MCU‑class parts.          |
| **Zero‑alloc determinism** retained—the generated C owns all buffers. | ONNX op coverage still misses Hilbert, complex wavelets, etc.  |
| **Chain stages** by loading multiple manifests.                       | State tensor *must* be threaded through each graph.            |

**Next steps**

* Export multi‑channel or framed filters by bumping the tensor sizes.
* Chain FIR → STFT → dense layers inside one ONNX graph and still load via the same ABI.
* Add unit tests that compare Rust outputs against NumPy ground truth.

---

## 6  Static‑shape caveat: when do you have to re‑compile?

**onnx2c hard‑codes every tensor dimension into the generated C.** If you later change any of these:

| What you change                                   | Do you need to regenerate `model.c`?                                           |
| ------------------------------------------------- | ------------------------------------------------------------------------------ |
| **Batch / frame length** (e.g.                    |                                                                                |
| 32→64 samples)                                    | **Yes** – tensor sizes appear in array declarations.                           |
| **Filter order** (IIR1 → IIR2)                    | **Yes** – graph topology changes.                                              |
| **Weights only** (tweaking `a`, `b`, kernel taps) | **No** – you can edit the `.h` weight block or rebuild quickly; ABI unchanged. |
| **Data type** (f32 → q15)                         | **Yes** – regenerate or hand‑edit the emitted C.                               |

> **Tip:** if you foresee multiple frame sizes, export several ONNX graphs (32, 64, 128) and pick the right plugin at runtime via YAML.

---

## 7  Alternatives to onnx2c for MCU‑class inference

| Engine                                     | Footprint             | Determinism    | SIMD/GPU                  | Pros                                 | Cons                                      |
| ------------------------------------------ | --------------------- | -------------- | ------------------------- | ------------------------------------ | ----------------------------------------- |
| **CMSIS‑DSP/N N**                          | 10‑30 kB              | Cycle‑accurate | ARMv7E‑M/ MVE             | Best for hand‑rolled FIR/IIR; mature | No ONNX import; manual wiring             |
| **TensorFlow Lite Micro**                  | 20‑80 kB              | Deterministic  | Optional CMSIS‑NN kernels | Large op set; community              | Interpreter overhead; static arena needed |
| **TVM / microTVM**                         | 30‑60 kB code + model | Deterministic  | Autotuned SIMD            | Auto‑tunes kernels                   | More exotic toolchain                     |
| **μONNX**                                  | 15‑25 kB              | Deterministic  | Some SIMD                 | Direct ONNX parser                   | Slower than onnx2c on tight loops         |
| \*\*Hand‑coded Rust + \*\***`core::arch`** | Whatever you write    | Max            | Full control              | Peak speed for tiny kernels          | Highest dev cost                          |

If you already have **PyTorch‑trained models** and like a single‑file, zero‑alloc output, **onnx2c remains the most friction‑free path**.

---
## 8  Two different sensors pytorch
Thinking this thru. If two sensors are on differnt clock domains and drift relative to eachother.

```python
# imu2eeg_resampler.py
import torch
import torch.nn as nn
import torch.nn.functional as F

class Imu2EegResampler(nn.Module):
    """
    Upsamples a 6-axis IMU frame (1 kHz) to match a 4 kHz EEG grid.
    State = [offset, drift]  -- both in seconds.

    ─ offset : absolute time skew  (s)
    ─ drift  : fractional drift    (Δt / t, e.g. 200 ppm = 2e-4)
    """
    def __init__(self,
                 fs_src: int = 1000,
                 fs_tgt: int = 4000,
                 frame_src: int =  20):  # e.g. 20 ms → 20 imu samples, 80 eeg
        super().__init__()
        self.fs_src = fs_src
        self.fs_tgt = fs_tgt
        self.ratio  = fs_tgt // fs_src
        self.frame_src = frame_src          # N
        self.frame_tgt = frame_src * self.ratio  # M

        # constants as buffers so they survive ONNX export
        self.register_buffer("T_frame",
                             torch.tensor([frame_src / fs_src], dtype=torch.float32))

    def forward(self, imu_frame, state_in):
        """
        imu_frame : (N, 6)        -- N = frame_src
        state_in  : (2,)          -- [offset, drift]
        Returns:
            eeg_frame : (M, 6)    -- M = N * ratio
            state_out : (2,)
        """
        offset, drift = state_in  # each is scalar tensor
        N = imu_frame.shape[0]
        assert N == self.frame_src, "unexpected frame length"

        # ----- 1.  upsample IMU data to EEG rate (linear) -----
        imu_expand = imu_frame.T.unsqueeze(0).unsqueeze(0)  # (1,1,6,N)
        eeg_frame  = F.interpolate(
            imu_expand,
            size=self.frame_tgt,
            mode='linear',
            align_corners=True
        ).squeeze().T                                       # (M,6)

        # ----- 2.  update state (Brownian drift) -----
        # Here we assume drift is random-walk updated *outside* the graph
        # and passed back in; the graph only propagates offset.
        offset_next = offset + drift * self.T_frame  # shift by drift over frame
        state_out   = torch.stack([offset_next, drift])

        return eeg_frame, state_out


if __name__ == "__main__":
    # Dummy export
    model = Imu2EegResampler()
    imu_dummy = torch.zeros(model.frame_src, 6)
    state_dummy = torch.tensor([0.0, 2e-4])   # 200 ppm drift
    torch.onnx.export(
        model,
        (imu_dummy, state_dummy),
        "imu2eeg.onnx",
        input_names=["imu_frame", "state_in"],
        output_names=["eeg_frame", "state_out"],
        opset_version=17,
    )
    print("Exported imu2eeg.onnx")
```
---

## References

* [onnx2c – ONNX → pure C compiler](https://github.com/kraiskil/onnx2c)
* [ONNX issue #3190 – stateful models](https://github.com/onnx/onnx/issues/3190)
