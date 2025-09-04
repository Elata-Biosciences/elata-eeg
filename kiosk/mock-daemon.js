const { createServer } = require('http');
const { parse } = require('url');
const WebSocket = require('ws');

// Mock EEG configuration
const mockConfig = {
  board_driver: "MockEeg",
  channels: [
    { id: 0, name: "Fp1", enabled: true },
    { id: 1, name: "Fp2", enabled: true },
    { id: 2, name: "F3", enabled: true },
    { id: 3, name: "F4", enabled: true },
    { id: 4, name: "C3", enabled: true },
    { id: 5, name: "C4", enabled: true },
    { id: 6, name: "P3", enabled: true },
    { id: 7, name: "P4", enabled: true },
  ],
  sample_rate: 1000,
  powerline_filter_hz: 60,
  gain: 24,
};

// EEG Generator class (based on Rust implementation)
class EegGenerator {
  constructor(sampleRate, numChannels) {
    this.sampleRate = sampleRate;
    this.numChannels = numChannels;
    
    // Phase accumulators for each frequency band and channel
    this.deltaPhase = new Array(numChannels).fill(0).map(() => Math.random() * 2 * Math.PI);
    this.thetaPhase = new Array(numChannels).fill(0).map(() => Math.random() * 2 * Math.PI);
    this.alphaPhase = new Array(numChannels).fill(0).map(() => Math.random() * 2 * Math.PI);
    this.betaPhase = new Array(numChannels).fill(0).map(() => Math.random() * 2 * Math.PI);
    this.gammaPhase = new Array(numChannels).fill(0).map(() => Math.random() * 2 * Math.PI);
    this.lineNoise50Phase = new Array(numChannels).fill(0).map(() => Math.random() * 2 * Math.PI);
    this.lineNoise60Phase = new Array(numChannels).fill(0).map(() => Math.random() * 2 * Math.PI);
    
    // Random walk state for each channel
    this.randomWalk = new Array(numChannels).fill(0);
    
    // Baseline offset for each channel to separate them visually
    this.channelBaseline = new Array(numChannels).fill(0).map((_, i) => {
      // Create distinct baselines for each channel (smaller offset for UI compatibility)
      return (i - numChannels/2) * 1000; // Smaller offset that works with UI scaling
    });
    
    // Frequencies for each band (Hz)
    this.deltaFreq = 2.5;
    this.thetaFreq = 6.0;
    this.alphaFreq = 10.0;
    this.betaFreq = 20.0;
    this.gammaFreq = 40.0;
    
    // Channel weights [delta, theta, alpha, beta, gamma] based on brain regions
    // Optimized amplitudes to match device visualization
    this.channelWeights = [
      [1.5, 1.2, 0.8, 0.4, 0.1],  // Fp1 - Frontal left
      [1.5, 1.2, 0.8, 0.4, 0.1],  // Fp2 - Frontal right  
      [1.2, 1.0, 1.4, 0.6, 0.1],  // F3 - Frontal left
      [1.2, 1.0, 1.4, 0.6, 0.1],  // F4 - Frontal right
      [1.0, 0.8, 1.4, 0.6, 0.12], // C3 - Central left
      [1.0, 0.8, 1.4, 0.6, 0.12], // C4 - Central right
      [0.8, 0.6, 2.0, 0.7, 0.1],  // P3 - Parietal left (stronger alpha)
      [0.8, 0.6, 2.0, 0.7, 0.1],  // P4 - Parietal right (stronger alpha)
    ];
    
    // Line noise amplitude for each channel (reduced for cleaner visualization)
    this.lineNoiseAmplitude = new Array(numChannels).fill(0).map(() => 0.05 + Math.random() * 0.1);
  }
  
