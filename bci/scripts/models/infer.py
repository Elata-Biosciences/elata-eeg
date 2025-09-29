#!/usr/bin/env python3
"""
Generic offline inference via model registry with checkpoint-driven hyperparameters.

Usage:
  # Auto-detect model and use hyperparameters from checkpoint meta
  python bci/scripts/models/infer.py --weights gpt1.pt --npz bci/scripts/data/session2.npz

  # Force a specific model key and override window/hop (disables meta for those)
  python bci/scripts/models/infer.py --model gpt1 --weights gpt1.pt --npz bci/scripts/data/session2.npz --no_meta --window_s 1.0 --hop_s 0.25

  # Save predictions to CSV
  python bci/scripts/models/infer.py --weights gpt1.pt --npz bci/scripts/data/session2.npz --save_csv preds.csv
"""

from __future__ import annotations

import argparse
from pathlib import Path
from typing import Any, Dict, Optional, Tuple, List
import sys

import numpy as np
import torch

# Path setup so running this file directly works
HERE = Path(__file__).resolve().parent                 # .../bci/scripts/models
SCRIPTS_DIR = HERE.parent                              # .../bci/scripts
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

# Registry + tools
from registry import get_spec  # type: ignore
try:
    from tools import load_file, window_stage  # type: ignore
except Exception as e:
    print(f"❌ Could not import tools (load_file, window_stage): {e}")
    print("Ensure PYTHONPATH includes bci/scripts or run from repo root.")
    sys.exit(1)


def load_weights(weights_path: Path) -> Tuple[Dict[str, Any], Dict[str, Any]]:
    obj = torch.load(str(weights_path), map_location="cpu")
    meta: Dict[str, Any] = {}
    if isinstance(obj, dict) and "state_dict" in obj:
        state_dict = obj["state_dict"]
        meta = (obj.get("meta") or {})
    else:
        # raw state dict
        state_dict = obj
    return state_dict, meta


def resolve_model_key(arg_model: str, meta: Dict[str, Any]) -> str:
    if arg_model and arg_model.lower() != "auto":
        return arg_model
    # Prefer new hparams.model_key, fallback to legacy 'model'
    h = meta.get("hparams") if isinstance(meta.get("hparams"), dict) else {}
    mk = (h.get("model_key") or meta.get("model") or "gpt1")
    return str(mk)


def pick_value(name: str, cli_value: float, meta: Dict[str, Any], use_meta: bool, default: float) -> float:
    """Choose value for window_s/hop_s from meta if enabled; else fall back to CLI or default."""
    if not use_meta:
        return float(cli_value if cli_value is not None else default)
    h = meta.get("hparams") if isinstance(meta.get("hparams"), dict) else {}
    if name in h:
        return float(h[name])
    if name in meta:
        return float(meta[name])
    return float(cli_value if cli_value is not None else default)

def parse_chs_arg(chs: str, total_ch: int):
    """
    Parse --chs hyperparameter.
    - "": returns None (use meta or default)
    - "all" or "*": returns "all"
    - "1,2,3": returns a list of zero-based indices [0,1,2], filtered to valid range and deduped.
    """
    s = (chs or "").strip()
    if not s:
        return None
    s_lower = s.lower()
    if s_lower in ("all", "*"):
        return "all"
    parts = [p.strip() for p in s.split(",") if p.strip()]
    idxs = []
    seen = set()
    for p in parts:
        if not p.isdigit():
            continue
        i1 = int(p)
        i0 = i1 - 1  # convert to 0-based
        if 0 <= i0 < int(total_ch) and i0 not in seen:
            seen.add(i0)
            idxs.append(i0)
    if not idxs:
        return None
    return idxs

