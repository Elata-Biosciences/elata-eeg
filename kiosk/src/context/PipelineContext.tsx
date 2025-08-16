'use client';

import React, { createContext, useContext, useState, ReactNode, useEffect, useCallback, useMemo, useRef } from 'react';
import { getPipelines, startPipeline as apiStartPipeline, stopPipeline as apiStopPipeline, getPipelineState, sendCommand as apiSendCommand } from '../utils/api';
import { SystemConfig } from '@/types/eeg';
import { useEventStream } from './EventStreamContext';

interface Pipeline {
  id: string;
  name: string;
}

export interface PipelineState {
  status: 'initializing' | 'stopped' | 'starting' | 'started' | 'error';
  config: SystemConfig | null;
}

interface PipelineContextType {
  pipelines: Pipeline[];
  selectedPipeline: Pipeline | null;
  pipelineConfig: SystemConfig | null;
  pipelineStatus: 'initializing' | 'stopped' | 'starting' | 'started' | 'error';
  selectAndStartPipeline: (id: string) => Promise<void>;
  startPipeline: (id: string) => Promise<void>;
  stopPipeline: () => Promise<void>;
  sendCommand: (command: string, params: any) => Promise<void>;
  sendPowerlineFilterCommand: (value: number | null) => Promise<void>;
  pipelineState: {
    status: string;
    config: any;
  };
}

const PipelineContext = createContext<PipelineContextType | undefined>(undefined);

interface PipelineProviderProps {
  children: ReactNode;
}

