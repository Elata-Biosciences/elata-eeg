'use client';

import React, { createContext, useContext, useState, ReactNode, useMemo, useRef, useCallback, useEffect } from 'react';
import { useEegDataHandler } from '../components/EegDataHandler';
import { useEegConfig } from '../components/EegConfig';

// Constants for data management
const MAX_SAMPLE_CHUNKS = 100;
const RECONNECTION_DATA_RETENTION_MS = 5000; // Keep data for 5 seconds during reconnections
import { EegSample, SampleChunk } from '../types/eeg'; // Import shared types

// Callback type for live data subscribers
type RawDataCallback = (data: SampleChunk[]) => void;

// Type for the full FFT data packet, matching the backend and FftDisplay component
export interface FftPacket {
  psd_packets: { channel: number; psd: number[] }[];
  fft_config: {
    fft_size: number;
    sample_rate: number;
    window_function: string;
  };
  timestamp: number;
  source_frame_id: number;
}

 // Define the shape of the context data
interface EegDataContextType {
  dataVersion: number; // Increments on new data
  getRawSamples: () => SampleChunk[]; // Function to get the current samples
  subscribeRaw: (callback: RawDataCallback) => () => void; // Returns an unsubscribe function
  fftData: Record<number, number[]>; // Latest FFT data per channel
  fullFftPacket: FftPacket | null; // The complete, most recent FFT packet
  config: any;
  dataStatus: {
    dataReceived: boolean;
    driverError: string | null;
    wsStatus: string;
    isReconnecting: boolean;
  };
  // Add methods for data management
  clearOldData: () => void;
  // Subscription management
  subscribe: (topics: string[]) => void;
  unsubscribe: (topics: string[]) => void;
}

// Create the context with a default value
const EegDataContext = createContext<EegDataContextType | undefined>(undefined);

// Define the props for the provider component
interface EegDataProviderProps {
  children: ReactNode;
}

