export class ResizableRingBuffer {
  private buffer: Float32Array;
  private writePtr: number = 0;
  private readPtr: number = 0;
  private isFull: boolean = false;

  constructor(initialSizeOrBuffer: number | SharedArrayBuffer) {
    if (typeof initialSizeOrBuffer === 'number') {
      this.buffer = new Float32Array(new SharedArrayBuffer(initialSizeOrBuffer * Float32Array.BYTES_PER_ELEMENT));
    } else {
      this.buffer = new Float32Array(initialSizeOrBuffer);
    }
  }

  write(data: Float32Array) {
    const availableSpace = this.buffer.length - this.writePtr;
    if (data.length > availableSpace) {
      const remaining = data.length - availableSpace;
      this.buffer.set(data.subarray(0, availableSpace), this.writePtr);
      this.buffer.set(data.subarray(availableSpace), 0);
      this.writePtr = remaining;
    } else {
      this.buffer.set(data, this.writePtr);
      this.writePtr += data.length;
    }
  }

  read(size: number): Float32Array {
    const out = new Float32Array(size);
    const availableData = this.writePtr - this.readPtr;
    if (size > availableData) {
      // Not enough data to read
      return out;
    }
    if (this.readPtr + size > this.buffer.length) {
      const firstChunkSize = this.buffer.length - this.readPtr;
      out.set(this.buffer.subarray(this.readPtr, this.buffer.length));
      out.set(this.buffer.subarray(0, size - firstChunkSize), firstChunkSize);
      this.readPtr = size - firstChunkSize;
    } else {
      out.set(this.buffer.subarray(this.readPtr, this.readPtr + size));
      this.readPtr += size;
    }
    return out;
  }

  get capacity(): number {
    return this.buffer.length;
  }
}