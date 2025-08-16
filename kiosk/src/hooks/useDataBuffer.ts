import {useRef, useCallback, useMemo} from 'react';

export function useDataBuffer<T>(maxSize: number) {
  const buffer = useRef<T[]>([]);

  const addData = useCallback(
    (newData: T[]) => {
      buffer.current.push(...newData);
      if (buffer.current.length > maxSize) {
        buffer.current.splice(0, buffer.current.length - maxSize);
      }
    },
    [maxSize]
  );

  const getAndClearData = useCallback(() => {
    const data = buffer.current;
    buffer.current = [];
    return data;
  }, []);

  const clear = useCallback(() => {
    buffer.current = [];
  }, []);

  return useMemo(() => ({addData, getAndClearData, clear}), [addData, getAndClearData, clear]);
}