export const EegDataProvider = ({ children }: EegDataProviderProps) => {
  const { config } = useEegConfig();
  const rawSamplesRef = useRef<SampleChunk[]>([]);
  const [dataVersion, setDataVersion] = useState(0);
  const [fftData, setFftData] = useState<Record<number, number[]>>({});
  const [fullFftPacket, setFullFftPacket] = useState<FftPacket | null>(null);
  const [dataReceived, setDataReceived] = useState(false);
  const [driverError, setDriverError] = useState<string | null>(null);
  const [isReconnecting, setIsReconnecting] = useState(false);
  const [subscriptions, setSubscriptions] = useState<string[]>([]);
  const rawDataSubscribersRef = useRef<Set<RawDataCallback>>(new Set());

  useEffect(() => {
    // Automatically subscribe to the raw EEG data topic when the provider mounts.
    // This is the primary data stream for the main graphs.
    subscribe(['FilteredEeg']);

    // No cleanup needed, subscriptions are managed by the component lifecycle.
  }, []); // Empty dependency array ensures this runs only once on mount.
  
  // Use refs to track data timestamps for cleanup
  const sampleTimestamps = useRef<number[]>([]);
  const lastCleanupTime = useRef<number>(Date.now());
  
  // Create stable refs for EegDataHandler to prevent unnecessary reconnections
  const lastDataChunkTimeRef = useRef<number[]>([]);
  const latestTimestampRef = useRef<number>(0);
  const debugInfoRef = useRef({ lastPacketTime: 0, packetsReceived: 0, samplesProcessed: 0 });

  const handleSamples = useCallback((channelSamples: { values: Float32Array; timestamps: BigUint64Array }[]) => {
    const now = Date.now();
    const currentChannelCount = config?.channels?.length || 1;
    const currentSampleRate = config?.sample_rate || 250;

    // Create one SampleChunk with all channel data combined, preserving temporal order
    const allSamples: EegSample[] = [];
    const batchSize = channelSamples[0]?.values.length || 0;
    
    // Reconstruct the original interleaved temporal order
    for (let timeIndex = 0; timeIndex < batchSize; timeIndex++) {
      for (let channelIndex = 0; channelIndex < channelSamples.length; channelIndex++) {
        const channelData = channelSamples[channelIndex];
        if (channelData && timeIndex < channelData.values.length) {
          allSamples.push({
            value: channelData.values[timeIndex],
            timestamp: channelData.timestamps[timeIndex],
            channelIndex: channelIndex,
          });
        }
      }
    }

    const newSampleChunk: SampleChunk = {
      config: {
        channelCount: currentChannelCount,
        sampleRate: currentSampleRate,
      },
      samples: allSamples,
    };

    const newSampleChunks: SampleChunk[] = [newSampleChunk];

    // Debug logging to verify channel assignment
    if (debugInfoRef.current.packetsReceived % 50 === 0) {
      const channelCounts = new Map<number, number>();
      allSamples.forEach(sample => {
        channelCounts.set(sample.channelIndex, (channelCounts.get(sample.channelIndex) || 0) + 1);
      });
      console.log(`[EegDataContext] Packet #${debugInfoRef.current.packetsReceived}: Channel distribution:`,
        Array.from(channelCounts.entries()).map(([ch, count]) => `Ch${ch}:${count}`).join(', '));
    }

    const newSamples = [...rawSamplesRef.current, ...newSampleChunks];
    sampleTimestamps.current.push(...Array(newSampleChunks.length).fill(now));

    // Enforce a hard limit on the number of chunks to prevent memory leaks
    if (newSamples.length > MAX_SAMPLE_CHUNKS) {
      const excess = newSamples.length - MAX_SAMPLE_CHUNKS;
      sampleTimestamps.current.splice(0, excess);
      rawSamplesRef.current = newSamples.slice(excess);
    } else {
      rawSamplesRef.current = newSamples;
    }
    
    setDataVersion(v => v + 1);
    
    // Publish the new data to all subscribers
    rawDataSubscribersRef.current.forEach(callback => callback(newSampleChunks));
    
    // Periodic cleanup of old data (every 10 seconds)
    if (now - lastCleanupTime.current > 10000) {
      cleanupOldData();
      lastCleanupTime.current = now;
    }
  }, [config]);

  const handleFftData = useCallback((data: FftPacket) => {
    setFullFftPacket(data); // Store the full packet

    // Also update the simplified fftData for compatibility if needed elsewhere
    if (data && Array.isArray(data.psd_packets)) {
      const newFftData: Record<number, number[]> = {};
      for (const packet of data.psd_packets) {
        newFftData[packet.channel] = packet.psd;
      }
      setFftData(prevFftData => ({
        ...prevFftData,
        ...newFftData,
      }));
    }
  }, []);

  const cleanupOldData = useCallback(() => {
    const now = Date.now();
    const cutoffTime = now - RECONNECTION_DATA_RETENTION_MS;
    
    // Find the first index to keep
    const firstValidIndex = sampleTimestamps.current.findIndex(timestamp => timestamp > cutoffTime);
    
    if (firstValidIndex > 0) {
      // Remove old timestamps and samples
      sampleTimestamps.current.splice(0, firstValidIndex);
      rawSamplesRef.current = rawSamplesRef.current.slice(firstValidIndex);
      setDataVersion(v => v + 1); // Notify consumers of the change
    }
  }, []);

  const clearOldData = useCallback(() => {
    rawSamplesRef.current = [];
    sampleTimestamps.current = [];
    setDataVersion(v => v + 1);
    console.log('[EegDataContext] Cleared old data due to manual request');
  }, []);

  const getRawSamples = useCallback(() => {
    return rawSamplesRef.current;
  }, []);

 const subscribeRaw = useCallback((callback: RawDataCallback) => {
   rawDataSubscribersRef.current.add(callback);
   // Return an unsubscribe function
   return () => {
     rawDataSubscribersRef.current.delete(callback);
   };
 }, []);

  const subscribe = useCallback((topics: string[]) => {
    setSubscriptions(prev => [...new Set([...prev, ...topics])]);
  }, []);

  const unsubscribe = useCallback((topics: string[]) => {
    setSubscriptions(prev => prev.filter(t => !topics.includes(t)));
  }, []);

  // Clear buffer when configuration changes to prevent misalignment
  // Create a stable key for the configuration to prevent unnecessary effect runs
  const configKey = useMemo(() => {
    if (!config) return null;
    // Sort channels to ensure key is consistent regardless of order
    const sortedChannels = config.channels.slice().sort((a, b) => a - b).join(',');
    return `${config.sample_rate}-${sortedChannels}`;
  }, [config]);

  // Clear buffer when the stable configuration key changes
  useEffect(() => {
    // Don't clear the buffer on the initial load when configKey is null
    if (configKey === null) return;

    rawSamplesRef.current = [];
    sampleTimestamps.current = [];
    setDataVersion(v => v + 1);
    console.log('[EegDataContext] Cleared buffer due to configuration change');
  }, [configKey]);

  // Handle WebSocket status changes to detect reconnections
  const handleDataUpdate = useCallback((received: boolean) => {
    setDataReceived(received);
    if (received && isReconnecting) {
      setIsReconnecting(false);
      console.log('[EegDataContext] Reconnection successful, data flow restored');
    }
  }, [isReconnecting]);

  const handleError = useCallback((error: string | null) => {
    setDriverError(error);
    if (error && !isReconnecting) {
      setIsReconnecting(true);
      console.log('[EegDataContext] Connection error detected, entering reconnection mode');
    }
  }, [isReconnecting]);

  const { status: wsStatus } = useEegDataHandler({
    config,
    onDataUpdate: handleDataUpdate,
    onError: handleError,
    onSamples: handleSamples,
    onFftData: handleFftData,
    subscriptions, // Pass subscriptions to the handler
    // Use stable refs to prevent unnecessary WebSocket reconnections
    lastDataChunkTimeRef,
    latestTimestampRef,
    debugInfoRef,
  });

  const value = useMemo(() => ({
    dataVersion,
    getRawSamples,
    subscribeRaw,
    fftData,
    fullFftPacket,
    config,
    dataStatus: {
      dataReceived,
      driverError,
      wsStatus,
      isReconnecting,
    },
    clearOldData,
    subscribe,
    unsubscribe,
  }), [dataVersion, fftData, fullFftPacket, config, dataReceived, driverError, wsStatus, isReconnecting, getRawSamples, subscribeRaw, clearOldData, subscribe, unsubscribe]);

  return (
    <EegDataContext.Provider value={value}>
      {children}
    </EegDataContext.Provider>
  );
};

// Custom hook to use the EEG data context
export const useEegData = () => {
  const context = useContext(EegDataContext);
  if (context === undefined) {
    throw new Error('useEegData must be used within an EegDataProvider');
  }
  return context;
};