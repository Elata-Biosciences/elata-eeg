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

          {activeView === 'appletBrainWaves' &&
            fullFftPacket &&
            fullFftPacket.psd_packets && (
              <FftRenderer
                data={fullFftPacket}
                isActive={activeView === 'appletBrainWaves'}
                containerWidth={containerSize.width}
                containerHeight={containerSize.height}
              />
            )}
        </>
      ) : (
        <div className="absolute inset-0 flex items-center justify-center text-gray-400">
            Initializing...
        </div>
      )}
    </div>
  );
}