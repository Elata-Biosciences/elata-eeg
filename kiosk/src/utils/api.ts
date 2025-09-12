// This file centralizes all API calls to the backend daemon.

/**
 * Fetches the list of available pipelines from the backend.
 * @returns A promise that resolves to an array of available pipelines.
 */
export const getPipelines = async () => {
  try {
    const protocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'https' : 'http';
    const host = typeof window !== 'undefined' ? window.location.hostname : '127.0.0.1';
    const base = `${protocol}://${host}:9000`;
    const response = await fetch(`${base}/api/pipelines`);
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
    const protocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'https' : 'http';
    const host = typeof window !== 'undefined' ? window.location.hostname : '127.0.0.1';
    const base = `${protocol}://${host}:9000`;
    const response = await fetch(`${base}/api/pipelines/${id}/start`, {
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
    const protocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'https' : 'http';
    const host = typeof window !== 'undefined' ? window.location.hostname : '127.0.0.1';
    const base = `${protocol}://${host}:9000`;
    const response = await fetch(`${base}/api/state`);
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
    const protocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'https' : 'http';
    const host = typeof window !== 'undefined' ? window.location.hostname : '127.0.0.1';
    const base = `${protocol}://${host}:9000`;
    const response = await fetch(`${base}/api/pipelines/stop`, {
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
    const protocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'https' : 'http';
    const host = typeof window !== 'undefined' ? window.location.hostname : '127.0.0.1';
    const base = `${protocol}://${host}:9000`;

    // Send control commands through the pipeline control endpoint
    // For unit variants like StartRecording/StopRecording, send the JSON payload as a string (serde unit variant)
    const isUnitVariant = command === 'StartRecording' || command === 'StopRecording' || command === 'Start' || command === 'Pause' || command === 'Resume' || command === 'Shutdown' || command === 'Drain';
    const body = isUnitVariant ? JSON.stringify(command) : JSON.stringify({ [command]: params });

    const response = await fetch(`${base}/api/pipelines/${pipelineId}/control`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body,
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