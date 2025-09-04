'use client'; // Required for useState

import EegMonitor from '@/components/EegMonitor';
import { PipelineProvider, usePipeline } from '@/context/PipelineContext';
import { EventStreamProvider } from '@/context/EventStreamContext';
import { EegDataProvider, useEegStatus } from '@/context/EegDataContext';

// This wrapper component now determines readiness based on the new isReady flag
function EegMonitorWrapper() {
  const { pipelineStatus } = usePipeline();
  const { isReady, dataStatus } = useEegStatus();

  if (!isReady) {
    return (
      <div className="flex items-center justify-center h-screen relative overflow-hidden" style={{
        background: 'radial-gradient(ellipse at center, var(--color-off-cream) 0%, var(--color-cream1) 70%, var(--color-cream2) 100%)'
      }}>
        {/* Animated background elements */}
        <div className="absolute inset-0">
          <div className="absolute top-1/4 left-1/4 w-2 h-2 rounded-full" style={{
            backgroundColor: 'var(--color-elata-green)',
            opacity: 0.1,
            animation: 'float 6s ease-in-out infinite'
          }}></div>
          <div className="absolute top-3/4 right-1/4 w-1 h-1 rounded-full" style={{
            backgroundColor: 'var(--color-elata-green)',
            opacity: 0.15,
            animation: 'float 4s ease-in-out infinite reverse'
          }}></div>
          <div className="absolute bottom-1/3 left-1/3 w-1.5 h-1.5 rounded-full" style={{
            backgroundColor: 'var(--color-elata-green)',
            opacity: 0.08,
            animation: 'float 8s ease-in-out infinite'
          }}></div>
        </div>

        <div className="text-center p-20 max-w-3xl mx-4 relative z-10" style={{
          backgroundColor: 'rgba(255, 255, 255, 0.98)',
          backdropFilter: 'blur(30px)',
          boxShadow: '0 40px 80px rgba(0,0,0,0.15), 0 20px 40px rgba(0,0,0,0.1), inset 0 1px 0 rgba(255,255,255,0.8)',
          border: '1px solid rgba(255,255,255,0.3)',
          borderRadius: '40px'
        }}>
          {/* Logo with advanced animation */}
          <div className="mb-12 relative">
            <div className="relative">
              <div className="relative w-24 h-24 mx-auto mb-10">
                <div className="absolute inset-0 rounded-full" style={{
                  background: 'linear-gradient(135deg, var(--color-elata-green), rgba(96, 114, 116, 0.8))',
                  animation: 'spin 8s linear infinite',
                  opacity: 0.1
                }}></div>
                <img 
                  src="/logo.png" 
                  alt="Elata" 
                  className="relative z-10 w-20 h-20 mx-auto"
                  style={{
                    filter: 'drop-shadow(0 16px 32px rgba(0,0,0,0.2))',
                    objectFit: 'contain',
                    animation: 'logoFloat 3s ease-in-out infinite',
                    transform: 'translateX(2px) translateY(2px)'
                  }}
                />
              </div>
              
              {/* Orbital progress indicators - perfectly centered around logo */}
              <div className="absolute inset-0 flex items-center justify-center">
                <div className="relative w-32 h-32">
                  <div className="absolute inset-0 rounded-full border-2 border-transparent" style={{
                    borderTopColor: 'var(--color-elata-green)',
                    borderRightColor: 'var(--color-elata-green)',
                    animation: 'spin 3s linear infinite',
                    opacity: 0.6
                  }}></div>
                  <div className="absolute inset-3 rounded-full border border-transparent" style={{
                    borderTopColor: 'var(--color-elata-green)',
                    animation: 'spin 2s linear infinite reverse',
                    opacity: 0.4
                  }}></div>
                  <div className="absolute inset-6 rounded-full border border-transparent" style={{
                    borderTopColor: 'var(--color-elata-green)',
                    borderLeftColor: 'var(--color-elata-green)',
                    animation: 'spin 4s linear infinite',
                    opacity: 0.3
                  }}></div>
                </div>
              </div>
            </div>
          </div>
          
          <h1 className="text-4xl font-bold mb-6" style={{
            color: 'var(--color-off-black)',
            fontFamily: 'var(--font-family-ui)',
            animation: 'fadeInUp 1s ease-out 0.3s both',
            letterSpacing: '-0.02em'
          }}>Initializing EEG Monitor</h1>
          
          <div className="space-y-4" style={{
            animation: 'fadeInUp 1s ease-out 0.6s both'
          }}>
            <div className="mb-4">
              {/* <p className="text-xl font-semibold" style={{
                color: 'var(--color-elata-green)',
                fontFamily: 'var(--font-family-ui)'
              }}>
                {pipelineStatus === 'starting' ? '⚡ Starting Pipeline' : (dataStatus.wsStatus ? `🔗 ${dataStatus.wsStatus}` : '🔄 Connecting to System')}
              </p> */}
            </div>
            
            {/* Progress dots */}
            <div className="flex items-center justify-center space-x-3 mt-6">
              <div className="flex space-x-1">
                <div className="w-2 h-2 rounded-full" style={{
                  backgroundColor: 'var(--color-elata-green)',
                  animation: 'progressDot 1.5s ease-in-out infinite'
                }}></div>
                <div className="w-2 h-2 rounded-full" style={{
                  backgroundColor: 'var(--color-elata-green)',
                  animation: 'progressDot 1.5s ease-in-out infinite 0.2s'
                }}></div>
                <div className="w-2 h-2 rounded-full" style={{
                  backgroundColor: 'var(--color-elata-green)',
                  animation: 'progressDot 1.5s ease-in-out infinite 0.4s'
                }}></div>
              </div>
            </div>
            
            <div className="flex items-center justify-center space-x-6 text-sm mt-8" style={{
              color: 'var(--color-gray3)',
              fontFamily: 'var(--font-family-ui)',
              opacity: 0.8
            }}>
              <span className="flex items-center">
                <div className="w-1.5 h-1.5 rounded-full mr-2" style={{ backgroundColor: 'var(--color-elata-green)' }}></div>
                Neural Interface
              </span>
              <span className="flex items-center">
                <div className="w-1.5 h-1.5 rounded-full mr-2" style={{ backgroundColor: 'var(--color-elata-green)' }}></div>
                Signal Processing
              </span>
              <span className="flex items-center">
                <div className="w-1.5 h-1.5 rounded-full mr-2" style={{ backgroundColor: 'var(--color-elata-green)' }}></div>
                Real-time Analysis
              </span>
            </div>
          </div>
        </div>
      </div>
    );
  }

  return <EegMonitor />;
}

export default function Home() {
  return (
    <main className="flex flex-col h-screen" style={{
      backgroundColor: 'var(--color-off-cream)',
      color: 'var(--color-off-black)'
    }}>
      <EventStreamProvider>
        <PipelineProvider>
          <EegDataProvider>
            <EegMonitorWrapper />
          </EegDataProvider>
        </PipelineProvider>
      </EventStreamProvider>
    </main>
  );
}
