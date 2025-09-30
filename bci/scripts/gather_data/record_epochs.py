#!/usr/bin/env python3
# record_epochs.py — EEG + label recorder with cued mode + no-data watchdog
# Python 3.9+

import argparse, asyncio, contextlib, json, sys, time, pathlib
from typing import Any, Dict, List, Optional, Tuple, Union
from collections import deque, Counter

import numpy as np
from ws_client import start_data_ws  # your websocket client

LABELS = {"down": 0, "up": 1, "rest": 2}
INV_LABELS = {v: k for k, v in LABELS.items()}

def now_s() -> float:
    return time.time()

def parse_labels(csv: str) -> List[str]:
    names = [s.strip().lower() for s in csv.split(",") if s.strip()]
    unknown = [n for n in names if n not in LABELS]
    if unknown:
        raise ValueError(f"Unknown labels: {unknown}. Allowed: {list(LABELS.keys())}")
    return names

def parse_chs(csv: Optional[str]) -> Optional[List[int]]:
    """
    Parse comma-separated 1-based integer channel indices.
    Returns a list of unique zero-based indices, or None for 'all'.
    Examples:
      '1,2,8' -> [0,1,7]
      None or '' -> None (meaning: use all channels)
    """
    if not csv:
        return None
    raw = [s.strip() for s in csv.split(",") if s.strip()]
    idxs_1based: List[int] = []
    for s in raw:
        if not s.isdigit():
            raise ValueError(f"--chs expects 1-based integers like '1,2,8' (got '{s}')")
        idxs_1based.append(int(s))
    # dedupe while preserving order
    seen = set()
    idxs_0 = []
    for k in idxs_1based:
        if k not in seen:
            seen.add(k)
            idxs_0.append(k - 1)  # convert to zero-based
    return idxs_0

class Recorder:
    def __init__(self, label_shift_s: float, beep: bool):
        self.fs: Optional[float] = None
        self.ch_names: Optional[List[str]] = None
        self.t0: float = now_s()

        self._data: List[np.ndarray] = []
        self._labels: List[np.ndarray] = []
        self._events: List[Tuple[float, int]] = []
        self._notes: List[Dict[str, Any]] = []

        self.current_label: int = LABELS["rest"]
        self._label_shift_s: float = label_shift_s
        self._label_delay_samples: int = 0
        self._label_buffer: deque = deque()

        self._samples_total: int = 0
        self._sec_by_label = Counter()
        self._last_label_change = self.t0
        self._beep = beep

        # packet debug
        self.pkts_seen: int = 0
        self.last_pkt_time: float = 0.0

    def _ensure_meta(self, fs: float, chs: List[str]):
        if self.fs is None:
            self.fs = float(fs)
            self.ch_names = list(chs) if chs else [f"ch{i}" for i in range((self.peek_channels() or 1))]
            self._label_delay_samples = int(round(self._label_shift_s * self.fs))
            self._events.append((0.0, self.current_label))
            print(f"[record] fs={self.fs:.2f} Hz, channels={len(self.ch_names)} → label_shift={self._label_delay_samples} samples")

    def peek_channels(self) -> Optional[int]:
        if self._data:
            return self._data[0].shape[0]
        return None

    def set_label(self, name: str):
        name = name.strip().lower()
        if name not in LABELS:
            print(f"[record] Unknown label '{name}'. Use: {list(LABELS.keys())}")
            return
        new_lbl = LABELS[name]
        if new_lbl == self.current_label:
            print(f"[record] Label already '{name}'.")
            return
        now = now_s()
        self._sec_by_label[self.current_label] += max(0.0, now - self._last_label_change)
        self._last_label_change = now

        self.current_label = new_lbl
        t_rel = now - self.t0
        self._events.append((t_rel, self.current_label))
        if self._beep:
            print("\a", end="", flush=True)
        print(f"[record] Label → {name.upper()} @ +{t_rel:.2f}s")

    def add_note(self, text: str):
        t_rel = now_s() - self.t0
        self._notes.append({"t": float(t_rel), "note": text})
        print(f"[record] Note @ +{t_rel:.2f}s: {text}")

    def status_line(self) -> str:
        elapsed = int(now_s() - self.t0)
        secs = dict((INV_LABELS[k], int(v)) for k, v in self._sec_by_label.items())
        running = int(now_s() - self._last_label_change)
        secs[INV_LABELS[self.current_label]] = secs.get(INV_LABELS[self.current_label], 0) + running
        parts = [
            f"label={INV_LABELS[self.current_label].upper():>4}",
            f"elapsed={elapsed:>4}s",
        ]
        parts += [f"{k}:{v}s" for k, v in sorted(secs.items())]
        return " | ".join(parts)

    def hud_tick(self, stale_after_s: float = 3.0):
        no_data = (self.pkts_seen == 0)
        stale = (not no_data) and (now_s() - self.last_pkt_time > stale_after_s)
        prefix = "[NO DATA]" if no_data else ("[STALE]" if stale else "[record]")
        print("\r%s %s%s" % (prefix, self.status_line(), " " * 8), end="", flush=True)

    def process_batch(self, x_batch: np.ndarray, fs: float, chs: Optional[List[str]]):
        if x_batch.ndim != 2:
            return
        self.pkts_seen += 1
        self.last_pkt_time = now_s()

        C = x_batch.shape[1]
        self._ensure_meta(fs, chs or [f"ch{i}" for i in range(C)])

        xT = x_batch.T.astype(np.float32, copy=False)
        self._label_buffer.extend([self.current_label] * x_batch.shape[0])

        emit = max(0, len(self._label_buffer) - self._label_delay_samples)
        if emit > 0:
            lbls = [self._label_buffer.popleft() for _ in range(emit)]
            lbl_arr = np.array(lbls, dtype=np.uint8)
            x_emit = xT[:, -emit:]
            self._data.append(x_emit)
            self._labels.append(lbl_arr)
            self._samples_total += emit

    def finalize(self):
        self._sec_by_label[self.current_label] += max(0.0, now_s() - self._last_label_change)
        self._label_buffer.clear()

    def save_npz(self, out_path: Union[str, pathlib.Path], extra_meta: Dict[str, Any]):
        if self.fs is None or self.ch_names is None or not self._labels:
            print("\n[record] Nothing to save (no data).")
            return
        data = np.concatenate(self._data, axis=1) if len(self._data) > 1 else self._data[0]
        labels = np.concatenate(self._labels, axis=0) if len(self._labels) > 1 else self._labels[0]
        events = np.array(self._events, dtype=np.float64) if self._events else np.zeros((0,2), dtype=np.float64)
        meta_json = json.dumps(extra_meta, indent=2, sort_keys=True)
        notes_json = json.dumps(self._notes, indent=2, sort_keys=True)
        np.savez_compressed(
            out_path,
            data=data,
            labels=labels,
            fs=np.array(self.fs, dtype=np.float64),
            ch_names=np.array(self.ch_names, dtype=object),
            t0=np.array(self.t0, dtype=np.float64),
            events=events,
            notes_json=np.array(notes_json),
            meta_json=np.array(meta_json),
        )
        dur = data.shape[1] / self.fs
        print(f"\n[record] Saved {out_path}  shape={data.shape}  dur={dur:.1f}s  labels={labels.shape}")

