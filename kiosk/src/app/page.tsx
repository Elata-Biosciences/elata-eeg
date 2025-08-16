'use client'; // Required for useState

import EegMonitor from '@/components/EegMonitor';
import { PipelineProvider, usePipeline } from '@/context/PipelineContext';
import { EventStreamProvider } from '@/context/EventStreamContext';
import { EegDataProvider, useEegStatus } from '@/context/EegDataContext';

// This wrapper component now determines readiness based on the new isReady flag
function EegMonitorWrapper() {
  const { pipelineStatus } = usePipeline();
  const { isReady, dataStatus } = useEegStatus();

  if (!isReady) {
    return (
      <div className="flex items-center justify-center h-screen">
        <div className="text-center">
          <h1 className="text-2xl font-bold mb-2">Initializing EEG Monitor...</h1>
          <p className="text-lg text-gray-400">
            Status: {pipelineStatus === 'starting' ? 'Starting pipeline...' : (dataStatus.wsStatus || 'Waiting for configuration...')}
          </p>
        </div>
      </div>
    );
  }

  return <EegMonitor />;
}

export default function Home() {
  return (
    <main className="flex flex-col h-screen bg-gray-900 text-white">
      <EventStreamProvider>
        <PipelineProvider>
          <EegDataProvider>
            <EegMonitorWrapper />
          </EegDataProvider>
        </PipelineProvider>
      </EventStreamProvider>
    </main>
  );
}
