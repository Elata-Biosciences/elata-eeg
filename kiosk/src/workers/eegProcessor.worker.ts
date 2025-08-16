import { ResizableRingBuffer } from '../utils/ringBuffer';

let ringBuffer: ResizableRingBuffer | null = null;

self.onmessage = (event) => {
  const { type, payload } = event.data;

  switch (type) {
    case 'init':
      ringBuffer = new ResizableRingBuffer(payload.buffer);
      break;
    case 'data':
      if (ringBuffer) {
        const { samples, meta } = payload;
        const numMetaChannels = meta.channel_names.length;
        if (numMetaChannels === 0) return;

        // Set NCH to the actual number of channels from metadata
        const NCH = numMetaChannels;
        
        // Write the full interleaved batch through
        const batch = new Float32Array(samples);
        ringBuffer.write(batch);
      }
      break;
  }
};