def infer(args):
    weights_path = Path(args.weights).resolve()
    if not weights_path.exists():
        raise SystemExit(f"Weights not found: {weights_path}")

    # Load weights/meta first to resolve model and hyperparameters
    state_dict, meta = load_weights(weights_path)
    model_key = resolve_model_key(args.model, meta)
    use_meta = not bool(args.no_meta)

    # Resolve model spec from registry
    spec = get_spec(model_key)
    ModelClass = spec["model_class"]
    offline_prepare_X4 = spec["offline_prepare_X4"]
    select_fp_indices = spec["select_fp_indices"]

    # Load data
    npz_path = Path(args.npz).resolve()
    if not npz_path.exists():
        raise SystemExit(f"NPZ not found: {npz_path}")
    d = load_file(str(npz_path))
    fs = float(d.fs)
    cont = d.data.astype(np.float32)  # (C,T)

    # Window parameters (prefer checkpoint meta if enabled)
    window_s = pick_value("window_s", args.window_s, meta, use_meta, default=1.0)
    hop_s = pick_value("hop_s", args.hop_s, meta, use_meta, default=0.25)

    # Window
    X, y = window_stage(cont, d.labels, fs, window_s=float(window_s), hop_s=float(hop_s))  # X:(N,C,T)
    print(f"Model={model_key} | Windows: {X.shape[0]} | X shape={X.shape} | fs={fs:.1f}Hz | win={window_s} hop={hop_s}")

    # Channel selection via --chs (1-based), meta chs_indices, or legacy Fp selection
    try:
        ch_names = getattr(d, "chan_names", None)
    except Exception:
        ch_names = None

    total_ch = X.shape[1]
    # Helper for nice printing
    def _safe_names(indices):
        if ch_names:
            return [str(ch_names[i]) if 0 <= i < len(ch_names) else f"ch{i}" for i in indices]
        return [f"ch{i}" for i in indices]

    # 1) CLI --chs
    chs_arg = parse_chs_arg(getattr(args, "chs", ""), total_ch=total_ch)
    chs_used: List[int]
    used_fp = False
    if chs_arg is None:
        # 2) Try checkpoint meta chs_indices
        if use_meta:
            h = meta.get("hparams") if isinstance(meta.get("hparams"), dict) else {}
            ci = h.get("chs_indices") if isinstance(h.get("chs_indices"), (list, tuple)) else meta.get("chs_indices")
            if isinstance(ci, (list, tuple)) and len(ci) >= 1:
                chs_used = [int(i) for i in ci if 0 <= int(i) < total_ch]
            else:
                chs_used = []
        else:
            chs_used = []
        # 3) Legacy fallback: pick Fp1/Fp2 (falls back to 0,1)
        if not chs_used:
            idx_fp1, idx_fp2 = select_fp_indices(ch_names)
            chs_used = [int(idx_fp1), int(idx_fp2)]
            used_fp = True
    elif chs_arg == "all":
        chs_used = list(range(total_ch))
    else:
        chs_used = list(map(int, chs_arg))

    if not chs_used:
        raise SystemExit("No valid channels selected. Use --chs 'all' or a comma list like --chs 1,2,3")

    X = X[:, chs_used, :]  # (N,C,T) with selected channels
    print(f"Using channels: indices={chs_used} names={_safe_names(chs_used)}")

    # Preprocess to (N,C,T)
    Xp = offline_prepare_X4(X, fs).astype(np.float32)

    # Build model and load weights
    if "classifier.weight" in state_dict:
        n_classes = int(state_dict["classifier.weight"].shape[0])
    else:
        classes = meta.get("hparams", {}).get("classes") if isinstance(meta.get("hparams"), dict) else meta.get("classes", [])
        n_classes = int(len(classes) or 3)

    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    n_in_ch = int(Xp.shape[1])
    try:
        model = ModelClass(n_classes=n_classes, n_in_ch=n_in_ch).to(device)
    except TypeError:
        model = ModelClass(n_classes=n_classes).to(device)

    try:
        model.load_state_dict(state_dict, strict=True)
    except Exception as e:
        print(f"[warn] strict load failed ({e}); retrying with strict=False")
        model.load_state_dict(state_dict, strict=False)
    model.eval()

    # Inference
    X_tensor = torch.from_numpy(Xp).to(device)
    with torch.no_grad():
        logits = model(X_tensor)
        preds = torch.argmax(logits, dim=1).cpu().numpy()
        probs = torch.softmax(logits, dim=1).cpu().numpy()

    print(f"Classes: {n_classes} | n_in_ch={n_in_ch} | First 20 preds: {preds[:20]}")
    uniq, cnts = np.unique(preds, return_counts=True)
    print("Class histogram:", {int(u): int(c) for u, c in zip(uniq, cnts)})

    if args.save_csv:
        out = Path(args.save_csv)
        np.savetxt(out, preds, fmt="%d", delimiter=",")
        print(f"Saved predictions to {out}")

    return 0


def parse_args():
    ap = argparse.ArgumentParser(description="Generic offline inference via model registry (checkpoint-driven hyperparams by default)")
    ap.add_argument("--model", type=str, default="auto", help="Model key (e.g., gpt1, gpt2) or 'auto' to read from checkpoint meta")
    ap.add_argument("--weights", type=str, default="gpt1.pt", help="Path to model weights .pt")
    ap.add_argument("--npz", type=str, default=str(Path(__file__).resolve().parents[1] / "data" / "session2.npz"))
    ap.add_argument("--window_s", type=float, default=1.0)
    ap.add_argument("--hop_s", type=float, default=0.25)
    ap.add_argument("--device", type=str, default=None, help="cuda|cpu (auto if not set)")
    ap.add_argument("--save_csv", type=str, default="", help="Optional path to save predictions as CSV")
    ap.add_argument("--no_meta", action="store_true", help="Disable reading hyperparameters from checkpoint meta")
    ap.add_argument("--chs", type=str, default="", help="Channel selection hyperparameter: 'all' or comma-separated 1-based indices (e.g., '1,2,3,4'). Empty uses meta or legacy Fp selection")
    return ap.parse_args()


def main():
    args = parse_args()
    try:
        code = infer(args)
        raise SystemExit(code)
    except KeyboardInterrupt:
        print("\nInterrupted. Exiting.")
        raise SystemExit(130)


if __name__ == "__main__":
    main()