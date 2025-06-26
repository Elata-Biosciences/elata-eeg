'use client';

import { useEffect, useState, createContext, useContext, useRef, useCallback, useMemo } from 'react';

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
  refreshConfig: () => void;
  isConfigReady: boolean;
}

export const EegConfigContext = createContext<EegConfigContextType>({
  config: null,
  status: 'Initializing...',
  refreshConfig: () => { console.warn('EegConfigContext: refreshConfig called before provider initialization'); },
  isConfigReady: false,
});

// Hook to use the EEG configuration
export const useEegConfig = () => useContext(EegConfigContext);

// Provider component
export function EegConfigProvider({ children }: { children: React.ReactNode }) {
  const [config, setConfig] = useState<EegConfig | null>(null);
  const configRef = useRef<EegConfig | null>(null); // Add a ref for the latest config
  const [status, setStatus] = useState('Initializing...'); // Start as Initializing
  const [isConfigReady, setIsConfigReady] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttemptsRef = useRef<number>(0);
  const isProduction = process.env.NODE_ENV === 'production';

  // Helper function to deeply compare relevant parts of EEG configurations
  // Compares server-provided data against the current state (excluding client-added 'fps')
  const areConfigsEqual = (currentConfig: EegConfig | null, newServerData: any): boolean => {
    if (!currentConfig) return false;
    // Create string representations to safely compare.
    const currentChannels = JSON.stringify(currentConfig.channels?.slice().sort());
    const newChannels = JSON.stringify(newServerData.channels?.slice().sort());

    return (
      currentConfig.sample_rate === newServerData.sample_rate &&
      currentConfig.gain === newServerData.gain &&
      currentConfig.board_driver === newServerData.board_driver &&
      currentConfig.batch_size === newServerData.batch_size &&
      currentConfig.powerline_filter_hz === newServerData.powerline_filter_hz &&
      currentChannels === newChannels
    );
  };

  // Main connection effect
  useEffect(() => {
    const connectWebSocket = () => {
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
    const wsProtocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${wsProtocol}://${wsHost}:8080/config`);
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
        // Only update if the new config is actually different from the current one.
        if (!areConfigsEqual(configRef.current, data)) {
          console.log('Config WebSocket (EegConfigProvider): Received new configuration, applying update.');
          const FPS = 60.0;
          // Create a new config object with the server data and client-side FPS
          const newConfig = { ...data, fps: FPS };
          setConfig(newConfig); // This will trigger a re-render for consumers
          
          if (!isConfigReady) {
            setIsConfigReady(true);
          }
        } else {
            if (!isProduction) {
                console.log('Config WebSocket (EegConfigProvider): Received identical configuration. No update needed.');
            }
        }
        // Always ensure status is 'Connected' after a message, as it confirms the link is alive.
        setStatus('Connected');
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

    };

    connectWebSocket();

    // Cleanup function for the main effect
    return () => {
      console.log("Cleaning up EegConfigProvider effect.");
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      const ws = wsRef.current;
      if (ws) {
        // Prevent reconnect logic during cleanup
        ws.onclose = null;
        ws.onerror = null;
        ws.close();
        wsRef.current = null;
      }
    };
  }, [isProduction, isConfigReady]); // Rerun if isProduction changes

  // Effect to keep configRef updated
  useEffect(() => {
    configRef.current = config;
  }, [config]);

  const refreshConfig = useCallback(() => {
    if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify({ command: 'get_config' }));
    }
  }, []);

  const contextValue = useMemo(() => ({
    config,
    status,
    refreshConfig,
    isConfigReady,
  }), [config, status, refreshConfig, isConfigReady]);

  return (
    <EegConfigContext.Provider value={contextValue}>
      {children}
    </EegConfigContext.Provider>
  );
}

// Display component removed