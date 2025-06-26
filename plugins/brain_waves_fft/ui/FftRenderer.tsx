'use client';

import React, { useEffect, useRef } from 'react';
import { getChannelColor } from '../../../kiosk/src/utils/colorUtils';

// --- Constants for Styling and Layout ---
const GRID_COLOR = 'rgba(64, 64, 64, 1)'; // Darker gray for grid lines
const LABEL_COLOR = '#bbbbbb'; // Light gray for labels
const AXIS_TITLE_COLOR = '#dddddd';
const CANVAS_BG_COLOR = 'rgba(13, 13, 13, 1)'; // Dark background

const MARGIN_LEFT = 50;
const MARGIN_BOTTOM = 40;
const MARGIN_TOP = 20;
const MARGIN_RIGHT = 20;

const DATA_Y_MIN = -4;
const DATA_Y_MAX = 4.0;
const FFT_MIN_FREQ_HZ = 1;
const FFT_MAX_FREQ_HZ = 70;

interface FftData {
  psd_packets: { channel: number; psd: number[] }[];
  fft_config: {
    fft_size: number;
    sample_rate: number;
    window_function: string;
  };
}

interface FftRendererProps {
  data: FftData | null;
  isActive: boolean;
  containerWidth: number;
  containerHeight: number;
  targetFps?: number;
}

