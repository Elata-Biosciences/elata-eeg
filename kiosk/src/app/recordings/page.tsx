'use client';

import { useState, useEffect } from 'react';
import Link from 'next/link';

interface Recording {
  name: string;
  path: string;
  size: number;
  created: string;
}

export default function RecordingsPage() {
  const [recordings, setRecordings] = useState<Recording[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    async function fetchRecordings() {
      try {
        const response = await fetch('/api/recordings');
        if (!response.ok) {
          throw new Error('Failed to fetch recordings');
        }
        const data = await response.json();
        setRecordings(data.files);
      } catch (err) {
        setError('Error loading recordings. Please try again later.');
        console.error(err);
      } finally {
        setLoading(false);
      }
    }

    fetchRecordings();
  }, []);

  // Function to format file size
  const formatFileSize = (bytes: number) => {
    if (bytes < 1024) return bytes + ' B';
    else if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(2) + ' KB';
    else if (bytes < 1024 * 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(2) + ' MB';
    else return (bytes / (1024 * 1024 * 1024)).toFixed(2) + ' GB';
  };

  // Function to format date
  const formatDate = (dateString: string) => {
    const date = new Date(dateString);
    return date.toLocaleString();
  };

  return (
    <div className="min-h-screen" style={{
      background: 'linear-gradient(to bottom right, var(--color-off-cream), var(--color-cream1))'
    }}>
      <div className="container mx-auto px-4 py-6 max-w-4xl">
        {/* Header */}
        <div className="flex flex-col sm:flex-row sm:justify-between sm:items-center mb-8 space-y-4 sm:space-y-0">
          <div className="flex items-center">
            <img 
              src="/logo.png" 
              alt="Elata" 
              className="w-10 h-10 mr-4"
              style={{
                filter: 'drop-shadow(0 2px 4px rgba(0,0,0,0.1))',
                objectFit: 'contain'
              }}
            />
            <div>
              <h1 className="text-2xl font-bold" style={{
                fontFamily: 'var(--font-family-ui)',
                color: 'var(--color-off-black)'
              }}>EEG Recordings</h1>
              <p className="text-sm" style={{
                color: 'var(--color-gray3)',
                fontFamily: 'var(--font-family-system)'
              }}>Manage your EEG recording files</p>
            </div>
          </div>
          
          <Link 
            href="/" 
            className="flex items-center px-6 py-3 font-medium text-sm transition-all duration-200 shadow-sm hover:shadow-md"
            style={{
              backgroundColor: 'var(--color-off-black)',
              color: 'var(--color-white)',
              fontFamily: 'var(--font-family-ui)',
              textDecoration: 'none',
              borderRadius: '0' // Square button with no radius
            }}
          >
            <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10 19l-7-7m0 0l7-7m-7 7h18" />
            </svg>
            Back to Dashboard
          </Link>
        </div>

        {loading ? (
          <div className="text-center py-16">
            <div className="w-8 h-8 mx-auto mb-4 rounded-full flex items-center justify-center" style={{
              backgroundColor: 'color-mix(in srgb, var(--color-elata-green) 20%, transparent)'
            }}>
              <div className="w-4 h-4 rounded-full animate-spin" style={{
                border: '2px solid var(--color-elata-green)',
                borderTopColor: 'transparent'
              }}></div>
            </div>
            <p style={{
              color: 'var(--color-gray3)',
              fontFamily: 'var(--font-family-system)'
            }}>Loading recordings...</p>
          </div>
        ) : error ? (
          <div className="text-center py-16 px-4">
            <div className="card-elata max-w-md mx-auto p-6" style={{
              backgroundColor: 'color-mix(in srgb, var(--color-accent-red) 10%, var(--color-white))',
              border: '1px solid color-mix(in srgb, var(--color-accent-red) 30%, transparent)'
            }}>
              <p style={{
                color: 'var(--color-accent-red)',
                fontFamily: 'var(--font-family-system)'
              }}>{error}</p>
            </div>
          </div>
        ) : recordings.length === 0 ? (
          <div className="text-center py-16">
            <div className="card-elata max-w-md mx-auto p-8">
              <svg className="w-16 h-16 mx-auto mb-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" style={{ color: 'var(--color-gray3)' }}>
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1} d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
              </svg>
              <p style={{
                color: 'var(--color-gray3)',
                fontFamily: 'var(--font-family-system)'
              }}>No recordings found.</p>
            </div>
          </div>
        ) : (
          <div className="space-y-4">
            {recordings.map((recording, index) => (
              <div key={index} className="card-elata p-6 hover:shadow-lg transition-all duration-300">
                <div className="flex flex-col sm:flex-row sm:justify-between sm:items-start space-y-3 sm:space-y-0">
                  <div className="flex-1">
                    <h3 className="font-bold text-lg mb-2" style={{
                      fontFamily: 'var(--font-family-display)',
                      color: 'var(--color-off-black)'
                    }}>{recording.name}</h3>
                    <div className="flex flex-wrap items-center gap-4 text-sm" style={{
                      color: 'var(--color-gray3)',
                      fontFamily: 'var(--font-family-system)'
                    }}>
                      <span>📁 {formatFileSize(recording.size)}</span>
                      <span>📅 {formatDate(recording.created)}</span>
                    </div>
                  </div>
                  
                  <div className="flex items-center">
                    <button
                      onClick={() => {
                        // Try to open file in system (if supported)
                        window.open(recording.path, '_blank');
                      }}
                      className="flex items-center px-6 py-3 font-medium text-sm transition-all duration-200 shadow-sm hover:shadow-md transform hover:scale-105"
                      style={{
                        backgroundColor: 'var(--color-elata-green)',
                        color: 'var(--color-white)',
                        fontFamily: 'var(--font-family-ui)',
                        border: 'none',
                        borderRadius: '9999px' // Max radius
                      }}
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
                      </svg>
                      Open File
                    </button>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}