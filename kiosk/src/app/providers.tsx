'use client';

import React, { useEffect, ReactNode } from 'react';
import { EventStreamProvider } from "@/context/EventStreamContext";
import { PipelineProvider, usePipeline } from "@/context/PipelineContext";
import { EegDataProvider } from "@/context/EegDataContext";
import { EegConfigProvider } from "@/components/EegConfig";

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
        <EegDataProvider>
          <EegConfigProvider>
            <PipelineInitializer>
              {children}
            </PipelineInitializer>
          </EegConfigProvider>
        </EegDataProvider>
      </PipelineProvider>
    </EventStreamProvider>
  );
};

export function AppProviders({ children }: { children: React.ReactNode }) {
  return <ComposedProviders>{children}</ComposedProviders>;
}