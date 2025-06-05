'use client';

import { useRef, useEffect, useLayoutEffect, useState, useMemo } from 'react';
// @ts-ignore: WebglLine might be missing from types or setY might be, but setY exists at runtime
import { WebglPlot, ColorRGBA, WebglLine } from 'webgl-plot';
import { getChannelColor } from '../../../../kiosk/src/utils/colorUtils';
import { FFT_MIN_FREQ_HZ, FFT_MAX_FREQ_HZ } from '../../../../kiosk/src/utils/eegConstants'; // Import constants

const DATA_Y_MAX = 10000.0; // Expected maximum for FFT power data (µV²/Hz)
const Y_AXIS_LOG_MIN_POWER_CLAMP = 0.01; // Clamp input power to this minimum before log10
const LOG_Y_MIN_DISPLAY = Math.log10(Y_AXIS_LOG_MIN_POWER_CLAMP); // e.g., -2 for 0.01
const LOG_Y_MAX_DISPLAY = Math.ceil(Math.log10(DATA_Y_MAX));    // e.g., Math.ceil(log10(4000)) = 4
                                                              // This defines the log display range, e.g., [-2, 4]

const GRID_COLOR = new ColorRGBA(0.25, 0.25, 0.25, 1); // Darker gray for grid lines
const LABEL_COLOR = '#bbbbbb'; // Light gray for labels
const AXIS_TITLE_COLOR = '#dddddd';
const CANVAS_BG_COLOR = new ColorRGBA(0.05, 0.05, 0.05, 1); // Dark background

// Margins for labels
const MARGIN_LEFT = 50; // Space for Y-axis labels
const MARGIN_BOTTOM = 40; // Space for X-axis labels & title
const MARGIN_TOP = 20; // Space for plot title (if any) or just padding
const MARGIN_RIGHT = 20; // Padding

interface LabelInfo {
  value: string;
  position: number; // Pixel position on the canvas dimension
  normalized: number; // Normalized WebGL coordinate (-1 to 1)
}

interface FftRendererProps {
  config: any; // EEG configuration (for sample_rate, channels)
  containerWidth: number;
  containerHeight: number;
  targetFps?: number; // Optional, for controlling update rate
}

// Interface for DSP WebSocket response
interface ChannelFftResult {
  power: number[];
  frequencies: number[];
}

interface BrainWavesAppletResponse {
  timestamp: number;
  fft_results: ChannelFftResult[];
  error?: string;
}

