# Web UI Fix and Modernization Plan

This document outlines the plan to fix the data visualization issue in the Kiosk UI and modernize its architecture to align with the new pipeline system.

## 1. The Core Problem: Race Condition in `EegDataHandler`

The primary issue is a race condition preventing the WebSocket from connecting to the data service.

- **`EegDataContext`** initializes with `config: null`.
- **`useEegDataHandler`** is called with this `null` config.
- The handler's main `useEffect` runs once (`[]` dependency), sees `config` is null, and enters a "config not ready" retry loop.
- **`EegMonitor`** receives the `SourceReady` event and updates the `config`.
- The `useEegDataHandler` hook never re-runs its connection logic with the new `config`, so the WebSocket connection is never properly established.

## 2. The Fix: Make `EegDataHandler` Reactive to Configuration

The solution is to make the `useEegDataHandler` hook react to changes in its `config` prop.

### Step 1: Modify `useEegDataHandler` Effect

In `kiosk/src/components/EegDataHandler.tsx`:

1.  Change the main `useEffect`'s dependency array from `[]` to `[configKey]`. The `configKey` is a memoized value that changes only when relevant config properties (like `sample_rate` and `channels`) change. This already exists in the code.
2.  This ensures that `connectWebSocket()` is re-invoked whenever the configuration is ready or changes significantly.

```typescript
// In kiosk/src/components/EegDataHandler.tsx

// ...

  // The key already exists, we just need to use it.
  const configKey = useMemo(() => {
    if (!config) return null;
    const channelKey = config.channels?.slice().sort().join(',') || '';
    return `${config.sample_rate}-${channelKey}`;
  }, [config]);

  useEffect(() => {
    console.log(`[EegDataHandler] Effect running to establish WebSocket connection.`);
    let isMounted = true;

    // ... (rest of the effect)

    connectWebSocket();

    return () => {
      // ... (cleanup logic)
    };
  }, [configKey]); // <--- CHANGE THIS LINE
```

This single change should resolve the immediate problem of the graph not appearing.

## 3. Architectural Improvement: Simplify the Data Flow

The current architecture has several layers of contexts and prop drilling (`EegMonitor` -> `EegDataContext` -> `EegDataHandler`). We can simplify this.

### Step 2: Consolidate Logic in `EegDataContext`

1.  **Move `useEventStream`:** Move the `useEventStream` hook from `EegMonitor.tsx` directly into `EegDataContext.tsx`. The context that manages data should be responsible for getting its own configuration.
2.  **Handle `SourceReady` in Context:** The `useEffect` that listens for the `SourceReady` event should also be moved from `EegMonitor.tsx` into `EegDataContext.tsx`.

This change centralizes all data-related logic in one place, making the system easier to understand and maintain. `EegMonitor` will become a simpler component focused only on rendering the UI based on the context's state.

## 4. Plan Summary

1.  **Fix the Bug:** Update the dependency array in `useEegDataHandler` to be `[configKey]`.
2.  **Refactor for Clarity:** Move the event stream and `SourceReady` logic from `EegMonitor` into `EegDataContext`.

This plan will not only fix the bug but also improve the overall architecture of the frontend application, making it more robust and maintainable for the future.