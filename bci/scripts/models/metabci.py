#!/usr/bin/env python3
"""
metaBCI: minimal train→infer BCI that reads EEG over a websocket and outputs UP/DOWN

Now with **DataHub integration** (your binary WS @ :9000/ws/data):
- Modes:
  1) JSON stream (original): `train` / `infer`
  2) DataHub stream (your code): `train-datahub` / `infer-datahub`

DataHub assumptions (from your snippet):
  - Connect to ws://<host>:9000/ws/data
  - Send JSON subscribes: {"type":"subscribe","topic":<str>,"epoch":<int>}
  - Receive `meta_update` text frames with per-topic `meta` (ideally has fs, ch_names)
  - Receive binary frames with a 4-byte big-endian header length, followed by a JSON header, then raw samples.
  - `ws_data_to_np_array` yields an array shaped (batch_size, num_channels) of <i32 or <f32.

We *transpose* to (n_channels, n_samples) internally.

Outputs (infer mode) are JSON messages broadcast to /output websocket:
  {
    "t": <unix seconds, decision time>,
    "y": "up" | "down",
    "p": {"up": <prob>, "down": <prob>},
    "latency_ms": <processing latency>
  }

Dependencies:
  python -m pip install websockets numpy scipy scikit-learn joblib

Examples:
  Training (DataHub, 10 min):
    python metabci.py train-datahub --host raspberrypi.local --topic eeg_voltage --epoch 1

  Inference (DataHub → decisions on ws://127.0.0.1:8766/output):
    python metabci.py infer-datahub --host raspberrypi.local --topic eeg_voltage --epoch 1

Notes:
  - If your meta lacks fs/ch_names, pass --fs and --channels CSV.
  - Feature extraction uses Welch bandpower in bands: theta, alpha(mu), beta, low gamma.
"""
from __future__ import annotations

import argparse
import asyncio
import contextlib
import json
import math
import pathlib
import struct
import time
from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Dict, Iterable, List, Optional, Tuple, Union

import numpy as np
from joblib import dump, load
from scipy.signal import butter, iirnotch, filtfilt, sosfiltfilt, welch
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import accuracy_score
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import StandardScaler

try:
    import websockets
except ImportError as e:
    raise SystemExit("Please `pip install websockets`.")

# ----------------------------- Config -----------------------------
DEFAULT_CHANNELS_HINT = ["ch1", "ch2", "ch3", "ch4", "ch5", "ch6", "ch7", "ch8"]  # prefer sensorimotor
BANDS = [(4, 7), (8, 12), (13, 30), (30, 45)]  # theta, mu(alpha), beta, low-gamma
WINDOW_SECONDS = 1.25
HOP_SECONDS = 0.25
TRAIN_TOTAL_MIN = 5
BLOCK_SECONDS = 5.0
FILTER_MARGIN_S = 1.0
NOTCH_Q = 40.0

@dataclass
class StreamState:
    fs: float
    ch_names: List[str]
    ring: np.ndarray  # [n_channels, ring_len]
    write_pos: int

# ----------------------------- Helpers -----------------------------

def choose_channels(ch_names: List[str]) -> List[int]:
    # Use every available channel so lateralization averages remain stable.
    return list(range(len(ch_names)))

async def parse_json_stream(msg: str) -> Tuple[np.ndarray, float, float, List[str]]:
    """Original JSON schema: return (data[n_ch,n_samp], fs, t0, ch_names)."""
    obj = json.loads(msg)
    data = np.asarray(obj["data"], dtype=np.float64)
    fs = float(obj.get("fs", 250.0))
    t0 = float(obj.get("t0", time.time()))
    ch_names = obj.get("ch_names", [f"ch{i}" for i in range(data.shape[0])])
    return data, fs, t0, ch_names

# Basic 1-45 Hz bandpass
_DEF_SOS_CACHE: Dict[float, np.ndarray] = {}

def bp_filter(x: np.ndarray, fs: float) -> np.ndarray:
    sos = _DEF_SOS_CACHE.get(fs)
    if sos is None:
        low, high = 1.0, 45.0
        sos = butter(4, [low / (fs / 2), high / (fs / 2)], btype="band", output="sos")
        _DEF_SOS_CACHE[fs] = sos
    return sosfiltfilt(sos, x, axis=1)


