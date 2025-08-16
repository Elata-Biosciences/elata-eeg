'use client';

/**
 * EegDataHandler.tsx
 *
 * This component handles WebSocket connections to the EEG data server and processes incoming data.
 *
 * This implementation uses a constant FPS rendering approach, removing the need for
 * render flags and simplifying the overall rendering process.
 */

import { useEegData } from '../context/EegDataContext';

/**
 * This is a placeholder component. The data handling logic has been moved to EegDataContext.
 * This component can be used for any future UI related to data handling.
 */
export function EegDataHandler() {
  const { dataStatus } = useEegData();

  return (
    <div style={{ display: 'none' }}>
      {/* This component is now a consumer of EegDataContext */}
      {/* It can be used to display status or trigger actions based on dataStatus */}
      <p>WebSocket Status: {dataStatus.wsStatus}</p>
      <p>Data Received: {dataStatus.dataReceived ? 'Yes' : 'No'}</p>
      {dataStatus.driverError && <p>Error: {dataStatus.driverError}</p>}
    </div>
  );
}

// The hook is no longer needed as the logic is in EegDataContext
export function useEegDataHandler() {
    // This hook can be removed or repurposed if needed.
    // For now, it returns a dummy status.
    return { status: 'managed_by_context' };
}
