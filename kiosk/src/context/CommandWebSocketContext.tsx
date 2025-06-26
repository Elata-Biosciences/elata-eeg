'use client';

import { createContext, useState, useEffect, useContext, useMemo, useCallback } from 'react';

interface CommandWebSocketContextType {
  ws: WebSocket | null;
  wsConnected: boolean;
  startRecording: () => void;
  stopRecording: () => void;
  sendPowerlineFilterCommand: (value: number | null) => void; // Added
  recordingStatus: string;
  recordingFilePath: string | null;
  isStartRecordingPending: boolean;
}

const CommandWebSocketContext = createContext<CommandWebSocketContextType | undefined>(
  undefined
);

export const CommandWebSocketProvider = ({
  children,
}: {
  children: React.ReactNode;
}) => {
  const [ws, setWs] = useState<WebSocket | null>(null);
  const [wsConnected, setWsConnected] = useState(false);
  const [recordingStatus, setRecordingStatus] = useState('Not recording');
  const [recordingFilePath, setRecordingFilePath] = useState<string | null>(null);
  const [isStartRecordingPending, setIsStartRecordingPending] = useState(false);

  useEffect(() => {
    const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
    const wsProtocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss' : 'ws';
    const newWs = new WebSocket(`${wsProtocol}://${wsHost}:8080/command`);
 
    newWs.onopen = () => {
      console.log('[Command] WebSocket connected.');
      setWsConnected(true);
    };

    newWs.onmessage = (event) => {
      console.log(`[Command] Received message:`, event.data);
      try {
        const response = JSON.parse(event.data);

        if (response.status === 'ok') {
          const isRecording = response.message.startsWith('Currently recording');
          const isStopped = response.message.includes('Recording stopped');
          const failedToStart = response.message.includes('Failed to start recording');
          let filePath = null;

          if (isRecording) {
            const match = response.message.match(/Currently recording to (.+)/);
            if (match && match[1]) {
              filePath = match[1];
              console.log(`[Command] Recording started. File path: ${filePath}`);
            }
          } else if (isStopped) {
            console.log(`[Command] Recording stopped.`);
          }

          setRecordingStatus(response.message);
          setRecordingFilePath(filePath);

          // Clear pending state if we received a definitive response
          if (isRecording || failedToStart || isStopped) {
            if(isStartRecordingPending) {
              console.timeEnd('startRecordingCommand');
              setIsStartRecordingPending(false);
            }
          }
        } else {
          console.error('[Command] Received error:', response.message);
          if (isStartRecordingPending) {
             console.timeEnd('startRecordingCommand');
             setIsStartRecordingPending(false);
          }
        }
      } catch (error) {
        console.error('[Command] Error parsing response JSON:', error);
      }
    };

    newWs.onclose = () => {
      console.log('[Command] WebSocket disconnected.');
      setWsConnected(false);
    };

    newWs.onerror = (error) => {
      console.error('[Command] WebSocket error:', error);
      setWsConnected(false);
    };

    setWs(newWs);

    return () => {
      newWs.close();
    };
  }, []);

  const startRecording = useCallback(() => {
    if (ws && ws.readyState === WebSocket.OPEN && !isStartRecordingPending) {
      setIsStartRecordingPending(true);
      console.log('[Command] Sending "start" command...');
      console.time('startRecordingCommand');
      ws.send(JSON.stringify({ command: 'start' }));
    }
  }, [ws, isStartRecordingPending]);

  const stopRecording = useCallback(() => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      console.log('[Command] Sending "stop" command...');
      ws.send(JSON.stringify({ command: 'stop' }));
    }
  }, [ws]);

  const sendPowerlineFilterCommand = useCallback((value: number | null) => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      console.log(`[Command] Sending "set_powerline_filter" command with value: ${value}`);
      ws.send(JSON.stringify({ command: 'set_powerline_filter', value: value }));
    } else {
      console.warn('[Command] WebSocket not open, cannot send "set_powerline_filter" command.');
    }
  }, [ws]);

  const value = useMemo(() => ({
    ws,
    wsConnected,
    startRecording,
    stopRecording,
    sendPowerlineFilterCommand, // Added
    recordingStatus,
    recordingFilePath,
    isStartRecordingPending,
  }), [ws, wsConnected, startRecording, stopRecording, sendPowerlineFilterCommand, recordingStatus, recordingFilePath, isStartRecordingPending]);

  return (
    <CommandWebSocketContext.Provider value={value}>
      {children}
    </CommandWebSocketContext.Provider>
  );
};

export const useCommandWebSocket = () => {
  const context = useContext(CommandWebSocketContext);
  if (!context) {
    throw new Error(
      'useCommandWebSocket must be used within a CommandWebSocketProvider'
    );
  }
  return context;
};