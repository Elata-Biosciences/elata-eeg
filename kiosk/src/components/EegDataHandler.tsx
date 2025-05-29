'use client';

/**
 * EegDataHandler.tsx
 *
 * This component handles WebSocket connections to the EEG data server and processes incoming data.
 *
 * This implementation uses a constant FPS rendering approach, removing the need for
 * render flags and simplifying the overall rendering process.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import {
    DEFAULT_SAMPLE_RATE,
    DEFAULT_BATCH_SIZE,
    WINDOW_DURATION,
    FFT_WINDOW_DURATION_MS, // Import from constants
    FFT_HOP_DURATION_MS     // Import from constants
} from '../utils/eegConstants';
// import { calculateFft } from '../utils/fftUtils'; // Removed: FFT calculation is now backend-driven
 
interface EegDataHandlerProps {
  config: any;
  onDataUpdate: (dataReceived: boolean) => void;
  onError?: (error: string) => void;
  linesRef: React.MutableRefObject<any[]>; // Array of WebglStep instances (e.g., WebglLineRoll)
  lastDataChunkTimeRef: React.MutableRefObject<number[]>; // Ref holding array of per-channel timestamps
  latestTimestampRef: React.MutableRefObject<number>; // Ref holding the single latest timestamp
  debugInfoRef: React.MutableRefObject<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>; // Ref for debug information including packet count
  onFftData?: (channelIndex: number, fftOutput: number[]) => void; // New callback for FFT data
}

export function useEegDataHandler({
  config,
  onDataUpdate,
  onError,
  linesRef,
  lastDataChunkTimeRef,
  latestTimestampRef,
  debugInfoRef,
  onFftData // Destructure the new FFT callback
}: EegDataHandlerProps) {
  const [status, setStatus] = useState('Connecting...');
  const wsRef = useRef<WebSocket | null>(null);
  // handleMessageRef is no longer needed at this scope
  const dataReceivedTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttemptsRef = useRef<number>(0);
  const isProduction = process.env.NODE_ENV === 'production';
  // No queues or animation frame needed for immediate display
  const sampleBuffersRef = useRef<Float32Array[]>([]); // For raw data display
  // const fftBuffersRef = useRef<number[][]>([]); // Removed: FFT calculation is now backend-driven
  // const samplesSinceLastFftRef = useRef<number[]>([]); // Removed: FFT calculation is now backend-driven
  
  // createMessageHandler logic is now moved inside connectWebSocket



  /**
   * Function to establish WebSocket connection with automatic reconnection
   */
  const connectWebSocket = useCallback((currentConfig: any) => {
    // Clear any existing reconnect timeout
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current);
      reconnectTimeoutRef.current = null;
    }
    
    // Close existing connection if any
    if (wsRef.current) {
      try {
        wsRef.current.close();
      } catch (e) {
        // Ignore errors on close
      }
    }
    
    setStatus('Connecting...');
    
    // Use currentConfig passed to the function
    if (!currentConfig) {
        console.warn("connectWebSocket called without config.");
        return;
    }
 
    const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
    const ws = new WebSocket(`ws://${wsHost}:8080/eeg`);
    wsRef.current = ws;
    
    // Set binary type for WebSocket
    ws.binaryType = 'arraybuffer';
    
    ws.onopen = () => {
      setStatus('Connected');
      reconnectAttemptsRef.current = 0; // Reset reconnect attempts on successful connection
      if (!isProduction) {
        console.log('WebSocket connection established');
      }
    };
    
    // --- WebSocket Message Handler ---
    // Initialize queues based on channel count whenever config changes (handled in useEffect below)

    const handleWebSocketMessage = (event: MessageEvent) => {
      try {
        if (!(event.data instanceof ArrayBuffer)) {
          return;
        }
        const dataView = new DataView(event.data);
        let bufferOffset = 0;

        // const timestamp = dataView.getBigUint64(bufferOffset, true); // BigInt, not directly used yet
        bufferOffset += 8;

        const errorFlag = dataView.getUint8(bufferOffset);
        bufferOffset += 1;

        const fftFlag = dataView.getUint8(bufferOffset);
        bufferOffset += 1;

        if (errorFlag === 1) {
          const errorBytes = new Uint8Array(event.data.slice(bufferOffset));
          const errorMessage = new TextDecoder().decode(errorBytes);
          console.error("EEG driver error:", errorMessage);
          if (typeof onError === 'function') onError(`EEG driver error: ${errorMessage}`);
          return;
        }

        const configuredChannelCount = currentConfig?.channels?.length || 0;
        if (configuredChannelCount === 0) return;

        // --- FFT Data Parsing ---
        if (fftFlag === 1) {
          const numFftChannels = dataView.getUint8(bufferOffset);
          bufferOffset += 1;

          for (let i = 0; i < numFftChannels; i++) {
            // Power Spectrum
            const powerSpectrumLen = dataView.getUint32(bufferOffset, true);
            bufferOffset += 4;
            const powerSpectrum = new Float32Array(powerSpectrumLen);
            for (let j = 0; j < powerSpectrumLen; j++) {
              powerSpectrum[j] = dataView.getFloat32(bufferOffset, true);
              bufferOffset += 4;
            }

            // Frequency Bins
            const freqBinsLen = dataView.getUint32(bufferOffset, true);
            bufferOffset += 4;
            const freqBins = new Float32Array(freqBinsLen); // Though onFftData expects number[] for power
            for (let j = 0; j < freqBinsLen; j++) {
              freqBins[j] = dataView.getFloat32(bufferOffset, true);
              bufferOffset += 4;
            }
            // Assuming onFftData is primarily interested in the power spectrum for now.
            // The `fftOutput` in `onFftData` was originally just the power.
            // If `freqBins` are also needed by the renderer, `onFftData` signature and FftRenderer might need updates.
            if (onFftData) {
              onFftData(i, Array.from(powerSpectrum)); // Pass channel index and power spectrum
            }
          }
        }
        // --- End FFT Data Parsing ---

        // --- Raw EEG Sample Parsing ---
        // Calculate remaining bytes for raw samples
        const remainingBytes = event.data.byteLength - bufferOffset;
        // Each sample is 4 bytes (float32). configuredChannelCount is from the UI/config.
        const samplesPerChannel = Math.floor(remainingBytes / 4 / configuredChannelCount);

        if (samplesPerChannel <= 0 && fftFlag === 0) { // Only warn if no FFT and no raw samples
            if (!isProduction) console.warn("Received packet with no raw EEG samples.");
            // Do not return if there was FFT data, as that might be the primary content.
            if (fftFlag === 0) return;
        }
        
        if (sampleBuffersRef.current.length < configuredChannelCount) {
          sampleBuffersRef.current = Array(configuredChannelCount).fill(null).map((_, i) => sampleBuffersRef.current[i] || null);
        }

        for (let ch = 0; ch < configuredChannelCount; ch++) {
          let currentSampleBuffer = sampleBuffersRef.current[ch];

          if (!currentSampleBuffer || currentSampleBuffer.length !== samplesPerChannel) {
            currentSampleBuffer = new Float32Array(samplesPerChannel > 0 ? samplesPerChannel : 0);
            sampleBuffersRef.current[ch] = currentSampleBuffer;
          }
          
          if (samplesPerChannel > 0) {
            for (let i = 0; i < samplesPerChannel; i++) {
              const sampleBufferOffset = bufferOffset + i * 4;
              if (sampleBufferOffset + 4 <= event.data.byteLength) {
                const rawValue = dataView.getFloat32(sampleBufferOffset, true);
                currentSampleBuffer[i] = isFinite(rawValue) ? rawValue : 0;
              } else {
                currentSampleBuffer[i] = 0;
                if (!isProduction) console.warn(`Buffer overrun for raw samples at sample ${i}, channel ${ch}`);
              }
            }
            bufferOffset += samplesPerChannel * 4;
          }


          if (lastDataChunkTimeRef.current && lastDataChunkTimeRef.current[ch] !== undefined) {
              lastDataChunkTimeRef.current[ch] = performance.now();
          }

          if (linesRef.current && linesRef.current[ch] && samplesPerChannel > 0) {
            linesRef.current[ch].shiftAdd(currentSampleBuffer);
          }
        }
        // --- End Raw EEG Sample Parsing ---
        
        if (latestTimestampRef) {
            latestTimestampRef.current = performance.now();
        }

        // Update debug info - increment packets received counter
        if (debugInfoRef) {
          debugInfoRef.current.packetsReceived++;
          debugInfoRef.current.lastPacketTime = performance.now();
          debugInfoRef.current.samplesProcessed += samplesPerChannel * configuredChannelCount;
        }

        if (typeof onDataUpdate === 'function') {
          onDataUpdate(true);
        }

        if (dataReceivedTimeoutRef.current) {
          clearTimeout(dataReceivedTimeoutRef.current);
        }
        dataReceivedTimeoutRef.current = setTimeout(() => {
          if (typeof onDataUpdate === 'function') {
            onDataUpdate(false);
          }
        }, 1000);

      } catch (error) {
        console.error("Error parsing EEG binary data:", error);
        if (typeof onError === 'function') onError(`Error parsing EEG data: ${error}`);
      }
    }; // End of handleWebSocketMessage

    // Assign the raw message handler
    ws.onmessage = handleWebSocketMessage;
    // --- End of moved message handler logic ---
    
    ws.onclose = (event) => {
      if (!isProduction) {
        console.log(`WebSocket closed with code: ${event.code}, reason: ${event.reason}`);
      }
      
      setStatus('Disconnected');
      
      // Implement exponential backoff for reconnection
      const maxReconnectDelay = 5000; // Maximum delay of 5 seconds
      const baseDelay = 500; // Start with 500ms delay
      const reconnectDelay = Math.min(
        maxReconnectDelay,
        baseDelay * Math.pow(1.5, reconnectAttemptsRef.current)
      );
      
      reconnectAttemptsRef.current++;
      
      if (!isProduction) {
        console.log(`Attempting to reconnect in ${reconnectDelay}ms (attempt ${reconnectAttemptsRef.current})`);
      }
      
      // Schedule reconnection
      reconnectTimeoutRef.current = setTimeout(() => {
        if (!isProduction) {
          console.log('Attempting to reconnect...');
        }
        // Pass the config again when reconnecting
        connectWebSocket(currentConfig);
      }, reconnectDelay);
    };
    
    ws.onerror = (error) => {
      if (!isProduction) {
        console.error('WebSocket error:', error);
      }
      // Don't update timestamp on error, just report it
      // if (latestTimestampRef) {
      //     latestTimestampRef.current = performance.now();
      // }
      if (typeof onError === 'function') onError(`WebSocket error: ${error}`);
      // onclose will handle reconnection attempt
    };

  // Dependencies: Only include stable references or primitives if possible.
  // config is passed directly when called.
  // linesRef, lastDataChunkTimeRef, latestTimestampRef, debugInfoRef, onDataUpdate, onError, onFftData are refs/callbacks assumed stable.
  }, [isProduction, linesRef, lastDataChunkTimeRef, latestTimestampRef, debugInfoRef, onDataUpdate, onError, onFftData]);
  
  /**
   * Effect for managing WebSocket connection and sample processing interval.
   */
  useEffect(() => {
    const currentConfig = config; // Capture config for this effect run
    const numChannels = currentConfig?.channels?.length || 0;
    const sampleRate = currentConfig?.sample_rate || DEFAULT_SAMPLE_RATE;

    // Initialize/Reset FFT buffers when config changes (e.g., channel count) - REMOVED
    // if (numChannels > 0) {
    //   fftBuffersRef.current = Array(numChannels).fill(null).map(() => []);
    //   samplesSinceLastFftRef.current = Array(numChannels).fill(0);
    // } else {
    //   fftBuffersRef.current = [];
    //   samplesSinceLastFftRef.current = [];
    // }
 
    // --- Connect WebSocket ---
    if (currentConfig) {
      connectWebSocket(currentConfig);
    }

    // --- No Sample Processing Loop Needed ---
    // Data is processed directly in handleWebSocketMessage

    // --- Cleanup Function ---
    return () => {
      if (!isProduction) {
        console.log("Cleaning up EegDataHandler effect...");
      }

      // Clear reconnect timeout
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
        reconnectTimeoutRef.current = null;
        if (!isProduction) console.log("Reconnect timeout cleared.");
      }

      // Clear data received timeout
      if (dataReceivedTimeoutRef.current) {
        clearTimeout(dataReceivedTimeoutRef.current);
        dataReceivedTimeoutRef.current = null;
        if (!isProduction) console.log("Data received timeout cleared.");
      }

      // Close WebSocket connection
      if (wsRef.current) {
        if (!isProduction) console.log("Closing WebSocket connection...");
        try {
          wsRef.current.onclose = null; // Prevent reconnect logic during manual close
          wsRef.current.onerror = null;
          wsRef.current.close();
        } catch (e) {
          // Ignore errors during cleanup close
        }
        wsRef.current = null;
      }
    };
  // Dependencies: Re-run effect if config changes that affect connection or processing rate/channels.
  // Also include connectWebSocket as it's defined outside but used inside.
  // onFftData is added to dependencies of connectWebSocket, so not strictly needed here if connectWebSocket handles it.
  // However, including config directly ensures re-initialization of FFT buffers if channel count changes.
  }, [config, connectWebSocket, isProduction, linesRef, latestTimestampRef, debugInfoRef]);
  // Return status (FPS is now implicitly handled by sample rate)
  return { status };
}