"use client";

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useSearchParams } from "next/navigation";

function parseNumberList(s: string | null, def: number[]): number[] {
  if (!s) return def;
  const arr = s
    .split(",")
    .map((x) => parseFloat(x.trim()))
    .filter((x) => Number.isFinite(x) && x > 0);
  return arr.length > 0 ? arr : def;
}

function parseStringList(s: string | null, def: string[]): string[] {
  if (!s) return def;
  const arr = s
    .split(",")
    .map((x) => x.trim())
    .filter((x) => x.length > 0);
  return arr.length > 0 ? arr : def;
}

export default function SsvEpFlickerPage() {
  const params = useSearchParams();

  // Defaults designed for a 60 Hz display. Choose even divisors where possible for square-wave stability.
  const defaultFreqs = useMemo(() => [15, 12, 10, 7.5], []); // Hz (60/4, 60/5, 60/6, 60/8)
  const defaultLabels = useMemo(() => ["left", "right", "up", "down"], []);

  const freqs = useMemo(
    () => parseNumberList(params.get("freqs"), defaultFreqs).slice(0, 4),
    [params, defaultFreqs]
  );
  const labels = useMemo(
    () => parseStringList(params.get("labels"), defaultLabels).slice(0, 4),
    [params, defaultLabels]
  );
  const invert = (params.get("invert") ?? "0") !== "0"; // invert colors (start bright vs dark)

  // Internal flicker state per section
  type SecState = { on: boolean; lastSwitchMs: number; halfPeriodMs: number };
  const [running, setRunning] = useState(true);
  const [showHud, setShowHud] = useState(true);
  const secRef = useRef<SecState[]>([]);
  const rafRef = useRef<number | null>(null);

  // Initialize per-section timing derived from frequencies
  useEffect(() => {
    const now = performance.now();
    secRef.current = freqs.map((f, i) => ({
      on: invert ? true : false,
      lastSwitchMs: now,
      halfPeriodMs: 500.0 / Math.max(0.001, f), // half period in ms (1000/f / 2)
    }));
  }, [freqs, invert]);

  const animate = useCallback((t: number) => {
    if (!running) return;
    const arr = secRef.current;
    for (let i = 0; i < arr.length; i++) {
      const st = arr[i];
      const due = st.lastSwitchMs + st.halfPeriodMs;
      if (t >= due) {
        // Catch up if frames were dropped
        const missed = Math.floor((t - st.lastSwitchMs) / st.halfPeriodMs);
        st.lastSwitchMs += missed * st.halfPeriodMs;
        st.on = !st.on;
      }
    }
    rafRef.current = requestAnimationFrame(animate);
  }, [running]);

  useEffect(() => {
    if (rafRef.current) cancelAnimationFrame(rafRef.current);
    if (running) rafRef.current = requestAnimationFrame(animate);
    return () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    };
  }, [animate, running]);

  // Fullscreen helper
  const requestFullscreen = useCallback(() => {
    const el = document.documentElement as any;
    if (el.requestFullscreen) el.requestFullscreen();
  }, []);

  // Keyboard shortcuts: f=fullscreen, space=pause, h=toggle HUD
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "f") requestFullscreen();
      else if (e.key === " ") setRunning((r) => !r);
      else if (e.key === "h") setShowHud((s) => !s);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [requestFullscreen]);

  // Render 4 horizontal sections (top→bottom)
  const rows = 4;
  const sections = new Array(rows).fill(0).map((_, i) => i);

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: invert ? "#fff" : "#000",
        color: invert ? "#000" : "#fff",
        overflow: "hidden",
        fontFamily: "system-ui, -apple-system, Segoe UI, Roboto, Helvetica, Arial, sans-serif",
      }}
    >
      {sections.map((i) => {
        const st = secRef.current[i] || { on: false } as SecState;
        const isOn = !!st.on;
        const bg = isOn ? (invert ? "#000" : "#fff") : (invert ? "#fff" : "#000");
        const heightPct = 100 / rows;
        return (
          <div
            key={i}
            style={{
              position: "absolute",
              left: 0,
              right: 0,
              top: `${i * heightPct}%`,
              height: `${heightPct}%`,
              backgroundColor: bg,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              transition: "background-color 0.0s linear",
              borderTop: i === 0 ? "none" : `1px solid ${invert ? "#ddd" : "#222"}`,
              borderBottom: i === rows - 1 ? "none" : `1px solid ${invert ? "#ddd" : "#222"}`,
            }}
          >
            {showHud && (
              <div
                style={{
                  padding: "8px 12px",
                  borderRadius: 8,
                  background: isOn ? (invert ? "rgba(255,255,255,0.6)" : "rgba(0,0,0,0.6)") : (invert ? "rgba(0,0,0,0.6)" : "rgba(255,255,255,0.6)"),
                  color: isOn ? (invert ? "#000" : "#fff") : (invert ? "#fff" : "#000"),
                  fontSize: 24,
                  letterSpacing: 0.5,
                  textTransform: "uppercase",
                  display: "flex",
                  gap: 12,
                }}
              >
                <span>{labels[i] ?? `target ${i + 1}`}</span>
                <span style={{ opacity: 0.8 }}>{freqs[i]?.toFixed(2)} Hz</span>
              </div>
            )}
          </div>
        );
      })}

      {showHud && (
        <div
          style={{
            position: "absolute",
            left: 12,
            bottom: 10,
            padding: "6px 10px",
            borderRadius: 8,
            background: "rgba(0,0,0,0.4)",
            color: "#fff",
            fontSize: 12,
          }}
        >
          <div><strong>SSVEP Flicker</strong> — 4 horizontal sections</div>
          <div>Keys: f=fullscreen, space=pause, h=labels</div>
          <div>Query params: freqs=15,12,10,7.5 labels=left,right,up,down invert=1</div>
          <div>Tip: Use a 60 Hz display for stable timing.</div>
        </div>
      )}
    </div>
  );
}

