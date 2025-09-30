#!/usr/bin/env python3
"""
Lightweight SSVEP stimulus window with 4 horizontal sections using only stdlib tkinter.
- No extra dependencies
- Frequencies and labels configurable via CLI
- Optional fullscreen
- Optional auto-exit after N seconds (so it pairs with recording duration)

Examples:
  python3 bci/scripts/ssvep/stimulus.py --freqs 15,12,10,7.5 --labels left,right,up,down --fullscreen --seconds 300
  python3 bci/scripts/ssvep/stimulus.py --invert 1 --seconds 120

Controls:
  f: toggle fullscreen
  space: pause/resume flicker
  h: toggle labels
  q or Esc: quit
"""
from __future__ import annotations

import argparse
import time
import math
import tkinter as tk
from typing import List


def parse_number_list(s: str | None, default: List[float]) -> List[float]:
    if not s:
        return default
    out: List[float] = []
    for part in s.split(","):
        try:
            v = float(part.strip())
            if v > 0:
                out.append(v)
        except Exception:
            pass
    return out or default


def parse_string_list(s: str | None, default: List[str]) -> List[str]:
    if not s:
        return default
    out = [p.strip() for p in s.split(",") if p.strip()]
    return out or default


def main():
    ap = argparse.ArgumentParser(description="Simple 4-band SSVEP stimulus (tkinter)")
    ap.add_argument("--freqs", type=str, default="15,12,10,7.5", help="Comma-separated Hz for 4 sections (top→bottom)")
    ap.add_argument("--labels", type=str, default="left,right,up,down", help="Comma-separated labels for 4 sections")
    ap.add_argument("--invert", type=int, default=0, help="Invert colors (1=light bg)")
    ap.add_argument("--fullscreen", action="store_true", help="Start in fullscreen")
    ap.add_argument("--seconds", type=float, default=0.0, help="Auto-close after this many seconds (0=manual)")
    ap.add_argument("--mode", type=str, default="square", choices=["square", "sine"], help="Modulation: square (frame-locked) or sine (smooth)")
    ap.add_argument("--contrast", type=float, default=0.6, help="0..1 luminance modulation depth (lower = more subtle)")
    ap.add_argument("--duty", type=float, default=0.5, help="Square-wave duty cycle (0..1)")
    ap.add_argument("--gamma", type=float, default=2.2, help="Gamma for luminance->sRGB mapping")
    ap.add_argument("--block", type=float, default=5.0, help="Cued label block length (s) for highlighting")
    ap.add_argument("--countdown", type=float, default=3.0, help="Seconds to count down before first cue")
    ap.add_argument("--outline", type=float, default=0.10, help="Outline thickness as fraction of band height (e.g., 0.10)")
    args = ap.parse_args()

    freqs = parse_number_list(args.freqs, [15.0, 12.0, 10.0, 7.5])[:4]
    labels = parse_string_list(args.labels, ["left", "right", "up", "down"])[:4]
    while len(freqs) < 4:
        freqs.append(freqs[-1] if freqs else 10.0)
    while len(labels) < 4:
        labels.append(f"target {len(labels)+1}")

    root = tk.Tk()
    root.title("SSVEP Stimulus")
    if args.fullscreen:
        root.attributes("-fullscreen", True)
    else:
        root.geometry("1200x800")

    # Colors
    bg_base = "#000000" if not args.invert else "#ffffff"
    fg_base = "#ffffff" if not args.invert else "#000000"
    # Per-row highlight colors (top->bottom). Keep high contrast for both themes.
    if args.invert:
        highlight_colors = ["#00aa66", "#0077cc", "#cc9900", "#cc3333"]
    else:
        highlight_colors = ["#00ff88", "#00b7ff", "#ffd400", "#ff4d4d"]

    canvas = tk.Canvas(root, highlightthickness=0, bd=0, bg=bg_base)
    canvas.pack(fill=tk.BOTH, expand=True)

    # Timing and state
    periods = [1.0 / max(0.001, f) for f in freqs]
    half_periods = [p * args.duty for p in periods]  # for square duty use
    start_time = time.perf_counter()
    flicker_start_time = start_time + float(args.countdown)

    rects: List[int] = []
    outlines: List[int] = []
    texts: List[int] = []
    cue_text: int | None = None

    paused = False
    show_hud = True

    def layout():
        nonlocal cue_text
        canvas.delete("all")
        w = canvas.winfo_width()
        h = canvas.winfo_height()
        rects.clear()
        outlines.clear()
        texts.clear()
        row_h = h / 4.0
        for i in range(4):
            y0 = int(i * row_h)
            y1 = int((i + 1) * row_h)
            # Start from background color; tick() will modulate
            r = canvas.create_rectangle(0, y0, w, y1, fill=bg_base, outline="")
            rects.append(r)
            # Outline (for active cue highlight), color per band
            out = canvas.create_rectangle(
                2, y0 + 2, w - 2, y1 - 2,
                outline=highlight_colors[i],
                width=max(4, int(row_h * float(args.outline)))
            )
            outlines.append(out)
            canvas.itemconfigure(out, state='hidden')
            if show_hud:
                tx = canvas.create_text(
                    w // 2,
                    (y0 + y1) // 2,
                    text=f"{labels[i]}  {freqs[i]:.2f} Hz",
                    fill=fg_base,
                    font=("Helvetica", int(max(12, row_h * 0.12))),
                )
                texts.append(tx)
        # Top-center cue text (current target or countdown)
        cue_text = canvas.create_text(
            w // 2,
            int(h * 0.08),
            text="",
            fill="#00ff88",
            font=("Helvetica", int(max(14, h * 0.06))),
        )

    def srgb_gray(level: float) -> str:
        # level in [0,1]; apply gamma mapping to approximate perceptual uniformity
        level = max(0.0, min(1.0, level))
        v = int(round((level ** (1.0 / float(args.gamma))) * 255))
        return f"#{v:02x}{v:02x}{v:02x}"

    def tick():
        nonlocal paused
        now = time.perf_counter()
        if args.seconds and now - start_time >= float(args.seconds):
            root.destroy()
            return

        # Determine cue index based on countdown and block length
        cue_idx = None
        if not paused:
            if now < flicker_start_time:
                # Countdown
                remaining = max(0, int(round(flicker_start_time - now)))
                canvas.itemconfigure(cue_text, text=f"Get ready... {remaining}")
                # Hide outlines during countdown
                for out in outlines:
                    canvas.itemconfigure(out, state='hidden')
            else:
                elapsed = now - flicker_start_time
                if args.block > 0:
                    cue_idx = int(elapsed // float(args.block)) % 4
                # Show current cue
                if cue_text is not None:
                    label = labels[cue_idx] if cue_idx is not None else ""
                    canvas.itemconfigure(cue_text, text=f"Gaze: {label}")
                    if cue_idx is not None:
                        canvas.itemconfigure(cue_text, fill=highlight_colors[cue_idx])
                for i, out in enumerate(outlines):
                    canvas.itemconfigure(
                        out,
                        state=('normal' if cue_idx is not None and i == cue_idx else 'hidden')
                    )

        # Update flicker colors
        if not paused and rects:
            t0 = flicker_start_time if now >= flicker_start_time else now
            for i in range(4):
                f = freqs[i]
                if args.mode == 'sine':
                    # Smooth sine modulation 0..1
                    m = 0.5 * (1.0 + math.sin(2.0 * math.pi * f * (now - t0)))
                else:
                    # Square with duty cycle
                    phase = (now - t0) % (1.0 / f)
                    m = 1.0 if phase < (args.duty * (1.0 / f)) else 0.0
                # Map to subtle contrast around background
                if args.invert:
                    gray = 1.0 - (args.contrast * m)
                else:
                    gray = args.contrast * m
                canvas.itemconfig(rects[i], fill=srgb_gray(gray))
        root.after(16, tick)

    def on_key(event):
        nonlocal paused, show_hud
        k = event.keysym.lower()
        if k in ("escape", "q"):
            root.destroy()
        elif k == "f":
            root.attributes("-fullscreen", not bool(root.attributes("-fullscreen")))
        elif k == "space":
            paused = not paused
        elif k == "h":
            show_hud = not show_hud
            layout()

    root.bind("<Configure>", lambda e: layout())
    root.bind("<Key>", on_key)

    layout()
    root.after(16, tick)
    root.mainloop()


if __name__ == "__main__":
    main()

