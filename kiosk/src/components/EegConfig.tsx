'use client';

import { useEffect, useState, createContext, useContext, useRef, useCallback } from 'react';

// Define the EEG configuration interface
export interface EegConfig {
  // Defined from daemon
  sample_rate: number;
  channels: number[];
  gain: number;
  board_driver: string;
  batch_size: number;
  // Defined from config file
  fps: number;
  // Powerline filter setting
  powerline_filter_hz: number | null; // Changed from optional to required, can be null
}

// Create a context to share the configuration across components
interface EegConfigContextType {
  config: EegConfig | null;
  status: string;
  // refreshConfig is no longer needed as config updates will be pushed by the server
}

export const EegConfigContext = createContext<EegConfigContextType>({
  config: null,
  status: 'Initializing...',
  // refreshConfig: () => { console.warn('EegConfigContext: refreshConfig called before provider initialization'); } // Default no-op
});

// Hook to use the EEG configuration
export const useEegConfig = () => useContext(EegConfigContext);

// Provider component
export function EegConfigProvider({ children }: { children: React.ReactNode }) {
  const [config, setConfig] = useState<EegConfig | null>(null);
  const [status, setStatus] = useState('Initializing...'); // Start as Initializing
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttemptsRef = useRef<number>(0);
  const isProduction = process.env.NODE_ENV === 'production';

  // Helper function to deeply compare relevant parts of EEG configurations
  // Compares server-provided data against the current state (excluding client-added 'fps')
  const areConfigsEqual = (currentConfig: EegConfig | null, newServerData: any): boolean => {
    if (!currentConfig) return false; // If no current config, new data means change

    // Compare server-provided fields
    if (currentConfig.sample_rate !== newServerData.sample_rate) {
      console.log('Config comparison: sample_rate differs', currentConfig.sample_rate, newServerData.sample_rate);
      return false;
    }
    if (currentConfig.gain !== newServerData.gain) {
      console.log('Config comparison: gain differs', currentConfig.gain, newServerData.gain);
      return false;
    }
    if (currentConfig.board_driver !== newServerData.board_driver) {
      console.log('Config comparison: board_driver differs', currentConfig.board_driver, newServerData.board_driver);
      return false;
    }
    if (currentConfig.batch_size !== newServerData.batch_size) {
      console.log('Config comparison: batch_size differs', currentConfig.batch_size, newServerData.batch_size);
      return false;
    }
    
    // Special handling for powerline_filter_hz which can be null, undefined, or a number
    if (currentConfig.powerline_filter_hz !== newServerData.powerline_filter_hz) {
      console.log('Config comparison: powerline_filter_hz differs',
        currentConfig.powerline_filter_hz, newServerData.powerline_filter_hz,
        'Types:', typeof currentConfig.powerline_filter_hz, typeof newServerData.powerline_filter_hz);
      return false;
    }

    // Compare channels array
    if (!newServerData.channels || !Array.isArray(newServerData.channels) ||
        currentConfig.channels.length !== newServerData.channels.length) {
      console.log('Config comparison: channels array structure differs',
        currentConfig.channels, newServerData.channels);
      return false;
    }
    
    for (let i = 0; i < currentConfig.channels.length; i++) {
      if (currentConfig.channels[i] !== newServerData.channels[i]) {
        console.log('Config comparison: channel at index', i, 'differs',
          currentConfig.channels[i], newServerData.channels[i]);
        return false;
      }
    }

    console.log('Config comparison: configs are equal');
    return true;
  };

  const connectWebSocket = useCallback(() => {
    // Clear any existing reconnect timeout
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current);
      reconnectTimeoutRef.current = null;
    }

    // Close existing connection if any
    if (wsRef.current) {
      try {
        // Remove listeners before closing to prevent triggering reconnect on manual close
        wsRef.current.onopen = null;
        wsRef.current.onmessage = null;
        wsRef.current.onerror = null;
        wsRef.current.onclose = null;
        wsRef.current.close();
      } catch (e) { /* Ignore */ }
      wsRef.current = null;
    }

    setStatus('Connecting...');
    const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
    const ws = new WebSocket(`ws://${wsHost}:8080/config`);
    wsRef.current = ws;

    ws.onopen = () => {
      if (!isProduction) console.log('Config WebSocket: Connection opened');
      setStatus('Connected');
      reconnectAttemptsRef.current = 0; // Reset attempts on success
    };

    ws.onmessage = (event) => {
      try {
        console.log('Config WebSocket (EegConfigProvider): Raw message received:', event.data);
        const data = JSON.parse(event.data as string);
        console.log('Config WebSocket (EegConfigProvider): Parsed data:', data);
        
        // Check if it's a CommandResponse (status and message)
        // This provider should primarily care about full config objects.
        // Error/status messages from the /config endpoint are handled by EegMonitor's WebSocket.
        if (data.status && data.message) {
            console.log('Config WebSocket (EegConfigProvider): Received status message, ignoring:', data);
            // Potentially set status here if it's an error directly affecting this provider's connection
            // For now, we assume EegMonitor handles user-facing config update statuses.
            return;
        }

        // Assume it's a full config object (variable 'data' holds the parsed new server data)
        console.log('Config WebSocket (EegConfigProvider): Current config:', config);
        console.log('Config WebSocket (EegConfigProvider): New config data:', data);
        console.log('Config WebSocket (EegConfigProvider): Current powerline_filter_hz:', config?.powerline_filter_hz);
        console.log('Config WebSocket (EegConfigProvider): New powerline_filter_hz:', data.powerline_filter_hz);
        
        const configsEqual = areConfigsEqual(config, data);
        console.log('Config WebSocket (EegConfigProvider): areConfigsEqual result:', configsEqual);
        
        if (configsEqual) {
          console.log('Config WebSocket (EegConfigProvider): Received EEG configuration, but it is identical to the current one. No update needed.', data);
          
          // If config is already set and status is 'Connected', no need to update status again.
          // If config was null, and this is the first valid data, status should be updated.
          if (!config) {
            const FPS = 60.0; // Keep client-side FPS calculation for now
            const configWithFps = { ...data, fps: FPS };
            setConfig(configWithFps); // Set initial config
            setStatus('Connected');
            console.log('Config WebSocket (EegConfigProvider): Set initial EEG configuration:', configWithFps);
          }
          return; // Configs are the same, do nothing further
        }

        // Configs are different, or it's the first config
        const FPS = 60.0; // Keep client-side FPS calculation for now
        const configWithFps = { ...data, fps: FPS };
        console.log('Config WebSocket (EegConfigProvider): Setting new config with FPS:', configWithFps);
        setConfig(configWithFps);
        console.log('Config WebSocket (EegConfigProvider): Received and applied updated EEG configuration:', configWithFps);
        setStatus('Connected'); // Ensure status reflects that we have a valid config
      } catch (error) {
        console.error('Config WebSocket (EegConfigProvider): Error parsing config data:', error);
        setStatus('Error parsing data');
      }
    };

    ws.onclose = (event) => {
      if (!isProduction) console.log(`Config WebSocket: Connection closed (Code: ${event.code}, Reason: ${event.reason})`);
      // Only attempt reconnect if the closure was unexpected
      if (wsRef.current === ws) { // Check if this is still the active WebSocket instance
          setStatus('Disconnected');
          wsRef.current = null; // Clear the ref

          // Exponential backoff reconnection logic
          const maxReconnectDelay = 5000; // 5 seconds max
          const baseDelay = 500; // 0.5 seconds base
          const reconnectDelay = Math.min(
              maxReconnectDelay,
              baseDelay * Math.pow(1.5, reconnectAttemptsRef.current)
          );

          reconnectAttemptsRef.current++;
          if (!isProduction) console.log(`Config WebSocket: Attempting reconnect in ${reconnectDelay}ms (Attempt ${reconnectAttemptsRef.current})`);

          reconnectTimeoutRef.current = setTimeout(connectWebSocket, reconnectDelay);
      }
    };

    ws.onerror = (event) => {
      console.error('Config WebSocket: Error occurred:', event);
      setStatus('Error');
      // The onclose event will usually fire after an error, triggering reconnect logic there.
    };

  }, [isProduction]); // Removed 'config' from dependencies

  useEffect(() => {
    connectWebSocket(); // Initial connection attempt

    // Cleanup function
    return () => {
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      if (wsRef.current) {
        try {
          // Prevent reconnect logic during cleanup
          wsRef.current.onclose = null;
          wsRef.current.onerror = null;
          wsRef.current.close();
        } catch (e) { /* Ignore */ }
        wsRef.current = null;
      }
    };
  }, [connectWebSocket]); // useEffect depends on the stable connectWebSocket callback

  // refreshConfig is removed as updates are now pushed by the server.
  // The existing connectWebSocket handles initial connection and reconnections on error/close.

  return (
    <EegConfigContext.Provider value={{ config, status }}>
      {children}
    </EegConfigContext.Provider>
  );
}

