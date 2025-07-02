'use client';
import React from 'react'; // Added to resolve React.Fragment error

import { useRef, useState, useEffect } from 'react';
import { useEegConfig } from './EegConfig';
import EegRecordingControls from './EegRecordingControls'; // Import the actual controls
import { useCommandWebSocket } from '../context/CommandWebSocketContext';
import { useEegData } from '../context/EegDataContext';
import EegDataVisualizer from './EegDataVisualizer';

export default function EegMonitorWebGL() {
  type DataView = 'signalGraph' | 'appletBrainWaves';
  type ActiveView = DataView | 'settings';
 
  const [activeView, setActiveView] = useState<ActiveView>('signalGraph');
  const [lastActiveDataView, setLastActiveDataView] = useState<DataView>('signalGraph');
  
  const [configWebSocket, setConfigWebSocket] = useState<WebSocket | null>(null); // Restored
  const [isConfigWsOpen, setIsConfigWsOpen] = useState(false); // Restored
  const [configUpdateStatus, setConfigUpdateStatus] = useState<string | null>(null); // Kept for user feedback
  const [uiVoltageScaleFactor, setUiVoltageScaleFactor] = useState<number>(0.25); // Added for UI Voltage Scaling
  const settingsScrollRef = useRef<HTMLDivElement>(null); // Ref for settings scroll container
  const [canScrollSettings, setCanScrollSettings] = useState(false); // True if settings panel has enough content to scroll
  const [isAtSettingsBottom, setIsAtSettingsBottom] = useState(false); // True if scrolled to the bottom of settings

  // Get all data and config from the new central context
  const { config, dataStatus } = useEegData();
  const { dataReceived, driverError, wsStatus } = dataStatus;
  const { status: configStatus, refreshConfig } = useEegConfig(); // Keep for settings UI

  // State for UI selections, initialized from config when available
  const [selectedChannelCount, setSelectedChannelCount] = useState<string | undefined>(undefined);
  const [selectedSampleRate, setSelectedSampleRate] = useState<string | undefined>(undefined);
  const [selectedPowerlineFilter, setSelectedPowerlineFilter] = useState<string | undefined>(undefined);

  useEffect(() => {
    if (config) {
      if (config.channels?.length !== undefined) {
        setSelectedChannelCount(String(config.channels.length));
      }
      if (config.sample_rate !== undefined) {
        setSelectedSampleRate(String(config.sample_rate));
      }
      if (config.powerline_filter_hz !== undefined) {
        setSelectedPowerlineFilter(config.powerline_filter_hz === null ? 'off' : String(config.powerline_filter_hz));
      }
    }
  }, [config]);

  const { sendPowerlineFilterCommand } = useCommandWebSocket(); // Keep for potential direct use if needed

  const handleUpdateConfig = () => {
    if (!configWebSocket || configWebSocket.readyState !== WebSocket.OPEN) {
      console.error('Config WebSocket (EegMonitor) not connected or not ready.');
      setConfigUpdateStatus('Error: Config service not connected. Cannot send update.');
      return;
    }

    if (recordingStatus.startsWith('Currently recording')) {
        setConfigUpdateStatus('Cannot change configuration during recording.');
        return;
    }

    const newConfigPayload: { channels?: number[]; sample_rate?: number; powerline_filter_hz?: number | null } = {};
    let changesMade = false;

    if (selectedChannelCount !== undefined) {
      const numChannels = parseInt(selectedChannelCount, 10);
      if (!isNaN(numChannels) && numChannels >= 0 && numChannels <= 8) { // Max 8 channels for ADS1299
        const currentChannels = config?.channels || [];
        const newChannelsArray = Array.from({ length: numChannels }, (_, i) => i);
        // Compare arrays properly
        if (JSON.stringify(currentChannels) !== JSON.stringify(newChannelsArray)) {
            newConfigPayload.channels = newChannelsArray;
            changesMade = true;
        }
      } else {
        setConfigUpdateStatus('Invalid number of channels selected.');
        return;
      }
    }

    if (selectedSampleRate !== undefined) {
      const rate = parseInt(selectedSampleRate, 10);
      const validRates = [250, 500, 1000, 2000]; // Example valid rates
      if (!isNaN(rate) && validRates.includes(rate)) {
        if (config?.sample_rate !== rate) {
            newConfigPayload.sample_rate = rate;
            changesMade = true;
        }
      } else {
        setConfigUpdateStatus(`Invalid sample rate: ${rate}. Valid: ${validRates.join(', ')}`);
        return;
      }
    }
    
    if (selectedPowerlineFilter !== undefined) {
      let filterValue: number | null = null;
      if (selectedPowerlineFilter === 'off') {
        filterValue = null;
      } else {
        const parsedFilter = parseInt(selectedPowerlineFilter, 10);
        if (!isNaN(parsedFilter) && (parsedFilter === 50 || parsedFilter === 60)) {
          filterValue = parsedFilter;
        } else {
          setConfigUpdateStatus(`Invalid powerline filter value: ${selectedPowerlineFilter}`);
          return;
        }
      }
      if (config?.powerline_filter_hz !== filterValue) {
        newConfigPayload.powerline_filter_hz = filterValue;
        changesMade = true;
      }
    }
    
    if (!changesMade) {
      setConfigUpdateStatus('No changes selected to update.');
      console.log('No changes to send for config update.');
      return;
    }
    
    console.log('Sending config update via EegMonitor /config WebSocket:', newConfigPayload);
    setConfigUpdateStatus('Sending configuration update...');
    configWebSocket.send(JSON.stringify(newConfigPayload));
  };

  // Use the command WebSocket context
  const {
    wsConnected,
    startRecording,
    stopRecording,
    recordingStatus,
    recordingFilePath,
  } = useCommandWebSocket();
 
  // Effect to update lastActiveDataView when activeView changes (and is not settings)
  useEffect(() => {
    if (activeView !== 'settings') {
      setLastActiveDataView(activeView as DataView);
    }
  }, [activeView]);

  const UI_SCALE_FACTORS = [0.125, 0.25, 0.5, 1, 2, 4, 8];

  const getViewName = (view: DataView | 'settings'): string => {
    switch (view) {
        case 'signalGraph': return 'Signal Graph';
        case 'appletBrainWaves': return 'Brain Waves (FFT)'; // Updated name
        case 'settings': return 'Settings';
        default: return '';
    }
  };

  // Handler for cycling between Signal Graph, and FFT Applet
  const handleToggleSignalFftView = () => {
    if (activeView === 'signalGraph') {
      setActiveView('appletBrainWaves');
    } else if (activeView === 'appletBrainWaves') {
      setActiveView('signalGraph');
    } else if (activeView === 'settings') {
      setActiveView('signalGraph');
    }
  };
 
  // Handler for the "Settings" / "Back to [View]" button
  const handleToggleSettingsView = () => {
    if (activeView !== 'settings') {
        setActiveView('settings');
    } else {
        setActiveView(lastActiveDataView);
    }
  };
  

  // Effect for settings panel scroll detection
  useEffect(() => {
    const scrollElement = settingsScrollRef.current;

    const checkScroll = () => {
      if (scrollElement) {
        const hasScrollbar = scrollElement.scrollHeight > scrollElement.clientHeight;
        const atBottom = scrollElement.scrollTop + scrollElement.clientHeight >= scrollElement.scrollHeight - 5;
        
        setCanScrollSettings(hasScrollbar && !atBottom);
        setIsAtSettingsBottom(hasScrollbar && atBottom);
        
        if (!hasScrollbar) {
            setIsAtSettingsBottom(true);
        }

      } else {
        setCanScrollSettings(false);
        setIsAtSettingsBottom(false);
      }
    };

    if (activeView === 'settings' && scrollElement) {
      const timerId = setTimeout(checkScroll, 100);

      scrollElement.addEventListener('scroll', checkScroll);
      const resizeObserver = new ResizeObserver(checkScroll);
      resizeObserver.observe(scrollElement);
      Array.from(scrollElement.children).forEach(child => resizeObserver.observe(child));


      return () => {
        clearTimeout(timerId);
        scrollElement.removeEventListener('scroll', checkScroll);
        resizeObserver.disconnect();
      };
    } else {
      setCanScrollSettings(false);
      setIsAtSettingsBottom(false);
    }
  }, [activeView, config, uiVoltageScaleFactor]);


  return (
    <div className="h-screen w-screen bg-gray-900 flex flex-col">
      {/* Header with controls */}
      <div className="flex justify-between items-center p-2 bg-gray-800 border-b border-gray-700">
        <div className="flex items-center">
          <h1 className="text-xl font-bold text-white mr-4">EEG Monitor</h1>
          <div className="flex items-center text-white">
            <span>Status:</span>
            <span className={`inline-block w-3 h-3 rounded-full mx-2 ${dataReceived ? 'bg-green-500' : 'bg-gray-500'}`}></span>
            <span>{dataReceived ? 'receiving data' : 'no data'}</span>
          </div>
          <div className="ml-4 text-xs text-gray-300">
            <span>WS: {wsStatus}</span>
          </div>
        </div>
        <div className="flex items-baseline space-x-2">
          <EegRecordingControls />
          
          <a
            href="/recordings"
            className="px-4 py-1 rounded-md bg-purple-600 hover:bg-purple-700 text-white flex items-center"
          >
            <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
            </svg>
            Recordings
          </a>
          
          <button
            onClick={handleToggleSignalFftView}
            className="px-4 py-1 rounded-md bg-teal-600 hover:bg-teal-700 text-white"
            disabled={activeView === 'settings'}
          >
            {activeView === 'signalGraph' ?  'Show FFT' : 'Show Signal'}
          </button>
 
          <button
            onClick={handleToggleSettingsView}
            className="px-4 py-1 rounded-md bg-blue-600 hover:bg-blue-700 text-white"
          >
            {activeView === 'settings'
              ? `Back to ${getViewName(lastActiveDataView)}`
              : 'Settings'}
          </button>
        </div>
      </div>
      
      {recordingStatus.startsWith('Currently recording') && (
        <div className="bg-red-900 text-white px-2 py-1 text-sm flex justify-between">
          <div className="flex items-center">
            <span className="inline-block w-2 h-2 rounded-full bg-red-500 animate-pulse mr-2"></span>
            {recordingStatus}
          </div>
          {recordingFilePath && (
            <div className="text-gray-300 truncate">
              File: {recordingFilePath}
            </div>
          )}
        </div>
      )}
      
      {driverError && (
        <div className="bg-yellow-800 text-white px-2 py-1 text-sm flex items-center">
          <svg xmlns="http://www.w3.org/2000/svg" className="h-5 w-5 mr-2 text-yellow-300" viewBox="0 0 20 20" fill="currentColor">
            <path fillRule="evenodd" d="M8.257 3.099c.765-1.36 2.722-1.36 3.486 0l5.58 9.92c.75 1.334-.213 2.98-1.742 2.98H4.42c-1.53 0-2.493-1.646-1.743-2.98l5.58-9.92zM11 13a1 1 0 11-2 0 1 1 0 012 0zm-1-8a1 1 0 00-1 1v3a1 1 0 002 0V6a1 1 0 00-1-1z" clipRule="evenodd" />
          </svg>
          <span>Driver Error: {driverError}</span>
        </div>
      )}
      
      {/* Main content area */}
      <main className="flex-grow relative bg-gray-950">
        {activeView !== 'settings' ? (
          <EegDataVisualizer
            activeView={activeView}
            config={config}
            uiVoltageScaleFactor={uiVoltageScaleFactor}
          />
        ) : (
          // Settings Panel
          <div ref={settingsScrollRef} className="h-full overflow-y-auto bg-gray-800 text-white p-4 relative">
            <h2 className="text-2xl font-bold mb-4 border-b border-gray-600 pb-2">Settings</h2>
            
            {/* Configuration Update Status */}
            {configUpdateStatus && (
              <div className={`p-2 mb-4 rounded text-sm ${configUpdateStatus.startsWith('Error') ? 'bg-red-800' : 'bg-blue-800'}`}>
                {configUpdateStatus}
              </div>
            )}

            {/* Channel Count */}
            <div className="mb-4">
              <label htmlFor="channel-count" className="block mb-1 font-semibold">Channels</label>
              <select
                id="channel-count"
                value={selectedChannelCount ?? ''}
                onChange={(e) => setSelectedChannelCount(e.target.value)}
                className="w-full p-2 rounded bg-gray-700 border border-gray-600"
                disabled={!config}
              >
                {[...Array(9).keys()].map(i => <option key={i} value={i}>{i === 0 ? 'All Off' : `${i} channel${i > 1 ? 's' : ''}`}</option>)}
              </select>
            </div>

            {/* Sample Rate */}
            <div className="mb-4">
              <label htmlFor="sample-rate" className="block mb-1 font-semibold">Sample Rate (Hz)</label>
              <select
                id="sample-rate"
                value={selectedSampleRate ?? ''}
                onChange={(e) => setSelectedSampleRate(e.target.value)}
                className="w-full p-2 rounded bg-gray-700 border border-gray-600"
                disabled={!config}
              >
                {[250, 500, 1000, 2000].map(rate => <option key={rate} value={rate}>{rate}</option>)}
              </select>
            </div>

            {/* Powerline Filter */}
            <div className="mb-4">
              <label htmlFor="powerline-filter" className="block mb-1 font-semibold">Powerline Filter</label>
              <select
                id="powerline-filter"
                value={selectedPowerlineFilter ?? 'off'}
                onChange={(e) => setSelectedPowerlineFilter(e.target.value)}
                className="w-full p-2 rounded bg-gray-700 border border-gray-600"
                disabled={!config}
              >
                <option value="off">Off</option>
                <option value="50">50 Hz</option>
                <option value="60">60 Hz</option>
              </select>
            </div>

            {/* Voltage Scale */}
            <div className="mb-4">
                <label htmlFor="voltage-scale" className="block mb-1 font-semibold">Voltage Scale</label>
                <div className="flex items-center">
                    <input
                        id="voltage-scale"
                        type="range"
                        min="0"
                        max={UI_SCALE_FACTORS.length - 1}
                        step="1"
                        value={UI_SCALE_FACTORS.indexOf(uiVoltageScaleFactor)}
                        onChange={(e) => setUiVoltageScaleFactor(UI_SCALE_FACTORS[parseInt(e.target.value, 10)])}
                        className="w-full h-2 bg-gray-700 rounded-lg appearance-none cursor-pointer"
                    />
                    <span className="ml-4 text-sm w-12 text-right">{uiVoltageScaleFactor}x</span>
                </div>
            </div>

            {/* Update Button */}
            <button
              onClick={handleUpdateConfig}
              className="w-full px-4 py-2 rounded-md bg-green-600 hover:bg-green-700 text-white font-bold"
              disabled={!wsConnected || !config}
            >
              Apply Changes
            </button>

            {/* Scroll indicators */}
            {canScrollSettings && (
                <div className="absolute bottom-2 left-1/2 -translate-x-1/2 animate-bounce">
                    <svg className="w-6 h-6 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" d="M19 9l-7 7-7-7"></path></svg>
                </div>
            )}
            {isAtSettingsBottom && (
                <div className="text-center text-gray-500 text-xs mt-4 border-t border-gray-700 pt-2">End of settings</div>
            )}
          </div>
        )}
      </main>
   </div>
  );
}