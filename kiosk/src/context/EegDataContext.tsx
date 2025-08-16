'use client';

import React, { createContext, useContext, useState, ReactNode, useMemo, useRef, useCallback, useEffect } from 'react';
import { useEventStream } from './EventStreamContext';
import { usePipeline } from './PipelineContext'; // Import the usePipeline hook
import { SampleChunk, SensorMeta, MetaUpdateMsg, DataPacketHeader } from '../types/eeg'; // Import shared types

// Constants for data management
const MAX_SAMPLE_CHUNKS = 100;
const RECONNECTION_DATA_RETENTION_MS = 5000; // Keep data for 5 seconds during reconnections
const MAX_PROCESSING_TIME_MS = 8; // Max time to spend processing data per frame

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
// --- Start of new context structure ---

// 1. Stable Context: For functions and stable configuration
interface EegDataStableContextType {
  subscribeRaw: (callback: RawDataCallback) => () => void;
  getRawSamples: () => SampleChunk[];
  clearOldData: () => void;
  config: any; // Config is considered stable; changes should be infrequent
}

// 2. Dynamic Context: For frequently updated data
interface EegDataDynamicContextType {
  dataVersion: number;
  fftData: Record<number, number[]>;
  fullFftPacket: FftPacket | null;
}

// 3. Status Context: For connection and data flow status
interface EegDataStatusContextType {
  dataStatus: {
    dataReceived: boolean;
    driverError: string | null;
    wsStatus: string;
    isReconnecting: boolean;
  };
  isReady: boolean;
}

const EegDataStableContext = createContext<EegDataStableContextType | undefined>(undefined);
const EegDataDynamicContext = createContext<EegDataDynamicContextType | undefined>(undefined);
const EegDataStatusContext = createContext<EegDataStatusContextType | undefined>(undefined);

// --- End of new context structure ---

// Define the props for the provider component
interface EegDataProviderProps {
  children: ReactNode;
}