  generateSample(channel) {
    // Phase increments for each oscillator
    const deltaPhaseInc = 2 * Math.PI * this.deltaFreq / this.sampleRate;
    const thetaPhaseInc = 2 * Math.PI * this.thetaFreq / this.sampleRate;
    const alphaPhaseInc = 2 * Math.PI * this.alphaFreq / this.sampleRate;
    const betaPhaseInc = 2 * Math.PI * this.betaFreq / this.sampleRate;
    const gammaPhaseInc = 2 * Math.PI * this.gammaFreq / this.sampleRate;
    const line50Inc = 2 * Math.PI * 50.0 / this.sampleRate;
    const line60Inc = 2 * Math.PI * 60.0 / this.sampleRate;
    
    // Update phases
    this.deltaPhase[channel] += deltaPhaseInc;
    this.thetaPhase[channel] += thetaPhaseInc;
    this.alphaPhase[channel] += alphaPhaseInc;
    this.betaPhase[channel] += betaPhaseInc;
    this.gammaPhase[channel] += gammaPhaseInc;
    this.lineNoise50Phase[channel] += line50Inc;
    this.lineNoise60Phase[channel] += line60Inc;
    
    // Wrap phases to avoid precision issues
    this.deltaPhase[channel] = this.deltaPhase[channel] % (2 * Math.PI);
    this.thetaPhase[channel] = this.thetaPhase[channel] % (2 * Math.PI);
    this.alphaPhase[channel] = this.alphaPhase[channel] % (2 * Math.PI);
    this.betaPhase[channel] = this.betaPhase[channel] % (2 * Math.PI);
    this.gammaPhase[channel] = this.gammaPhase[channel] % (2 * Math.PI);
    this.lineNoise50Phase[channel] = this.lineNoise50Phase[channel] % (2 * Math.PI);
    this.lineNoise60Phase[channel] = this.lineNoise60Phase[channel] % (2 * Math.PI);
    
    // Get channel weights
    const weights = this.channelWeights[channel % this.channelWeights.length];
    
    // Generate signals for each frequency band
    const delta = Math.sin(this.deltaPhase[channel]) * weights[0];
    const theta = Math.sin(this.thetaPhase[channel]) * weights[1];
    const alpha = Math.sin(this.alphaPhase[channel]) * weights[2];
    const beta = Math.sin(this.betaPhase[channel]) * weights[3];
    const gamma = Math.sin(this.gammaPhase[channel]) * weights[4];
    
    // Add line noise (reduced)
    const lineNoise50 = Math.sin(this.lineNoise50Phase[channel]) * this.lineNoiseAmplitude[channel] * 0.3;
    const lineNoise60 = Math.sin(this.lineNoise60Phase[channel]) * this.lineNoiseAmplitude[channel] * 0.2;
    
    // Random walk for realistic drift (reduced)
    this.randomWalk[channel] += (Math.random() - 0.5) * 0.02;
    this.randomWalk[channel] *= 0.9995; // Stronger decay factor
    
    // Random noise (1/f approximation, reduced)
    const noise = (Math.random() - 0.5) * 0.05;
    
    // Occasional eye blinks for frontal channels (reduced amplitude)
    let eyeBlink = 0;
    if (channel < 2 && Math.random() < 0.00005) { // Fp1, Fp2, less frequent
      eyeBlink = (Math.random() - 0.5) * 5; // Much smaller blinks
    }
    
    // Combine all components
    const signal = delta + theta + alpha + beta + gamma + lineNoise50 + lineNoise60 + noise + this.randomWalk[channel] + eyeBlink;
    
    // Scale and add channel baseline offset to separate channels visually
    const amplitude = 5000.0; // Amplitude for the waveform
    const scaledSignal = signal * amplitude;
    const finalSignal = scaledSignal + this.channelBaseline[channel];
    
    return Math.round(finalSignal);
  }
}

// Global EEG generator instance
let eegGenerator = null;

// Track SSE connections for broadcasting
const sseConnections = new Set();

// Broadcast event to all SSE connections
function broadcastSSEEvent(event) {
  const eventData = `data: ${JSON.stringify(event)}\n\n`;
  sseConnections.forEach(res => {
    try {
      res.write(eventData);
    } catch (error) {
      console.error('[Mock Daemon] Error broadcasting SSE event:', error);
      sseConnections.delete(res);
    }
  });
}

