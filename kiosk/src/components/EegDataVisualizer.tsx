'use client';
import React, { useRef, useState, useEffect, useLayoutEffect } from 'react';
import { EegRenderer } from './EegRenderer';
import { FftRenderer } from '../../../plugins/brain_waves_fft/ui/FftRenderer';
import { useEegData, useEegDynamicData } from '../context/EegDataContext';
import { useDataBuffer } from '../hooks/useDataBuffer';
import { SampleChunk } from '../types/eeg';

type DataView = 'signalGraph' | 'appletBrainWaves';

interface EegDataVisualizerProps {
  activeView: DataView;
  config: any; // Consider defining a more specific type for config
  uiVoltageScaleFactor: number;
}

export default function EegDataVisualizer({ activeView, config, uiVoltageScaleFactor }: EegDataVisualizerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [viewReadyState, setViewReadyState] = useState({ signalGraph: false, appletBrainWaves: false });
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const signalGraphBuffer = useDataBuffer<SampleChunk>(1000);

  const { subscribeRaw } = useEegData();
  const { fftData, fullFftPacket } = useEegDynamicData();

  // Local FFT fallback when backend FFT packets are not available
  const [localFft, setLocalFft] = useState<any | null>(null);
  const perChannelRef = useRef<number[][]>([]);
  const sampleRateRef = useRef<number>(250);


  // Track a fallback channel list derived from incoming data when config is not yet available
  const [fallbackChannels, setFallbackChannels] = useState<number[] | null>(null);

  // Effect for managing raw data subscription for the signal graph
  useEffect(() => {
    let unsubscribe: (() => void) | null = null;

    if (activeView === 'signalGraph') {
      console.log('[Visualizer] Subscribing to raw data for signalGraph.');
      // Clear previous data to ensure a fresh start
      signalGraphBuffer.clear();

      unsubscribe = subscribeRaw((newSampleChunks) => {
        if (newSampleChunks.length > 0) {
          // Derive channel count from metadata of the newest chunk
          const lastChunk = newSampleChunks[newSampleChunks.length - 1];
          const n = lastChunk?.meta?.channel_names?.length;
          if (!config?.channels?.length && typeof n === 'number' && n > 0) {
            setFallbackChannels(Array.from({ length: n }, (_, i) => i));
          }
          signalGraphBuffer.addData(newSampleChunks);
        }
      });
    }

    // Cleanup function to unsubscribe when the component unmounts or dependencies change
    return () => {
      if (unsubscribe) {
        console.log('[Visualizer] Unsubscribing from raw data for signalGraph.');
        unsubscribe();
      }
    };
  }, [activeView, subscribeRaw, config?.channels]);

  // Effect for managing FFT data subscription
  useEffect(() => {
    if (activeView === 'appletBrainWaves') {
      console.log('[Visualizer] View is appletBrainWaves, FFT data is handled by EegDataContext.');
      setViewReadyState(s => ({ ...s, appletBrainWaves: true }));
    }
  }, [activeView]);
  // When FFT view is active but backend FFT is absent, build a local FFT fallback from raw samples
  useEffect(() => {
    if (activeView !== 'appletBrainWaves') return;

    let unsubscribe: (() => void) | null = null;
    perChannelRef.current = [];

    unsubscribe = subscribeRaw((newSampleChunks) => {
      if (!newSampleChunks || newSampleChunks.length === 0) return;
      const lastChunk = newSampleChunks[newSampleChunks.length - 1] as any;
      const n = lastChunk?.meta?.channel_names?.length;
      const sr = lastChunk?.meta?.sample_rate;
      if (typeof sr === 'number' && sr > 0) sampleRateRef.current = sr;
      if (typeof n === 'number' && n > 0) {
        if (perChannelRef.current.length !== n) {
          perChannelRef.current = Array.from({ length: n }, () => [] as number[]);
        }
        for (const chunk of newSampleChunks) {
          const channels = (chunk as any)?.meta?.channel_names?.length ?? n;
          const samples = (chunk as any)?.samples as Float32Array | Int32Array;
          if (!samples || samples.length === 0 || !channels) continue;
          const frames = Math.floor(samples.length / channels);
          for (let f = 0; f < frames; f++) {
            for (let ch = 0; ch < channels; ch++) {
              const v = samples[f * channels + ch] as number;
              perChannelRef.current[ch].push(typeof v === 'number' ? v : Number(v));
            }
          }
        }
        // Cap length
        for (let ch = 0; ch < perChannelRef.current.length; ch++) {
          const arr = perChannelRef.current[ch];
          if (arr.length > 4096) perChannelRef.current[ch] = arr.slice(arr.length - 4096);
        }
      }
    });

    return () => {
      if (unsubscribe) unsubscribe();
    };
  }, [activeView, subscribeRaw]);

  // Periodically compute local FFT if backend FFT is not available
  useEffect(() => {
    if (activeView !== 'appletBrainWaves') return;

    const N = 512;
    const timer = setInterval(() => {
      if (fullFftPacket && (fullFftPacket as any).psd_packets) {
        if (localFft !== null) setLocalFft(null);
        return;
      }
      const buffers = perChannelRef.current;
      if (!buffers || buffers.length === 0) return;
      const channels = buffers.length;
      const sr = sampleRateRef.current || 250;

      const psd_packets: { channel: number; psd: number[] }[] = [];
      for (let ch = 0; ch < channels; ch++) {
        const arr = buffers[ch];
        if (!arr || arr.length < N) continue;
        const segment = arr.slice(arr.length - N);
        // Hann window
        const windowed = segment.map((v, i) => v * (0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (N - 1))));
        const half = Math.floor(N / 2);
        const psd: number[] = new Array(half).fill(0);
        for (let k = 0; k < half; k++) {
          let re = 0, im = 0;
          const ang = -2 * Math.PI * k / N;
          for (let n = 0; n < N; n++) {
            const theta = ang * n;
            const w = windowed[n];
            re += w * Math.cos(theta);
            im += w * Math.sin(theta);
          }
          const mag2 = (re * re + im * im) / N;
          psd[k] = Math.log10(mag2 + 1e-9);
        }
        psd_packets.push({ channel: ch, psd });
      }

      if (psd_packets.length > 0) {
        setLocalFft({
          psd_packets,
          fft_config: { fft_size: N, sample_rate: sr, window_function: 'hann' },
        });
      }
    }, 700);

    return () => clearInterval(timer);
  }, [activeView, fullFftPacket, localFft]);

  // Effect to setup ResizeObserver
  useLayoutEffect(() => {
    const target = containerRef.current;
    if (!target) return;

    const resizeObserver = new ResizeObserver(entries => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        setContainerSize({ width, height });
      }
    });

    resizeObserver.observe(target);

    // Set initial size
    setContainerSize({
        width: target.offsetWidth,
        height: target.offsetHeight,
    });

    return () => resizeObserver.disconnect();
  }, []);

  return (
    <div ref={containerRef} className="w-full h-full relative bg-gray-950">
      {containerSize.width > 0 && containerSize.height > 0 ? (
        <>
          {activeView === 'signalGraph' && (() => {
            const effectiveChannels = (config?.channels && Array.isArray(config.channels) && config.channels.length > 0)
              ? config.channels
              : (fallbackChannels && fallbackChannels.length > 0 ? fallbackChannels : null);
            if (!effectiveChannels) {
              return (
                <div className="absolute inset-0 flex items-center justify-center text-gray-400">
                  Waiting for channel configuration...
                </div>
              );
            }
            const renderConfig = (config?.channels && config.channels.length > 0)
              ? config
              : { channels: effectiveChannels };
            return (
              <div className="relative h-full min-h-[300px]">
                <EegRenderer
                  key={effectiveChannels.join(',')}
                  isActive={activeView === 'signalGraph'}
                  config={renderConfig as any}
                  dataBuffer={signalGraphBuffer}
                  width={containerSize.width}
                  height={containerSize.height}
                  uiVoltageScaleFactor={uiVoltageScaleFactor}
                />
              </div>
            );
          })()}

          {activeView === 'appletBrainWaves' && (() => {
            const fftForRender = fullFftPacket && (fullFftPacket as any).psd_packets ? fullFftPacket : localFft;
            if (!fftForRender) {
              return (
                <div className="absolute inset-0 flex items-center justify-center text-gray-400">
                  No FFT data available yet...
                </div>
              );
            }
            return (
              <FftRenderer
                data={fftForRender as any}
                isActive={activeView === 'appletBrainWaves'}
                containerWidth={containerSize.width}
                containerHeight={containerSize.height}
              />
            );
          })()}
        </>
      ) : (
        <div className="absolute inset-0 flex items-center justify-center text-gray-400">
            Initializing...
        </div>
      )}
    </div>
  );
}