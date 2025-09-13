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

// Create a singleton instance of the event stream manager
class EventStreamManager {
  private eventSource: EventSource | null = null;
  private listeners: Record<string, Record<string, (data: any) => void>> = {};
  private reconnectTimer: NodeJS.Timeout | null = null;
  private reconnectAttempts = 0;
  public isConnected = false;
  public error: string | null = null;
  public fatalError: string | null = null;
  private stateChangeCallback: (() => void) | null = null;

  constructor() {
    if (typeof window !== 'undefined') {
      this.connect();
    }
  }

  private setState(updater: Partial<EventStreamManager>) {
    Object.assign(this, updater);
    this.stateChangeCallback?.();
  }

  public registerStateChangeCallback(callback: () => void) {
    this.stateChangeCallback = callback;
  }

  public connect() {
    if (this.eventSource) return;

    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }

    console.log('[EventStream] Connecting to SSE endpoint...');
    const daemonUrl = process.env.NEXT_PUBLIC_DAEMON_URL || 'http://localhost:9000';
    this.eventSource = new EventSource(`${daemonUrl}/api/events`);

    this.eventSource.onopen = () => {
      console.log('[EventStream] SSE connection established.');
      this.setState({ isConnected: true, error: null });
      this.reconnectAttempts = 0;
    };

    this.eventSource.onmessage = (event) => {
      try {
        const parsedData = JSON.parse(event.data);
        const eventType = Object.keys(parsedData)[0] as EventType;
        const eventPayload = parsedData[eventType];
        
        if (eventType === 'PipelineFailed') {
          console.error(`[EventStream] Fatal pipeline error: ${(eventPayload as any).error}`);
          this.setState({ fatalError: (eventPayload as any).error, isConnected: false });
          this.eventSource?.close();
        }

        if (this.listeners[eventType]) {
          Object.values(this.listeners[eventType]).forEach(callback => callback(eventPayload));
        }
      } catch (err) {
        console.error('[EventStream] Error parsing event data:', err);
      }
    };

    this.eventSource.onerror = (err) => {
      console.error('[EventStream] SSE connection error:', err);
      this.eventSource?.close();
      this.eventSource = null;
      this.setState({ isConnected: false });

      const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000);
      this.reconnectAttempts++;
      this.setState({ error: `Connection lost. Retrying in ${delay / 1000}s...` });
      console.log(`[EventStream] Attempting to reconnect in ${delay}ms (attempt ${this.reconnectAttempts})`);
      this.reconnectTimer = setTimeout(() => this.connect(), delay);
    };
  }

  public disconnect() {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.eventSource?.close();
    this.eventSource = null;
    this.setState({ isConnected: false });
    console.log('[EventStream] Disconnected from SSE endpoint');
  }

  public subscribe(eventType: string, callback: (data: any) => void) {
    const id = Math.random().toString(36).substring(2, 9);
    if (!this.listeners[eventType]) {
      this.listeners[eventType] = {};
    }
    this.listeners[eventType][id] = callback;
    return () => {
      delete this.listeners[eventType][id];
      if (Object.keys(this.listeners[eventType]).length === 0) {
        delete this.listeners[eventType];
      }
    };
  }
}

const eventStreamManager = new EventStreamManager();

export function EventStreamProvider({ children }: { children: React.ReactNode }) {
  const [, forceUpdate] = useState({});

  useEffect(() => {
    const callback = () => forceUpdate({});
    eventStreamManager.registerStateChangeCallback(callback);
    return () => eventStreamManager.registerStateChangeCallback(() => {});
  }, []);

  const stableValue = useMemo(() => ({
    subscribe: eventStreamManager.subscribe.bind(eventStreamManager),
    connect: eventStreamManager.connect.bind(eventStreamManager),
    disconnect: eventStreamManager.disconnect.bind(eventStreamManager),
  }), []);

  const dynamicValue = useMemo(() => ({
    isConnected: eventStreamManager.isConnected,
    error: eventStreamManager.error,
    fatalError: eventStreamManager.fatalError,
  }), [eventStreamManager.isConnected, eventStreamManager.error, eventStreamManager.fatalError]);

  return (
    <EventStreamStableContext.Provider value={stableValue}>
      <EventStreamDynamicContext.Provider value={dynamicValue}>
        {children}
      </EventStreamDynamicContext.Provider>
    </EventStreamStableContext.Provider>
  );
}