import { createContext, useContext, useState, useEffect, useCallback, ReactNode } from 'react';
import { configWebSocket } from '../utils/api';
import { AdcConfig } from '../types/eeg';

interface EegConfigState {
  authoritative: AdcConfig | null;
  draft: AdcConfig | null;
  pending: boolean;
  error: string | null;
}

interface EegConfigContextType extends EegConfigState {
  applyConfig: (config: AdcConfig) => void;
  updateDraft: (config: Partial<AdcConfig>) => void;
}

const EegConfigContext = createContext<EegConfigContextType | undefined>(undefined);

export const useEegConfig = () => {
  const context = useContext(EegConfigContext);
  if (!context) {
    throw new Error('useEegConfig must be used within an EegConfigProvider');
  }
  return context;
};

export const EegConfigProvider = ({ children }: { children: ReactNode }) => {
  const [state, setState] = useState<EegConfigState>({
    authoritative: null,
    draft: null,
    pending: false,
    error: null,
  });

  const handleMessage = useCallback((event: MessageEvent) => {
    try {
      const message = JSON.parse(event.data);
      if (message.type === 'Applied') {
        const newConfig = message.config as AdcConfig;
        setState(prevState => ({
          ...prevState,
          authoritative: newConfig,
          draft: prevState.pending ? prevState.draft : newConfig, // Preserve draft if pending
          pending: false,
          error: null,
        }));
      } else if (message.type === 'Rejected') {
        setState(prevState => ({
          ...prevState,
          pending: false,
          error: message.reason,
        }));
      }
    } catch (error) {
      console.error('Failed to parse WebSocket message:', error);
    }
  }, []);

  useEffect(() => {
    configWebSocket.onmessage = handleMessage;
    configWebSocket.onopen = () => {
      console.log('Config WebSocket connected');
    };
    configWebSocket.onclose = () => {
      console.log('Config WebSocket disconnected. Attempting to reconnect...');
    };
    configWebSocket.onerror = (error) => {
      console.error('Config WebSocket error:', error);
    };
  }, [handleMessage]);

  const applyConfig = useCallback((config: AdcConfig) => {
    setState(prevState => ({ ...prevState, pending: true, error: null }));
    const proposal = { config };
    configWebSocket.send(JSON.stringify(proposal));
  }, []);

  const updateDraft = useCallback((update: Partial<AdcConfig>) => {
    setState(prevState => ({
      ...prevState,
      draft: { ...prevState.draft!, ...update },
    }));
  }, []);

  const value = {
    ...state,
    applyConfig,
    updateDraft,
  };

  return (
    <EegConfigContext.Provider value={value}>
      {children}
    </EegConfigContext.Provider>
  );
};