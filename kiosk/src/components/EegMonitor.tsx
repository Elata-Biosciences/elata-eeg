'use client';
import React from 'react'; // Added to resolve React.Fragment error

import { useRef, useState, useEffect, useContext } from 'react';
import EegRecordingControls from './EegRecordingControls';
import { useEegStatus } from '../context/EegDataContext';
import { useEegConfig } from './EegConfig';
import { usePipeline } from '@/context/PipelineContext';
import { useEventStream, useEventStreamData } from '../context/EventStreamContext';
import EegDataVisualizer from './EegDataVisualizer';
import CustomSelect from './CustomSelect';

export default function EegMonitorWebGL() {
  type DataView = 'signalGraph' | 'appletBrainWaves';
  type ActiveView = DataView | 'settings';
  
  const [activeView, setActiveView] = useState<ActiveView>('signalGraph');
  const [lastActiveDataView, setLastActiveDataView] = useState<DataView>('signalGraph');
  
  // configWebSocket state is no longer needed as we use SSE for configuration updates
  const [configUpdateStatus, setConfigUpdateStatus] = useState<string | null>(null); // Kept for user feedback
  const [uiVoltageScaleFactor, setUiVoltageScaleFactor] = useState<number>(1.0); // Added for UI Voltage Scaling
  const settingsScrollRef = useRef<HTMLDivElement>(null); // Ref for settings scroll container
  const [canScrollSettings, setCanScrollSettings] = useState(false); // True if settings panel has enough content to scroll
  const [isAtSettingsBottom, setIsAtSettingsBottom] = useState(false); // True if scrolled to the bottom of settings
  const showSignalButtonRef = useRef<HTMLButtonElement>(null); // Ref for show signal button

  // useRef for tracking last configuration to prevent duplicate commands
  const lastConfigRef = useRef<any>(null);

  // Get all data and config from the new central context
  const { config, updateConfig } = useEegConfig();
  const { dataStatus } = useEegStatus();
  const { dataReceived, driverError, wsStatus } = dataStatus;
  const { fatalError } = useEventStreamData();
  const { subscribe } = useEventStream();
  const [isRecording, setIsRecording] = useState(false);

  useEffect(() => {
    const handleRecordingState = (data: any) => {
      if (data.event === 'started') {
        setIsRecording(true);
      } else if (data.event === 'stopped') {
        setIsRecording(false);
      }
    };

    const unsubscribe = subscribe('recording_state', handleRecordingState);
    return () => unsubscribe();
  }, [subscribe]);

  // State for UI selections, initialized from config when available
  const [selectedChannelCount, setSelectedChannelCount] = useState<string | undefined>(undefined);
  const [selectedSampleRate, setSelectedSampleRate] = useState<string | undefined>(undefined);
  const [selectedPowerlineFilter, setSelectedPowerlineFilter] = useState<string | undefined>(undefined);
  const [selectedGain, setSelectedGain] = useState<string | undefined>(undefined);

  useEffect(() => {
    if (config) {
      if (config.channels?.length !== undefined) {
        setSelectedChannelCount(String(config.channels.length));
      }
      if (config.sample_rate !== undefined) {
        setSelectedSampleRate(String(config.sample_rate));
      }
      if ((config as any).gain !== undefined) {
        const g = Number((config as any).gain);
        const allowed = [1, 2, 4, 6, 8, 12, 24];
        const normalized = allowed.includes(g) ? g : allowed.reduce((prev, curr) => Math.abs(curr - g) < Math.abs(prev - g) ? curr : prev, allowed[0]);
        setSelectedGain(String(normalized));
      }
      if (config.powerline_filter_hz !== undefined) {
        setSelectedPowerlineFilter(config.powerline_filter_hz === null ? 'off' : String(config.powerline_filter_hz));
      }
    }
  }, [config]);

  const { sendPowerlineFilterCommand } = usePipeline();

  const handleUpdateConfig = () => {
    if (isRecording) {
      setConfigUpdateStatus('Cannot change configuration during recording.');
      return;
    }

    // Ensure all selections are made before proceeding
    if (selectedChannelCount === undefined || selectedSampleRate === undefined || selectedPowerlineFilter === undefined || selectedGain === undefined) {
      setConfigUpdateStatus('Please make a selection for all configuration options.');
      return;
    }

    const numChannels = parseInt(selectedChannelCount, 10);
    const sampleRate = parseInt(selectedSampleRate, 10);
    const powerlineFilter = selectedPowerlineFilter === 'off' ? null : parseInt(selectedPowerlineFilter, 10);
    const gain = parseInt(selectedGain, 10);

    setConfigUpdateStatus('Sending configuration update...');
    try {
      // Simplified call to centralized updateConfig function.
      // It will handle constructing the full, valid payload.
      updateConfig({
        channels: numChannels,
        sample_rate: sampleRate,
        powerline_filter_hz: powerlineFilter,
        gain,
      });
      setConfigUpdateStatus('Configuration update sent successfully.');
    } catch (error) {
      console.error('Failed to send configuration update:', error);
      setConfigUpdateStatus(`Error: ${error instanceof Error ? error.message : 'Unknown error'}`);
    }
  };
 
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
        // Reset scroll to top when entering settings
        setTimeout(() => {
          if (settingsScrollRef.current) {
            settingsScrollRef.current.scrollTop = 0;
          }
        }, 100);
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

  // Effect to ensure Show Signal button always stays white
  useEffect(() => {
    const button = showSignalButtonRef.current;
    if (button && activeView !== 'settings') {
      button.style.backgroundColor = 'var(--color-white)';
      button.style.color = 'var(--color-off-black)';
      button.style.borderColor = 'var(--color-gray2)';
    }
  }, [activeView]);



  return (
    <div className="h-screen w-screen flex flex-col" style={{
      backgroundColor: 'var(--color-off-cream)'
    }}>
      {/* Header with controls */}
      <div className="flex flex-col sm:flex-row sm:justify-between sm:items-center p-4 space-y-3 sm:space-y-0" style={{
        background: 'var(--color-white)',
        borderBottom: '1px solid color-mix(in srgb, var(--color-gray2) 30%, transparent)',
        boxShadow: '0 2px 8px color-mix(in srgb, var(--color-gray3) 8%, transparent)'
      }}>
        {/* Logo and Status Row */}
        <div className="flex items-center justify-between sm:justify-start">
          <div className="flex items-center">
            {/* Real Elata Logo */}
            <img 
              src="/logo.png" 
              alt="Elata" 
              className="w-10 h-10 mr-3"
              style={{
                filter: 'drop-shadow(0 1px 3px rgba(0,0,0,0.12))',
                objectFit: 'contain'
              }}
            />
            <div>
              <h1 className="text-lg font-bold leading-tight" style={{
                fontFamily: 'var(--font-family-ui)',
                color: 'var(--color-off-black)'
              }}>Elata EEG</h1>
              <div className="flex items-center text-xs" style={{
                color: 'var(--color-gray3)',
                fontFamily: 'var(--font-family-system)'
              }}>
                <span className={`inline-block w-2 h-2 rounded-full mr-1 transition-colors duration-300`} style={{
                  backgroundColor: dataReceived ? 'var(--color-success)' : 'var(--color-gray3)',
                  animation: dataReceived ? 'pulse 3s ease-in-out infinite' : 'none'
                }}></span>
                <span>{dataReceived ? 'Live Data' : 'No Data'}</span>
                <span className="mx-2">•</span>
                <span>{wsStatus === 'Idle' ? 'Connected' : wsStatus}</span>
                {config && (
                  <>
                    <span className="mx-2">•</span>
                    <span>{(config as any).board_driver || 'Unknown'}</span>
                  </>
                )}
              </div>
            </div>
          </div>
        </div>

        {/* Controls Row */}
        <div className="flex items-center justify-center sm:justify-end space-x-3 flex-wrap" style={{ minHeight: '44px' }}>
          <EegRecordingControls />
          
          {/* White buttons with max radius */}
          <a
            href={activeView === 'settings' ? undefined : "/recordings"}
            className="flex items-center px-4 py-2 font-medium text-sm transition-all duration-200 shadow-sm hover:shadow-md hover:scale-105"
            style={{
              backgroundColor: 'var(--color-white)',
              color: 'var(--color-off-black)',
              border: '1px solid var(--color-gray2)',
              textDecoration: 'none',
              borderRadius: '9999px', // Max radius
              fontFamily: 'var(--font-family-ui)',
              minHeight: '44px',
              opacity: activeView === 'settings' ? 0.5 : 1,
              cursor: activeView === 'settings' ? 'not-allowed' : 'pointer',
              pointerEvents: activeView === 'settings' ? 'none' : 'auto'
            }}
            onMouseEnter={(e) => {
              if (activeView !== 'settings') {
                e.currentTarget.style.backgroundColor = 'var(--color-off-black)';
                e.currentTarget.style.color = 'var(--color-white)';
              }
            }}
            onMouseLeave={(e) => {
              if (activeView !== 'settings') {
                e.currentTarget.style.backgroundColor = 'var(--color-white)';
                e.currentTarget.style.color = 'var(--color-off-black)';
              }
            }}
          >
            <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
            </svg>
            <span className="hidden sm:inline">Recordings</span>
            <span className="sm:hidden">Files</span>
          </a>
          
          <button
            ref={showSignalButtonRef}
            onClick={handleToggleSignalFftView}
            disabled={activeView === 'settings'}
            className="flex items-center px-4 py-2 font-medium text-sm transition-all duration-200 shadow-sm hover:shadow-md hover:scale-105"
            style={{
              backgroundColor: 'var(--color-white)',
              color: 'var(--color-off-black)',
              border: '1px solid var(--color-gray2)',
              opacity: activeView === 'settings' ? 0.5 : 1,
              cursor: activeView === 'settings' ? 'not-allowed' : 'pointer',
              borderRadius: '9999px', // Max radius
              fontFamily: 'var(--font-family-ui)',
              minHeight: '44px'
            }}
            onMouseEnter={(e) => {
              if (activeView !== 'settings') {
                e.currentTarget.style.backgroundColor = 'var(--color-off-black)';
                e.currentTarget.style.color = 'var(--color-white)';
                e.currentTarget.style.borderColor = 'var(--color-off-black)';
              }
            }}
            onMouseLeave={(e) => {
              if (activeView !== 'settings') {
                e.currentTarget.style.backgroundColor = 'var(--color-white)';
                e.currentTarget.style.color = 'var(--color-off-black)';
                e.currentTarget.style.borderColor = 'var(--color-gray2)';
              }
            }}
          >
            {activeView === 'signalGraph' ? (
              // FFT icon (frequency analysis)
              <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z" />
              </svg>
            ) : (
              // Signal/waveform icon
              <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 7h8m0 0v8m0-8l-8 8-4-4-6 6" />
              </svg>
            )}
            {activeView === 'signalGraph' ? 'Show FFT' : 'Show Signal'}
          </button>
 
          {/* Settings button with text and no border radius */}
          <button
            onClick={handleToggleSettingsView}
            className="flex items-center px-4 py-2 font-medium text-sm transition-all duration-200 shadow-sm hover:shadow-md"
            style={{
              backgroundColor: 'var(--color-off-black)',
              color: 'var(--color-white)',
              border: 'none',
              borderRadius: '0px', // No border radius
              fontFamily: 'var(--font-family-ui)',
              minHeight: '44px'
            }}
          >
            <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              {activeView === 'settings' ? (
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10 19l-7-7m0 0l7-7m-7 7h18" />
              ) : (
                <>
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                </>
              )}
            </svg>
            {activeView === 'settings' ? 'Back' : 'Settings'}
          </button>
        </div>
      </div>
      

      {fatalError && (
        <div className="p-4 flex items-center justify-center" style={{
          background: 'linear-gradient(to right, var(--color-accent-red), rgb(220 38 38))',
          color: 'var(--color-white)',
          animation: 'fadeInUp 0.4s ease-out forwards'
        }}>
          <svg xmlns="http://www.w3.org/2000/svg" className="h-6 w-6 mr-3 flex-shrink-0" viewBox="0 0 20 20" fill="currentColor" style={{
            color: 'color-mix(in srgb, var(--color-white) 90%, transparent)'
          }}>
            <path fillRule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z" clipRule="evenodd" />
          </svg>
          <div className="flex-1 min-w-0">
            <p className="font-bold text-sm sm:text-base" style={{
              fontFamily: 'var(--font-family-ui)'
            }}>Pipeline Fatal Error</p>
            <p className="text-xs sm:text-sm mt-1 break-all" style={{
              fontFamily: 'var(--font-family-mono)'
            }}>{fatalError}</p>
            <p className="text-xs mt-2 opacity-90">Check daemon logs and restart application.</p>
          </div>
        </div>
      )}
      
      {driverError && !fatalError && (
        <div className="px-3 py-2 text-sm flex items-center" style={{
          background: 'linear-gradient(to right, var(--color-warning), rgb(234 179 8))',
          color: 'var(--color-white)',
          animation: 'fadeInUp 0.4s ease-out forwards'
        }}>
          <svg xmlns="http://www.w3.org/2000/svg" className="h-5 w-5 mr-2 flex-shrink-0" viewBox="0 0 20 20" fill="currentColor" style={{
            color: 'color-mix(in srgb, var(--color-white) 90%, transparent)'
          }}>
            <path fillRule="evenodd" d="M8.257 3.099c.765-1.36 2.722-1.36 3.486 0l5.58 9.92c.75 1.334-.213 2.98-1.742 2.98H4.42c-1.53 0-2.493-1.646-1.743-2.98l5.58-9.92zM11 13a1 1 0 11-2 0 1 1 0 012 0zm-1-8a1 1 0 00-1 1v3a1 1 0 002 0V6a1 1 0 00-1-1z" clipRule="evenodd" />
          </svg>
          <span style={{
            fontFamily: 'var(--font-family-system)'
          }}>Driver Error: {driverError}</span>
        </div>
      )}
      
      {/* Main content area */}
      <main className="flex-grow relative" style={{
        background: activeView === 'settings' 
          ? 'linear-gradient(to bottom right, var(--color-cream1), var(--color-cream2))'
          : 'var(--color-white)'
      }}>
        {activeView !== 'settings' ? (
          <EegDataVisualizer
            activeView={activeView}
            config={config}
            uiVoltageScaleFactor={uiVoltageScaleFactor}
          />
        ) : (
          // Settings Panel
          <div ref={settingsScrollRef} className="h-full overflow-y-auto p-6 relative" style={{
            background: 'linear-gradient(to bottom right, var(--color-cream1), var(--color-cream2))',
            color: 'var(--color-off-black)',
            animation: 'fadeInScale 0.3s ease-out forwards',
            scrollBehavior: 'smooth'
          }}>
            
            {/* Configuration Update Status */}
            {configUpdateStatus && (
              <div className="p-3 mb-4 rounded-lg text-sm" style={{
                fontFamily: 'var(--font-family-system)',
                backgroundColor: configUpdateStatus.startsWith('Error') 
                  ? 'color-mix(in srgb, var(--color-accent-red) 20%, transparent)'
                  : 'color-mix(in srgb, var(--color-elata-green) 20%, transparent)',
                border: `1px solid ${configUpdateStatus.startsWith('Error') 
                  ? 'color-mix(in srgb, var(--color-accent-red) 30%, transparent)'
                  : 'color-mix(in srgb, var(--color-elata-green) 30%, transparent)'}`,
                color: configUpdateStatus.startsWith('Error') 
                  ? 'var(--color-accent-red)'
                  : 'var(--color-elata-green)',
                animation: 'fadeInUp 0.4s ease-out forwards'
              }}>
                {configUpdateStatus}
              </div>
            )}

            {/* Channel Count */}
            <div className="mb-6" data-setting="channels">
              <label htmlFor="channel-count" className="block mb-3 font-semibold text-sm" style={{
                fontFamily: 'var(--font-family-ui)',
                color: 'var(--color-off-black)'
              }}>Channels</label>
              <CustomSelect
                id="channel-count"
                value={selectedChannelCount ?? ''}
                onChange={setSelectedChannelCount}
                disabled={!config}
                options={[...Array(17).keys()].map(i => ({
                  value: i.toString(),
                  label: i === 0 ? 'All Off' : `${i} channel${i !== 1 ? 's' : ''}`
                }))}
                placeholder="Select channels..."
                onFocus={() => {
                  setTimeout(() => {
                    const element = document.querySelector('[data-setting="channels"]');
                    element?.scrollIntoView({ behavior: 'smooth', block: 'center' });
                  }, 100);
                }}
              />
            </div>

            {/* Sample Rate */}
            <div className="mb-6" data-setting="sample-rate">
              <label htmlFor="sample-rate" className="block mb-3 font-semibold text-sm" style={{
                fontFamily: 'var(--font-family-ui)',
                color: 'var(--color-off-black)'
              }}>Sample Rate (Hz)</label>
              <CustomSelect
                id="sample-rate"
                value={selectedSampleRate ?? ''}
                onChange={setSelectedSampleRate}
                disabled={!config}
                options={[250, 500, 1000, 2000].map(rate => ({
                  value: rate.toString(),
                  label: `${rate} Hz`
                }))}
                placeholder="Select sample rate..."
                onFocus={() => {
                  setTimeout(() => {
                    const element = document.querySelector('[data-setting="sample-rate"]');
                    element?.scrollIntoView({ behavior: 'smooth', block: 'center' });
                  }, 100);
                }}
              />
            </div>

            {/* Gain */}
            <div className="mb-6" data-setting="gain">
              <label htmlFor="gain" className="block mb-3 font-semibold text-sm" style={{
                fontFamily: 'var(--font-family-ui)',
                color: 'var(--color-off-black)'
              }}>Gain</label>
              <CustomSelect
                id="gain"
                value={selectedGain ?? ''}
                onChange={setSelectedGain}
                disabled={!config}
                options={[1, 2, 4, 6, 8, 12, 24].map(g => ({
                  value: g.toString(),
                  label: `${g}x`
                }))}
                placeholder="Select gain..."
                onFocus={() => {
                  setTimeout(() => {
                    const element = document.querySelector('[data-setting="gain"]');
                    element?.scrollIntoView({ behavior: 'smooth', block: 'center' });
                  }, 100);
                }}
              />
            </div>

            {/* Powerline Filter */}
            <div className="mb-6" data-setting="powerline">
              <label htmlFor="powerline-filter" className="block mb-3 font-semibold text-sm" style={{
                fontFamily: 'var(--font-family-ui)',
                color: 'var(--color-off-black)'
              }}>Powerline Filter</label>
              <CustomSelect
                id="powerline-filter"
                value={selectedPowerlineFilter ?? 'off'}
                onChange={setSelectedPowerlineFilter}
                disabled={!config}
                options={[
                  { value: 'off', label: 'Off' },
                  { value: '50', label: '50 Hz' },
                  { value: '60', label: '60 Hz' }
                ]}
                placeholder="Select filter..."
                onFocus={() => {
                  setTimeout(() => {
                    const element = document.querySelector('[data-setting="powerline"]');
                    element?.scrollIntoView({ behavior: 'smooth', block: 'center' });
                  }, 100);
                }}
              />
            </div>


            {/* Voltage Scale */}
            <div className="mb-6" data-setting="voltage">
                <label htmlFor="voltage-scale" className="block mb-2 font-semibold text-sm" style={{
                  fontFamily: 'var(--font-family-ui)',
                  color: 'var(--color-off-black)'
                }}>Voltage Scale</label>
                <div className="flex items-center space-x-4">
                    <input
                        id="voltage-scale"
                        type="range"
                        min="0"
                        max={UI_SCALE_FACTORS.length - 1}
                        step="1"
                        value={UI_SCALE_FACTORS.indexOf(uiVoltageScaleFactor)}
                        onChange={(e) => setUiVoltageScaleFactor(UI_SCALE_FACTORS[parseInt(e.target.value, 10)])}
                        className="flex-1 h-2 rounded-lg appearance-none cursor-pointer slider-elata"
                        style={{
                          backgroundColor: 'color-mix(in srgb, var(--color-gray2) 50%, transparent)'
                        }}
                    />
                    <span className="text-sm w-12 text-right px-2 py-1 rounded" style={{
                      fontFamily: 'var(--font-family-mono)',
                      color: 'var(--color-gray3)',
                      backgroundColor: 'color-mix(in srgb, var(--color-white) 50%, transparent)'
                    }}>{uiVoltageScaleFactor}x</span>
                </div>
            </div>

            {/* Update Button */}
            <button
              onClick={handleUpdateConfig}
              className="btn-elata-primary w-full py-3 font-semibold"
              disabled={!config}
              style={{
                fontFamily: 'var(--font-family-ui)',
                opacity: !config ? 0.5 : 1,
                cursor: !config ? 'not-allowed' : 'pointer',
                transform: !config ? 'none' : undefined
              }}
            >
              Apply Configuration
            </button>
            
            {/* Bottom padding for comfortable scrolling */}
            <div className="h-24"></div>

            {/* Scroll indicators */}
            {canScrollSettings && (
                <div className="absolute bottom-4 left-1/2 -translate-x-1/2 animate-bounce rounded-full p-2 shadow-lg" style={{
                  backgroundColor: 'color-mix(in srgb, var(--color-white) 80%, transparent)'
                }}>
                    <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" style={{
                      color: 'var(--color-gray3)'
                    }}><path strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" d="M19 9l-7 7-7-7"></path></svg>
                </div>
            )}
            {isAtSettingsBottom && (
                <div className="text-center text-xs mt-6 pt-3" style={{
                  color: 'var(--color-gray3)',
                  borderTop: '1px solid color-mix(in srgb, var(--color-gray2) 30%, transparent)',
                  fontFamily: 'var(--font-family-system)'
                }}>End of configuration</div>
            )}
          </div>
        )}
      </main>
   </div>
  );
}
