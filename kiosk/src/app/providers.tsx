'use client';

import React, { useEffect, ReactNode, useState } from 'react';
import { EventStreamProvider } from "../context/EventStreamContext";
import { PipelineProvider, usePipeline } from "../context/PipelineContext";
import { EegDataProvider } from "../context/EegDataContext";
import { EegConfigProvider } from "../hooks/useEegConfig";

// A component to handle the pipeline initialization logic.
const PipelineInitializer = ({ children }: { children: ReactNode }) => {
  const { pipelines, selectAndStartPipeline, pipelineStatus } = usePipeline();

  useEffect(() => {
    // When pipelines are loaded and none is running, start the first one.
    if (pipelines.length > 0 && pipelineStatus === 'stopped') {
      // Automatically start the first available pipeline.
      selectAndStartPipeline(pipelines[0].id);
    }
  }, [pipelines, pipelineStatus, selectAndStartPipeline]);

  return <>{children}</>;
};

// All providers composed into a single, stable component.
const ComposedProviders = ({ children }: { children: ReactNode }) => {
  return (
    <EventStreamProvider>
      <PipelineProvider>
        <EegConfigProvider>
          <EegDataProvider>
            <PipelineInitializer>
              {children}
            </PipelineInitializer>
          </EegDataProvider>
        </EegConfigProvider>
      </PipelineProvider>
    </EventStreamProvider>
  );
};

export function AppProviders({ children }: { children: React.ReactNode }) {
  const [isClient, setIsClient] = useState(false);

  useEffect(() => {
    setIsClient(true);
  }, []);

  if (!isClient) {
    // Return children without providers during SSR
    return <>{children}</>;
  }

  return <ComposedProviders>{children}</ComposedProviders>;
}