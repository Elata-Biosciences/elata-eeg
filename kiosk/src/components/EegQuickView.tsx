'use client';

import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useEegData, useEegStatus, useEegDynamicData } from '@/context/EegDataContext';

function Sparkline({ series, width = 120, height = 32 }: { series: number[]; width?: number; height?: number }) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, width, height);
    if (!series || series.length < 2) return;

    // Compute min/max for scaling
    let min = Infinity, max = -Infinity;
    for (const v of series) { if (v < min) min = v; if (v > max) max = v; }
    if (min === Infinity || max === -Infinity) return;
    if (min === max) { min -= 1; max += 1; }

    const n = series.length;
    ctx.strokeStyle = '#0ea5e9';
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let i = 0; i < n; i++) {
      const x = (i / (n - 1)) * (width - 1);
      const y = height - 1 - ((series[i] - min) / (max - min)) * (height - 1);
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.stroke();
  }, [series, width, height]);
  return <canvas ref={canvasRef} width={width} height={height} style={{ display: 'block' }} />;
}

function Toolbar() {
  const [, setBump] = useState(0);
  const setAssumed = (n: number | null) => {
    // @ts-ignore
    (window as any).__eeg_assumed_channels = n;
    setBump(x => x + 1);
  };
  const toggleHeaders = () => {
    // @ts-ignore
    (window as any).__eeg_debug_headers = !(window as any).__eeg_debug_headers;
    setBump(x => x + 1);
  };
  // @ts-ignore
  const assumed = (typeof window !== 'undefined') ? (window as any).__eeg_assumed_channels ?? null : null;
  // @ts-ignore
  const debugOn = (typeof window !== 'undefined') ? !!(window as any).__eeg_debug_headers : false;
  return (
    <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 8 }}>
      <div style={{ fontWeight: 600 }}>Assume Channels:</div>
      <button onClick={() => setAssumed(null)} style={{ padding: '2px 8px', background: assumed==null? '#e2e8f0':'#f1f5f9' }}>Auto</button>
      <button onClick={() => setAssumed(4)} style={{ padding: '2px 8px', background: assumed===4? '#e2e8f0':'#f1f5f9' }}>4</button>
      <button onClick={() => setAssumed(8)} style={{ padding: '2px 8px', background: assumed===8? '#e2e8f0':'#f1f5f9' }}>8</button>
      <div style={{ width: 1, background: '#cbd5e1', height: 18 }} />
      <button onClick={toggleHeaders} style={{ padding: '2px 8px', background: debugOn? '#fde68a':'#f1f5f9' }}>Header Logs {debugOn? 'ON':'OFF'}</button>
    </div>
  );
}

export default function EegQuickView() {
  const { subscribeRaw, getRawSamples } = useEegData();
  const { dataStatus } = useEegStatus();
  const { fullFftPacket } = useEegDynamicData();

  const [chunkCount, setChunkCount] = useState(0);
  const [lastTs, setLastTs] = useState<number | null>(null);
  const [latestPerChan, setLatestPerChan] = useState<number[]>([]);
  const [seriesPerChan, setSeriesPerChan] = useState<number[][]>([]);

  // Accumulate per-channel latest and tiny history from incoming chunks
  useEffect(() => {
    const unsub = subscribeRaw((chunks) => {
      const last = chunks[chunks.length - 1];
      const values = Array.from(last.samples);
      const numCh = last.meta?.channel_names?.length ?? 0;
      if (!numCh || values.length < numCh) {
        setChunkCount((c) => c + chunks.length);
        setLastTs(last.timestamp);
        return;
      }

      const groups = Math.floor(values.length / numCh);
      const startGroup = Math.max(0, groups - 64); // last 64 sample-frames per channel

      // Prepare series
      setSeriesPerChan((prev) => {
        const next: number[][] = Array.from({ length: numCh }, (_, i) => prev[i] ? [...prev[i]] : []);
        for (let g = startGroup; g < groups; g++) {
          const base = g * numCh;
          for (let ch = 0; ch < numCh; ch++) {
            const v = values[base + ch];
            const arr = next[ch];
            arr.push(v);
            if (arr.length > 128) arr.shift();
          }
        }
        return next;
      });

      // Latest per channel from last group
      const lastBase = (groups - 1) * numCh;
      const latest = new Array(numCh).fill(0).map((_, ch) => values[lastBase + ch]);
      setLatestPerChan(latest);

      setChunkCount((c) => c + chunks.length);
      setLastTs(last.timestamp);
    });
    return () => unsub();
  }, [subscribeRaw]);

  const latestChunk = useMemo(() => getRawSamples().slice(-1)[0], [getRawSamples, chunkCount]);
  const channelNames = useMemo(() => latestChunk?.meta?.channel_names ?? [], [latestChunk]);

  return (
    <div style={{ fontFamily: 'monospace', padding: 12 }}>
      <h2 style={{ marginBottom: 8 }}>EEG Quick View</h2>
      <Toolbar />
      <div style={{ display: 'flex', gap: 24, flexWrap: 'wrap', marginBottom: 8 }}>
        <div>WS: {dataStatus.wsStatus}</div>
        <div>Chunks: {chunkCount}</div>
        <div>Last ts: {lastTs ?? '-'}</div>
        <div>Channels: {channelNames.length || '-'}</div>
      </div>

      {/* Per-channel table with sparklines */}
      {channelNames.length > 0 ? (
        <div style={{ borderTop: '1px solid #ddd', paddingTop: 8 }}>
          <div style={{ display: 'grid', gridTemplateColumns: 'auto 1fr auto', gap: 8, alignItems: 'center' }}>
            {channelNames.map((name: string, idx: number) => (
              <React.Fragment key={idx}>
                <div style={{ color: '#334155' }}>{name}</div>
                <div>
                  <Sparkline series={seriesPerChan[idx] || []} />
                </div>
                <div style={{ textAlign: 'right', width: 120 }}>
                  {Number.isFinite(latestPerChan[idx]) ? latestPerChan[idx].toFixed(7) : '-'}
                </div>
              </React.Fragment>
            ))}
          </div>
        </div>
      ) : (
        <div style={{ color: '#64748b' }}>Waiting for channel metadata... streaming will appear once metadata/config arrives.</div>
      )}

      {/* Raw tail debug */}
      <div style={{ marginTop: 12 }}>
        <div style={{ marginBottom: 4, color: '#334155' }}>Last chunk tail (flat):</div>
        <pre style={{ whiteSpace: 'pre-wrap', margin: 0 }}>{JSON.stringify(Array.from(latestChunk?.samples || []).slice(-32))}</pre>
      </div>

      {fullFftPacket && (
        <div style={{ marginTop: 8, color: '#334155' }}>
          FFT: fft_size={fullFftPacket.fft_config.fft_size} sr={fullFftPacket.fft_config.sample_rate}
        </div>
      )}
    </div>
  );
}