export const FftRenderer: React.FC<FftRendererProps> = ({
  data,
  isActive,
  containerWidth,
  containerHeight,
  targetFps = 30,
}) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const animationFrameIdRef = useRef<number | null>(null);
  const lastUpdateTimeRef = useRef<number>(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !isActive || containerWidth === 0 || containerHeight === 0) {
      if (animationFrameIdRef.current) {
        cancelAnimationFrame(animationFrameIdRef.current);
      }
      return;
    }

    const context = canvas.getContext('2d');
    if (!context) return;

    const dpr = window.devicePixelRatio || 1;
    const physicalWidth = Math.round(containerWidth * dpr);
    const physicalHeight = Math.round(containerHeight * dpr);

    if (canvas.width !== physicalWidth || canvas.height !== physicalHeight) {
      canvas.width = physicalWidth;
      canvas.height = physicalHeight;
      canvas.style.width = `${containerWidth}px`;
      canvas.style.height = `${containerHeight}px`;
      context.scale(dpr, dpr);
    }

    const plotWidth = containerWidth - MARGIN_LEFT - MARGIN_RIGHT;
    const plotHeight = containerHeight - MARGIN_TOP - MARGIN_BOTTOM;

    const drawGrid = () => {
      context.save();
      context.strokeStyle = GRID_COLOR;
      context.lineWidth = 0.5;

      // X-axis grid
      const xTicks = [1, 10, 20, 30, 40, 50, 60, 70];
      xTicks.forEach(freq => {
        if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
          const x = MARGIN_LEFT + ((freq - FFT_MIN_FREQ_HZ) / (FFT_MAX_FREQ_HZ - FFT_MIN_FREQ_HZ)) * plotWidth;
          context.beginPath();
          context.moveTo(x, MARGIN_TOP);
          context.lineTo(x, MARGIN_TOP + plotHeight);
          context.stroke();
        }
      });

      // Y-axis grid
      const yTicks = [DATA_Y_MIN, 0, 2, 4, 6, DATA_Y_MAX];
      yTicks.forEach(power => {
        const y = MARGIN_TOP + plotHeight - ((power - DATA_Y_MIN) / (DATA_Y_MAX - DATA_Y_MIN)) * plotHeight;
        context.beginPath();
        context.moveTo(MARGIN_LEFT, y);
        context.lineTo(MARGIN_LEFT + plotWidth, y);
        context.stroke();
      });
      context.restore();
    };

    const drawLabels = () => {
      context.save();
      context.fillStyle = LABEL_COLOR;
      context.font = '10px Arial';

      // X-axis labels
      const xTicks = [1, 10, 20, 30, 40, 50, 60, 70];
      xTicks.forEach(freq => {
        if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
          const x = MARGIN_LEFT + ((freq - FFT_MIN_FREQ_HZ) / (FFT_MAX_FREQ_HZ - FFT_MIN_FREQ_HZ)) * plotWidth;
          context.textAlign = 'center';
          context.fillText(freq.toString(), x, containerHeight - MARGIN_BOTTOM + 15);
        }
      });

      // Y-axis labels
      const yTicks = [DATA_Y_MIN, 0, 2, 4, 6, DATA_Y_MAX];
      yTicks.forEach(power => {
        const y = MARGIN_TOP + plotHeight - ((power - DATA_Y_MIN) / (DATA_Y_MAX - DATA_Y_MIN)) * plotHeight;
        context.textAlign = 'right';
        context.textBaseline = 'middle';
        context.fillText(power.toString(), MARGIN_LEFT - 10, y);
      });
      context.restore();
    };

    const drawTitles = () => {
        context.save();
        context.fillStyle = AXIS_TITLE_COLOR;
        context.font = '12px Arial';
        context.textAlign = 'center';
        context.fillText('Frequency (Hz)', MARGIN_LEFT + plotWidth / 2, containerHeight - 5);

        context.translate(15, MARGIN_TOP + plotHeight / 2);
        context.rotate(-Math.PI / 2);
        context.fillText('log₁₀ Power (µV²/Hz)', 0, 0);
        context.restore();
    };

    const animate = (timestamp: number) => {
      animationFrameIdRef.current = requestAnimationFrame(animate);
      const updateInterval = 1000 / targetFps;
      if (timestamp - lastUpdateTimeRef.current < updateInterval) {
        return;
      }
      lastUpdateTimeRef.current = timestamp;

      context.fillStyle = CANVAS_BG_COLOR;
      context.fillRect(0, 0, containerWidth, containerHeight);

      drawGrid();
      drawLabels();
      drawTitles();

      if (data && data.psd_packets) {
        const { sample_rate } = data.fft_config;
        const maxFreq = sample_rate / 2;

        data.psd_packets.forEach((packet) => {
          const colorTuple = getChannelColor(packet.channel);
          context.strokeStyle = `rgba(${colorTuple[0] * 255}, ${colorTuple[1] * 255}, ${colorTuple[2] * 255}, 1)`;
          context.lineWidth = 1.5;
          context.beginPath();

          const numPoints = packet.psd.length;
          for (let j = 0; j < numPoints; j++) {
            const freq = (j * maxFreq) / (numPoints > 1 ? numPoints - 1 : 1);
            if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
              const psdVal = Math.min(Math.max(packet.psd[j], DATA_Y_MIN), DATA_Y_MAX);
              const x = MARGIN_LEFT + ((freq - FFT_MIN_FREQ_HZ) / (FFT_MAX_FREQ_HZ - FFT_MIN_FREQ_HZ)) * plotWidth;
              const y = MARGIN_TOP + plotHeight - ((psdVal - DATA_Y_MIN) / (DATA_Y_MAX - DATA_Y_MIN)) * plotHeight;
              if (j === 0 || ( ( (j-1) * maxFreq) / (numPoints > 1 ? numPoints - 1 : 1) < FFT_MIN_FREQ_HZ) ) {
                context.moveTo(x, y);
              } else {
                context.lineTo(x, y);
              }
            }
          }
          context.stroke();
        });
      }
    };

    animate(0);

    return () => {
      if (animationFrameIdRef.current) {
        cancelAnimationFrame(animationFrameIdRef.current);
      }
    };
  }, [data, isActive, containerWidth, containerHeight, targetFps]);

  return (
    <canvas
      ref={canvasRef}
      style={{ display: isActive ? 'block' : 'none', width: '100%', height: '100%' }}
    />
  );
};