'use client';

import { createContext, useContext, useEffect, useState, useCallback, useMemo, useRef } from 'react';

// Define the types for the events based on the API documentation
type EventType =
  | 'pipeline_state'
  | 'parameter_update'
  | 'error'
  | 'info'
  | 'data_update'
  | 'PipelineFailed'
  | 'SourceReady';

interface PipelineStateEvent {
  type: 'pipeline_state';
  data: {
    id: string;
    name: string;
    status: 'running' | 'stopped' | 'error';
    stages: Array<{
      id: string;
      name: string;
      parameters: Record<string, any>;
    }>;
  };
}

interface ParameterUpdateEvent {
  type: 'parameter_update';
  data: {
    stage_id: string;
    parameter_id: string;
    value: any;
  };
}

interface ErrorEvent {
  type: 'error';
  data: {
    message: string;
    code?: string;
  };
}

interface InfoEvent {
  type: 'info';
  data: {
    message: string;
  };
}

interface DataUpdateEvent {
  type: 'data_update';
  data: {
    timestamp: number;
    sample_count: number;
  };
}

interface SourceReadyEvent {
  type: 'SourceReady';
  data: Record<string, any>;
}

interface PipelineFailedEvent {
  type: 'PipelineFailed';
  data: {
    error: string;
  };
}

type EventData =
  | PipelineStateEvent
  | ParameterUpdateEvent
  | ErrorEvent
  | InfoEvent
  | DataUpdateEvent
  | SourceReadyEvent
  | PipelineFailedEvent;

// Separate stable and dynamic context values
interface EventStreamContextStableType {
  subscribe: (eventType: string, callback: (data: any) => void) => () => void;
  connect: () => void;
  disconnect: () => void;
}

interface EventStreamContextDynamicType {
  isConnected: boolean;
  error: string | null;
  fatalError: string | null;
}

const EventStreamStableContext = createContext<EventStreamContextStableType | undefined>(undefined);
const EventStreamDynamicContext = createContext<EventStreamContextDynamicType | undefined>(undefined);

export const useEventStream = () => {
  const context = useContext(EventStreamStableContext);
  if (!context) {
    throw new Error('useEventStream must be used within an EventStreamProvider');
  }
  return context;
};

export const useEventStreamData = () => {
  const context = useContext(EventStreamDynamicContext);
  if (!context) {
    throw new Error('useEventStreamData must be used within an EventStreamProvider');
  }
  return context;
};

export function EventStreamProvider({ children }: { children: React.ReactNode }) {
  const [isConnected, setIsConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fatalError, setFatalError] = useState<string | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);
  const listeners = useRef<Record<string, Record<string, (data: any) => void>>>({});
  const reconnectTimerRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttemptsRef = useRef(0);

  const subscribe = useCallback((eventType: string, callback: (data: any) => void) => {
    const id = Math.random().toString(36).substring(2, 9);
    if (!listeners.current[eventType]) {
      listeners.current[eventType] = {};
    }
    listeners.current[eventType][id] = callback;

    // Return an unsubscribe function
    return () => {
      delete listeners.current[eventType][id];
      if (Object.keys(listeners.current[eventType]).length === 0) {
        delete listeners.current[eventType];
      }
    };
  }, []);

  const disconnect = useCallback(() => {
    if (eventSourceRef.current) {
      console.log('[EventStream] Disconnecting from SSE endpoint');
      eventSourceRef.current.close();
      eventSourceRef.current = null;
      setIsConnected(false);
    }
  }, []);

  const connect = useCallback(() => {
    if (eventSourceRef.current) {
      console.log('[EventStream] Already connected or connecting.');
      return;
    }

    // Clear any existing reconnect timer
    if (reconnectTimerRef.current) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }

    console.log('[EventStream] Connecting to SSE endpoint...');
    const eventSource = new EventSource('/api/events');
    eventSourceRef.current = eventSource;
    setFatalError(null);

    eventSource.onopen = () => {
      console.log('[EventStream] SSE connection established.');
      setIsConnected(true);
      setError(null);
      reconnectAttemptsRef.current = 0; // Reset reconnect attempts on successful connection
    };

    eventSource.onmessage = (event) => {
      try {
        const parsedData = JSON.parse(event.data);
        const eventType = Object.keys(parsedData)[0] as EventType;
        const eventPayload = parsedData[eventType];
        const eventData = { type: eventType, data: eventPayload } as EventData;

        if (eventData.type === 'PipelineFailed') {
          console.error(`[EventStream] Fatal pipeline error: ${eventData.data.error}`);
          setFatalError(eventData.data.error);
          eventSource.close();
          setIsConnected(false);
        }

        if (listeners.current[eventType]) {
          Object.values(listeners.current[eventType]).forEach(callback => {
            try {
              callback(eventPayload);
            } catch (e) {
              console.error(`[EventStream] Error in event listener for ${eventType}:`, e);
            }
          });
        }
      } catch (err) {
        console.error('[EventStream] Error parsing event data:', err);
      }
    };

    eventSource.onerror = (err) => {
      console.error('[EventStream] SSE connection error:', err);
      setIsConnected(false);
      eventSource.close();
      eventSourceRef.current = null;

      // Implement exponential backoff for reconnection
      const attempt = reconnectAttemptsRef.current;
      const delay = Math.min(1000 * Math.pow(2, attempt), 30000); // Max 30s delay
      reconnectAttemptsRef.current++;

      setError(`Connection lost. Retrying in ${delay / 1000}s...`);
      console.log(`[EventStream] Attempting to reconnect in ${delay}ms (attempt ${attempt + 1})`);

      reconnectTimerRef.current = setTimeout(connect, delay);
    };
  }, []);

  useEffect(() => {
    connect();
    return () => {
      disconnect();
    };
  }, [connect, disconnect]);

  const stableValue = useMemo(() => ({
    subscribe,
    connect,
    disconnect,
  }), [subscribe, connect, disconnect]);

  const dynamicValue = useMemo(() => ({
    isConnected,
    error,
    fatalError,
  }), [isConnected, error, fatalError]);

  return (
    <EventStreamStableContext.Provider value={stableValue}>
      <EventStreamDynamicContext.Provider value={dynamicValue}>
        {children}
      </EventStreamDynamicContext.Provider>
    </EventStreamStableContext.Provider>
  );
}