_notch_cache: Dict[Tuple[float, float, float], Tuple[np.ndarray, np.ndarray]] = {}


def iir_notch_zero_phase(x: np.ndarray, fs: float, f0: float, q: float = NOTCH_Q) -> np.ndarray:
    """Apply a cached zero-phase IIR notch."""
    key = (fs, f0, q)
    coeffs = _notch_cache.get(key)
    if coeffs is None:
        coeffs = iirnotch(w0=f0, Q=q, fs=fs)
        _notch_cache[key] = coeffs
    b, a = coeffs
    # Require enough samples for filtfilt padding; otherwise leave data untouched.
    min_len = 3 * (max(len(a), len(b)) - 1)
    if x.shape[1] <= min_len:
        return x
    return filtfilt(b, a, x, axis=1)


def clean(x: np.ndarray, fs: float) -> np.ndarray:
    """Apply band-pass and harmonic notch filtering."""
    y = bp_filter(x, fs)
    y = iir_notch_zero_phase(y, fs, 60.0)
    y = iir_notch_zero_phase(y, fs, 120.0)
    return y


def bandpower(f: np.ndarray, Pxx: np.ndarray, lo: float, hi: float) -> float:
    mask = (f >= lo) & (f <= hi)
    return float(np.trapz(Pxx[mask], f[mask]))


def compact_features(x: np.ndarray, fs: float) -> np.ndarray:
    """Compute per-channel relative bandpower plus global lateralization."""
    nperseg = int(0.5 * fs)
    noverlap = int(0.25 * fs)
    eps = 1e-12

    mu_vals: List[float] = []
    beta_vals: List[float] = []
    rel_feats: List[float] = []

    for ch in range(x.shape[0]):
        f, Pxx = welch(x[ch], fs=fs, nperseg=nperseg, noverlap=noverlap)
        tot = bandpower(f, Pxx, 1, 45) + eps
        mu = bandpower(f, Pxx, 8, 12) + eps
        beta = bandpower(f, Pxx, 13, 30) + eps
        mu_vals.append(mu)
        beta_vals.append(beta)
        rel_feats.append(np.log(mu / tot))
        rel_feats.append(np.log(beta / tot))

    mu_arr = np.array(mu_vals)
    beta_arr = np.array(beta_vals)

    n_ch = x.shape[0]
    if n_ch >= 2:
        mid = n_ch // 2 or 1
        left_mu = float(np.mean(mu_arr[:mid]))
        right_mu = float(np.mean(mu_arr[mid:])) if mid < n_ch else left_mu
        left_beta = float(np.mean(beta_arr[:mid]))
        right_beta = float(np.mean(beta_arr[mid:])) if mid < n_ch else left_beta
    else:
        left_mu = right_mu = float(mu_arr[0])
        left_beta = right_beta = float(beta_arr[0])

    li_mu = (right_mu - left_mu) / (right_mu + left_mu + eps)
    li_beta = (right_beta - left_beta) / (right_beta + left_beta + eps)

    feats = rel_feats + [li_mu, li_beta]
    return np.array(feats, dtype=np.float64)


def bandpower_features(x: np.ndarray, fs: float, bands=BANDS) -> np.ndarray:
    nperseg = int(0.5 * fs)
    noverlap = int(0.25 * fs)
    feats = []
    for ch in range(x.shape[0]):
        f, Pxx = welch(x[ch], fs=fs, nperseg=nperseg, noverlap=noverlap)
        for lo, hi in bands:
            mask = (f >= lo) & (f <= hi)
            bp = np.trapz(Pxx[mask], f[mask])
            feats.append(bp)
    return np.array(feats, dtype=np.float64)

