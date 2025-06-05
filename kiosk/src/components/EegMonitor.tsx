'use client';
import React from 'react'; // Added to resolve React.Fragment error

import { useRef, useState, useEffect, useCallback, useLayoutEffect } from 'react';
import { useEegConfig } from './EegConfig';
// EegConfigDisplay is removed as we are inlining and modifying its logic.
// EegChannelConfig is also not used in the settings panel.
import { useEegDataHandler } from './EegDataHandler';
import { EegRenderer } from './EegRenderer';
import EegRecordingControls from './EegRecordingControls'; // Import the actual controls
// import { ScrollingBuffer } from '../utils/ScrollingBuffer'; // Removed - Unused and file doesn't exist
import { GRAPH_HEIGHT, WINDOW_DURATION, TIME_TICKS } from '../utils/eegConstants';
import { useCommandWebSocket } from '../context/CommandWebSocketContext';
/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-ignore: WebglStep might be missing from types but exists at runtime
import { WebglStep, ColorRGBA } from 'webgl-plot';
import { getChannelColor } from '../utils/colorUtils';
import BrainWavesDisplay from '../../../plugins/ui/brain_waves/ui/BrainWavesDisplay';
 
export default function EegMonitorWebGL() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const windowSizeRef = useRef<number>(500); // Default, will be updated based on config
  const dataRef = useRef<any[]>([]); // Re-added definition
  const [dataReceived, setDataReceived] = useState(false);
  const [driverError, setDriverError] = useState<string | null>(null);
  // Ref to hold the last update timestamp for each channel's data chunk
  const channelTimestampRef = useRef<number[]>([]);
  // Ref to hold the single latest timestamp of any received data packet
  const latestTimestampRef = useRef<number>(performance.now());
  // Removed canvasDimensions state, EegRenderer handles this

  type DataView = 'signalGraph' | 'appletBrainWaves'; // fftGraph removed
  type ActiveView = DataView | 'settings';
 
  const [activeView, setActiveView] = useState<ActiveView>('signalGraph');
  const [lastActiveDataView, setLastActiveDataView] = useState<DataView>('signalGraph'); // To store the last non-settings view
  const [linesReady, setLinesReady] = useState(false); // State to track line readiness
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 }); // State for container dimensions
  const [dataVersion, setDataVersion] = useState(0); // Version counter for dataRef updates
  const fftDataRef = useRef<Record<number, number[]>>({}); // Ref to store latest FFT data for all channels
  const [fftDataVersion, setFftDataVersion] = useState(0); // Version counter for fftDataRef updates
  const [configWebSocket, setConfigWebSocket] = useState<WebSocket | null>(null); // Restored
  const [isConfigWsOpen, setIsConfigWsOpen] = useState(false); // Restored
  const [configUpdateStatus, setConfigUpdateStatus] = useState<string | null>(null); // Kept for user feedback
  const [uiVoltageScaleFactor, setUiVoltageScaleFactor] = useState<number>(0.5); // Added for UI Voltage Scaling
  const settingsScrollRef = useRef<HTMLDivElement>(null); // Ref for settings scroll container
  const [canScrollSettings, setCanScrollSettings] = useState(false); // True if settings panel has enough content to scroll
  const [isAtSettingsBottom, setIsAtSettingsBottom] = useState(false); // True if scrolled to the bottom of settings

  // Get configuration from context
  const { config, status: configStatus } = useEegConfig(); // refreshConfig removed

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

  // Effect to manage the /config WebSocket connection (Restored)
  useEffect(() => {
    if (activeView === 'settings') {
      console.log('Attempting to connect to /config WebSocket for EegMonitor');
      const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
      const newConfigWs = new WebSocket(`ws://${wsHost}:8080/config`);
      setConfigWebSocket(newConfigWs);
      setIsConfigWsOpen(false); // Initially false until onopen
      setConfigUpdateStatus('Connecting to config service...');

      const connectionTimeout = setTimeout(() => {
        if (newConfigWs.readyState !== WebSocket.OPEN) {
          console.warn('/config WebSocket connection timed out for EegMonitor.');
          setConfigUpdateStatus('Connection to config service timed out. Please check daemon.');
          setIsConfigWsOpen(false);
          if (newConfigWs.readyState === WebSocket.CONNECTING) {
            newConfigWs.close();
          }
        }
      }, 5000);

      newConfigWs.onopen = () => {
        clearTimeout(connectionTimeout);
        console.log('/config WebSocket connected for EegMonitor');
        setIsConfigWsOpen(true);
        setConfigUpdateStatus('Connected to config service. Ready to send updates.');
      };

      newConfigWs.onmessage = (event) => {
        console.log('/config WebSocket message (EegMonitor):', event.data);
        try {
          const response = JSON.parse(event.data as string);
          if (response.status && response.message) { // This is a CommandResponse
            if (response.status === 'ok') {
              setConfigUpdateStatus(`Config update request successful: ${response.message}. Waiting for applied config from EegConfigProvider.`);
            } else {
              setConfigUpdateStatus(`Config update error: ${response.message}`);
            }
          } else {
            // This is likely a full config broadcast, which EegConfigProvider handles.
            // EegMonitor's direct /config WS is primarily for sending updates.
            console.log('EegMonitor received a full config object via its /config WS, EegConfigProvider should handle this state update.');
          }
        } catch (e) {
          console.error('Error parsing /config WebSocket message in EegMonitor:', e);
          setConfigUpdateStatus('Error processing message from config service.');
        }
      };

      newConfigWs.onclose = () => {
        clearTimeout(connectionTimeout);
        console.log('/config WebSocket disconnected for EegMonitor');
        setConfigWebSocket(null);
        setIsConfigWsOpen(false);
        setConfigUpdateStatus(prevStatus =>
          prevStatus && (prevStatus.includes('Error') || prevStatus.includes('timed out'))
          ? prevStatus
          : 'Disconnected from config service.'
        );
      };

      newConfigWs.onerror = (error) => {
        clearTimeout(connectionTimeout);
        console.error('/config WebSocket error for EegMonitor:', error);
        setConfigWebSocket(null);
        setIsConfigWsOpen(false);
        setConfigUpdateStatus('Error connecting to config service.');
      };

      return () => {
        clearTimeout(connectionTimeout);
        console.log('Closing /config WebSocket for EegMonitor');
        if (newConfigWs.readyState === WebSocket.OPEN || newConfigWs.readyState === WebSocket.CONNECTING) {
          newConfigWs.close();
        }
        setConfigWebSocket(null);
        setIsConfigWsOpen(false);
        setConfigUpdateStatus(null); // Clear status when settings view is left
      };
    } else {
      if (configWebSocket) {
        console.log('Closing /config WebSocket for EegMonitor because settings are hidden');
        configWebSocket.close();
        setConfigWebSocket(null);
        setIsConfigWsOpen(false);
      }
      setConfigUpdateStatus(null); // Clear status when settings view is left
    }
  }, [activeView]);


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

  // Debug info reference (ensure it's defined)
  const debugInfoRef = useRef<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>({
    lastPacketTime: 0,
    packetsReceived: 0,
    samplesProcessed: 0
  });

  // Use the command WebSocket context
  const {
    wsConnected,
    startRecording,
    stopRecording,
    // sendPowerlineFilterCommand is now available here but used in handleUpdateConfig
    recordingStatus,
    recordingFilePath,
  } = useCommandWebSocket();

  // Update canvas dimensions based on container size
  // Effect to update the windowSizeRef based on config and container width
  useEffect(() => {
    // Function to calculate and update windowSizeRef
    const updateWindowSize = () => {
      if (containerRef.current && config?.sample_rate) {
        const { width } = containerRef.current.getBoundingClientRect();
        const sampleRate = config.sample_rate;
        // Calculate samples needed based on container width, sample rate, and window duration
        const samplesNeeded = Math.ceil((width / 800) * (sampleRate * WINDOW_DURATION / 1000));
        windowSizeRef.current = samplesNeeded;
        console.log(`Window size ref updated: ${samplesNeeded} samples (based on width ${width}, rate ${sampleRate})`);
      }
    };
    
    // Update initially when config is available
    updateWindowSize();
    
    // Optional: Add resize listener if window size should adapt dynamically to container resize
    // Note: EegRenderer already uses ResizeObserver, so this might be redundant
    // If needed, consider using the ResizeObserver from EegRenderer or adding one here.
    // For now, we only update based on config load.
  }, [config?.sample_rate]); // Remove containerRef dependency here, handled by ResizeObserver below

  // Handle data updates (memoized with useCallback)
  const handleDataUpdate = useCallback((received: boolean) => {
    setDataReceived(received);
  }, [setDataReceived]); // setDataReceived is stable

  // Handle driver errors (memoized with useCallback)
  const handleDriverError = useCallback((error: string) => {
    console.error("Driver error:", error);
    setDriverError(error);
    // Auto-clear error after 10 seconds
    const timer = setTimeout(() => setDriverError(null), 10000);
    // Optional: Cleanup timeout if component unmounts or error changes before timeout fires
    // return () => clearTimeout(timer); // Note: useCallback doesn't directly support cleanup return like useEffect
  }, [setDriverError]); // setDriverError is stable

  // Handle FFT data updates
  const handleFftData = useCallback((channelIndex: number, fftOutput: number[]) => {
    fftDataRef.current[channelIndex] = fftOutput;
    setFftDataVersion(v => v + 1);
    // Optional: Add logging for FFT data if needed for debugging
    // console.log(`[EegMonitor] Received FFT data for Ch ${channelIndex}:`, fftOutput.slice(0, 5));
  }, [setFftDataVersion]); // setFftDataVersion is stable

  // Effect to initialize/resize channelTimestampRef based on channel count
  useEffect(() => {
    const numChannels = config?.channels?.length || 0;
    if (numChannels > 0 && channelTimestampRef.current.length !== numChannels) {
      // Initialize or resize the array, filling with the current time
      channelTimestampRef.current = Array(numChannels).fill(performance.now());
      console.log(`Initialized/Resized channelTimestampRef for ${numChannels} channels.`);
    }
  }, [config?.channels?.length]); // Depend on the number of channels

  // Get data handler status and FPS
  const { status } = useEegDataHandler({
    config,
    onDataUpdate: handleDataUpdate,
    onError: handleDriverError,
    linesRef: dataRef, // Pass dataRef as linesRef (holds WebglStep instances)
    lastDataChunkTimeRef: channelTimestampRef, // Pass the array ref for per-channel times
    latestTimestampRef: latestTimestampRef,     // Pass the single ref for the overall latest time
    debugInfoRef: debugInfoRef, // Pass debugInfoRef for packet counting
    onFftData: undefined // Applet view handles its own data via WebSocket
  });
 
  // Effect to update lastActiveDataView when activeView changes (and is not settings)
  useEffect(() => {
    if (activeView !== 'settings') {
      setLastActiveDataView(activeView as DataView);
    }
  }, [activeView]);

  // Removed dedicated useLayoutEffect for ResizeObserver

  // Effect to create WebGL lines when config and CONTAINER SIZE are ready
  // Constants for scaling
  const MICROVOLT_CONVERSION_FACTOR = 1e6; // V to uV
  const BASE_VISUAL_AMPLITUDE_SCALE = 1; // Renamed from VISUAL_AMPLITUDE_SCALE for UI scaling
  const UI_SCALE_FACTORS = [0.125, 0.25, 0.5, 1, 2, 4, 8]; // Added UI Scale Factors

  useEffect(() => {
    // --- ResizeObserver Setup ---
    let resizeObserver: ResizeObserver | null = null;
    const target = containerRef.current;
    let sizeUpdateTimeoutId: NodeJS.Timeout | null = null;

    // Setup ResizeObserver only for graph views ('signalGraph')
    if (activeView === 'signalGraph' && target) { // fftGraph condition removed
        console.log(`[EegMonitor LineEffect] Setting up ResizeObserver for ${activeView}.`);
        resizeObserver = new ResizeObserver(entries => {
          for (let entry of entries) {
            const { width, height } = entry.contentRect;
            console.log(`[EegMonitor ResizeObserver] Observed size change: ${width}x${height}. Current activeView: ${activeView}`);
            
            // Additional debugging for height issues
            if (height === 0) {
              console.warn(`[EegMonitor ResizeObserver] WARNING: Container height is 0! This will make the graph invisible.`);
              console.log(`[EegMonitor ResizeObserver] Container element:`, target);
              console.log(`[EegMonitor ResizeObserver] Container computed style:`, target ? window.getComputedStyle(target) : 'N/A');
              console.log(`[EegMonitor ResizeObserver] Container parent:`, target?.parentElement);
              console.log(`[EegMonitor ResizeObserver] Container parent computed style:`, target?.parentElement ? window.getComputedStyle(target.parentElement) : 'N/A');
            }
            
            // Clear any existing timeout to avoid multiple rapid updates
            if (sizeUpdateTimeoutId) {
              clearTimeout(sizeUpdateTimeoutId);
            }
            
            // Use a small timeout to ensure the size is stable
            sizeUpdateTimeoutId = setTimeout(() => {
              setContainerSize(prevSize => {
                if (prevSize.width !== width || prevSize.height !== height) {
                  console.log(`[EegMonitor ResizeObserver] Setting container size: ${width}x${height}`);
                  return { width, height };
                }
                return prevSize;
              });
            }, 50); // Small delay to ensure DOM is fully laid out
          }
        });
        resizeObserver.observe(target);

        // Check initial size when observer is set up
        const initialRect = target.getBoundingClientRect();
        if (initialRect.width > 0 && initialRect.height > 0 && (containerSize.width !== initialRect.width || containerSize.height !== initialRect.height)) {
            console.log(`[EegMonitor ResizeObserver] Setting initial size: ${initialRect.width}x${initialRect.height}`);
            setContainerSize({ width: initialRect.width, height: initialRect.height });
        }
    }
    // --- End ResizeObserver Setup ---


    console.log(`[EegMonitor LineEffect RUNS] activeView: ${activeView}, containerSize: ${JSON.stringify(containerSize)}, Config: ${!!config}, Channels: ${config?.channels?.length}, ContainerWidth: ${containerSize.width}`);

    // Logic for creating/clearing lines based on activeView
    if (activeView === 'signalGraph') { // fftGraph condition removed
      // Add a minimum width threshold to ensure container is properly measured
      const MIN_VALID_WIDTH = 50; // Minimum width to consider container properly measured
      
      // Depend on config, channels, and the container SIZE state
      // More robust condition checking: config exists, channels array exists and has elements, container width is valid
      if (config && config.channels && Array.isArray(config.channels) && config.channels.length > 0 && containerSize.width > MIN_VALID_WIDTH) {
        const numChannels = config.channels.length;
        const width = containerSize.width; // Use width from state
        const sampleRate = config.sample_rate || 250; // Use default if needed

        // Calculate points needed based on current width
        const initialNumPoints = Math.max(10, Math.ceil((width / 800) * (sampleRate * WINDOW_DURATION / 1000))); // Ensure at least 10 points

        console.log(`[EegMonitor] Measured container width: ${width}`); // Log width
        console.log(`[EegMonitor] Calculated initialNumPoints: ${initialNumPoints}`); // Log points
        console.log(`[EegMonitor] Container size from state:`, containerSize);
        console.log(`[EegMonitor] Container element rect:`, containerRef.current?.getBoundingClientRect());
        console.log(`[EegMonitor] activeView:`, activeView);

      // Skip condition removed to ensure lines are always reconfigured when config.channels changes.
      // The useEffect dependency on config.channels handles triggering this.
      console.log(`[EegMonitor LineEffect] Proceeding to create/update lines. Current lines: ${dataRef.current?.length}, Required: ${numChannels}. Current points: ${dataRef.current?.[0]?.numPoints}, Required: ${initialNumPoints}`);

      console.log(`[EegMonitor] Creating/Updating ${numChannels} WebGL lines with ${initialNumPoints} points each (Width: ${width}).`);

      const lines: WebglStep[] = [];
      const ySpacing = 2.0 / numChannels; // Total Y range is 2 (-1 to 1)

      for (let i = 0; i < numChannels; i++) {
        // Reuse existing line instance if possible, otherwise create new
        const line = dataRef.current?.[i] instanceof WebglStep
                     ? dataRef.current[i]
                     : new WebglStep(new ColorRGBA(1, 1, 1, 1), initialNumPoints);

        // Ensure numPoints is updated if it changed
        if (line.numPoints !== initialNumPoints) {
            line.numPoints = initialNumPoints;
        }

        // Set color - MOVED TO EegRenderer
        /*
        try {
          const colorTuple = getChannelColor(i);
          // Ensure color values are in 0-1 range for WebglPlot ColorRGBA
          line.color = new ColorRGBA(
            colorTuple[0] / 255,
            colorTuple[1] / 255,
            colorTuple[2] / 255,
            1
          );
        } catch {
          line.color = new ColorRGBA(1, 1, 1, 1); // fallback white
        }
        */

        line.lineWidth = 1;
        // Original Scale Y: Convert to microvolts AND scale for visual spacing/amplitude
        // const calculatedScaleY = (ySpacing * VISUAL_AMPLITUDE_SCALE) * MICROVOLT_CONVERSION_FACTOR;
        // New calculation using uiVoltageScaleFactor:
        const finalVisualAmplitudeScale = BASE_VISUAL_AMPLITUDE_SCALE * uiVoltageScaleFactor;
        
        // Since we're now using filtered data from the DSP, the data should be in the proper range
        // Start with a moderate scaling and adjust based on the actual data range
        const calculatedScaleY = (ySpacing * finalVisualAmplitudeScale) * 1000; // Use 1000 instead of 1e6 for more reasonable scaling
        line.scaleY = calculatedScaleY;
        console.log(`[EegMonitor LineEffect] Ch ${i}: ySpacing=${ySpacing.toFixed(4)}, finalVisualAmplitudeScale=${finalVisualAmplitudeScale}, calculatedScaleY=${calculatedScaleY} (using filtered data with moderate scaling)`);

        // Center channel i vertically within its allocated space
        line.offsetY = 1 - (i + 0.5) * ySpacing;

        // Set horizontal spacing
        line.lineSpaceX(-1, 2 / initialNumPoints);

        lines.push(line);
      }
      dataRef.current = lines;
      console.log(`[EegMonitor] Assigned ${lines.length} lines to dataRef. Bumping version.`);
      setLinesReady(true); // Mark lines as ready
      setDataVersion(v => v + 1); // Increment version

      } else {
          // This block executes if not in settings view, AND (config is null, or channels are null/empty, or containerSize.width is invalid)
          // Provide detailed logging for debugging
          const configExists = !!config;
          const channelsExist = !!config?.channels;
          const channelsIsArray = Array.isArray(config?.channels);
          const channelsLength = config?.channels?.length || 0;
          const containerWidthValid = containerSize.width > MIN_VALID_WIDTH;
          
          console.log(`[EegMonitor LineEffect SKIPPING line creation for ${activeView}] Condition not met:`);
          console.log(`  - config exists: ${configExists}`);
          console.log(`  - config.channels exists: ${channelsExist}`);
          console.log(`  - config.channels is array: ${channelsIsArray}`);
          console.log(`  - config.channels.length: ${channelsLength}`);
          console.log(`  - containerSize.width (${containerSize.width}) > MIN_VALID_WIDTH (${MIN_VALID_WIDTH}): ${containerWidthValid}`);
          
          if (linesReady || dataRef.current.length > 0) { // If lines were previously ready or data exists
              console.log(`[EegMonitor LineEffect] Clearing existing lines due to failed conditions.`);
              dataRef.current = [];
              setLinesReady(false);
              setDataVersion(v => v + 1);
          }
      }
    } else { // activeView is 'settings' or 'appletBrainWaves'
        console.log(`[EegMonitor LineEffect] In ${activeView} view, ensuring lines are cleared (if they existed).`);
        if (dataRef.current.length > 0 || linesReady) {
             console.log(`[EegMonitor LineEffect] Clearing lines for ${activeView} view.`);
             dataRef.current = [];
             setLinesReady(false);
             setDataVersion(v => v + 1);
        }
    }
    
    // Cleanup function for the effect
    return () => {
        if (resizeObserver && target) {
            console.log("[EegMonitor LineEffect] Cleaning up ResizeObserver.");
            resizeObserver.unobserve(target);
            resizeObserver.disconnect();
        }
    };
    // Depend on config, container size state, activeView, AND uiVoltageScaleFactor
  }, [config?.channels, config?.sample_rate, containerSize, activeView, uiVoltageScaleFactor]);
  
  // Use the FPS from config with no fallback
  const displayFps = config?.fps || 0;

  const getViewName = (view: DataView | 'settings'): string => {
    switch (view) {
        case 'signalGraph': return 'Signal Graph';
        // case 'fftGraph': return 'FFT Graph'; // Removed
        case 'appletBrainWaves': return 'Brain Waves (FFT)'; // Updated name
        case 'settings': return 'Settings';
        default: return '';
    }
  };

  // Handler for toggling between Signal Graph and FFT Applet
  const handleToggleSignalFftView = () => {
    if (activeView === 'signalGraph') {
      setActiveView('appletBrainWaves');
    } else if (activeView === 'appletBrainWaves') {
      setActiveView('signalGraph');
    }
    // If currently in settings, this function shouldn't be called, but handle gracefully
    // by defaulting to signal graph
    else if (activeView === 'settings') {
      setActiveView('signalGraph');
    }
  };
 
  // Handler for the "Settings" / "Back to [View]" button
  const handleToggleSettingsView = () => {
    if (activeView !== 'settings') {
        // lastActiveDataView is already updated by an effect
        setActiveView('settings');
    } else {
        setActiveView(lastActiveDataView);
        // If switching from settings to a graph view, ensure container size is reset
        if (lastActiveDataView === 'signalGraph') { // fftGraph condition removed
            console.log("[EegMonitor handleToggleSettingsView] Switching from settings to graph view. Resetting containerSize.");
            setContainerSize({ width: 0, height: 0 });
        }
    }
  };
  
  // Effect to handle transition from settings to graph view (signalGraph)
  useEffect(() => {
    if (activeView === 'signalGraph' && containerRef.current) { // fftGraph condition removed
      console.log(`[EegMonitor TransitionEffect RUNS for ${activeView}] Scheduling size check.`);
      
      // Use a timeout to ensure the DOM has updated after the view switch
      const transitionTimeoutId = setTimeout(() => {
        const rect = containerRef.current?.getBoundingClientRect();
        console.log(`[EegMonitor TransitionEffect TIMEOUT] Measured rect: ${JSON.stringify(rect)}`);
        if (rect && rect.width > 0) {
          console.log(`[EegMonitor] Post-transition size check: ${rect.width}x${rect.height}`);
          // Update container size if it's not already set correctly
          setContainerSize(prevSize => {
            if (prevSize.width !== rect.width || prevSize.height !== rect.height) {
              console.log(`[EegMonitor TransitionEffect TIMEOUT] Updating container size from ${JSON.stringify(prevSize)} to ${rect.width}x${rect.height}`);
              return { width: rect.width, height: rect.height };
            }
            console.log(`[EegMonitor TransitionEffect TIMEOUT] Container size already correct: ${JSON.stringify(prevSize)}`);
            return prevSize;
          });
        } else {
          console.log(`[EegMonitor TransitionEffect TIMEOUT] Rect not valid or width is 0. rect: ${JSON.stringify(rect)}`);
        }
      }, 100); // Small delay to ensure DOM is updated
      
      return () => clearTimeout(transitionTimeoutId);
    }
  }, [activeView]);

  // Effect for settings panel scroll detection
  useEffect(() => {
    const scrollElement = settingsScrollRef.current;

    const checkScroll = () => {
      if (scrollElement) {
        // Check if there's enough content to scroll (scrollbar is visible)
        const hasScrollbar = scrollElement.scrollHeight > scrollElement.clientHeight;
        // Check if scrolled to the bottom (with a small buffer for precision)
        const atBottom = scrollElement.scrollTop + scrollElement.clientHeight >= scrollElement.scrollHeight - 5;
        
        setCanScrollSettings(hasScrollbar && !atBottom); // Only show arrow if scrollable and not at bottom
        setIsAtSettingsBottom(hasScrollbar && atBottom); // Show end marker if scrollable and at bottom
        
        // If no scrollbar, effectively at bottom for "end" marker logic if content fits perfectly
        if (!hasScrollbar) {
            setIsAtSettingsBottom(true);
        }

      } else {
        setCanScrollSettings(false);
        setIsAtSettingsBottom(false);
      }
    };

    if (activeView === 'settings' && scrollElement) {
      // Initial check
      // Use a timeout to allow content to render fully before checking scroll height
      const timerId = setTimeout(checkScroll, 100);


      // Listen for scroll events
      scrollElement.addEventListener('scroll', checkScroll);
      // Also check on resize of the window or content (e.g. config load)
      const resizeObserver = new ResizeObserver(checkScroll);
      resizeObserver.observe(scrollElement);
      // Observe children too, as their size changes can affect scrollHeight
      Array.from(scrollElement.children).forEach(child => resizeObserver.observe(child));


      return () => {
        clearTimeout(timerId);
        scrollElement.removeEventListener('scroll', checkScroll);
        resizeObserver.disconnect(); // Disconnects from all observed elements
      };
    } else {
      // Reset when settings are hidden
      setCanScrollSettings(false);
      setIsAtSettingsBottom(false);
    }
  }, [activeView, config, uiVoltageScaleFactor]); // Re-check when settings are shown, config (content height might change), or other UI elements change

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
        </div>
        <div className="flex items-baseline space-x-2">
          {/* Use the EegRecordingControls component */}
          <EegRecordingControls />
          
          {/* Recordings button */}
          <a
            href="/recordings"
            className="px-4 py-1 rounded-md bg-purple-600 hover:bg-purple-700 text-white flex items-center"
          >
            <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
            </svg>
            Recordings
          </a>
          
          {/* Signal / FFT Graph toggle button */}
          <button
            onClick={handleToggleSignalFftView}
            className="px-4 py-1 rounded-md bg-teal-600 hover:bg-teal-700 text-white"
            disabled={activeView === 'settings'}
          >
            {activeView === 'signalGraph' ? 'Show FFT' : 'Show Signal'}
          </button>
 
          {/* Settings button */}
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
      
      {/* Recording status indicator */}
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
      
      {/* Driver error display */}
      {driverError && (
        <div className="bg-yellow-800 text-white px-2 py-1 text-sm flex items-center">
          <svg xmlns="http://www.w3.org/2000/svg" className="h-5 w-5 mr-2 text-yellow-300" viewBox="0 0 20 20" fill="currentColor">
            <path fillRule="evenodd" d="M8.257 3.099c.765-1.36 2.722-1.36 3.486 0l5.58 9.92c.75 1.334-.213 2.98-1.742 2.98H4.42c-1.53 0-2.493-1.646-1.743-2.98l5.58-9.92zM11 13a1 1 0 11-2 0 1 1 0 012 0zm-1-8a1 1 0 00-1 1v3a1 1 0 002 0V6a1 1 0 00-1-1z" clipRule="evenodd" />
          </svg>
          <span>Driver Error: {driverError}</span>
        </div>
      )}
      
      {/* Main content area */}
      <div className="flex-grow overflow-hidden">
        {activeView === 'settings' ? (
          <div ref={settingsScrollRef} className="relative h-full p-4 overflow-auto"> {/* Settings Panel Component (Existing UI) */}
            {/* Scroll Down Indicator */}
            {canScrollSettings && (
              <div className="absolute top-1/2 right-2 transform -translate-y-1/2 z-10 text-gray-400 animate-bounce pointer-events-none">
                <span className="block text-2xl">↓</span>
                <span className="block text-2xl -mt-4">↓</span>
                <span className="block text-2xl -mt-4">↓</span>
              </div>
            )}
            <div>
              <h2 className="text-xl font-semibold mb-4 text-gray-200 border-b border-gray-700 pb-2">EEG Settings</h2>
              <div className="mb-2 mt-4">
                <span className="font-medium text-gray-400">Connection Status:</span>
                <span className={`ml-2 px-2 py-0.5 rounded-full text-sm ${
                  configStatus === 'Connected' ? 'bg-green-700 text-green-100' :
                  configStatus === 'Connecting...' || configStatus === 'Initializing...' ? 'bg-yellow-700 text-yellow-100' :
                  'bg-red-700 text-red-100'
                }`}>
                  {configStatus}
                </span>
              </div>

              {config ? (
                <div className="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-4 mt-4 text-gray-300">
                  {/* Sample Rate */}
                  <div className="md:col-span-2 flex items-center space-x-4">
                    <label htmlFor="sampleRate" className="block text-sm font-medium text-gray-400 w-48">Sample Rate (SPS):</label>
                    <select
                      id="sampleRate"
                      name="sampleRate"
                      value={selectedSampleRate || ''}
                      onChange={(e) => setSelectedSampleRate(e.target.value)}
                      className="block w-40 pl-3 pr-10 py-2 text-base bg-gray-700 border-gray-600 focus:outline-none focus:ring-blue-500 focus:border-blue-500 sm:text-sm rounded-md text-white"
                    >
                      <option value="250">250 SPS</option>
                      <option value="500">500 SPS</option>
                    </select>
                    <span className="text-sm text-gray-500">(Currently: {config.sample_rate} Hz)</span>
                  </div>

                  {/* Number of Active Channels */}
                  <div className="md:col-span-2 flex items-center space-x-4">
                    <label htmlFor="channelCount" className="block text-sm font-medium text-gray-400 w-48">Number of Active Channels:</label>
                    <select
                      id="channelCount"
                      name="channelCount"
                      value={selectedChannelCount || ''}
                      onChange={(e) => setSelectedChannelCount(e.target.value)}
                      className="block w-40 pl-3 pr-10 py-2 text-base bg-gray-700 border-gray-600 focus:outline-none focus:ring-blue-500 focus:border-blue-500 sm:text-sm rounded-md text-white"
                    >
                      {[...Array(9).keys()].map(i => (
                        <option key={i} value={String(i)}>{i} Channels</option>
                      ))}
                    </select>
                    <span className="text-sm text-gray-500">(Currently: {config.channels?.length} channels - [{config.channels?.join(', ')}])</span>
                  </div>
                  
                  {/* Powerline Filter */}
                  <div className="md:col-span-2 flex items-center space-x-4">
                    <label htmlFor="powerlineFilter" className="block text-sm font-medium text-gray-400 w-48">Powerline Filter:</label>
                    <select
                      id="powerlineFilter"
                      name="powerlineFilter"
                      value={selectedPowerlineFilter || ''}
                      onChange={(e) => setSelectedPowerlineFilter(e.target.value)}
                      className="block w-40 pl-3 pr-10 py-2 text-base bg-gray-700 border-gray-600 focus:outline-none focus:ring-blue-500 focus:border-blue-500 sm:text-sm rounded-md text-white"
                    >
                      <option value="off">Off</option>
                      <option value="50">50 Hz</option>
                      <option value="60">60 Hz</option>
                    </select>
                    <span className="text-sm text-gray-500">
                      (Currently: {config.powerline_filter_hz === null ? 'Off' :
                                  config.powerline_filter_hz ? `${config.powerline_filter_hz} Hz` : 'Not set'})
                    </span>
                  </div>
                  
                  {/* Other Config Items - Display Only */}
                  <div className="text-sm"><span className="font-medium text-gray-400">Gain:</span> {config.gain}</div>
                  <div className="text-sm"><span className="font-medium text-gray-400">Board Driver:</span> {config.board_driver}</div>

                  {/* Update Config Button */}
                  <div className="md:col-span-2 mt-6">
                    <button
                      onClick={handleUpdateConfig}
                      disabled={!isConfigWsOpen || recordingStatus.startsWith('Currently recording')}
                      className={`px-6 py-2 rounded-md text-white focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-opacity-50 ${
                        (!isConfigWsOpen || recordingStatus.startsWith('Currently recording'))
                          ? 'bg-gray-500 cursor-not-allowed'
                          : 'bg-blue-600 hover:bg-blue-700'
                      }`}
                    >
                      Update Config
                    </button>
                    {configUpdateStatus && (
                      <p className={`mt-2 text-xs ${configUpdateStatus.includes('Error') || configUpdateStatus.includes('error') || configUpdateStatus.includes('Invalid') || configUpdateStatus.includes('timed out') ? 'text-red-400' : 'text-gray-400'}`}>
                        {configUpdateStatus}
                      </p>
                    )}
                    <p className="mt-2 text-xs text-gray-500">Note: Updating configuration may restart the data stream if successful.</p>
                  </div>
                </div>
              ) : (
                <p className="text-gray-500 mt-4">Waiting for configuration data...</p>
              )}
            </div>

            {/* UI Settings Section */}
            <div className="mt-8"> {/* Add some margin-top */}
              <h2 className="text-xl font-semibold mb-4 text-gray-200 border-b border-gray-700 pb-2">UI Settings</h2>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-4 mt-4 text-gray-300">
                {/* Moved Batch Size and Effective FPS */}
                {config && (
                  <>
                    <div className="text-sm"><span className="font-medium text-gray-400">Batch Size:</span> {config.batch_size} samples</div>
                    <div className="text-sm"><span className="font-medium text-gray-400">Effective FPS:</span> {config.fps?.toFixed(2)} frames/sec</div>
                  </>
                )}

                {/* UI Voltage Scaling Control */}
                <div className="md:col-span-2 flex items-center space-x-3">
                  <label className="block text-sm font-medium text-gray-400 w-48">UI Voltage Scaling:</label>
                  <button
                    onClick={() => {
                      const currentIndex = UI_SCALE_FACTORS.indexOf(uiVoltageScaleFactor);
                      if (currentIndex > 0) {
                        setUiVoltageScaleFactor(UI_SCALE_FACTORS[currentIndex - 1]);
                      }
                    }}
                    disabled={UI_SCALE_FACTORS.indexOf(uiVoltageScaleFactor) === 0}
                    className="px-3 py-1 rounded-md bg-gray-600 hover:bg-gray-500 text-white disabled:opacity-50"
                  >
                    Zoom Out
                  </button>
                  <span className="text-sm w-16 text-center">{uiVoltageScaleFactor}x</span>
                  <button
                    onClick={() => {
                      const currentIndex = UI_SCALE_FACTORS.indexOf(uiVoltageScaleFactor);
                      if (currentIndex < UI_SCALE_FACTORS.length - 1) {
                        setUiVoltageScaleFactor(UI_SCALE_FACTORS[currentIndex + 1]);
                      }
                    }}
                    disabled={UI_SCALE_FACTORS.indexOf(uiVoltageScaleFactor) === UI_SCALE_FACTORS.length - 1}
                    className="px-3 py-1 rounded-md bg-gray-600 hover:bg-gray-500 text-white disabled:opacity-50"
                  >
                    Zoom In
                  </button>
                </div>
              </div>
            </div>
            {/* EegChannelConfig (Per-Channel Settings) section remains removed */}
          </div>
        ) : activeView === 'signalGraph' ? (
          <div className="h-full p-4"> {/* This outer div might be slightly different from original, ensure it matches structure */}
            {/* Show loading state if config is not ready */}
            {!config || !config.channels || !Array.isArray(config.channels) || config.channels.length === 0 ? (
              <div className="h-full flex items-center justify-center">
                <div className="text-center">
                  <div className="text-gray-400 text-lg mb-2">Loading EEG Configuration...</div>
                  <div className="text-gray-500 text-sm">
                    Status: {configStatus}
                  </div>
                  {config && (
                    <div className="text-gray-600 text-xs mt-2">
                      Config loaded but channels: {config.channels ? `${config.channels.length} channels` : 'not available'}
                    </div>
                  )}
                </div>
              </div>
            ) : (
              <>
                {/* Time markers */}
                <div className="relative h-full"> {/* This div is important for positioning */}
                  <div className="absolute w-full flex justify-between px-2 -top-6 text-gray-400 text-sm">
                    {[...TIME_TICKS].reverse().map(time => (
                      <div key={time}>{time}s</div>
                    ))}
                  </div>
                  
                  <div className="relative h-full min-h-[300px]" ref={containerRef}> {/* EegRenderer and related elements - added min-height */}
                {/* Channel labels */}
                <div className="absolute -left-8 h-full flex flex-col justify-between">
                  {config?.channels && config.channels.length > 0 ? (
                    config.channels.map((chIdx) => (
                      <div key={chIdx} className="text-gray-400 font-medium">Ch{chIdx}</div>
                    ))
                  ) : (
                    <div className="text-gray-400 font-medium">No channel info</div>
                  )}
                </div>
                
                <canvas
                  ref={canvasRef}
                  className="w-full h-full border border-gray-700 rounded-lg"
                />
                
                <EegRenderer
                  canvasRef={canvasRef}
                  dataRef={dataRef}
                  config={config}
                  latestTimestampRef={latestTimestampRef}
                  debugInfoRef={debugInfoRef}
                  containerWidth={containerSize.width}
                  containerHeight={containerSize.height}
                  linesReady={linesReady}
                  dataVersion={dataVersion}
                  targetFps={displayFps}
                />
                </div>
              </div>
            </>
            )}
          </div>
        ) : activeView === 'appletBrainWaves' ? (
          // Brain Waves Applet View
          <div className="relative h-full p-2" ref={containerRef}> {/* Added padding for aesthetics */}
            {config && containerSize.width > 0 && containerSize.height > 0 ? (
              <BrainWavesDisplay
                eegConfig={config}
                containerWidth={containerSize.width}
                containerHeight={containerSize.height} // Use full container height
              />
            ) : (
              <div className="flex items-center justify-center h-full text-white">
                <p>Loading Brain Waves Applet or waiting for configuration/size...</p>
              </div>
            )}
            {/* Optional: Keep a minimal status bar if desired, or let the applet handle its own status */}
            {/* <EegStatusBar
              status={status} // This status is for the main /eeg/data WebSocket
              fps={displayFps}
              config={config}
              latestTimestampRef={latestTimestampRef} // This is for the main /eeg/data WebSocket
              debugInfoRef={debugInfoRef}
            /> */}
          </div>
       ) : null}
     </div>
   </div>
  );
}