# -------------------- cued mode --------------------
async def cued_loop(rec: Recorder, labels_cycle: List[str], end_time: float, block_s: float):
    """Cycle labels until end_time, changing every block_s seconds."""
    idx = 0
    rec.set_label(labels_cycle[idx])
    while True:
        if now_s() >= end_time:
            break
        remaining = max(0.0, end_time - now_s())
        await asyncio.sleep(min(block_s, remaining))
        if now_s() >= end_time:
            break
        idx = (idx + 1) % len(labels_cycle)
        rec.set_label(labels_cycle[idx])

# -------------------- main --------------------
async def stdin_reader():
    loop = asyncio.get_running_loop()
    return await loop.run_in_executor(None, sys.stdin.readline)

async def record_main(args):
    outp = pathlib.Path(args.out).expanduser().resolve()
    outp.parent.mkdir(parents=True, exist_ok=True)
    rec = Recorder(label_shift_s=args.label_shift, beep=args.beep)

    # Derive a single end_time used by EVERYTHING
    duration_min = (args.minutes if args.minutes is not None else (5.0 if args.cued else None))
    end_time: Optional[float] = None
    if duration_min is not None:
        end_time = now_s() + float(duration_min) * 60.0

    # Pre-parse desired channels (zero-based), but validate against actual count on first packet
    desired_chs = parse_chs(args.chs)  # None means "all"
    if desired_chs is not None and len(desired_chs) == 0:
        print("[warn] --chs parsed to empty set; will record all channels.")
        desired_chs = None

    def on_packet(header, samples, meta):
        # Lazy-init one time debug print
        if not hasattr(on_packet, "_dbg"):
            print(f"\n[record] first packet: topic={header.get('topic')} shape={samples.shape}")
            on_packet._dbg = True

        fs = float(meta.get("fs")) if meta and meta.get("fs") else args.fs or 250.0
        chs_meta = (meta.get("ch_names") or meta.get("channels")) if meta else None
        if chs_meta is None:
            chs_meta = [f"ch{i}" for i in range(samples.shape[1])]

        # Apply channel selection once, after we know #channels
        if desired_chs is not None and not hasattr(on_packet, "_chs_idx"):
            total = samples.shape[1]
            valid = [i for i in desired_chs if 0 <= i < total]
            invalid = [i for i in desired_chs if not (0 <= i < total)]
            if invalid:
                # report using 1-based for friendliness
                bad_str = ",".join(str(i+1) for i in invalid)
                print(f"[warn] Ignoring out-of-range channel(s): {bad_str} (total={total})")
            if not valid:
                print("[warn] All requested channels invalid; recording all channels instead.")
                on_packet._chs_idx = None
            else:
                on_packet._chs_idx = valid
                friendly = ",".join(str(i+1) for i in valid)
                print(f"[record] Selecting channels: {friendly} (of total {total})")
        idx = getattr(on_packet, "_chs_idx", None)

        if idx:
            samples_sel = samples[:, idx]
            chs_sel = [chs_meta[i] for i in idx]
        else:
            samples_sel = samples
            chs_sel = chs_meta

        rec.process_batch(samples_sel, fs, chs_sel)

    ws_task = asyncio.create_task(start_data_ws(args.host, [(args.topic, args.epoch)], on_packet))

    async def hud_loop():
        try:
            while True:
                rec.hud_tick(stale_after_s=args.stale_after)
                await asyncio.sleep(args.hud_interval)
        except asyncio.CancelledError:
            pass
    hud_task = asyncio.create_task(hud_loop())

    # no-data watchdog
    async def data_watchdog():
        if args.data_timeout <= 0:
            return
        t0 = now_s()
        while rec.pkts_seen == 0 and (now_s() - t0) < args.data_timeout:
            await asyncio.sleep(0.2)
        if rec.pkts_seen == 0:
            print(f"\n[error] No data received within {args.data_timeout:.1f}s. Check host/topic/epoch.")
            raise SystemExit(2)
    watchdog_task = asyncio.create_task(data_watchdog())

    cued_task = None
    if args.cued:
        labels_cycle = parse_labels(args.labels)
        if end_time is None:
            end_time = now_s() + 5.0 * 60.0
        print(f"[record] CUED MODE: labels={labels_cycle}, block={args.block}s, until {time.strftime('%H:%M:%S', time.localtime(end_time))}")
        cued_task = asyncio.create_task(cued_loop(rec, labels_cycle, end_time, args.block))

    try:
        while True:
            if end_time is not None and now_s() >= end_time:
                print("\n[record] Time limit reached.")
                break
            try:
                line = await asyncio.wait_for(stdin_reader(), timeout=args.hud_interval)
            except asyncio.TimeoutError:
                continue
            if not line:
                continue
            line = line.strip()
            if line in ("up", "down", "rest"):
                rec.set_label(line)
            elif line.startswith("mark "):
                rec.add_note(line[5:].strip())
            elif line == "status":
                # Keep compatibility: print the status line once
                print("\n" + rec.status_line())
            elif line == "quit":
                break
    except KeyboardInterrupt:
        print("\n[record] Interrupted.")
    finally:
        for t in [cued_task, hud_task, watchdog_task, ws_task]:
            if t:
                t.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await t
        rec.finalize()
        extra = dict(args=vars(args))
        rec.save_npz(outp, extra)

