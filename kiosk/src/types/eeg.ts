// Defines the core data structures for EEG data handling and rendering.

/**
 * Represents a single EEG data point from a specific channel at a specific time.
 */
export interface EegSample {
  value: number;
  timestamp: bigint;
  channelIndex: number;
}

/**
 * A collection of EEG samples from a single data packet, along with the
 * configuration under which they were captured.
 */
export interface SampleChunk {
  config: {
    channelCount: number;
    sampleRate: number;
  };
  samples: EegSample[];
}