// Generate mock FFT data
function generateFFTData() {
  const fftSize = 512;
  const sampleRate = mockConfig.sample_rate;
  const numChannels = mockConfig.channels.length;
  
  // Generate PSD (Power Spectral Density) data for each channel
  const psd_packets = mockConfig.channels.map((channel, channelIndex) => {
    const psd = new Array(fftSize / 2).fill(0).map((_, freqIndex) => {
      const frequency = (freqIndex * sampleRate) / fftSize;
      
      // Create realistic brain wave patterns
      let power = 0;
      
      // Delta (1-4 Hz): Higher power, decreases with frequency
      if (frequency >= 1 && frequency <= 4) {
        power += (5 - frequency) * (2 + Math.random());
      }
      
      // Theta (4-8 Hz): Moderate power
      if (frequency >= 4 && frequency <= 8) {
        power += 1.5 + Math.random() * 0.5;
      }
      
      // Alpha (8-12 Hz): Peak around 10 Hz
      if (frequency >= 8 && frequency <= 12) {
        const alphaPeak = 10;
        const distFromPeak = Math.abs(frequency - alphaPeak);
        power += Math.max(0, 3 - distFromPeak * 0.5) + Math.random() * 0.3;
      }
      
      // Beta (12-30 Hz): Lower power, varies by channel
      if (frequency >= 12 && frequency <= 30) {
        power += (0.8 + Math.random() * 0.4) * (channelIndex % 2 === 0 ? 1.2 : 0.8);
      }
      
      // Gamma (30-70 Hz): Very low power
      if (frequency >= 30 && frequency <= 70) {
        power += 0.2 + Math.random() * 0.1;
      }
      
      // Add some 1/f noise (decreases with frequency)
      power += (1 / Math.max(1, frequency)) * (0.1 + Math.random() * 0.05);
      
      // Add channel-specific variation
      power *= (0.8 + (channelIndex * 0.05) + Math.random() * 0.2);
      
      return Math.max(0.001, power); // Ensure positive values
    });
    
    return {
      channel: channelIndex,
      psd: psd
    };
  });
  
  return {
    psd_packets,
    fft_config: {
      fft_size: fftSize,
      sample_rate: sampleRate,
      window_function: "Hanning"
    },
    source_frame_id: Math.floor(Date.now() / 100) % 1000000,
    timestamp: Date.now() * 1000 // microseconds
  };
}

// Generate realistic EEG data using the stateful generator
function generateEEGSample() {
  const timestamp = Date.now() * 1000; // microseconds
  
  // Initialize generator if needed
  if (!eegGenerator) {
    eegGenerator = new EegGenerator(mockConfig.sample_rate, mockConfig.channels.length);
  }
  
  // Generate samples for each channel using the stateful generator
  const channels = mockConfig.channels.map((channel, index) => {
    return eegGenerator.generateSample(index);
  });
  
  return {
    timestamp,
    channels,
    sample_count: Math.floor(Date.now() / 10) % 1000000,
  };
}



// Mock API endpoints
function handleAPI(req, res) {
  const parsedUrl = parse(req.url, true);
  const { pathname } = parsedUrl;
  
  res.setHeader('Content-Type', 'application/json');
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Methods', 'GET, POST, PUT, DELETE, OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type');
  
  if (req.method === 'OPTIONS') {
    res.writeHead(200);
    res.end();
    return;
  }
  
  console.log(`[Mock Daemon] ${req.method} ${pathname}`);
  
  if (pathname === '/api/config') {
    if (req.method === 'GET') {
      res.writeHead(200);
      res.end(JSON.stringify(mockConfig));
    } else if (req.method === 'POST') {
      let body = '';
      req.on('data', chunk => body += chunk.toString());
      req.on('end', () => {
        try {
          const newConfig = JSON.parse(body);
          Object.assign(mockConfig, newConfig);
          console.log('[Mock Daemon] Config updated:', newConfig);
          res.writeHead(200);
          res.end(JSON.stringify({ status: 'success', config: mockConfig }));
        } catch (e) {
          res.writeHead(400);
          res.end(JSON.stringify({ error: 'Invalid JSON' }));
        }
      });
    }
  } else if (pathname === '/api/pipelines') {
    // This is what the Next.js app is looking for
    res.writeHead(200);
    res.end(JSON.stringify([{
      id: 'default',
      name: 'Default EEG Pipeline',
      status: 'running',
      config: mockConfig
    }]));
  } else if (pathname === '/api/recordings') {
    // Return list of mock recordings in the format the page expects
    res.writeHead(200);
    res.end(JSON.stringify({
      files: [
        {
          name: 'eeg_recording_2025_01_20_14_30.csv',
          path: '/api/recordings/download/eeg_recording_2025_01_20_14_30.csv',
          size: 2457600, // 2.4 MB in bytes
          created: new Date(Date.now() - 3600000).toISOString() // 1 hour ago
        },
        {
          name: 'eeg_recording_2025_01_20_15_45.csv',
          path: '/api/recordings/download/eeg_recording_2025_01_20_15_45.csv', 
          size: 1887436, // 1.8 MB in bytes
          created: new Date(Date.now() - 1800000).toISOString() // 30 minutes ago
        },
        {
          name: 'eeg_recording_2025_01_20_16_12.csv',
          path: '/api/recordings/download/eeg_recording_2025_01_20_16_12.csv',
          size: 3145728, // 3.0 MB in bytes
          created: new Date(Date.now() - 900000).toISOString() // 15 minutes ago
        }
      ]
    }));
  } else if (pathname === '/api/recording/start') {
    console.log('[Mock Daemon] Recording start requested');
    res.writeHead(200);
    res.end(JSON.stringify({ status: 'started', message: 'Recording started' }));
    
    // Send recording_state event via SSE
    setTimeout(() => {
      broadcastSSEEvent({
        "recording_state": { event: 'started', message: 'Recording started' }
      });
    }, 100);
    
  } else if (pathname === '/api/recording/stop') {
    console.log('[Mock Daemon] Recording stop requested');
    res.writeHead(200);
    res.end(JSON.stringify({ status: 'stopped', message: 'Recording stopped' }));
    
    // Send recording_state event via SSE
    setTimeout(() => {
      broadcastSSEEvent({
        "recording_state": { event: 'stopped', message: 'Recording stopped' }
      });
    }, 100);
  } else if (pathname === '/api/status') {
    res.writeHead(200);
    res.end(JSON.stringify({ 
      status: 'running', 
      dataReceived: true, 
      wsStatus: 'connected',
      driverError: null 
    }));
  } else if (pathname === '/api/state') {
    // Pipeline state endpoint that the Next.js app is looking for
    res.writeHead(200);
    res.end(JSON.stringify({
      id: 'default',
      status: 'running',
      stages: [
        {
          id: 'data_acquisition',
          name: 'Data Acquisition',
          status: 'running',
          config: mockConfig
        },
        {
          id: 'signal_processing', 
          name: 'Signal Processing',
          status: 'running',
          config: {}
        },
        {
          id: 'output',
          name: 'Output',
          status: 'running', 
          config: {}
        }
      ],
      config: mockConfig,
      recording: false,
      dataReceived: true,
      wsStatus: 'connected',
      driverError: null,
      lastUpdate: Date.now()
    }));
  } else {
    res.writeHead(404);
    res.end(JSON.stringify({ error: 'Not found' }));
  }
}

