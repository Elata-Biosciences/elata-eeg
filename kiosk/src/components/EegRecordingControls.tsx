'use client';

import { useCommandWebSocket } from '../context/CommandWebSocketContext';

export default function EegRecordingControls() {
  const {
    wsConnected,
    startRecording,
    stopRecording,
    recordingStatus,
    recordingFilePath,
    isStartRecordingPending,
  } = useCommandWebSocket();


  return (
    <button
      onClick={isStartRecordingPending ? undefined : (recordingStatus.startsWith('Currently recording') ? stopRecording : startRecording)}
      disabled={!wsConnected || isStartRecordingPending}
      className={`px-4 py-1 rounded-md flex items-center ${
        ((value) => {
          return !wsConnected
            ? 'bg-gray-700 text-gray-500 cursor-not-allowed'
            : value
              ? 'bg-yellow-500 text-white cursor-wait'
              : recordingStatus.startsWith('Currently recording')
                ? 'bg-red-600 hover:bg-red-700 text-white'
                : 'bg-green-600 hover:bg-green-700 text-white';
        })(isStartRecordingPending)
      }`}
    >
      {isStartRecordingPending ? (
        <>
          <svg className="animate-spin -ml-1 mr-3 h-5 w-5 text-white" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          Pending...
        </>
      ) : recordingStatus.startsWith('Currently recording') ? (
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
  );
}