# ----------------------------- Training Core -----------------------------
async def run_training_generic(chunk_iter, out_dir: str, total_min: float, block_s: float):
    outp = pathlib.Path(out_dir)
    outp.mkdir(parents=True, exist_ok=True)

    X: List[np.ndarray] = []
    y: List[int] = []  # 1=up, 0=down
    sample_blocks: List[int] = []

    # Cue schedule
    total_s = int(total_min * 60)
    blocks = []
    cur = 0.0
    cur_label = 1
    while cur < total_s:
        blocks.append((cur, cur + block_s, cur_label))
        cur += block_s
        cur_label = 1 - cur_label

    LABEL_SHIFT_S = 0.6
    EDGE_TRIM_S = 0.4
    blocks = [(s + LABEL_SHIFT_S, e + LABEL_SHIFT_S, lab) for (s, e, lab) in blocks]

    fs: Optional[float] = None
    ch_names: Optional[List[str]] = None
    sel_idx: Optional[List[int]] = None
    ring_len = 0
    ring = None
    write_pos = 0
    samples_seen = 0
    window_powers: List[float] = []

    t_start = time.time()
    next_cue_idx = 0
    next_cue_change = t_start + blocks[0][1]
    current_label = blocks[0][2]
    print(f"[trainer] CUE = {'UP' if current_label else 'DOWN'} until T+{blocks[0][1]:.1f}s")

    last_emit = t_start

    async for data, fs_msg, t_recv, chs in chunk_iter:
        if fs is None:
            fs = fs_msg
            ch_names = chs
            sel_idx = choose_channels(ch_names)
            m = int(WINDOW_SECONDS * fs)
            buffer_margin = int(FILTER_MARGIN_S * fs)
            ring_len = int(max(3 * fs, m + 2 * buffer_margin + 1))
            ring = np.zeros((len(sel_idx), ring_len), dtype=np.float64)
            print("[trainer] fs=", fs, "channels=", len(ch_names), "using", [ch_names[i] for i in sel_idx])
        sel = data[sel_idx, :]
        n = sel.shape[1]
        for i in range(n):
            ring[:, write_pos] = sel[:, i]
            write_pos = (write_pos + 1) % ring_len
        samples_seen += n

        now = time.time()
        if now - last_emit >= HOP_SECONDS and fs is not None:
            last_emit = now
            m = int(WINDOW_SECONDS * fs)
            context_len = min(ring_len, m + int(FILTER_MARGIN_S * fs))
            if m <= ring_len and samples_seen >= context_len:
                idxs = (np.arange(context_len) + write_pos - context_len) % ring_len
                xw_full = ring[:, idxs]
                rel_t = now - t_start

                block_idx = None
                block_label = None
                block_start = None
                block_end = None
                for idx, (start, end, lab) in enumerate(blocks):
                    if start <= rel_t <= end:
                        block_idx = idx
                        block_label = lab
                        block_start = start
                        block_end = end
                        break

                if block_idx is None:
                    continue

                if rel_t < (block_start + EDGE_TRIM_S) or rel_t > (block_end - EDGE_TRIM_S):
                    continue

                xf_full = clean(xw_full, fs)
                xf = xf_full[:, -m:]
                pow_1_45 = float(np.sum(xf ** 2))
                window_powers.append(pow_1_45)
                median_pow = float(np.median(window_powers)) if window_powers else pow_1_45
                if median_pow <= 0:
                    median_pow = pow_1_45
                if pow_1_45 > 5 * median_pow or pow_1_45 < 0.1 * median_pow:
                    continue

                feats = compact_features(xf, fs)
                X.append(feats)
                y.append(block_label)
                sample_blocks.append(block_idx)

        if now >= next_cue_change and next_cue_idx + 1 < len(blocks):
            next_cue_idx += 1
            _, endt, lab = blocks[next_cue_idx]
            current_label = lab
            next_cue_change = t_start + endt
            print(f"[trainer] CUE switched → {'UP' if current_label else 'DOWN'} (t={now - t_start:.1f}s)")

        if now - t_start >= total_s:
            break

    if not X:
        print("[trainer] No valid windows collected; aborting training.")
        return

    X_arr = np.vstack(X)
    y_arr = np.array(y, dtype=np.int64)
    block_ids = np.array(sample_blocks, dtype=np.int64)
    print(
        f"[trainer] Collected windows: {len(y_arr)}; class balance: up={int(y_arr.sum())} down={int((y_arr==0).sum())}"
    )

    pipe = Pipeline([
        ("scaler", StandardScaler()),
        ("clf", LogisticRegression(max_iter=200, class_weight="balanced"))
    ])

    unique_blocks = sorted(set(block_ids.tolist()))

    if len(unique_blocks) < 2:
        print("[trainer] Not enough distinct blocks for holdout; training on all data.")
        pipe.fit(X_arr, y_arr)
        acc = accuracy_score(y_arr, pipe.predict(X_arr))
    else:
        split_at = max(1, int(round(len(unique_blocks) * 0.8)))
        if split_at >= len(unique_blocks):
            split_at = len(unique_blocks) - 1

        train_blocks = unique_blocks[:split_at]
        test_blocks = unique_blocks[split_at:]

        train_mask = np.isin(block_ids, train_blocks)
        test_mask = np.isin(block_ids, test_blocks)

        while len(np.unique(y_arr[train_mask])) < 2 and test_blocks:
            train_blocks.append(test_blocks.pop(0))
            train_mask = np.isin(block_ids, train_blocks)
            test_mask = np.isin(block_ids, test_blocks)

        if len(np.unique(y_arr[train_mask])) < 2:
            print("[trainer] Unable to obtain both classes in training; using all data.")
            train_mask = np.ones_like(block_ids, dtype=bool)
            test_mask = np.zeros_like(block_ids, dtype=bool)

        pipe.fit(X_arr[train_mask], y_arr[train_mask])

        if test_mask.any():
            yhat = pipe.predict(X_arr[test_mask])
            acc = accuracy_score(y_arr[test_mask], yhat)
        else:
            print("[trainer] No holdout samples available; reporting training accuracy.")
            acc = accuracy_score(y_arr[train_mask], pipe.predict(X_arr[train_mask]))
    print(f"[trainer] Holdout accuracy: {acc:.3f}")

    dump(pipe, outp / "model.joblib")
    cfg = {
        "bands": BANDS,
        "win_s": WINDOW_SECONDS,
        "hop_s": HOP_SECONDS,
        "sel_channels": [ch_names[i] for i in sel_idx] if ch_names and sel_idx else None,
        "fs_hint": fs,
        "holdout_acc": acc,
    }
    (outp / "config.json").write_text(json.dumps(cfg, indent=2))
    print("[trainer] Saved model to", str(outp / "model.joblib"))