export const PipelineProvider = ({ children }: PipelineProviderProps) => {
  const [pipelines, setPipelines] = useState<Pipeline[]>([]);
  const [selectedPipeline, setSelectedPipeline] = useState<Pipeline | null>(null);
  const [pipelineState, setPipelineState] = useState<PipelineState>({
    status: 'initializing',
    config: null,
  });
  const { subscribe } = useEventStream();
  const initializationStarted = useRef(false);

  const selectAndStartPipeline = useCallback(async (id: string) => {
    const pipelineToStart = pipelines.find((p: Pipeline) => p.id === id);
    if (!pipelineToStart) {
      console.error(`Pipeline with id ${id} not found.`);
      setPipelineState({ status: 'error', config: null });
      return;
    }

    setPipelineState(prevState => ({ ...prevState, status: 'starting' }));
    setSelectedPipeline(pipelineToStart);

    try {
      await apiStartPipeline(id);
      console.log(`Pipeline ${id} start command issued successfully.`);
      // State will be updated via SSE events.
    } catch (error) {
      console.error(`Failed to start pipeline ${id}:`, error);
      setPipelineState({ status: 'error', config: null });
    }
  }, [pipelines]);

  const sendCommand = useCallback(async (command: string, params: any) => {
    if (!selectedPipeline) {
      console.error("No pipeline selected, cannot send command.");
      return;
    }
    try {
      await apiSendCommand(selectedPipeline.id, command, params);
      console.log(`Command ${command} sent to pipeline ${selectedPipeline.id} with params:`, params);
    } catch (error) {
      console.error(`Failed to send command ${command} to pipeline ${selectedPipeline.id}:`, error);
    }
  }, [selectedPipeline]);

  const startPipeline = useCallback(async (id: string) => {
    try {
      await apiStartPipeline(id);
    } catch (error) {
      console.error(`Failed to start pipeline ${id}:`, error);
    }
  }, []);

  const stopPipeline = useCallback(async () => {
    if (!selectedPipeline) {
      console.error("No pipeline selected, cannot stop pipeline.");
      return;
    }
    try {
      await apiStopPipeline();
    } catch (error) {
      console.error(`Failed to stop pipeline:`, error);
    }
  }, [selectedPipeline]);

  const sendPowerlineFilterCommand = useCallback(async (value: number | null) => {
    if (!selectedPipeline) {
      console.error("No pipeline selected, cannot send command.");
      return;
    }
    const params = {
      target_stage: 'notch_filter_stage',
      parameter_id: 'frequency',
      value: value,
    };
    try {
      await apiSendCommand(selectedPipeline.id, 'SetParameter', params);
      console.log(`Command SetParameter sent to pipeline ${selectedPipeline.id} with params:`, params);
    } catch (error) {
      console.error(`Failed to send command SetParameter to pipeline ${selectedPipeline.id}:`, error);
    }
  }, [selectedPipeline]);

  useEffect(() => {
    if (initializationStarted.current) return;
    initializationStarted.current = true;

    const resilientFetch = async <T,>(fetchFunc: () => Promise<T>, attempts = 5, delay = 1000): Promise<T> => {
      for (let i = 0; i < attempts; i++) {
        try {
          return await fetchFunc();
        } catch (error: any) {
          if (error instanceof TypeError && error.message.includes('NetworkError')) {
            if (i === attempts - 1) throw error;
            console.warn(`Network error detected. Retrying in ${delay}ms... (Attempt ${i + 1}/${attempts})`);
            await new Promise(resolve => setTimeout(resolve, delay * Math.pow(2, i)));
          } else {
            throw error;
          }
        }
      }
      throw new Error("Max retries reached");
    };

    const initialize = async () => {
      try {
        const availablePipelines = await resilientFetch(getPipelines);
        setPipelines(availablePipelines);

        const currentState = await resilientFetch(getPipelineState);
        if (currentState && currentState.stages.length > 0) {
          console.log('[PipelineProvider] A pipeline is already running. Syncing state.');
          setPipelineState({ status: 'started', config: currentState });
          // When a pipeline is already running, select the 'default' pipeline
          const defaultPipeline = availablePipelines.find((p: Pipeline) => p.id === 'default');
          setSelectedPipeline(defaultPipeline || availablePipelines[0] || null);
        } else {
          console.log('[PipelineProvider] No pipeline running. Starting default pipeline.');
          const defaultPipeline = availablePipelines.find((p: Pipeline) => p.id === 'default');
          if (defaultPipeline) {
            setPipelineState(prevState => ({ ...prevState, status: 'starting' }));
            setSelectedPipeline(defaultPipeline);
            await resilientFetch(() => apiStartPipeline(defaultPipeline.id));
            console.log(`Pipeline ${defaultPipeline.id} start command issued successfully.`);
          } else {
            console.error("No 'default' pipeline found.");
            setPipelineState({ status: 'error', config: null });
          }
        }
      } catch (error) {
        console.error("Failed to initialize pipeline:", error);
        setPipelineState({ status: 'error', config: null });
      }
    };

    initialize();
  }, []);

  useEffect(() => {
    const handlePipelineState = async (data: any) => {
      const newStatus = data.status === 'running' ? 'started' : data.status;
      setPipelineState(prevState => ({
        ...prevState,
        status: newStatus,
      }));

      // If the pipeline has started or is running, fetch the full state
      // to ensure the config is up-to-date.
      if (newStatus === 'started') {
        try {
          const fullState = await getPipelineState();
          if (fullState && fullState.stages.length > 0) {
            setPipelineState(prevState => ({
              ...prevState,
              config: fullState,
            }));
          }
        } catch (error) {
          console.error("Failed to fetch full pipeline state after status change:", error);
        }
      }
    };

    const handlePipelineFailed = () => {
      setPipelineState(prevState => ({ ...prevState, status: 'error' }));
    };

    const unsubscribeState = subscribe('pipeline_state', handlePipelineState);
    const unsubscribeFailed = subscribe('PipelineFailed', handlePipelineFailed);

    return () => {
      unsubscribeState();
      unsubscribeFailed();
    };
  }, [subscribe]);

  const value = useMemo(() => ({
    pipelines,
    selectedPipeline,
    pipelineConfig: pipelineState.config,
    pipelineStatus: pipelineState.status,
    selectAndStartPipeline,
    startPipeline,
    stopPipeline,
    sendCommand,
    sendPowerlineFilterCommand,
    pipelineState,
  }), [pipelines, selectedPipeline, pipelineState, selectAndStartPipeline, startPipeline, stopPipeline, sendCommand, sendPowerlineFilterCommand]);

  return (
    <PipelineContext.Provider value={value}>
      {children}
    </PipelineContext.Provider>
  );
};

// Custom hook to use the pipeline context
export const usePipeline = () => {
  const context = useContext(PipelineContext);
  if (context === undefined) {
    throw new Error('usePipeline must be used within a PipelineProvider');
  }
  return context;
};