// Server-Sent Events endpoint
function handleSSE(req, res) {
  const parsedUrl = parse(req.url, true);
  const { pathname } = parsedUrl;
  
  if (pathname === '/api/events') {
    console.log('[Mock Daemon] SSE connection established');
    res.writeHead(200, {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      'Connection': 'keep-alive',
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Headers': 'Cache-Control'
    });
    
    // Track this connection
    sseConnections.add(res);
    
    // Send initial connection event
    res.write(`data: ${JSON.stringify({ info: { message: 'Mock SSE connected' } })}\n\n`);
    
    // Send SourceReady event quickly to reduce initialization time
    setTimeout(() => {
      const sourceReadyEvent = {
        SourceReady: {
          meta: {
            meta_rev: Date.now(), // Use timestamp to ensure uniqueness
            source_type: 'eeg_source',
            channel_names: ['Fp1', 'Fp2', 'F3', 'F4', 'C3', 'C4', 'P3', 'P4'],
            sample_rate: 1000,
            board_driver: 'MockEeg'
          }
        }
      };
      res.write(`data: ${JSON.stringify(sourceReadyEvent)}\n\n`);
      console.log('[Mock Daemon] Sent SourceReady event with meta_rev:', sourceReadyEvent.SourceReady.meta.meta_rev);
    }, 500); // Reduced from 2000ms
    
    // Send pipeline state event after SourceReady
    setTimeout(() => {
      const pipelineStateEvent = {
        pipeline_state: {
          status: 'started',
          pipeline: 'default',
          config: mockConfig
        }
      };
      res.write(`data: ${JSON.stringify(pipelineStateEvent)}\n\n`);
      console.log('[Mock Daemon] Sent pipeline_state event');
    }, 800); // Reduced from 3000ms
    
    // Send periodic status updates
    const interval = setInterval(() => {
      const event = {
        info: {
          message: 'System running normally',
          pipelineStatus: 'started',
          dataReceived: true,
          wsStatus: 'connected'
        }
      };
      res.write(`data: ${JSON.stringify(event)}\n\n`);
    }, 10000);
    
    req.on('close', () => {
      console.log('[Mock Daemon] SSE connection closed');
      clearInterval(interval);
      sseConnections.delete(res);
    });
  }
}

// Create HTTP server
const server = createServer((req, res) => {
  const parsedUrl = parse(req.url, true);
  const { pathname } = parsedUrl;

  if (pathname.startsWith('/api/events')) {
    handleSSE(req, res);
  } else if (pathname.startsWith('/api')) {
    handleAPI(req, res);
  } else {
    res.writeHead(404);
    res.end('Not found');
  }
});

// WebSocket server for real-time EEG data
const wss = new WebSocket.Server({ server, path: '/ws/data' });

