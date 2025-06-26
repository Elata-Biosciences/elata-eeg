'use client';
import React, { useRef, useState, useEffect, useLayoutEffect } from 'react';
import { EegRenderer } from './EegRenderer';
import { FftRenderer } from '../../../plugins/brain_waves_fft/ui/FftRenderer';
import { useEegData } from '../context/EegDataContext';
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
  const [localDataVersion, setLocalDataVersion] = useState(0);

    const signalGraphBuffer = useDataBuffer<SampleChunk>();

  const { subscribeRaw, subscribe, unsubscribe, fftData, fullFftPacket } = useEegData();

  // Effect for all data subscriptions
  useEffect(() => {
    // Always clean up previous subscriptions on re-run
    let unsubRaw: (() => void) | null = null;
    let isSubscribedToFft = false;

    if (activeView === 'signalGraph') {
      const targetBuffer = signalGraphBuffer;
      console.log(`[Visualizer] Subscribing to raw data for ${activeView}.`);
      targetBuffer.clear();
      unsubRaw = subscribeRaw((newSampleChunks) => {
        if (newSampleChunks.length > 0) {
          targetBuffer.addData(newSampleChunks);
          setLocalDataVersion(v => v + 1);
        }
      });
    } else if (activeView === 'appletBrainWaves') {
      console.log('[Visualizer] Subscribing to Fft');
      subscribe(['Fft']);
      isSubscribedToFft = true;
      setViewReadyState(s => ({ ...s, appletBrainWaves: true }));
    }

    // Return a cleanup function that handles all cases
    return () => {
      if (unsubRaw) {
        console.log(`[Visualizer] Unsubscribing from raw data for view: ${activeView}`);
        unsubRaw();
      }
      if (isSubscribedToFft) {
        console.log('[Visualizer] Unsubscribing from Fft');
        unsubscribe(['Fft']);
      }
    };
  }, [activeView, subscribeRaw, unsubscribe, subscribe, signalGraphBuffer]);

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
          {activeView === 'signalGraph' && (
            !config || !config.channels || !Array.isArray(config.channels) || config.channels.length === 0 ? (
              <div className="absolute inset-0 flex items-center justify-center text-gray-400">
                Waiting for channel configuration...
              </div>
            ) : (
              <div className="relative h-full min-h-[300px]">
                <EegRenderer
                  isActive={activeView === 'signalGraph'}
                  config={config}
                  dataBuffer={signalGraphBuffer}
                  width={containerSize.width}
                  height={containerSize.height}
                  uiVoltageScaleFactor={uiVoltageScaleFactor}
                />
              </div>
            )
          )}

          {activeView === 'appletBrainWaves' && (
            <FftRenderer
              data={fullFftPacket as any}
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