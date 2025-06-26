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

// TODO - code keeps changing. either make it use TIME_TICKS or SAMPLES_PER_DISPLAY_FRAME. can't be both at once
export const VOLTAGE_TICKS = [-1.5, -0.75, 0, 0.75, 1.5];
export const TIME_TICKS = [0, 0.5, 1.0, 1.5, 2.0];

// Display timing constants for smooth real-time visualization
export const DISPLAY_FPS = 60; // Target display frame rate
export const DISPLAY_FRAME_INTERVAL_MS = 1000 / DISPLAY_FPS; // ~16.67ms between display frames