// Display component
export default function EegConfigDisplay() {
  const { config, status } = useEegConfig();

  return (
    <div className="p-4 bg-gray-900 text-white rounded-lg mb-4">
      <h2 className="text-xl font-bold mb-2">EEG Configuration</h2>
      <div className="mb-2">Status: {status}</div>
      
      {config ? (
        <div className="grid grid-cols-2 gap-2">
          <div className="col-span-2 font-semibold text-blue-400">System Parameters</div>
          
          <div>Sample Rate:</div>
          <div>{config.sample_rate} Hz</div>
          
          <div>Channels:</div>
          <div>{config.channels.join(', ')}</div>
          
          <div>Gain:</div>
          <div>{config.gain}</div>
          
          <div>Board Driver:</div>
          <div>{config.board_driver}</div>
          
          <div>Batch Size:</div>
          <div>{config.batch_size} samples</div>
          
          <div>Powerline Filter:</div>
          <div>{config.powerline_filter_hz === null ? 'Off' :
                config.powerline_filter_hz ? `${config.powerline_filter_hz} Hz` : 'Not set'}</div>
          
          <div>Effective FPS:</div>
          <div>{config.fps?.toFixed(2)} frames/sec</div>
        </div>
      ) : (
        <div className="text-gray-400">Waiting for configuration data...</div>
      )}
    </div>
  );
}