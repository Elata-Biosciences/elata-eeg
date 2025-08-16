# Kiosk WebSocket Refactor Plan

This document outlines the necessary steps to adapt the Kiosk UI to the new centralized WebSocket broker architecture.

## 1. Overview

The backend WebSocket handling has been refactored. Instead of connecting to a dynamic port per data stream, the UI will now connect to a single, static endpoint and subscribe to data topics.

-   **Static WebSocket URL:** `ws://localhost:9001/ws/data`
-   **Subscription Message:** `{"subscribe": "eeg_voltage"}`
-   **Data Format:** JSON-encoded `RtPacket`

## 2. Files to Modify

-   `kiosk/src/components/EegDataHandler.tsx`: This file contains the primary WebSocket connection and data handling logic.

## 3. Implementation Steps

### Step 1: Update WebSocket URL Generation in `EegDataHandler.tsx`

The current implementation dynamically generates the WebSocket URL. This needs to be replaced with the static URL.

**File:** [`kiosk/src/components/EegDataHandler.tsx`](kiosk/src/components/EegDataHandler.tsx)

**Change:**

-   Locate the `useMemo` hook for `wsUrl`.
-   Replace the dynamic URL generation with the static URL.

**Current Code:**

```typescript
const wsUrl = useMemo(() => {
    const { status, config } = pipelineState;
    if (status !== 'started' || !config) {
      return null;
    }
    const wsProtocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss' : 'ws';
    const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
    const wsSinkStage = config.stages.find(s => s.type === 'websocket_sink');
    const wsAddr = wsSinkStage?.params?.addr || '0.0.0.0:9001';
    return `${wsProtocol}://${wsHost}:${wsAddr.split(':')[1]}`;
}, [pipelineState.status, pipelineState.config]);
```

**New Code:**

```typescript
const wsUrl = useMemo(() => {
    if (!enabled) {
        return null;
    }
    const wsProtocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss' : 'ws';
    const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
    // The port is now static per the new architecture.
    const wsPort = 9001; 
    return `${wsProtocol}://${wsHost}:${wsPort}/ws/data`;
}, [enabled]);
```

### Step 2: Send Subscription Message on Connection

After the WebSocket connection is established, we need to send the subscription message.

**File:** [`kiosk/src/components/EegDataHandler.tsx`](kiosk/src/components/EegDataHandler.tsx)

**Change:**

-   In the `ws.onopen` handler, send the JSON subscription message.

**Current Code:**

```typescript
ws.onopen = () => {
    if (!isMounted) return;
    setWsConnectionStatus('Connected');
    reconnectAttemptsRef.current = 0;
    const now = performance.now();
    setDebugInfo(prev => ({
      ...prev,
      connectionAttempts: prev.connectionAttempts + 1,
      lastConnectionTime: now,
    }));
    console.log(`[EegDataHandler] WebSocket connection established.`);
};
```

**New Code:**

```typescript
ws.onopen = () => {
    if (!isMounted) return;
    setWsConnectionStatus('Connected');
    reconnectAttemptsRef.current = 0;
    const now = performance.now();
    setDebugInfo(prev => ({
        ...prev,
        connectionAttempts: prev.connectionAttempts + 1,
        lastConnectionTime: now,
    }));
    console.log(`[EegDataHandler] WebSocket connection established.`);

    // Subscribe to the EEG data topic
    const subscriptionMessage = {
        subscribe: "eeg_voltage"
    };
    ws.send(JSON.stringify(subscriptionMessage));
    console.log('[EegDataHandler] Sent subscription request for "eeg_voltage"');
};
```

### Step 3: Update Message Handling Logic

The `onmessage` handler needs to be updated to parse JSON `RtPacket` data instead of the old binary format.

**File:** [`kiosk/src/components/EegDataHandler.tsx`](kiosk/src/components/EegDataHandler.tsx)

**Change:**

-   Replace the binary processing logic inside `ws.onmessage` with JSON parsing.
-   The new logic should expect a JSON object that is an `RtPacket`. The `data` field of the `RtPacket` will contain the EEG data.
-   The `handleEegPayload` function will need to be adapted or replaced to handle the new data structure.

**Current `onmessage` structure:**

```typescript
ws.onmessage = (event: MessageEvent) => {
    // ...
    if (event.data instanceof ArrayBuffer) {
        // ... binary processing logic ...
        const version = buffer[0];
        const topic = buffer[1];
        const payload = buffer.slice(2);

        switch (topic) {
            case 0: // FilteredEeg
                handleEegPayload(payload);
                break;
            // ... other cases
        }
    } else {
        // ...
    }
};
```

**New `onmessage` structure:**

```typescript
ws.onmessage = (event: MessageEvent) => {
    if (!isMounted) return;

    try {
        onDataUpdateRef.current?.(true);
        if (dataReceivedTimeoutRef.current) clearTimeout(dataReceivedTimeoutRef.current);
        dataReceivedTimeoutRef.current = setTimeout(() => onDataUpdateRef.current?.(false), 1000);

        const data = JSON.parse(event.data);

        // Check if it's an RtPacket and has the expected data
        if (data.topic === 'eeg_voltage' && data.payload.data) {
            // The actual data is in `data.payload.data`
            // This will likely be an array of numbers (samples)
            // The structure of the payload needs to be handled.
            // For now, let's assume it's an array of samples for each channel.
            // The `handleSamples` function expects a specific format.
            // We need to adapt the incoming data to what `handleSamples` expects.
            
            // This part requires knowing the exact structure of `data.payload.data`
            // For now, we will log it.
            console.log("Received RtPacket:", data.payload);

            // TODO: Adapt `data.payload` to the format expected by `onSamplesRef.current`.
            // The `onSamples` prop expects an array of objects, where each object has `values` and `timestamps`.
            // The new payload structure needs to be mapped to this.
        } else if (data.topic === 'fft') {
            onFftDataRef.current?.(data.payload);
        }

    } catch (error) {
        console.error("[EegDataHandler] Error in onmessage handler:", error);
        onErrorRef.current?.(`Error processing data: ${error}`);
    }
};
```

**Note:** The `handleEegPayload` function and other related binary parsing functions (`handleFftPayload`, `handleLogPayload`) will become obsolete and can be removed. The logic to transform the new `RtPacket` payload into the format expected by the `onSamples` callback will need to be implemented in their place. This may require further information about the exact structure of the `RtPacket` payload.