export const EegDataProvider = ({ children }: EegDataProviderProps) => {
  const rawSamplesRef = useRef<SampleChunk[]>([]);
  const configUpdateTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const animationFrameIdRef = useRef<number | null>(null);
  const [dataVersion, setDataVersion] = useState(0);
  const [fftData, setFftData] = useState<Record<number, number[]>>({});
  const [fullFftPacket, setFullFftPacket] = useState<FftPacket | null>(null);
  const [metadata, setMetadata] = useState<Record<string, SensorMeta>>({});
  const [dataReceived, setDataReceived] = useState(false);
  const [driverError, setDriverError] = useState<string | null>(null);
  const [isReconnecting, setIsReconnecting] = useState(false);
  const [isReady, setIsReady] = useState(false); // State to track final configuration readiness
  const [shouldConnect, setShouldConnect] = useState(false); // State to control when to connect
  const rawDataSubscribersRef = useRef({ raw: {} as Record<string, RawDataCallback> });

  const { pipelineConfig, pipelineStatus } = usePipeline(); // Get the pipeline state object
  const { subscribe } = useEventStream();
  const [sourceReadyMeta, setSourceReadyMeta] = useState<any | null>(null);
  const lastMetaRevRef = useRef<number | null>(null);
  const sourceReadyDebounceTimerRef = useRef<NodeJS.Timeout | null>(null);

  const handleSourceReady = useCallback((data: any) => {
    if (sourceReadyDebounceTimerRef.current) {
      clearTimeout(sourceReadyDebounceTimerRef.current);
    }

    sourceReadyDebounceTimerRef.current = setTimeout(() => {
      if (data.meta) {
        // Only perform a HARD RESET if the configuration is actually new.
        if (lastMetaRevRef.current !== null && data.meta.meta_rev <= lastMetaRevRef.current) {
          console.log(`[EegDataContext] Ignoring stale/duplicate SourceReady event with meta_rev: ${data.meta.meta_rev}`);
          return;
        }

        console.log(`[EegDataContext] HARD RESET: Received new SourceReady event. meta_rev: ${data.meta.meta_rev}`, data.meta);
        lastMetaRevRef.current = data.meta.meta_rev;

        // 1. Clear ALL existing data buffers
        rawSamplesRef.current = [];
        sampleTimestamps.current = [];

        // 2. Set the new metadata as the source of truth
        setSourceReadyMeta(data.meta);
        setMetadata(prev => ({ ...prev, ['eeg_voltage']: data.meta }));

        // 3. Force a re-render to propagate changes
        setDataVersion(v => v + 1);
      }
    }, 250); // Debounce for 250ms to handle rapid-fire events
  }, []);

  useEffect(() => {
    const unsubscribe = subscribe('SourceReady', handleSourceReady);
    return () => {
      unsubscribe();
      if (sourceReadyDebounceTimerRef.current) {
        clearTimeout(sourceReadyDebounceTimerRef.current);
      }
    };
  }, [subscribe, handleSourceReady]);

  const config = useMemo(() => {
    if (sourceReadyMeta) {
      const newChannelCount = sourceReadyMeta.channel_names?.length || 0;
      return {
        ...pipelineConfig,
        channels: Array.from({ length: newChannelCount }, (_, i) => i),
        sample_rate: sourceReadyMeta.sample_rate || 250,
      };
    }
    
    if (!pipelineConfig) {
      return null;
    }

    const eegSourceStage = pipelineConfig.stages.find(s => s.type === 'eeg_source');
    let channels: number[] = [];
    if (eegSourceStage && eegSourceStage.params?.driver?.chips?.length > 0) {
      // Sum up the number of channels from all chips
      const channelCount = eegSourceStage.params.driver.chips.reduce((acc: number, chip: any) => acc + (chip.channels?.length || 0), 0);
      channels = Array.from({ length: channelCount }, (_, i) => i);
    }

    return {
      ...pipelineConfig,
      channels,
      sample_rate: eegSourceStage?.params.sample_rate || 250,
    };
  }, [pipelineConfig, sourceReadyMeta]);
 
  // Create a ref to hold the latest config to avoid stale closures in WebSocket handler
  const configRef = useRef(config);
  useEffect(() => {
    configRef.current = config;
  }, [config]);
  
  // Use refs to track data timestamps for cleanup
  const sampleTimestamps = useRef<number[]>([]);
  const lastCleanupTime = useRef<number>(Date.now());
  
  // Create stable refs for EegDataHandler to prevent unnecessary reconnections
  const lastDataChunkTimeRef = useRef<number[]>([]);
  const latestTimestampRef = useRef<number>(0);
  const debugInfoRef = useRef({ lastPacketTime: 0, packetsReceived: 0, samplesProcessed: 0 });

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

  const incomingDataQueueRef = useRef<SampleChunk[]>([]);

  const processDataQueue = useCallback(() => {
    const startTime = performance.now();
    const newChunks: SampleChunk[] = [];

    while (incomingDataQueueRef.current.length > 0) {
      const chunk = incomingDataQueueRef.current.shift()!;
      newChunks.push(chunk);

      const now = Date.now();
      const newSamples = [...rawSamplesRef.current, chunk];
      sampleTimestamps.current.push(now);

      if (newSamples.length > MAX_SAMPLE_CHUNKS) {
        const excess = newSamples.length - MAX_SAMPLE_CHUNKS;
        sampleTimestamps.current.splice(0, excess);
        rawSamplesRef.current = newSamples.slice(excess);
      } else {
        rawSamplesRef.current = newSamples;
      }

      if (performance.now() - startTime > MAX_PROCESSING_TIME_MS) {
        break; // Defer remaining chunks to the next frame
      }
    }

    if (newChunks.length > 0) {
      setDataVersion(v => v + 1);
      handleDataUpdateRef.current(true);
      Object.values(rawDataSubscribersRef.current.raw).forEach(callback => callback(newChunks));
    }

    const now = Date.now();
    if (now - lastCleanupTime.current > 10000) {
      cleanupOldData();
      lastCleanupTime.current = now;
    }

    animationFrameIdRef.current = requestAnimationFrame(processDataQueue);
  }, [cleanupOldData]);

  useEffect(() => {
    animationFrameIdRef.current = requestAnimationFrame(processDataQueue);
    return () => {
      if (animationFrameIdRef.current) {
        cancelAnimationFrame(animationFrameIdRef.current);
      }
    };
  }, [processDataQueue]);

  const handleSamples = useCallback((newChunk: SampleChunk) => {
    incomingDataQueueRef.current.push(newChunk);
  }, []);

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

  // Create refs to hold the latest versions of the data handlers.
  // This prevents them from becoming dependencies in the main WebSocket useEffect.
  const handleSamplesRef = useRef(handleSamples);
  const handleFftDataRef = useRef(handleFftData);

  useEffect(() => {
    handleSamplesRef.current = handleSamples;
    handleFftDataRef.current = handleFftData;
  }, [handleSamples, handleFftData]);

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
    const id = Date.now().toString();
    rawDataSubscribersRef.current.raw[id] = callback;
    return () => {
      delete rawDataSubscribersRef.current.raw[id];
    };
  }, [rawDataSubscribersRef]);

 // Clear buffer when configuration changes to prevent misalignment
 // Create a stable key for the configuration to prevent unnecessary effect runs
  const configKey = useMemo(() => {
    if (!config) return null;
    // Sort channels to ensure key is consistent regardless of order
    const sortedChannels = config.channels.slice().sort((a: number, b: number) => a - b).join(',');
    return `${config.sample_rate}-${sortedChannels}`;
  }, [config]);

  // Use a window property to prevent duplicate logging in development mode
  const configChangeGuardKey = '__eeg_config_change_guard__';

  // Clear buffer when the stable configuration key changes
    useEffect(() => {
      // Don't clear the buffer on the initial load when configKey is null
      if (configKey === null) return;
  
      // Clear existing timeout
      if (configUpdateTimeoutRef.current) {
          clearTimeout(configUpdateTimeoutRef.current);
      }
  
      // Debounce buffer clearing
      configUpdateTimeoutRef.current = setTimeout(() => {
          rawSamplesRef.current = [];
          sampleTimestamps.current = [];
          setDataVersion(v => v + 1);
          console.log('[EegDataContext] Cleared buffer due to configuration change');
          
          // Check if we're in React Strict Mode development double-run scenario
          // @ts-ignore - Accessing custom property on window object
          if (process.env.NODE_ENV === 'development' && window[configChangeGuardKey]) {
            return;
          }
          
          // Set the guard to prevent duplicate logging in development mode
          if (process.env.NODE_ENV === 'development') {
            // @ts-ignore - Adding custom property to window object
            window[configChangeGuardKey] = true;
          }
      }, 100); // Small debounce to handle rapid config changes
  
    }, [configKey]);

  // Use a window property to prevent duplicate logging in development mode
  const systemReadyGuardKey = '__eeg_system_ready_guard__';

  // Effect to determine when the system is truly ready
  useEffect(() => {
    // Ready when pipeline is started and the final config with channel names is available
    if (pipelineStatus === 'started' && sourceReadyMeta?.channel_names) {
      setIsReady(true);
      setShouldConnect(true); // Signal that we should connect to data WebSocket
      
      // Check if we're in React Strict Mode development double-run scenario
      // @ts-ignore - Accessing custom property on window object
      if (!(process.env.NODE_ENV === 'development' && window[systemReadyGuardKey])) {
        if (process.env.NODE_ENV === 'development') {
          // @ts-ignore - Adding custom property to window object
          window[systemReadyGuardKey] = true;
        }
        console.log('[EegDataContext] System is ready. Final configuration has been received.');
      }
    } else {
      // Only reset isReady and shouldConnect if we're not in a reconnection state
      // During reconnection, we want to maintain the previous configuration
      if (!isReconnecting) {
        setIsReady(false);
        // Do not set shouldConnect to false here.
        // We want to keep the WebSocket connection alive during a pipeline restart
        // to avoid a "Disconnected" state on the frontend. The connection
        // will be reused when the new `SourceReady` event arrives.
        // Reset the guard when system is not ready
        if (process.env.NODE_ENV === 'development') {
          // @ts-ignore - Adding custom property to window object
          window[systemReadyGuardKey] = false;
        }
      }
    }
  }, [pipelineStatus, sourceReadyMeta]);

  // Handle WebSocket status changes to detect reconnections
  const handleDataUpdate = useCallback((received: boolean) => {
    setDataReceived(received);
    if (received && isReconnecting) {
      setIsReconnecting(false);
      console.log('[EegDataContext] Reconnection successful, data flow restored');
    }
  }, [isReconnecting]);

  const handleDataUpdateRef = useRef(handleDataUpdate);
  
  useEffect(() => {
    handleDataUpdateRef.current = handleDataUpdate;
  }, [handleDataUpdate]);

  const handleError = useCallback((error: string | null) => {
    setDriverError(error);
    if (error && !isReconnecting) {
      setIsReconnecting(true);
      console.log('[EegDataContext] Connection error detected, entering reconnection mode');
    }
  }, [isReconnecting]);

  const [wsStatus, setWsStatus] = useState('Idle');
  const ws = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<NodeJS.Timeout | null>(null); // For managing reconnection timer
  
  // Use window property to track connection attempts in React Strict Mode
  // This persists across double executions unlike refs which are reset per component instance
  const connectionGuardKey = '__eeg_websocket_connection_guard__';

  const connect = useCallback(() => {
    // Check if we should connect to WebSocket
    if (!shouldConnect) {
      return;
    }
    
    // Check if we're in React Strict Mode development double-run scenario
    // In Strict Mode, the first run sets the window guard, and the second run should be ignored
    // @ts-ignore - Accessing custom property on window object
    if (window[connectionGuardKey]) {
      console.log('[EegDataContext] Connection attempt already made, skipping duplicate connection attempt.');
      return;
    }

    // Ensure we don't create duplicate connections
    if (ws.current) {
      console.log('[EegDataContext] Duplicate connection.');
      return;
    }
    
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const host = window.location.hostname;
    const url = `${protocol}//${host}:9000/ws/data`;

    console.log('[EegDataContext] Connecting to WebSocket:', url);
    setWsStatus('Connecting...');
    const socket = new WebSocket(url);
    socket.binaryType = 'arraybuffer';
    
    // Set the connection guard on window to prevent duplicate connections
    // @ts-ignore - Adding custom property to window object
    window[connectionGuardKey] = true;
    ws.current = socket;

    socket.onopen = () => {
      console.log('[EegDataContext] WebSocket connection established');
      setWsStatus('Connected');

      // Dynamically subscribe to the topic from the sourceReady event
      if (sourceReadyMeta?.source_type && sourceReadyMeta?.meta_rev) {
        const topic = sourceReadyMeta.source_type === 'eeg_source' ? 'eeg_voltage' : 'fft';
        const subscriptionMessage = {
          type: 'subscribe',
          topic: topic,
          epoch: sourceReadyMeta.meta_rev,
        };
        socket.send(JSON.stringify(subscriptionMessage));
        console.log(`[EegDataContext] Subscribed to topic: ${subscriptionMessage.topic} with epoch ${subscriptionMessage.epoch}`);
      } else {
        console.warn('[EegDataContext] Could not subscribe to data topic: sourceReadyMeta or meta_rev is not available.');
      }
    };

    // Define the message handler inside the effect to create a stable closure
    // over the handleSamples and handleFftData callbacks.
    socket.onmessage = (event: MessageEvent) => {
      // Handle MetaUpdateMsg (Text)
      if (typeof event.data === 'string') {
        const msg = JSON.parse(event.data);
        if (msg.message_type === 'meta_update') {
          const metaUpdate = msg as MetaUpdateMsg;
          setMetadata(prev => ({ ...prev, [metaUpdate.topic]: metaUpdate.meta }));
        }
        return;
      }

      // Handle Data Packet (Binary)
      if (event.data instanceof ArrayBuffer) {
        const dataView = new DataView(event.data);
        
        // 1. Read JSON header length
        const jsonLen = dataView.getUint32(0, false); // Big-endian is correct
        
        // 2. Decode JSON header
        const jsonBytes = new Uint8Array(event.data, 4, jsonLen);
        const jsonString = new TextDecoder().decode(jsonBytes);
        const header = JSON.parse(jsonString) as DataPacketHeader;

        // 3. Look up the full metadata using meta_rev
        const topicMeta = metadata[header.topic];
        if (!topicMeta || topicMeta.meta_rev !== header.meta_rev) {
          // Silently drop the packet if metadata is not found or mismatched.
          // This handles race conditions during configuration changes.
          return;
        }

        // 4. Calculate sample data offset (4 bytes for length prefix)
        const samplesOffset = 4 + jsonLen;

        // 5. Process the samples based on the explicit packet_type
        const samplesBuffer = event.data.slice(samplesOffset);
         const samples =
           header.packet_type === 'RawI32'
            ? new Int32Array(samplesBuffer)
            : new Float32Array(samplesBuffer);
        
        // Now you have the full context: `header` and `topicMeta` to process the `samples`
        const newChunk: SampleChunk = {
            meta: topicMeta,
            samples: samples,
            timestamp: header.ts_ns,
        };
        handleSamplesRef.current(newChunk);
      }
    };

    socket.onerror = (err) => {
      if (socket.readyState === WebSocket.CONNECTING || socket.readyState === WebSocket.OPEN) {
        console.error('[EegDataContext] WebSocket error:', err);
        setWsStatus('Error');
        ws.current = null;
        // @ts-ignore
        window[connectionGuardKey] = false;
        setShouldConnect(false);
      }
    };

    socket.onclose = (event) => {
      console.log('[EegDataContext] WebSocket connection closed', event);
      if (ws.current === socket) {
        setWsStatus('Disconnected');
        ws.current = null;
        // @ts-ignore
        window[connectionGuardKey] = false;

        // If the server closed the connection with a stale epoch code,
        // do not immediately reconnect. Wait for a new SourceReady event.
        if (event.code === 4009) {
          console.warn('[EegDataContext] Connection closed due to stale epoch. Waiting for new configuration...');
          setShouldConnect(false); // Prevent automatic reconnection
          return;
        }
        
        if (!reconnectTimerRef.current) {
          reconnectTimerRef.current = setTimeout(() => {
            console.log('[EegDataContext] Attempting to reconnect WebSocket');
            connect();
          }, 2000); // Reconnect after 2 seconds
        }
      }
    };
    return () => {
      console.log('[EegDataContext] Cleanup: Closing WebSocket');
      if (ws.current) {
        ws.current.onopen = null;
        ws.current.onmessage = null;
        ws.current.onerror = null;
        ws.current.onclose = null;
        ws.current.close();
        ws.current = null;
      }
      // @ts-ignore
      window[connectionGuardKey] = false;
    };
  }, [shouldConnect, sourceReadyMeta]); // Add sourceReadyMeta as a dependency

  // This useEffect manages the WebSocket connection lifecycle.
  // It runs ONLY when shouldConnect changes from false to true.
  useEffect(() => {
    if (shouldConnect) {
      connect();
    }
  
    // Cleanup function to close WebSocket and clear timers
    return () => {
      if (ws.current) {
        ws.current.close();
        ws.current = null;
      }
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
      }
      // Reset the connection guard on cleanup
      // @ts-ignore
      window[connectionGuardKey] = false;
    };
  }, [shouldConnect, connect]);


  const stableValue = useMemo(() => ({
    subscribeRaw,
    getRawSamples,
    clearOldData,
    config,
 }), [subscribeRaw, getRawSamples, clearOldData, config]);

  const dynamicValue = useMemo(() => ({
    dataVersion,
    fftData,
    fullFftPacket,
  }), [dataVersion, fftData, fullFftPacket]);

  const statusValue = useMemo(() => ({
    dataStatus: {
      dataReceived,
      driverError,
      wsStatus,
      isReconnecting,
    },
    isReady,
  }), [dataReceived, driverError, wsStatus, isReconnecting, isReady]);

  return (
    <EegDataStableContext.Provider value={stableValue}>
      <EegDataDynamicContext.Provider value={dynamicValue}>
        <EegDataStatusContext.Provider value={statusValue}>
          {children}
        </EegDataStatusContext.Provider>
      </EegDataDynamicContext.Provider>
    </EegDataStableContext.Provider>
  );
};

// Custom hook to use the stable parts of the EEG data context
export const useEegData = () => {
  const context = useContext(EegDataStableContext);
  if (context === undefined) {
    throw new Error('useEegData must be used within an EegDataProvider');
  }
  return context;
};

// Custom hook to use the dynamic parts of the EEG data context
export const useEegDynamicData = () => {
  const context = useContext(EegDataDynamicContext);
  if (context === undefined) {
    throw new Error('useEegDynamicData must be used within an EegDataProvider');
  }
  return context;
};

// Custom hook to use the status parts of the EEG data context
export const useEegStatus = () => {
  const context = useContext(EegDataStatusContext);
  if (context === undefined) {
    throw new Error('useEegStatus must be used within an EegDataProvider');
  }
  return context;
};