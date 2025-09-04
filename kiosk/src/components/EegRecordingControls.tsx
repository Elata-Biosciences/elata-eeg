'use client';

import { useState, useEffect } from 'react';
import { usePipeline } from '../context/PipelineContext';
import { useEventStream } from '../context/EventStreamContext';

export default function EegRecordingControls() {
  const { sendCommand } = usePipeline();
  const { subscribe } = useEventStream();
  const [isRecording, setIsRecording] = useState(false);
  const [isPending, setIsPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const handleRecordingState = (data: any) => {
      if (data.event === 'started') {
        setIsRecording(true);
        setIsPending(false);
      } else if (data.event === 'stopped') {
        setIsRecording(false);
        setIsPending(false);
      } else if (data.event === 'error') {
        setError(data.message);
        setIsPending(false);
      }
    };

    const unsubscribe = subscribe('recording_state', handleRecordingState);
    return () => unsubscribe();
  }, [subscribe]);

  const startRecording = async () => {
    setIsPending(true);
    setError(null);
    await sendCommand('StartRecording', {});
  };

  const stopRecording = async () => {
    setIsPending(true);
    setError(null);
    await sendCommand('StopRecording', {});
  };

  return (
    <div className="flex items-center space-x-3">
      {/* Recording Status Indicator */}
      {isRecording && (
        <div className="flex items-center px-3 py-1 rounded-full" style={{
          backgroundColor: 'color-mix(in srgb, var(--color-accent-red) 15%, var(--color-white))',
          border: '1px solid color-mix(in srgb, var(--color-accent-red) 30%, transparent)'
        }}>
          <div className="w-2 h-2 rounded-full mr-2 animate-pulse" style={{
            backgroundColor: 'var(--color-accent-red)'
          }}></div>
          <span className="text-xs font-medium" style={{
            color: 'var(--color-accent-red)',
            fontFamily: 'var(--font-family-ui)'
          }}>REC</span>
        </div>
      )}
      
      {/* Record/Stop Button */}
      <button
        onClick={isPending ? undefined : (isRecording ? stopRecording : startRecording)}
        disabled={isPending}
        className="w-12 h-12 rounded-full flex items-center justify-center transition-all duration-200 shadow-sm hover:shadow-md transform hover:scale-105"
        style={{
          backgroundColor: isRecording ? 'var(--color-accent-red)' : 'var(--color-white)',
          color: isRecording ? 'var(--color-white)' : 'var(--color-off-black)',
          border: `1px solid ${isRecording ? 'var(--color-accent-red)' : 'var(--color-gray2)'}`,
          cursor: isPending ? 'wait' : 'pointer',
          minHeight: '44px'
        }}
        title={isPending ? 'Processing...' : isRecording ? 'Stop Recording' : 'Start Recording'}
      >
        {isPending ? (
          <svg className="animate-spin h-5 w-5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
        ) : isRecording ? (
          // Stop icon - solid square
          <svg className="h-5 w-5" xmlns="http://www.w3.org/2000/svg" fill="currentColor" viewBox="0 0 24 24">
            <rect x="7" y="7" width="10" height="10" rx="1" />
          </svg>
        ) : (
          // Record icon - solid circle
          <svg className="h-6 w-6" xmlns="http://www.w3.org/2000/svg" fill="currentColor" viewBox="0 0 24 24">
            <circle cx="12" cy="12" r="5" />
          </svg>
        )}
      </button>
      {error && (
        <div className="mt-1 text-xs" style={{
          color: 'var(--color-accent-red)',
          fontSize: 'var(--font-size-xs)'
        }}>
          {error}
        </div>
      )}
    </div>
  );
}