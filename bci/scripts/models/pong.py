#!/usr/bin/env python3
"""
pong.py — Stream EEG inference and control a Socket.IO-based Pong server.

This adapter reuses the inference pipeline from infer.py and emits game inputs:
  1) Connects to Socket.IO server (BASE + namespace /game)
  2) Emits 'join' once: { roomId, name }
  3) Emits 'input' events per prediction:
     - Directional: { roomId, dir: 'up'|'down', step? }
     - Absolute:    { roomId, paddleY }  // 0=top, 1=bottom

Usage example:
  python bci/scripts/models/pong.py \\
    --file bci/scripts/models/gpt1.py \\
    --mode stream \\
    --host raspberrypi.local --topic eeg_voltage --epoch 1 \\
    --weights bci/scripts/blinknet.pt \\
    --fp1_idx 0 --fp2_idx 1 --smooth 7 \\
    --game_url http://localhost:3000 --game_room arena-1 \\
    --game_mode directional --game_step 0.05 --game_rate_hz 10

Dependencies:
  pip install "python-socketio[asyncio_client]"
"""

from __future__ import annotations

import argparse
import asyncio
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import numpy as np
import torch

try:
    import socketio  # python-socketio
except ImportError as e:
    print("Missing dependency: python-socketio. Install with: pip install \"python-socketio[asyncio_client]\"")
    raise

# Reuse the existing inference helpers and stream state from infer.py (same folder)
# When executed as a file, Python adds this file's directory to sys.path, so plain import works.
try:
    from infer import (  # type: ignore
        load_model_and_spec,
        ensure_ws_client_on_path,
        StreamState,
        build_model,
    )
except Exception as e:
    print(f"Failed to import from infer.py in the same directory: {e}")
    raise


# -------------------- Game control mapping --------------------

@dataclass
class ControlConfig:
    mode: str  # 'directional'|'absolute'
    step: float
    rate_hz: float
    room: str
    name: str
    namespace: str
    base_url: str