# ----------------------------- Inference Core -----------------------------
class OutputHub:
    def __init__(self):
        self._clients = set()

    async def register(self, ws):
        self._clients.add(ws)

    async def unregister(self, ws):
        self._clients.discard(ws)

    async def broadcast(self, obj):
        if not self._clients:
            return
        msg = json.dumps(obj)
        dead = []
        for c in list(self._clients):
            try:
                await c.send(msg)
            except Exception:
                dead.append(c)
        for d in dead:
            await self.unregister(d)

async def output_server(hub: OutputHub, host: str, port: int):
    async def handler(ws):
        await hub.register(ws)
        try:
            await ws.wait_closed()
        finally:
            await hub.unregister(ws)
    async with websockets.serve(handler, host, port, max_size=2 * 1024 * 1024):
        print(f"[infer] Output WS listening on ws://{host}:{port}/output")
        await asyncio.Future()

async def run_inference_generic(chunk_iter, model_path: str, out_host: str, out_port: int):
    hub = OutputHub()
    server_task = asyncio.create_task(output_server(hub, out_host, out_port))
    pipe: Pipeline = load(model_path)

    alpha = 1 - math.exp(-HOP_SECONDS / 0.4)
    p_up_smooth = 0.5
    state = "down"

    fs = None
    ch_names = None
    sel_idx = None
    ring_len = 0
    ring = None
    write_pos = 0
    last_emit = time.time()
    samples_seen = 0

    try:
        async for data, fs_msg, t_recv, chs in chunk_iter:
            if fs is None:
                fs = fs_msg
                ch_names = chs
                sel_idx = choose_channels(ch_names)
                m = int(WINDOW_SECONDS * fs)
                buffer_margin = int(FILTER_MARGIN_S * fs)
                ring_len = int(max(3 * fs, m + 2 * buffer_margin + 1))
                ring = np.zeros((len(sel_idx), ring_len), dtype=np.float64)
                print("[infer] fs=", fs, "channels=", len(ch_names), "using", [ch_names[i] for i in sel_idx])
            sel = data[sel_idx, :]
            n = sel.shape[1]
            for i in range(n):
                ring[:, write_pos] = sel[:, i]
                write_pos = (write_pos + 1) % ring_len
            samples_seen += n

            now = time.time()
            if now - last_emit >= HOP_SECONDS and fs is not None:
                last_emit = now
                m = int(WINDOW_SECONDS * fs)
                context_len = min(ring_len, m + int(FILTER_MARGIN_S * fs))
                if m <= ring_len and samples_seen >= context_len:
                    idxs = (np.arange(context_len) + write_pos - context_len) % ring_len
                    xw_full = ring[:, idxs]
                    xf_full = clean(xw_full, fs)
                    xf = xf_full[:, -m:]
                    feats = compact_features(xf, fs)[None, :]
                    proba = pipe.predict_proba(feats)[0]
                    up_p = float(proba[1])
                    down_p = float(proba[0])
                    p_up_smooth = (1 - alpha) * p_up_smooth + alpha * up_p
                    if state != "up" and p_up_smooth > 0.65:
                        state = "up"
                    elif state != "down" and p_up_smooth < 0.35:
                        state = "down"
                    out = {
                        "t": time.time(),
                        "y": state,
                        "p": {"up": up_p, "down": down_p},
                        "latency_ms": int((time.time() - t_recv) * 1000),
                    }
                    await hub.broadcast(out)
    finally:
        server_task.cancel()
        with contextlib.suppress(asyncio.CancelledError):
            await server_task