export function AppletFftRenderer({ // Renamed from FftRenderer
  config,
  containerWidth,
  containerHeight,
  targetFps = 30, // Default FFT update FPS
}: FftRendererProps) {
  const internalCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const [fftLinesReady, setFftLinesReady] = useState(false);
  const wglpRef = useRef<WebglPlot | null>(null); // For WebglPlot instance
  const linesRef = useRef<WebglLine[]>([]); // Holds WebglLine line objects for FFT
  const gridLinesRef = useRef<WebglLine[]>([]); // Holds WebglLine objects for the grid
  const animationFrameIdRef = useRef<number | null>(null);
  const lastUpdateTimeRef = useRef<number>(0);
  const wsRef = useRef<WebSocket | null>(null);
  const [fftData, setFftData] = useState<ChannelFftResult[]>([]);
  const [wsError, setWsError] = useState<string | null>(null);

  const [axisLabels, setAxisLabels] = useState<{ x: LabelInfo[], y: LabelInfo[] }>({ x: [], y: [] });

  const plotWidth = useMemo(() => Math.max(0, containerWidth - MARGIN_LEFT - MARGIN_RIGHT), [containerWidth]);
  const plotHeight = useMemo(() => Math.max(0, containerHeight - MARGIN_TOP - MARGIN_BOTTOM), [containerHeight]);

  // WebSocket connection effect
  useEffect(() => {
    if (!config || !config.channels || config.channels.length === 0) {
      return;
    }

    let reconnectAttempts = 0;
    const maxReconnectAttempts = 5;
    let reconnectTimeout: number | null = null;

    const connectWebSocket = () => {
      try {
        // Connect to DSP WebSocket endpoint
        const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
        const wsUrl = `ws://${wsHost}:8080/applet/brain_waves/data`;
        console.log('[AppletFftRenderer] Attempting to connect to:', wsUrl);
        const ws = new WebSocket(wsUrl);
        wsRef.current = ws;

        ws.onopen = () => {
          console.log('[AppletFftRenderer] Connected to DSP WebSocket');
          setWsError(null);
          reconnectAttempts = 0; // Reset reconnection attempts on successful connection
        };

        ws.onmessage = (event) => {
          try {
            const response: BrainWavesAppletResponse = JSON.parse(event.data);
            
            console.log('[AppletFftRenderer] Received WebSocket message:', {
              timestamp: response.timestamp,
              fft_results_count: response.fft_results?.length || 0,
              error: response.error
            });
            
            if (response.error) {
              console.error('[AppletFftRenderer] DSP error:', response.error);
              setWsError(response.error);
              return;
            }

            if (response.fft_results && response.fft_results.length > 0) {
              // Debug: Log FFT data details
              response.fft_results.forEach((result, i) => {
                console.log(`[AppletFftRenderer] Channel ${i}: ${result.power.length} power bins, ${result.frequencies.length} freq bins`);
                if (result.power.length > 0) {
                  const maxPower = Math.max(...result.power);
                  console.log(`[AppletFftRenderer] Channel ${i} max power: ${maxPower}`);
                }
              });
              
              setFftData(response.fft_results);
              setWsError(null);
            } else {
              console.log('[AppletFftRenderer] No FFT results in message');
            }
          } catch (error) {
            console.error('[AppletFftRenderer] Error parsing WebSocket message:', error);
            setWsError('Failed to parse FFT data');
          }
        };

        ws.onerror = (error) => {
          console.error('[AppletFftRenderer] WebSocket error:', error);
          setWsError('WebSocket connection error');
        };

        ws.onclose = (event) => {
          console.log('[AppletFftRenderer] WebSocket connection closed', {
            code: event.code,
            reason: event.reason,
            wasClean: event.wasClean,
            reconnectAttempts
          });
          wsRef.current = null;
          
          // Only attempt to reconnect if it wasn't a clean close and we haven't exceeded max attempts
          if (!event.wasClean && event.code !== 1000 && reconnectAttempts < maxReconnectAttempts) {
            reconnectAttempts++;
            const delay = Math.min(2000 * reconnectAttempts, 10000); // Exponential backoff, max 10s
            console.log(`[AppletFftRenderer] Attempting to reconnect in ${delay}ms (attempt ${reconnectAttempts}/${maxReconnectAttempts})`);
            reconnectTimeout = window.setTimeout(connectWebSocket, delay);
          } else if (reconnectAttempts >= maxReconnectAttempts) {
            console.error('[AppletFftRenderer] Max reconnection attempts reached');
            setWsError('Connection failed after multiple attempts');
          }
        };
      } catch (error) {
        console.error('[AppletFftRenderer] Failed to create WebSocket:', error);
        setWsError('Failed to connect to DSP service');
      }
    };

    connectWebSocket();

    return () => {
      // Clear any pending reconnection timeout
      if (reconnectTimeout) {
        clearTimeout(reconnectTimeout);
        reconnectTimeout = null;
      }
      
      if (wsRef.current) {
        // Prevent the onclose handler from trying to reconnect
        // when we are intentionally closing due to unmount.
        wsRef.current.onclose = null;
        wsRef.current.close();
        wsRef.current = null;
        console.log('[AppletFftRenderer] WebSocket intentionally closed on unmount.');
      }
    };
  }, [config]);

  // Initialize WebGL context and plot instance
  useLayoutEffect(() => {
    if (internalCanvasRef.current && containerWidth > 0 && containerHeight > 0) {
      if (!wglpRef.current) {
        // @ts-ignore
        wglpRef.current = new WebglPlot(internalCanvasRef.current, {
          antialias: true,
          transparent: false, // Keep canvas opaque for performance
          powerPerformance: "high-performance", // Corrected property name
        });
        console.log('[AppletFftRenderer] WebGLPlot instance created.');
      }
      internalCanvasRef.current.width = containerWidth; // Full container size for canvas
      internalCanvasRef.current.height = containerHeight;
      // Viewport is set for the actual plotting area, offset by margins
      wglpRef.current.viewport(MARGIN_LEFT, MARGIN_BOTTOM, plotWidth, plotHeight);
    }

    // Cleanup function
    return () => {
      if (wglpRef.current) {
        // Clear all lines before destroying the plot
        try {
          if (typeof wglpRef.current.clear === 'function') {
            wglpRef.current.clear();
          } else if (typeof wglpRef.current.removeLine === 'function') {
            // Fallback if clear is not available for some reason, or to be very thorough
            [...gridLinesRef.current, ...linesRef.current].forEach(line => {
              if (wglpRef.current?.removeLine) { // Ensure removeLine exists before calling
                 wglpRef.current.removeLine(line);
              }
            });
          }
        } catch (error) {
          console.warn('[AppletFftRenderer] Error clearing lines during cleanup:', error);
        }
        
        gridLinesRef.current = [];
        linesRef.current = [];
        
        // WebGL plot instance is managed by React's lifecycle and garbage collection.
        // The canvas element will be removed from the DOM.
        // Previous steps (clear/removeLine) handle clearing plot contents.
        
        wglpRef.current = null;
        console.log('[AppletFftRenderer] WebGLPlot instance cleaned up.');
      }
    };
  }, [containerWidth, containerHeight, plotWidth, plotHeight]);

  // Effect to create/update grid lines and labels
  useLayoutEffect(() => {
    if (!wglpRef.current || plotWidth <= 0 || plotHeight <= 0) {
      return;
    }
    const wglp = wglpRef.current;

    // Clear existing grid lines - use a more robust approach
    if (gridLinesRef.current.length > 0) {
      try {
        // Try different methods to clear lines
        if (wglp && typeof wglp.removeAllLines === 'function') {
          wglp.removeAllLines();
        } else if (wglp && typeof wglp.clear === 'function') {
          wglp.clear();
        } else if (wglp && typeof wglp.removeLine === 'function') {
          gridLinesRef.current.forEach(line => wglp.removeLine(line));
        } else {
          console.warn('[AppletFftRenderer] No clear method available, recreating WebGL plot instance');
          // Recreate the WebGL plot instance as a fallback
          if (internalCanvasRef.current) {
            wglpRef.current = new WebglPlot(internalCanvasRef.current, {
              antialias: true,
              transparent: false,
              powerPerformance: "high-performance",
            });
            const newWglp = wglpRef.current;
            newWglp.viewport(MARGIN_LEFT, MARGIN_BOTTOM, plotWidth, plotHeight);
          }
        }
      } catch (error) {
        console.warn('[AppletFftRenderer] Error clearing grid lines:', error);
      }
      gridLinesRef.current = [];
    }

    const newGridLines: WebglLine[] = [];
    const newXLabels: LabelInfo[] = [];
    const newYLabels: LabelInfo[] = [];

    // X-axis grid lines and labels (Frequency)
    const xTicks = [1, 10, 20, 30, 40, 50, 60, 70]; // Hz
    xTicks.forEach(freq => {
      if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
        const normalizedX = 2 * (freq - FFT_MIN_FREQ_HZ) / (FFT_MAX_FREQ_HZ - FFT_MIN_FREQ_HZ) - 1;
        // Vertical grid line
        const gridX = new WebglLine(GRID_COLOR, 2);
        gridX.xy = new Float32Array([normalizedX, -1, normalizedX, 1]);
        newGridLines.push(gridX);

        // Label position (pixel space on canvas)
        const labelXPos = MARGIN_LEFT + (normalizedX + 1) / 2 * plotWidth;
        newXLabels.push({ value: freq.toString(), position: labelXPos, normalized: normalizedX });
      }
    });

    // Y-axis grid lines and labels (Log Power - Exponents)
    const yLogExponentTicks: number[] = [];
    for (let i = Math.floor(LOG_Y_MIN_DISPLAY); i <= Math.ceil(LOG_Y_MAX_DISPLAY); i++) {
      yLogExponentTicks.push(i);
    }

    yLogExponentTicks.forEach(logExponent => {
      const normalizedY = (logExponent - LOG_Y_MIN_DISPLAY) / (LOG_Y_MAX_DISPLAY - LOG_Y_MIN_DISPLAY) * 2 - 1;

      const gridY = new WebglLine(GRID_COLOR, 2);
      gridY.xy = new Float32Array([-1, normalizedY, 1, normalizedY]);
      newGridLines.push(gridY);
      
      const labelValue = logExponent.toString();
      
      const labelYPos = MARGIN_BOTTOM + (normalizedY + 1) / 2 * plotHeight; // Y is from bottom up for labels
      newYLabels.push({ value: labelValue, position: labelYPos, normalized: normalizedY });
    });

    newGridLines.forEach(line => wglp.addLine(line));
    gridLinesRef.current = newGridLines;
    setAxisLabels({ x: newXLabels, y: newYLabels.reverse() }); // Reverse Y labels for top-down display

  }, [plotWidth, plotHeight, FFT_MIN_FREQ_HZ, FFT_MAX_FREQ_HZ]);

  // Effect to create/update FFT lines based on WebSocket data
  useEffect(() => {
    setFftLinesReady(false); 

    if (!wglpRef.current || !config || !config.channels || !config.sample_rate || containerWidth <= 0) {
      if (linesRef.current.length > 0) {
        try {
          if (wglpRef.current && typeof wglpRef.current.removeAllLines === 'function') {
            wglpRef.current.removeAllLines();
          } else if (wglpRef.current && typeof wglpRef.current.removeLine === 'function') {
            linesRef.current.forEach(line => wglpRef.current!.removeLine(line));
          } else {
            console.warn('[AppletFftRenderer] No remove method available for FFT lines');
          }
        } catch (error) {
          console.warn('[AppletFftRenderer] Error clearing FFT lines:', error);
        }
        linesRef.current = [];
      }
      return;
    }

    const numChannels = config.channels.length;

    if (numChannels === 0 || fftData.length === 0) {
      if (linesRef.current.length > 0) {
        try {
          if (wglpRef.current && typeof wglpRef.current.removeAllLines === 'function') {
            wglpRef.current.removeAllLines();
          } else if (wglpRef.current && typeof wglpRef.current.removeLine === 'function') {
            linesRef.current.forEach(line => wglpRef.current!.removeLine(line));
          } else {
            console.warn('[AppletFftRenderer] No remove method available for FFT lines');
          }
        } catch (error) {
          console.warn('[AppletFftRenderer] Error clearing FFT lines:', error);
        }
        linesRef.current = [];
      }
      return;
    }

    // Use the first channel's frequency data to determine the relevant frequency range
    const firstChannelFreqs = fftData[0]?.frequencies || [];
    const relevantFreqIndices: number[] = [];
    const xCoords: number[] = [];

    firstChannelFreqs.forEach((freq, index) => {
      if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
        relevantFreqIndices.push(index);
        const normalizedX = 2 * (freq - FFT_MIN_FREQ_HZ) / (FFT_MAX_FREQ_HZ - FFT_MIN_FREQ_HZ) - 1;
        xCoords.push(normalizedX);
      }
    });

    if (xCoords.length === 0) {
      console.warn(`[AppletFftRenderer] No frequency bins found in the range ${FFT_MIN_FREQ_HZ}-${FFT_MAX_FREQ_HZ} Hz.`);
      if (linesRef.current.length > 0) {
        try {
          if (wglpRef.current && typeof wglpRef.current.removeAllLines === 'function') {
            wglpRef.current.removeAllLines();
          } else if (wglpRef.current && typeof wglpRef.current.removeLine === 'function') {
            linesRef.current.forEach(line => wglpRef.current!.removeLine(line));
          } else {
            console.warn('[AppletFftRenderer] No remove method available for FFT lines');
          }
        } catch (error) {
          console.warn('[AppletFftRenderer] Error clearing FFT lines:', error);
        }
        linesRef.current = [];
      }
      return;
    }

    // Clear existing lines
    if (linesRef.current.length > 0) {
      try {
        if (wglpRef.current && typeof wglpRef.current.removeAllLines === 'function') {
          wglpRef.current.removeAllLines();
        } else if (wglpRef.current && typeof wglpRef.current.removeLine === 'function') {
          linesRef.current.forEach(line => wglpRef.current!.removeLine(line));
        } else {
          console.warn('[AppletFftRenderer] No remove method available for FFT lines');
        }
      } catch (error) {
        console.warn('[AppletFftRenderer] Error clearing FFT lines:', error);
      }
    }
    linesRef.current = [];

    const newLines: WebglLine[] = [];

    for (let i = 0; i < numChannels; i++) {
      const colorTuple = getChannelColor(i);
      const color = new ColorRGBA(colorTuple[0], colorTuple[1], colorTuple[2], 1);
      
      const line = new WebglLine(color, xCoords.length);
      line.lineWidth = 1.5;
      
      if (line.xy) {
        for (let j = 0; j < xCoords.length; j++) {
          if ((j * 2 + 1) < line.xy.length) { 
            line.xy[j * 2] = xCoords[j];         
            line.xy[j * 2 + 1] = -0.5; // Default Y position          
          }
        }
      }

      line.scaleY = 2.0;
      line.offsetY = 0;

      newLines.push(line);
      if (wglpRef.current) {
        wglpRef.current.addLine(line);
      }
    }

    linesRef.current = newLines;
    setFftLinesReady(true); 

  }, [config, containerWidth, containerHeight, fftData]);

  // Animation loop to update FFT lines with WebSocket data
  useEffect(() => {
    if (!wglpRef.current || !fftLinesReady || linesRef.current.length === 0 || fftData.length === 0) {
      if (animationFrameIdRef.current) {
        cancelAnimationFrame(animationFrameIdRef.current);
        animationFrameIdRef.current = null;
      }
      return;
    }

    const updateInterval = 1000 / targetFps;

    const animate = (timestamp: number) => {
      animationFrameIdRef.current = requestAnimationFrame(animate);

      if (timestamp - lastUpdateTimeRef.current < updateInterval) {
        return;
      }
      lastUpdateTimeRef.current = timestamp;

      const activeChannels = config?.channels?.length || 0;

      for (let i = 0; i < activeChannels && i < fftData.length; i++) {
        const line = linesRef.current[i];
        const channelFftResult = fftData[i];

        if (line && channelFftResult && channelFftResult.power && channelFftResult.frequencies) {
          const frequencies = channelFftResult.frequencies;
          const powerData = channelFftResult.power;

          // Find relevant frequency indices for this channel
          const relevantIndices: number[] = [];
          frequencies.forEach((freq, index) => {
            if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
              relevantIndices.push(index);
            }
          });

          if (line.numPoints !== relevantIndices.length) {
            continue; 
          }

          for (let j = 0; j < relevantIndices.length; j++) {
            const fftBinIndex = relevantIndices[j];
            let currentMagnitude = powerData[fftBinIndex] || 0;

            // Clamp very small values to prevent log issues
            if (currentMagnitude <= 0 || !isFinite(currentMagnitude)) {
              currentMagnitude = Y_AXIS_LOG_MIN_POWER_CLAMP;
            }
            
            const logMagnitude = Math.log10(currentMagnitude);
            let normalizedMagnitude = (logMagnitude - LOG_Y_MIN_DISPLAY) / (LOG_Y_MAX_DISPLAY - LOG_Y_MIN_DISPLAY) - 0.5;

            if (!isFinite(normalizedMagnitude)) {
                normalizedMagnitude = -0.5;
            }

            if (line.xy && (j * 2 + 1) < line.xy.length) {
                line.xy[j * 2 + 1] = normalizedMagnitude;
            }
          }
        } else if (line) {
          // No data available, set to baseline
          if (line.xy) {
            for (let j = 0; j < line.numPoints; j++) {
                if ((j * 2 + 1) < line.xy.length) {
                    line.xy[j * 2 + 1] = -0.5; 
                }
            }
          }
        }
      }
      wglpRef.current?.update();
    };

    animationFrameIdRef.current = requestAnimationFrame(animate);

    return () => {
      if (animationFrameIdRef.current) {
        cancelAnimationFrame(animationFrameIdRef.current);
        animationFrameIdRef.current = null;
      }
    };
  }, [fftLinesReady, fftData, config, targetFps, plotWidth, plotHeight]); 

  return (
    <div style={{ width: containerWidth, height: containerHeight, position: 'relative', backgroundColor: `rgba(${CANVAS_BG_COLOR.r*255}, ${CANVAS_BG_COLOR.g*255}, ${CANVAS_BG_COLOR.b*255}, ${CANVAS_BG_COLOR.a})` }}>
      <canvas
        ref={internalCanvasRef}
        style={{
          width: containerWidth,
          height: containerHeight,
          display: 'block',
        }}
      />
      {/* Error display */}
      {wsError && (
        <div
          style={{
            position: 'absolute',
            top: 10,
            left: 10,
            color: '#ff6b6b',
            fontSize: '12px',
            backgroundColor: 'rgba(0, 0, 0, 0.7)',
            padding: '5px 10px',
            borderRadius: '3px',
          }}
        >
          DSP Error: {wsError}
        </div>
      )}
      {/* X-Axis Labels */}
      {axisLabels.x.map((label, index) => (
        <div
          key={`x-label-${index}`}
          style={{
            position: 'absolute',
            left: label.position,
            bottom: MARGIN_BOTTOM - 20, 
            transform: 'translateX(-50%)',
            color: LABEL_COLOR,
            fontSize: '10px',
          }}
        >
          {label.value}
        </div>
      ))}
      {/* Y-Axis Labels */}
      {axisLabels.y.map((label, index) => (
        <div
          key={`y-label-${index}`}
          style={{
            position: 'absolute',
            left: MARGIN_LEFT - 30, 
            bottom: label.position, 
            transform: 'translateY(50%)', 
            color: LABEL_COLOR,
            fontSize: '10px',
            width: '25px', 
            textAlign: 'right',
          }}
        >
          {label.value}
        </div>
      ))}
      {/* X-Axis Title */}
      <div
        style={{
          position: 'absolute',
          left: MARGIN_LEFT + plotWidth / 2,
          bottom: 5, 
          transform: 'translateX(-50%)',
          color: AXIS_TITLE_COLOR,
          fontSize: '12px',
        }}
      >
        Frequency (Hz)
      </div>
      {/* Y-Axis Title */}
      <div
        style={{
          position: 'absolute',
          top: MARGIN_TOP + plotHeight / 2,
          left: 10, 
          transform: 'translateY(-50%) rotate(-90deg)', 
          transformOrigin: 'left top',
          color: AXIS_TITLE_COLOR,
          fontSize: '12px',
          whiteSpace: 'nowrap',
        }}
      >
        Power (log₁₀ µV²/Hz)
      </div>
    </div>
  );
}