class GameController:
    """Encapsulates Socket.IO client and emit logic."""

    def __init__(self, cfg: ControlConfig):
        self.cfg = cfg
        self.sio = socketio.AsyncClient(
            reconnection=True,
            reconnection_attempts=0,  # infinite
            reconnection_delay=1.0,
            reconnection_delay_max=5.0,
            logger=False,
            engineio_logger=False,
        )
        self._connected = False
        self._last_emit_ts = 0.0
        self._min_period = 0.0 if cfg.rate_hz <= 0 else 1.0 / float(cfg.rate_hz)
        self._paddle_y = 0.5  # for absolute mode, start centered

        # Register simple handlers (optional)
        @self.sio.event(namespace=self.cfg.namespace)
        async def connect():
            self._connected = True
            print(f"[game] connected to {self.cfg.base_url}{self.cfg.namespace}")
            try:
                def _on_join_ack(data):
                    print(f"[game] join ack: {data}")
                await self.sio.emit(
                    "join",
                    {"roomId": self.cfg.room, "name": self.cfg.name},
                    namespace=self.cfg.namespace,
                    callback=_on_join_ack,
                )
                print(f"[game] join sent: roomId={self.cfg.room}, name={self.cfg.name}")
            except Exception as e:
                print(f"[game] join error: {e}")

        @self.sio.event(namespace=self.cfg.namespace)
        async def disconnect():
            self._connected = False
            print("[game] disconnected")

        # Connection error handler (namespace-specific)
        @self.sio.event(namespace=self.cfg.namespace)
        async def connect_error(data):
            print(f"[game] connect_error on {self.cfg.namespace}: {data}")

        # Optional: observe server state for debugging
        @self.sio.on("state", namespace=self.cfg.namespace)
        async def on_state(s):
            # s = { paddles: {left,right}, ball: {x,y} }
            # Keep minimal logging to avoid spam; uncomment for debug.
            # print(f"[game] state: {s}")
            return

    async def connect(self):
        # Force WebSocket transport and pre-connect to the /game namespace.
        # IMPORTANT: Do not append '/game' to the BASE URL; keep URL as http://host:port and use namespace.
        await self.sio.connect(
            self.cfg.base_url,
            transports=["websocket"],
            namespaces=[self.cfg.namespace],
        )
        # Wait briefly for namespace-level connect and join emit to run
        await asyncio.sleep(0.1)

    async def disconnect(self):
        try:
            await self.sio.disconnect()
        except Exception:
            pass

    def _rate_ok(self) -> bool:
        if self._min_period <= 0:
            return True
        now = time.time()
        if now - self._last_emit_ts < self._min_period:
            return False
        self._last_emit_ts = now
        return True

    async def emit_directional(self, direction: Optional[str]):
        """Emit a directional step if allowed by rate limiter."""
        if not self._connected or not direction or direction not in ("up", "down"):
            return
        if not self._rate_ok():
            return
        payload = {"roomId": self.cfg.room, "dir": direction}
        # Include step only if positive; server defaults ~0.04 otherwise.
        if self.cfg.step > 0:
            payload["step"] = float(self.cfg.step)
        try:
            await self.sio.emit("input", payload, namespace=self.cfg.namespace)
            # print(f"[game] emit directional: {payload}")
        except Exception as e:
            print(f"[game] emit directional error: {e}")

    async def emit_absolute(self, direction: Optional[str]):
        """Integrate direction into [0,1] paddleY and emit absolute position."""
        if not self._connected or not direction:
            return
        # Update local y
        dy = float(self.cfg.step) if self.cfg.step > 0 else 0.04
        if direction == "up":
            self._paddle_y = max(0.0, self._paddle_y - dy)
        elif direction == "down":
            self._paddle_y = min(1.0, self._paddle_y + dy)
        else:
            return  # nothing to do
        if not self._rate_ok():
            return
        payload = {"roomId": self.cfg.room, "paddleY": float(self._paddle_y)}
        try:
            await self.sio.emit("input", payload, namespace=self.cfg.namespace)
            # print(f"[game] emit absolute: {payload}")
        except Exception as e:
            print(f"[game] emit absolute error: {e}")

    async def send_control(self, direction: Optional[str]):
        if self.cfg.mode == "absolute":
            await self.emit_absolute(direction)
        else:
            await self.emit_directional(direction)


# -------------------- EEG → Prediction → Control --------------------

def guess_class_mapping(meta: Dict[str, Any], n_classes: int) -> Dict[int, str]:
    """
    Returns a mapping from class index to 'up'/'down'/None.
    Heuristic:
      - If meta['classes'] exists, map by substring match.
      - Else assume index 1 = 'up', index 2 = 'down' for 3-class model [neutral, up, down].
    """
    clmap: Dict[int, str] = {}
    classes = []
    try:
        classes = [str(x).lower() for x in (meta.get("classes") or [])]
    except Exception:
        classes = []
    if classes:
        for i, name in enumerate(classes):
            if "up" in name:
                clmap[i] = "up"
            elif "down" in name:
                clmap[i] = "down"
    if not clmap:
        # Fallback heuristic
        if n_classes >= 3:
            clmap = {1: "up", 2: "down"}
        elif n_classes == 2:
            clmap = {1: "down"}  # 0=up? This is ambiguous; users can override later if needed.
    return clmap


