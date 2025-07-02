'use client';

import { useRef, useCallback, useMemo } from 'react';

/**
 * A custom hook to manage a buffer for streaming data.
 * It's designed to decouple data reception from data processing/rendering.
 *
 * @template T The type of data chunks being buffered.
 * @returns A memoized object with methods to interact with the buffer.
 */
export function useDataBuffer<T>() {
  const buffer = useRef<T[]>([]);

  const addData = useCallback((newData: T[]) => {
    buffer.current.push(...newData);
  }, []);

  const getAndClearData = useCallback(() => {
    if (buffer.current.length === 0) {
      return [];
    }
    const bufferedData = buffer.current;
    buffer.current = [];
    return bufferedData;
  }, []);

  const clear = useCallback(() => {
    buffer.current = [];
  }, []);

  // Memoize the returned object to ensure its identity is stable across re-renders.
  // This is crucial for preventing unnecessary effect runs in consuming components.
  return useMemo(() => ({
    addData,
    getAndClearData,
    clear,
  }), [addData, getAndClearData, clear]);
}