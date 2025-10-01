#!/usr/bin/env python3
"""
pong_stream.py — Stream EEG inference and emit newline-delimited JSON to stdout.

Purpose:
- Reuse the inference pipeline (infer.py) to process EEG stream windows.
- For each window, output a single JSON line: {"probs":[...]} where probs is the
  softmax probability vector for model classes.
- Designed to be piped into another tool (in a different repo), e.g.:
    python3 bci/scripts/models/pong_stream.py \
      --file bci/scripts/models/gpt1.py \
      --weights bci/scripts/blinknet.pt \
      --host raspberrypi.local --topic eeg_voltage --epoch 1 \
      --fp1_idx 3 --fp2_idx 6 --smooth 0 \
    | python3 eeg_to_pongo.py --room default

Notes:
- All logs and meta info are printed to stderr to avoid contaminating stdout.
- Stdout contains ONLY NDJSON records with {"probs":[...]}.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

import numpy as np
import torch

# Reuse inference helpers from infer.py (same directory)
try:
    from .infer import (  # type: ignore
        load_model_and_spec,
        ensure_ws_client_on_path,
        StreamState,
        build_model,
    )
except Exception:
    # Fallback to plain import when running as a script
    try:
        from infer import (  # type: ignore
            load_model_and_spec,
            ensure_ws_client_on_path,
            StreamState,
            build_model,
        )
    except Exception as e:
        print(f"[pong_stream] Failed to import infer.py helpers: {e}", file=sys.stderr)
        raise


async def run_stream(args) -> None:
    model_file = Path(args.file)
    # Load model pipeline spec (defines transforms, channel selection, etc.)
    try:
        _, spec = load_model_and_spec(model_file)
    except Exception as e:
        print(f"[pong_stream] load_model_and_spec error: {e}", file=sys.stderr)
        raise

    # Ensure ws_client import path is available
    ensure_ws_client_on_path(model_file)
    try:
        from ws_client import start_data_ws  # type: ignore
    except Exception as e:
        print(f"[pong_stream] import ws_client error: {e}", file=sys.stderr)
        raise

    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    model_path = Path(args.weights)

    try:
        model, n_classes, meta = build_model(model_path, spec["model_class"], device)
    except Exception as e:
        print(f"[pong_stream] build_model error: {e}", file=sys.stderr)
        raise

    if meta:
        try:
            print(
                f"[pong_stream] weights meta: fs={meta.get('fs','?')}Hz "
                f"window_s={meta.get('window_s','?')} hop_s={meta.get('hop_s','?')} "
                f"band={meta.get('bandpass','?')} classes={meta.get('classes','?')}",
                file=sys.stderr,
                flush=True,
            )
        except Exception:
            pass

    # Build streaming state (same semantics as infer.py)
    state = StreamState(
        device=device,
        model=model,
        transform_factory=spec["make_stream_transform"],
        select_fp_indices=spec["select_fp_indices"],
        window_s=float(args.window_s),
        hop_s=float(args.hop_s),
        smooth_k=int(args.smooth) if args.smooth and args.smooth > 0 else 0,
        fp1_idx_override=(int(args.fp1_idx) if getattr(args, "fp1_idx", -1) is not None and int(args.fp1_idx) >= 0 else None),
        fp2_idx_override=(int(args.fp2_idx) if getattr(args, "fp2_idx", -1) is not None and int(args.fp2_idx) >= 0 else None),
    )

    # Packet handler: append samples, infer on ready windows, write NDJSON
    async def on_packet(header: Dict[str, Any], samples: np.ndarray, meta_in: Optional[Dict[str, Any]]):
        # Keep any diagnostics on stderr
        try:
            state.on_meta(header, meta_in, samples)
        except Exception as e:
            print(f"[pong_stream] on_meta error: {e}", file=sys.stderr)

        state.append(samples)

        while True:
            pair: Optional[Tuple[np.ndarray, np.ndarray]] = state.next_window()
            if pair is None:
                break

            fp1, fp2 = pair
            try:
                pred, probs = state.infer_window(fp1, fp2)
            except Exception as e:
                print(f"[pong_stream] infer_window error: {e}", file=sys.stderr)
                continue

            # We intentionally output RAW probabilities (pre-smoothing) to stdout.
            try:
                arr = probs.tolist()
                # Enforce Python floats (json module handles fine)
                out = {"probs": [float(x) for x in arr]}
                sys.stdout.write(json.dumps(out, separators=(",", ":")) + "\n")
                sys.stdout.flush()
            except Exception as e:
                print(f"[pong_stream] output error: {e}", file=sys.stderr)

    # Connect to data websocket and start streaming loop
    try:
        print(
            f"[pong_stream] 🚀 host={args.host} topic={args.topic} epoch={args.epoch} "
            f"| model={model_path.name} device={device} | emit=NDJSON({'probs'})",
            file=sys.stderr,
            flush=True,
        )
        await start_data_ws(args.host, [(args.topic, int(args.epoch))], on_packet)
    except asyncio.CancelledError:
        pass
    except Exception as e:
        print(f"[pong_stream] stream error: {e}", file=sys.stderr)
        raise


def parse_args():
    ap = argparse.ArgumentParser(
        description="Stream EEG inference and emit NDJSON lines like: {\"probs\": [p0, p1, ...]} to stdout"
    )
    ap.add_argument("--file", type=str, default=str(Path(__file__).resolve().parent / "gpt1.py"), help="Path to model pipeline file (e.g., gpt1.py)")
    ap.add_argument("--weights", type=str, default=str(Path(__file__).resolve().parents[1] / "blinknet.pt"), help="Path to model weights .pt")
    ap.add_argument("--device", type=str, default=None, help="cuda|cpu (auto if not set)")

    # Stream source
    ap.add_argument("--host", type=str, default="raspberrypi.local", help="EEG data websocket host")
    ap.add_argument("--topic", type=str, default="eeg_voltage", help="EEG data topic")
    ap.add_argument("--epoch", type=int, default=1, help="Epoch identifier (integer)")
    ap.add_argument("--window_s", type=float, default=1.0, help="Inference window length (seconds)")
    ap.add_argument("--hop_s", type=float, default=0.25, help="Window hop (seconds)")
    ap.add_argument("--fp1_idx", type=int, default=-1, help="Override index for Fp1 channel (0-based); -1=auto")
    ap.add_argument("--fp2_idx", type=int, default=-1, help="Override index for Fp2 channel (0-based); -1=auto")
    ap.add_argument("--smooth", type=int, default=0, help="Majority vote over last K predictions (0=off). Note: output remains RAW probs.")

    return ap.parse_args()


def main():
    args = parse_args()
    try:
        asyncio.run(run_stream(args))
    except KeyboardInterrupt:
        print("\n[pong_stream] Interrupted.", file=sys.stderr)


if __name__ == "__main__":
    main()