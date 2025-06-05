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
    const newWs = new WebSocket(`ws://${wsHost}:8080/command`);
 
    newWs.onopen = () => {
      console.log('Command WebSocket connected');
      setWsConnected(true);
    };

    newWs.onmessage = (event) => {
      try {
        const response = JSON.parse(event.data);

        if (response.status === 'ok') {
          const recording = response.message.startsWith('Currently recording');
          const failedToStart = response.message.includes('Failed to start recording'); // Or other relevant error messages
          let filePath = null;

          if (recording) {
            const match = response.message.match(/Currently recording to (.+)/);
            if (match && match[1]) {
              filePath = match[1];
            }
          }

          setRecordingStatus(response.message);
          setRecordingFilePath(filePath);

          if (recording || failedToStart) {
            console.timeEnd('startRecordingCommand');
            setIsStartRecordingPending(false);
          }
        } else {
          console.error('Command error:', response.message);
          // Potentially clear pending state if the error is related to a start command
          if (isStartRecordingPending) { // Check if a start command was pending
             console.timeEnd('startRecordingCommand'); // End timer if it was running
             setIsStartRecordingPending(false);
          }
        }
      } catch (error) {
        console.error('Error parsing command response:', error);
      }
    };

    newWs.onclose = () => {
      console.log('Command WebSocket disconnected');
      setWsConnected(false);
    };

    newWs.onerror = (error) => {
      console.error('Command WebSocket error:', error);
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
      console.time('startRecordingCommand');
      console.log('Attempting to start recording...');
      ws.send(JSON.stringify({ command: 'start' }));
    }
  }, [ws, isStartRecordingPending]);

  const stopRecording = useCallback(() => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ command: 'stop' }));
    }
  }, [ws]);

  const sendPowerlineFilterCommand = useCallback((value: number | null) => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      console.log(`Sending set_powerline_filter command with value: ${value}`);
      ws.send(JSON.stringify({ command: 'set_powerline_filter', value: value }));
    } else {
      console.warn('Command WebSocket not open, cannot send set_powerline_filter command.');
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