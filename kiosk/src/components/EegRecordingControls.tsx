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
    <div className="flex flex-col">
      <button
        onClick={isPending ? undefined : (isRecording ? stopRecording : startRecording)}
        disabled={isPending}
        className={`px-4 py-1 rounded-md flex items-center ${
          isPending
            ? 'bg-yellow-500 text-white cursor-wait'
            : isRecording
              ? 'bg-red-600 hover:bg-red-700 text-white'
              : 'bg-green-600 hover:bg-green-700 text-white'
        }`}
      >
        {isPending ? (
          <>
            <svg className="animate-spin -ml-1 mr-3 h-5 w-5 text-white" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
              <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            Pending...
          </>
        ) : isRecording ? (
          <>
            <span className="inline-block w-2 h-2 rounded-full bg-white mr-2"></span>
            Stop Recording
          </>
        ) : (
          <>
            <span className="inline-block w-2 h-2 rounded-full bg-white mr-2"></span>
            Start Recording
          </>
        )}
      </button>
      {error && (
        <div className="mt-1 text-red-400 text-xs">
          {error}
        </div>
      )}
    </div>
  );
}