wss.on('connection', (ws) => {
  console.log('[Mock Daemon] WebSocket client connected');
  let dataInterval = null;
  let fftInterval = null;
  let isEegSubscribed = false;
  let isFftSubscribed = false;
  let currentMetaRev = null;
  
  // Handle subscription messages from the client
  ws.on('message', (message) => {
    try {
      const data = JSON.parse(message.toString());
      console.log('[Mock Daemon] Received WebSocket message:', data);
      
      if (data.type === 'subscribe' && data.topic === 'eeg_voltage') {
        console.log('[Mock Daemon] Client subscribed to eeg_voltage topic');
        isEegSubscribed = true;
        currentMetaRev = data.epoch;
      } else if (data.type === 'subscribe' && data.topic === 'brain_waves_fft') {
        console.log('[Mock Daemon] Client subscribed to brain_waves_fft topic');
        isFftSubscribed = true;
        currentMetaRev = data.epoch;
        
        // Start sending FFT data at ~10Hz (100ms intervals)
        if (fftInterval) clearInterval(fftInterval);
        console.log('[Mock Daemon] Starting FFT data stream...');
        fftInterval = setInterval(() => {
          if (ws.readyState === WebSocket.OPEN && isFftSubscribed) {
            const fftData = generateFFTData();
            
            // Send FFT data as JSON (not binary like EEG data)
            const message = JSON.stringify({
              topic: 'brain_waves_fft',
              meta_rev: currentMetaRev || Date.now(),
              packet_type: 'FftPacket',
              timestamp: fftData.timestamp,
              data: fftData
            });
            
            ws.send(message);
            console.log(`[Mock Daemon] Sent FFT data packet`);
          }
        }, 100); // 10Hz
      }
      
      // Start EEG data streaming if subscribed
      if (isEegSubscribed && !dataInterval) {
        // Start sending EEG data at ~100Hz (10ms intervals) 
        console.log('[Mock Daemon] Starting EEG data stream...');
        dataInterval = setInterval(() => {
          if (ws.readyState === WebSocket.OPEN && isEegSubscribed) {
            const sample = generateEEGSample();
            
            // Create binary data packet as expected by React app
            const header = {
              topic: 'eeg_voltage',
              meta_rev: currentMetaRev || Date.now(),
              packet_type: 'RawI32',
              timestamp: sample.timestamp,
              sample_count: sample.sample_count
            };
            
            // Convert header to JSON bytes
            const headerJson = JSON.stringify(header);
            const headerBytes = new TextEncoder().encode(headerJson);
            
            // Convert EEG data to Int32Array (multiply by 1000 to convert to microvolts)
            const eegData = new Int32Array(sample.channels.map(ch => Math.round(ch * 1000)));
            
            // Create the complete binary packet
            const totalLength = 4 + headerBytes.length + eegData.byteLength;
            const packet = new ArrayBuffer(totalLength);
            const view = new DataView(packet);
            
            // Write header length (big-endian)
            view.setUint32(0, headerBytes.length, false);
            
            // Write header JSON
            const headerArray = new Uint8Array(packet, 4, headerBytes.length);
            headerArray.set(headerBytes);
            
            // Write EEG data
            const dataArray = new Uint8Array(packet, 4 + headerBytes.length);
            dataArray.set(new Uint8Array(eegData.buffer));
            
            try {
              ws.send(packet);
              // Log every 100th message to avoid spam
              if (sample.sample_count % 100 === 0) {
                console.log('[Mock Daemon] Sent binary EEG sample:', sample.sample_count);
              }
            } catch (error) {
              console.error('[Mock Daemon] Error sending WebSocket data:', error);
              clearInterval(dataInterval);
              isEegSubscribed = false;
            }
          }
        }, 10);
      }
    } catch (error) {
      console.error('[Mock Daemon] Error parsing WebSocket message:', error);
    }
  });
  
  ws.on('close', () => {
    console.log('[Mock Daemon] WebSocket client disconnected');
    if (dataInterval) clearInterval(dataInterval);
    if (fftInterval) clearInterval(fftInterval);
    isEegSubscribed = false;
    isFftSubscribed = false;
  });
  
  ws.on('error', (error) => {
    console.error('[Mock Daemon] WebSocket error:', error);
    if (dataInterval) clearInterval(dataInterval);
    if (fftInterval) clearInterval(fftInterval);
    isEegSubscribed = false;
    isFftSubscribed = false;
  });
});

server.listen(9000, (err) => {
  if (err) throw err;
  console.log('🚀 Mock EEG Daemon ready on http://localhost:9000');
  console.log('📡 WebSocket endpoint: ws://localhost:9000/ws/data');
  console.log('📊 API endpoints: /api/config, /api/pipelines, /api/events, /api/status');
});
