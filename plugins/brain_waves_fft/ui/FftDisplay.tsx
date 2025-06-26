import React, { useRef, useState, useEffect } from 'react';
import { FftRenderer } from './FftRenderer';

interface FftData {
  psd_packets: { channel: number; psd: number[] }[];
  fft_config: {
    fft_size: number;
    sample_rate: number;
    window_function: string;
  };
}

interface FftDisplayProps {
  isActive: boolean;
  data: FftData | null;
}

const FftDisplay: React.FC<FftDisplayProps> = ({ isActive, data }) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [dimensions, setDimensions] = useState({ width: 0, height: 0 });

  useEffect(() => {
    if (containerRef.current) {
      const resizeObserver = new ResizeObserver(entries => {
        if (entries[0]) {
          const { width, height } = entries[0].contentRect;
          setDimensions({ width, height });
        }
      });
      resizeObserver.observe(containerRef.current);
      return () => resizeObserver.disconnect();
    }
  }, []);

  if (!isActive) {
    return <div style={{ display: 'none' }} />;
  }
  
  if (!data) {
    return <div>No FFT data available</div>;
  }

  return (
    <div ref={containerRef} style={{ width: '100%', height: '100%' }}>
      <FftRenderer
        data={data}
        isActive={isActive}
        containerWidth={dimensions.width}
        containerHeight={dimensions.height}
      />
    </div>
  );
};

export default FftDisplay;