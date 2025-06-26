'use client'; // Required for useState

import EegMonitor from '@/components/EegMonitor';
import { EegConfigProvider, useEegConfig } from '@/components/EegConfig';
import { CommandWebSocketProvider } from '@/context/CommandWebSocketContext';
import { EegDataProvider } from '@/context/EegDataContext';
import { useContext } from 'react';

// A new component to wrap the EegMonitor and access the context
function EegMonitorWrapper() {
  const { isConfigReady, status } = useEegConfig();

  if (!isConfigReady) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center">
          <p className="text-2xl font-bold mb-2">Initializing EEG Monitor...</p>
          <p className="text-lg text-gray-400">Status: {status}</p>
        </div>
      </div>
    );
  }

  return <EegMonitor />;
}

export default function Home() {
  return (
    <div className="flex flex-col min-h-screen bg-gray-900 text-white">
      <EegConfigProvider>
        <CommandWebSocketProvider>
          <EegDataProvider>
            <div className="flex-grow">
              <EegMonitorWrapper />
            </div>
          </EegDataProvider>
        </CommandWebSocketProvider>
      </EegConfigProvider>
    </div>
  );
}
