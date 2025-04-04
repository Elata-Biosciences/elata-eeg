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
import throttle from 'lodash.throttle';
import { ScrollingBuffer } from '../utils/ScrollingBuffer';
import { DEFAULT_SAMPLE_RATE, DEFAULT_BATCH_SIZE, WINDOW_DURATION } from '../utils/eegConstants';

interface EegDataHandlerProps {
  config: any;
  onDataUpdate: (dataReceived: boolean) => void;
  dataRef: React.MutableRefObject<ScrollingBuffer[]>;
  windowSizeRef: React.MutableRefObject<number>;
  debugInfoRef: React.MutableRefObject<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>;
  latestTimestampRef: React.MutableRefObject<number>;
}

export function useEegDataHandler({
  config,
  onDataUpdate,
  dataRef,
  windowSizeRef,
  debugInfoRef,
  latestTimestampRef
}: EegDataHandlerProps) {
  const [status, setStatus] = useState('Connecting...');
  const wsRef = useRef<WebSocket | null>(null);
  const handleMessageRef = useRef<any>(null);
  const dataReceivedTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const lastTimestampRef = useRef<number>(Date.now());
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttemptsRef = useRef<number>(0);
  const isProduction = process.env.NODE_ENV === 'production';
  const lastWindowSizeRef = useRef<number>(windowSizeRef.current);

  // Calculate optimal throttle interval based on config
  const getThrottleInterval = useCallback(() => {
    // Always use the FPS from config to calculate the throttle interval
    // This gives us the time between frames in milliseconds
    return Math.max(8, 1000 / (config?.fps || 0));
  }, [config]);

  // Monitor changes to windowSizeRef and update buffer capacities
  useEffect(() => {
    // Check if window size has changed
    if (windowSizeRef.current !== lastWindowSizeRef.current) {
      const newSize = windowSizeRef.current;
      
      if (!isProduction) {
        console.log(`Window size changed: ${lastWindowSizeRef.current} -> ${newSize}`);
      }
      
      // Update existing buffers instead of recreating them
      dataRef.current.forEach((buffer, index) => {
        if (buffer) {
          if (!isProduction) {
            console.log(`Updating capacity for channel ${index}: ${buffer.getCapacity()} -> ${newSize}`);
          }
          buffer.updateCapacity(newSize);
        }
      });
      
      // Update last window size
      lastWindowSizeRef.current = newSize;
    }
    
    // This effect runs on every render, but only does work when windowSizeRef.current changes
  });

  // Ensure all buffers are initialized - but only when necessary
  useEffect(() => {
    // Initialize buffers for all channels
    const channelCount = config?.channels?.length || 4;
    
    // Only reinitialize if channel count changed or buffers not initialized
    const needsReinitialization =
      dataRef.current.length !== channelCount ||
      dataRef.current.length === 0;
    
    if (needsReinitialization) {
      dataRef.current = Array(channelCount).fill(null).map(() =>
        new ScrollingBuffer(windowSizeRef.current, config?.sample_rate || DEFAULT_SAMPLE_RATE)
      );
      
      if (!isProduction) {
        console.log(`Initialized ${channelCount} channel buffers in useEffect`);
      }
    } else {
      // Update sample rate and capacity if needed
      dataRef.current.forEach(buffer => {
        if (buffer) {
          // Update sample rate if needed
          const sampleRate = config?.sample_rate || DEFAULT_SAMPLE_RATE;
          if (buffer.getSampleRate() !== sampleRate) {
            buffer.setSampleRate(sampleRate);
          }
          
          // Update capacity if needed
          if (buffer.getCapacity() !== windowSizeRef.current) {
            buffer.updateCapacity(windowSizeRef.current);
          }
        }
      });
    }
  }, [config, dataRef, windowSizeRef, isProduction]);

  // Create message handler function with stabilized dependencies
  const createMessageHandler = useCallback(() => {
    const interval = getThrottleInterval();
    
    // Only create a new handler if the interval has changed or if no handler exists
    if (handleMessageRef.current && handleMessageRef.current.interval === interval) {
      return handleMessageRef.current;
    }
    
    if (!isProduction) {
      console.log(`Setting throttle interval to ${interval.toFixed(2)}ms (${(1000/interval).toFixed(2)} FPS)`);
    }
    
    // Cancel previous handler if it exists
    if (handleMessageRef.current) {
      handleMessageRef.current.cancel();
    }
    
    // Create new throttled handler
    const handler = throttle((event: MessageEvent) => {
      try {
        // For binary data format
        if (event.data instanceof ArrayBuffer) {
          const dataView = new DataView(event.data);
          
          // First 8 bytes are the timestamp (as BigInt64)
          let timestamp = Number(dataView.getBigInt64(0, true));
          const now = Date.now();
          lastTimestampRef.current = now;
          
          // Only convert seconds to milliseconds if needed
          const timeDiff = Math.abs(timestamp - now);
          if (timeDiff > 10000) {
            if (!isProduction) {
              console.log(`Timestamp adjustment: ${timestamp} -> ${timestamp < 1000000000000 ? timestamp * 1000 : timestamp}`);
            }
            
            // Only convert seconds to milliseconds if needed
            if (timestamp < 1000000000000) { // If timestamp is less than year 2001 in ms
              timestamp = timestamp * 1000; // Convert to milliseconds
            }
          }
          
          // Update the latest timestamp reference for our rendering window
          latestTimestampRef.current = timestamp;
          
          // Calculate how many samples per channel
          const channelCount = config?.channels?.length || 4;
          const samplesPerChannel = (event.data.byteLength - 8) / 4 / channelCount; // channelCount channels, 4 bytes per float
          const sampleRate = config?.sample_rate || DEFAULT_SAMPLE_RATE;
          const sampleInterval = 1000 / sampleRate;
          
          // Update debug info
          const debugInfo = debugInfoRef.current;
          debugInfo.packetsReceived++;
          debugInfo.samplesProcessed += samplesPerChannel * channelCount; // Use dynamic channel count
          
          // Notify the buffer that a new data chunk has arrived
          // This updates the timestamp for calculating render offset
          for (let ch = 0; ch < channelCount; ch++) {
            if (dataRef.current[ch]) {
              dataRef.current[ch].notifyNewDataChunk();
            }
          }
          
          // Set data received indicator
          onDataUpdate(true);
          
          // Clear previous timeout if it exists
          if (dataReceivedTimeoutRef.current) {
            clearTimeout(dataReceivedTimeoutRef.current);
          }
          
          // Reset data received indicator after 500ms of no data
          dataReceivedTimeoutRef.current = setTimeout(() => {
            onDataUpdate(false);
          }, 500);
          
          // Process each channel - optimized for performance
          // Use the channelCount already defined above
          for (let ch = 0; ch < channelCount; ch++) {
            if (!dataRef.current[ch]) {
              if (!isProduction) {
                console.warn(`Channel ${ch} buffer not initialized!`);
              }
              continue;
            }
            
            // Pre-calculate base offset for this channel to avoid repeated calculations
            const channelBaseOffset = 8 + (ch * samplesPerChannel * 4);
            
            // Log buffer capacity and current size occasionally for debugging
            if (!isProduction && Math.random() < 0.01) {
              console.log(`[EegDataHandler] Channel ${ch} buffer: capacity=${dataRef.current[ch].getCapacity()}, samples=${samplesPerChannel}`);
            }
            
            // Process all samples for this channel in a single loop
            for (let i = 0; i < samplesPerChannel; i++) {
              const offset = channelBaseOffset + (i * 4);
              
              // Add bounds checking to prevent "Offset is outside the bounds of the Dataview" error
              if (offset + 4 <= event.data.byteLength) {
                const value = dataView.getFloat32(offset, true); // true for little-endian
                
                // Fast path for valid values (most common case)
                if (isFinite(value) && Math.abs(value) <= 10) {
                  dataRef.current[ch].push(value);
                  continue;
                }
                
                // Handle edge cases
                if (isNaN(value) || !isFinite(value)) {
                  if (!isProduction) {
                    console.warn(`Invalid value for channel ${ch}: ${value}`);
                  }
                  dataRef.current[ch].push(0);
                } else {
                  // Clamp large values
                  dataRef.current[ch].push(Math.max(-3, Math.min(3, value)));
                }
              } else {
                // Handle out-of-bounds access
                if (!isProduction) {
                  console.warn(`Offset ${offset} outside bounds of DataView (size: ${event.data.byteLength})`);
                }
                // Push a default value to maintain continuity
                dataRef.current[ch].push(0);
              }
            }
          }
        }
        // Fallback for JSON data (for backward compatibility)
        else {
          const data = JSON.parse(event.data);
          lastTimestampRef.current = Date.now();
          
          // Process JSON data more efficiently
          data.channels.forEach((channel: number[], channelIndex: number) => {
            if (!dataRef.current[channelIndex]) return;
            
            // Process all values at once
            for (let i = 0; i < channel.length; i++) {
              const value = channel[i];
              if (isFinite(value) && Math.abs(value) <= 10) {
                dataRef.current[channelIndex].push(value);
              } else if (isNaN(value) || !isFinite(value)) {
                dataRef.current[channelIndex].push(0);
              } else {
                dataRef.current[channelIndex].push(Math.max(-3, Math.min(3, value)));
              }
            }
          });
          
          // Notify the buffer that a new data chunk has arrived
          // This updates the timestamp for calculating render offset
          const channelCount = config?.channels?.length || 4;
          for (let ch = 0; ch < channelCount; ch++) {
            if (dataRef.current[ch]) {
              dataRef.current[ch].notifyNewDataChunk();
            }
          }
        }
      } catch (error) {
        console.error('WebSocket error:', error);
      }
    }, interval, { trailing: true });
    
    // Store the interval on the handler for comparison in future calls
    (handler as any).interval = interval;
    
    handleMessageRef.current = handler;
    return handler;
  }, [getThrottleInterval, isProduction]); // Reduced dependencies to only essential ones

  // Update window size when config changes - with memoized config check
  const lastConfigRef = useRef<any>(null);
  
  useEffect(() => {
    // Only update if config has actually changed in a meaningful way
    const configChanged = !lastConfigRef.current ||
                          lastConfigRef.current.sample_rate !== config?.sample_rate ||
                          lastConfigRef.current.channels?.length !== config?.channels?.length;
    
    if (config && configChanged) {
      // Store current config for future comparison
      lastConfigRef.current = {
        sample_rate: config.sample_rate,
        channels: [...(config.channels || [])]
      };
      
      // Add safeguard for sample rate as suggested in the code review
      const safeSampleRate = Math.max(1, config.sample_rate || DEFAULT_SAMPLE_RATE);
      
      // Note: windowSizeRef.current is now updated in EegMonitor.tsx based on screen width
      // We don't need to update it here anymore
      
      // Get channel count from config
      const channelCount = config?.channels?.length || 4;
      
      // Check if we need to reinitialize buffers or just update them
      if (dataRef.current.length !== channelCount) {
        // Channel count changed, need to reinitialize
        dataRef.current = Array(channelCount).fill(null).map(() =>
          new ScrollingBuffer(windowSizeRef.current, safeSampleRate)
        );
        
        if (!isProduction) {
          console.log(`Reinitialized ${channelCount} channel buffers due to channel count change`);
        }
      } else {
        // Just update existing buffers
        dataRef.current.forEach(buffer => {
          if (buffer) {
            // Update sample rate
            buffer.setSampleRate(safeSampleRate);
            
            // Update capacity if needed
            if (buffer.getCapacity() !== windowSizeRef.current) {
              buffer.updateCapacity(windowSizeRef.current);
            }
          }
        });
        
        if (!isProduction) {
          console.log(`Updated ${channelCount} channel buffers with sample rate ${safeSampleRate}Hz`);
        }
      }
      
      // Recreate message handler with new throttle interval
      if (wsRef.current) {
        const handler = createMessageHandler();
        wsRef.current.onmessage = handler;
      }
    }
  }, [config, createMessageHandler, isProduction]); // Reduced dependencies

  /**
   * Function to establish WebSocket connection with automatic reconnection
   */
  const connectWebSocket = useCallback(() => {
    // Only initialize if buffers don't exist yet
    if (dataRef.current.length === 0) {
      const channelCount = config?.channels?.length || 4;
      dataRef.current = Array(channelCount).fill(null).map(() =>
        new ScrollingBuffer(windowSizeRef.current, config?.sample_rate || DEFAULT_SAMPLE_RATE)
      );
    }
    
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
    
    const ws = new WebSocket('ws://localhost:8080/eeg');
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
    
    // Create message handler with current config
    const handler = createMessageHandler();
    ws.onmessage = handler;
    
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
        connectWebSocket();
      }, reconnectDelay);
    };
    
    ws.onerror = (error) => {
      if (!isProduction) {
        console.error('WebSocket error:', error);
      }
      setStatus('Error');
      // Don't reconnect here - the onclose handler will be called after an error
    };
    
  }, [createMessageHandler, isProduction]); // Reduced dependencies to prevent unnecessary reconnections
  
  /**
   * Set up WebSocket connection with stable lifecycle
   */
  useEffect(() => {
    // Only connect once on initial mount
    connectWebSocket();
    
    return () => {
      // Clean up on component unmount - proper cleanup prevents memory leaks
      if (handleMessageRef.current) {
        handleMessageRef.current.cancel();
      }
      
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      
      if (dataReceivedTimeoutRef.current) {
        clearTimeout(dataReceivedTimeoutRef.current);
      }
      
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, []); // Empty dependency array ensures this only runs once on mount

  // Get FPS directly from config with no fallback
  const fps = config?.fps || 0;

  return { status, fps };
}