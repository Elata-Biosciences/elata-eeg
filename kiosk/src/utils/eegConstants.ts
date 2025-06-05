'use client';

// Default constants (will be overridden by server config when available)
export const DEFAULT_SAMPLE_RATE = 250;
export const DEFAULT_BATCH_SIZE = 32;
export const WINDOW_DURATION = 2000; // ms
export const GRAPH_HEIGHT = 100;
export const GRAPH_WIDTH = 400;

// Base colors for the first 4 channels
export const BASE_CHANNEL_COLORS = [
  [0.13, 0.59, 0.95, 1], // #2196f3 - Blue
  [0.30, 0.69, 0.31, 1], // #4caf50 - Green
  [0.96, 0.26, 0.21, 1], // #f44336 - Red
  [0.61, 0.15, 0.69, 1], // #9c27b0 - Purple
  [1.00, 0.76, 0.03, 1], // #ffc107 - Amber
  [0.00, 0.74, 0.83, 1], // #00bcd4 - Cyan
  [0.91, 0.12, 0.39, 1], // #e91e63 - Pink
  [0.55, 0.76, 0.29, 1]  // #8bc34a - Light Green
];

export const VOLTAGE_TICKS = [-1.5, -0.75, 0, 0.75, 1.5];
export const TIME_TICKS = [0, 0.5, 1.0, 1.5, 2.0];

// FFT Related Constants
export const FFT_WINDOW_DURATION_MS = 2000; // Duration of the window for FFT calculation in milliseconds
export const FFT_HOP_DURATION_MS = 1000;    // Hop size or interval between FFT calculations in milliseconds
export const FFT_WINDOW_TYPE = 'hann';      // Windowing function to apply before FFT (e.g., 'hann', 'hamming')

// Constants for FFT display range (can be adjusted as needed)
export const FFT_MIN_FREQ_HZ = 1;  // Minimum frequency to display on the FFT plot in Hz
export const FFT_MAX_FREQ_HZ = 70; // Maximum frequency to display on the FFT plot in Hz