async def run_stream_to_game(args):
    model_file = Path(args.file)
    mod, spec = load_model_and_spec(model_file)
    ensure_ws_client_on_path(model_file)
    from ws_client import start_data_ws  # type: ignore

    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    model, n_classes, meta = build_model(Path(args.weights), spec["model_class"], device)
    if meta:
        print(f"[weights] meta: fs={meta.get('fs','?')}Hz window_s={meta.get('window_s','?')} hop_s={meta.get('hop_s','?')} band={meta.get('bandpass','?')} classes={meta.get('classes','?')}")

    # Build stream state (same as infer.py)
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

    # Prepare game controller
    gcfg = ControlConfig(
        mode=args.game_mode,
        step=float(args.game_step),
        rate_hz=float(args.game_rate_hz),
        room=args.game_room,
        name=args.game_name,
        namespace=args.game_ns,
        base_url=args.game_url,
    )
    game = GameController(gcfg)
    class_map = guess_class_mapping(getattr(model, "_meta", {}) or {}, n_classes)
    print(f"[game] connect: {gcfg.base_url}{gcfg.namespace} | room={gcfg.room} | mode={gcfg.mode} | step={gcfg.step} | rate={gcfg.rate_hz}Hz")
    await game.connect()

    last_print_ts = 0.0

    async def on_packet(header: Dict[str, Any], samples: np.ndarray, meta_in: Optional[Dict[str, Any]]):
        nonlocal last_print_ts
        state.on_meta(header, meta_in, samples)
        state.append(samples)

        while True:
            pair = state.next_window()
            if pair is None:
                break
            fp1, fp2 = pair
            pred, probs = state.infer_window(fp1, fp2)
            smoothed = state.smooth(pred)

            # Map to control direction
            direction: Optional[str] = class_map.get(smoothed)
            # Send control (rate-limited inside)
            await game.send_control(direction)

            # Throttled debug print
            now = time.time()
            if now - last_print_ts > max(0.01, state.hop_len / max(1.0, state.fs) * 0.5):
                print(
                    f"[pred] raw={pred} smoothed={smoothed} dir={direction} "
                    f"probs={[round(float(x), 3) for x in probs.tolist()]} "
                    f"| fs={state.fs:.1f}Hz win={state.window_len} hop={state.hop_len}"
                )
                last_print_ts = now

    try:
        print(
            f"🚀 Streaming EEG → Pong: host={args.host} topic={args.topic} epoch={args.epoch} "
            f"| weights={args.weights} | device={device} | classes=auto"
        )
        await start_data_ws(args.host, [(args.topic, int(args.epoch))], on_packet)
    except asyncio.CancelledError:
        pass
    finally:
        await game.disconnect()


def parse_args():
    ap = argparse.ArgumentParser(description="Stream EEG inference and control a Socket.IO Pong server")
    ap.add_argument("--file", type=str, default=str(Path(__file__).resolve().parent / "gpt1.py"), help="Path to model pipeline file (e.g., gpt1.py)")
    ap.add_argument("--mode", type=str, choices=["stream"], default="stream", help="Inference mode (stream only)")
    ap.add_argument("--weights", type=str, default=str(Path(__file__).resolve().parents[1] / "blinknet.pt"), help="Path to model weights .pt")
    ap.add_argument("--device", type=str, default=None, help="cuda|cpu (auto if not set)")

    # Stream source
    ap.add_argument("--host", type=str, default="raspberrypi.local")
    ap.add_argument("--topic", type=str, default="eeg_voltage")
    ap.add_argument("--epoch", type=int, default=1)
    ap.add_argument("--window_s", type=float, default=1.0)
    ap.add_argument("--hop_s", type=float, default=0.25)
    ap.add_argument("--fp1_idx", type=int, default=-1, help="Override index for Fp1 channel (0-based); -1=auto")
    ap.add_argument("--fp2_idx", type=int, default=-1, help="Override index for Fp2 channel (0-based); -1=auto")
    ap.add_argument("--smooth", type=int, default=7, help="Majority vote over last K predictions (0=off)")

    # Game target
    ap.add_argument("--game_url", type=str, default="http://localhost:3000", help="Game server base URL")
    ap.add_argument("--game_ns", type=str, default="/game", help="Socket.IO namespace")
    ap.add_argument("--game_room", type=str, default="arena-1", help="Room ID")
    ap.add_argument("--game_name", type=str, default="EEG", help="Player name")
    ap.add_argument("--game_mode", type=str, choices=["directional", "absolute"], default="directional", help="Control mode")
    ap.add_argument("--game_step", type=float, default=0.05, help="Step for up/down or absolute integration")
    ap.add_argument("--game_rate_hz", type=float, default=10.0, help="Max emit rate to the game server")

    return ap.parse_args()


def main():
    args = parse_args()
    if args.mode != "stream":
        print("Only stream mode is supported in pong.py.")
        sys.exit(1)
    try:
        asyncio.run(run_stream_to_game(args))
    except KeyboardInterrupt:
        print("\nInterrupted. Exiting.")


if __name__ == "__main__":
    main()