def parse_args():
    ap = argparse.ArgumentParser(description="Record EEG stream with labels → .npz (cued mode + watchdog).")
    ap.add_argument("--host", default="raspberrypi.local")
    ap.add_argument("--topic", default="eeg_voltage")
    ap.add_argument("--epoch", type=int, default=1)
    ap.add_argument("--out", default="./session.npz")
    ap.add_argument("--fs", type=float, default=None)
    ap.add_argument("--label-shift", type=float, default=0.6)
    ap.add_argument("--minutes", type=float, default=None)  # total duration (minutes)
    ap.add_argument("--hud-interval", type=float, default=0.5)
    ap.add_argument("--beep", action="store_true")

    ap.add_argument("--cued", action="store_true")
    ap.add_argument("--labels", default="up,down,rest")
    ap.add_argument("--block", type=float, default=5.0)

    ap.add_argument("--stale-after", type=float, default=3.0)
    ap.add_argument("--data-timeout", type=float, default=10.0)

    # NEW: channel selection (1-based indices), e.g. --chs 1,2,8
    ap.add_argument("--chs", type=str, default=None,
                    help="Comma-separated 1-based channel indices to record (e.g., '1,2,8'). Omit to record all.")

    return ap.parse_args()

def main():
    args = parse_args()
    asyncio.run(record_main(args))

if __name__ == "__main__":
    main()