# ----------------------------- JSON Source (original) -----------------------------
async def json_chunk_iter(eeg_ws: str):
    print("[source-json] connecting:", eeg_ws)
    async with websockets.connect(eeg_ws, max_size=8 * 1024 * 1024) as ws:
        while True:
            msg = await asyncio.wait_for(ws.recv(), timeout=10.0)
            t_recv = time.time()
            data, fs, t0, chs = await parse_json_stream(msg)
            yield data, fs, t_recv, chs

# ----------------------------- DataHub Source (your integration) -----------------------------
MetaMap = Dict[str, Dict[str, Any]]
PacketHandler = Callable[
    [Dict[str, Any], np.ndarray, Optional[Dict[str, Any]]],
    Union[Awaitable[None], None],
]

def ws_data_to_np_array(header: Dict[str, Any], payload: bytes) -> np.ndarray:
    dtype = "<i4" if header.get("packet_type") == "RawI32" else "<f4"
    data = np.frombuffer(payload, dtype=dtype)
    channels = header.get("num_channels", 0) or 1
    batch = header.get("batch_size", 0) or (len(data) // channels)
    return data.reshape(batch, channels, order="C")

def ws_data_received(message: Union[str, bytes], metadata: MetaMap) -> Tuple[str, Any]:
    if isinstance(message, str):
        obj = json.loads(message)
        if obj.get("message_type") == "meta_update":
            metadata[obj["topic"]] = obj["meta"]
            return "meta", obj
        return "control", obj
    view = memoryview(message)
    (header_len,) = struct.unpack(">I", view[:4])
    header = json.loads(view[4 : 4 + header_len].tobytes())
    payload = view[4 + header_len :].tobytes()
    samples = ws_data_to_np_array(header, payload)
    meta = metadata.get(header["topic"]) if isinstance(metadata, dict) else None
    return "samples", (header, samples, meta)

async def start_data_ws(host: str, subscriptions: Iterable[Tuple[str, int]], on_packet: PacketHandler) -> None:
    url = f"ws://{host}:9000/ws/data"
    metadata: MetaMap = {}
    async with websockets.connect(url, max_size=8 * 1024 * 1024) as ws:
        for topic, epoch in subscriptions:
            await ws.send(json.dumps({"type": "subscribe", "topic": topic, "epoch": epoch}))
        async for message in ws:
            kind, payload = ws_data_received(message, metadata)
            if kind == "samples":
                header, samples, meta = payload
                result = on_packet(header, samples, meta)
                if asyncio.iscoroutine(result):
                    await result

async def datahub_chunk_iter(host: str, topic: str, epoch: int, fs_override: Optional[float], ch_override: Optional[List[str]]):
    """Yield (data[n_ch, n_samples], fs, t_recv, ch_names) from DataHub."""
    queue: asyncio.Queue = asyncio.Queue(maxsize=128)

    async def on_packet(header, samples: np.ndarray, meta):
        # samples: (batch, channels) → transpose to (channels, batch)
        x = samples.T.copy()
        t_recv = time.time()
        # fs & ch_names from meta or overrides
        fs = None
        chs = None
        if meta:
            fs = float(meta.get("fs")) if meta.get("fs") is not None else None
            chs = meta.get("ch_names") or meta.get("channels")
        if fs is None:
            fs = fs_override or 250.0
        if chs is None:
            n_ch = x.shape[0]
            chs = ch_override or [f"ch{i}" for i in range(n_ch)]
        await queue.put((x, fs, t_recv, chs))

    async def feeder():
        await start_data_ws(host, [(topic, epoch)], on_packet)

    feeder_task = asyncio.create_task(feeder())
    try:
        while True:
            yield await queue.get()
    finally:
        feeder_task.cancel()
        with contextlib.suppress(asyncio.CancelledError):
            await feeder_task

# ----------------------------- CLI -----------------------------

def main():
    ap = argparse.ArgumentParser(description="metaBCI train→infer over websockets (JSON or DataHub)")
    sub = ap.add_subparsers(dest="cmd", required=True)

    # Original JSON modes
    ap_tr = sub.add_parser("train", help="run cued training on JSON EEG stream and fit a model")
    ap_tr.add_argument("--eeg-ws", required=True, help="e.g. ws://pi5.local:8765/eeg")
    ap_tr.add_argument("--out-dir", default="./metabci_model", help="where to save model")
    ap_tr.add_argument("--minutes", type=float, default=TRAIN_TOTAL_MIN)
    ap_tr.add_argument("--block", type=float, default=BLOCK_SECONDS)

    ap_in = sub.add_parser("infer", help="run inference from JSON EEG stream")
    ap_in.add_argument("--eeg-ws", required=True)
    ap_in.add_argument("--model", default="./metabci_model/model.joblib")
    ap_in.add_argument("--host", default="127.0.0.1")
    ap_in.add_argument("--port", type=int, default=8766)

    # DataHub modes
    ap_trh = sub.add_parser("train-datahub", help="cued training using DataHub stream (:9000/ws/data)")
    ap_trh.add_argument("--host", required=True, help="raspberrypi.local or IP")
    ap_trh.add_argument("--topic", default="eeg_voltage")
    ap_trh.add_argument("--epoch", type=int, default=1)
    ap_trh.add_argument("--out-dir", default="./metabci_model")
    ap_trh.add_argument("--minutes", type=float, default=TRAIN_TOTAL_MIN)
    ap_trh.add_argument("--block", type=float, default=BLOCK_SECONDS)
    ap_trh.add_argument("--fs", type=float, default=None, help="override fs if not supplied by meta")
    ap_trh.add_argument("--channels", type=str, default=None, help="CSV of channel names if meta lacks them")

    ap_inh = sub.add_parser("infer-datahub", help="inference using DataHub stream (:9000/ws/data)")
    ap_inh.add_argument("--host", required=True)
    ap_inh.add_argument("--topic", default="eeg_voltage")
    ap_inh.add_argument("--epoch", type=int, default=1)
    ap_inh.add_argument("--model", default="./metabci_model/model.joblib")
    ap_inh.add_argument("--out-host", default="127.0.0.1")
    ap_inh.add_argument("--out-port", type=int, default=8766)
    ap_inh.add_argument("--fs", type=float, default=None)
    ap_inh.add_argument("--channels", type=str, default=None)

    args = ap.parse_args()

    if args.cmd == "train":
        chunker = json_chunk_iter(args.eeg_ws)
        asyncio.run(run_training_generic(chunker, args.out_dir, args.minutes, args.block))
        return

    if args.cmd == "infer":
        chunker = json_chunk_iter(args.eeg_ws)
        asyncio.run(run_inference_generic(chunker, args.model, args.host, args.port))
        return

    if args.cmd == "train-datahub":
        chans = args.channels.split(",") if args.channels else None
        chunker = datahub_chunk_iter(args.host, args.topic, args.epoch, args.fs, chans)
        asyncio.run(run_training_generic(chunker, args.out_dir, args.minutes, args.block))
        return

    if args.cmd == "infer-datahub":
        chans = args.channels.split(",") if args.channels else None
        chunker = datahub_chunk_iter(args.host, args.topic, args.epoch, args.fs, chans)
        asyncio.run(run_inference_generic(chunker, args.model, args.out_host, args.out_port))
        return

if __name__ == "__main__":
    main()
