'use client';

// Default constants (will be overridden by server config when available)
export const DEFAULT_SAMPLE_RATE = 250;
export const DEFAULT_BATCH_SIZE = 32;
export const WINDOW_DURATION = 2000; // ms
export const GRAPH_HEIGHT = 100;
export const GRAPH_WIDTH = 400;

// Elata theme colors for EEG channels (converted to 0-1 range for WebGL)
export const BASE_CHANNEL_COLORS = [
  [0.376, 0.447, 0.455, 1], // #607274 - elataGreen (Fp1)
  [0.565, 0.424, 0.376, 1], // #906C60 - warm brown (Fp2)
  [0.282, 0.345, 0.376, 1], // #485860 - dark teal (F3)
  [0.753, 0.565, 0.376, 1], // #C09060 - warm tan (F4)
  [0.376, 0.565, 0.424, 1], // #60906C - sage green (C3)
  [0.424, 0.376, 0.565, 1], // #6C6090 - muted purple (C4)
  [0.565, 0.376, 0.424, 1], // #90606C - dusty rose (P3)
  [0.376, 0.424, 0.565, 1]  // #606C90 - slate blue (P4)
];

// TODO - code keeps changing. either make it use TIME_TICKS or SAMPLES_PER_DISPLAY_FRAME. can't be both at once
export const VOLTAGE_TICKS = [-1.5, -0.75, 0, 0.75, 1.5];
export const TIME_TICKS = [0, 0.5, 1.0, 1.5, 2.0];

// Display timing constants for smooth real-time visualization
export const DISPLAY_FPS = 60; // Target display frame rate
export const DISPLAY_FRAME_INTERVAL_MS = 1000 / DISPLAY_FPS; // ~16.67ms between display frames