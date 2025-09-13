// This file centralizes all API calls to the backend daemon.

/**
 * Fetches the list of available pipelines from the backend.
 * @returns A promise that resolves to an array of available pipelines.
 */
export const getPipelines = async () => {
  try {
    const response = await fetch('/api/pipelines');
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    return await response.json();
  } catch (error) {
    console.error("Failed to fetch pipelines:", error);
    throw error;
  }
};

/**
 * Sends a request to start a specific pipeline by its ID.
 * @param id The ID of the pipeline to start.
 * @returns A promise that resolves when the request is successful.
 */
export const startPipeline = async (id: string) => {
  try {
    const response = await fetch(`/api/pipelines/${id}/start`, {
      method: 'POST',
    });
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    return response;
  } catch (error) {
    console.error(`Failed to start pipeline ${id}:`, error);
    throw error;
  }
};

/**
 * Fetches the current state of the running pipeline.
 * @returns A promise that resolves to the pipeline's configuration.
 */
export const getPipelineState = async () => {
  try {
    const response = await fetch('/api/state');
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    return await response.json();
  } catch (error) {
    console.error("Failed to fetch pipeline state:", error);
    throw error;
  }
};

/**
 * Sends a request to stop the currently running pipeline.
 * @returns A promise that resolves when the request is successful.
 */
export const stopPipeline = async () => {
  try {
    const response = await fetch(`/api/pipelines/stop`, {
      method: 'POST',
    });
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    return response;
  } catch (error) {
    console.error(`Failed to stop pipeline:`, error);
    throw error;
  }
};

/**
 * Sends a generic control command to the backend.
 * @param command The command payload to send.
 * @returns A promise that resolves when the request is successful.
 */

/**
 * Sends a command to a specific pipeline.
 * @param pipelineId The ID of the pipeline to command.
 * @param command The command to send (e.g., "SetParameter").
 * @param params The parameters for the command.
 * @returns A promise that resolves when the request is successful.
 */
export const sendCommand = async (pipelineId: string, command: string, params: any) => {
  try {
    const response = await fetch(`/api/pipelines/${pipelineId}/control`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ [command]: params }),
    });
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    return response;
  } catch (error) {
    console.error(`Failed to send command to pipeline ${pipelineId}:`, error);
    throw error;
  }
};
// WebSocket for configuration management with automatic reconnection
class ResilientWebSocket {
  private ws: WebSocket | null = null;
  private url: string;
  private reconnectInterval = 1000;
  private maxReconnectInterval = 30000;
  private reconnectAttempts = 0;

  constructor(url: string) {
    this.url = url;
    if (typeof window !== 'undefined') {
      this.connect();
    }
  }

  private connect() {
    if (typeof window === 'undefined') return;

    this.ws = new WebSocket(this.url);

    this.ws.onopen = () => {
      console.log('WebSocket connected to', this.url);
      this.reconnectAttempts = 0;
      this.reconnectInterval = 1000;
      this.onopen?.();
    };

    this.ws.onmessage = (event) => {
      this.onmessage?.(event);
    };

    this.ws.onerror = (event) => {
      console.error('WebSocket error:', event);
      this.onerror?.(event);
    };

    this.ws.onclose = () => {
      console.log('WebSocket disconnected from', this.url);
      this.scheduleReconnect();
      this.onclose?.();
    };
  }

  private scheduleReconnect() {
    this.reconnectAttempts++;
    const backoffTime = Math.min(
      this.maxReconnectInterval,
      this.reconnectInterval * Math.pow(2, this.reconnectAttempts)
    );

    console.log(`Scheduling reconnect in ${backoffTime / 1000}s`);

    setTimeout(() => this.connect(), backoffTime);
  }

  public send(data: string | ArrayBufferLike | Blob | ArrayBufferView) {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(data);
    } else {
      console.error('WebSocket is not open. ReadyState:', this.ws?.readyState);
    }
  }

  public addEventListener(type: string, listener: EventListenerOrEventListenerObject, options?: boolean | AddEventListenerOptions) {
    this.ws?.addEventListener(type, listener, options);
  }

  public removeEventListener(type: string, listener: EventListenerOrEventListenerObject, options?: boolean | EventListenerOptions) {
    this.ws?.removeEventListener(type, listener, options);
  }

  public onopen: (() => void) | null = null;
  public onmessage: ((event: MessageEvent) => void) | null = null;
  public onerror: ((event: Event) => void) | null = null;
  public onclose: (() => void) | null = null;
}

let configWebSocketInstance: ResilientWebSocket;

if (typeof window !== 'undefined') {
  // Use NEXT_PUBLIC_DAEMON_URL as the single source of truth for the daemon's location.
  // Default to localhost for local development.
  const daemonUrl = process.env.NEXT_PUBLIC_DAEMON_URL || 'http://localhost:9000';
  const wsUrl = daemonUrl.replace(/^http/, 'ws') + '/ws/config';
  configWebSocketInstance = new ResilientWebSocket(wsUrl);
}

// @ts-ignore
export { configWebSocketInstance as configWebSocket };
