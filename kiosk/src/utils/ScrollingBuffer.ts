'use client';

/**
 * ScrollingBuffer.ts
 *
 * Optimized fixed-size buffer for real-time scrolling display of EEG data.
 *
 * IMPORTANT: This class contains critical logic for maintaining graph continuity:
 * 1. Efficient data storage using Float32Array for performance
 * 2. Dirty flag handling to ensure rendering continues during connection issues
 * 3. Only resets dirty flag when data is actually available (line ~57)
 *
 * DO NOT MODIFY the dirty flag handling as it's essential for preventing graph clearing.
 */
export class ScrollingBuffer {
  private buffer: Float32Array;
  private size: number = 0;
  private dirty: boolean = false;
  
  constructor(private capacity: number) {
    this.buffer = new Float32Array(capacity);
  }
  
  // Add a new value to the buffer
  push(value: number) {
    // If buffer is full, shift everything left
    if (this.size === this.capacity) {
      // Shift all values left by one position
      this.buffer.copyWithin(0, 1);
      // Add new value at the end
      this.buffer[this.capacity - 1] = value;
    } else {
      // Buffer not full yet, just add to the end
      this.buffer[this.size] = value;
      this.size++;
    }
    this.dirty = true;
  }
  
  // Check if buffer has new data since last render
  isDirty(): boolean {
    return this.dirty;
  }
  
  // Get data for rendering without creating new arrays
  getData(points: Float32Array) {
    if (this.size === 0) {
      return 0;
    }
    
    // Calculate spacing between points in normalized coordinates
    const spacing = 1.0 / this.capacity;
    
    // Check if we might exceed buffer bounds and log warning
    if (this.size * 2 > points.length) {
      console.warn(`[ScrollingBuffer] Buffer overflow risk: size=${this.size}, points.length=${points.length}, capacity=${this.capacity}`);
      // Limit size to prevent buffer overflow
      this.size = Math.floor(points.length / 2);
    }
    
    // Fill the points array with x,y pairs
    for (let i = 0; i < this.size; i++) {
      // For traditional EEG style (right-to-left):
      // Map newest points (higher indices) to higher x values (right side)
      // This ensures consistent behavior between initial fill and steady state
      const normalizedX = (this.size - 1 - i) / (this.capacity - 1);
      points[i * 2] = normalizedX;
      
      // y = normalized value
      points[i * 2 + 1] = this.buffer[i];
    }
    
    // CRITICAL FIX: Only reset dirty flag if we actually have data
    // This ensures we keep trying to render if connection is lost
    // DO NOT MODIFY this condition as it's essential for preventing graph clearing
    if (this.size > 0) {
      this.dirty = false;
    }
    
    return this.size; // Return the number of points added
  }
  
  // Get the capacity of this buffer
  getCapacity(): number {
    return this.capacity;
  }
}