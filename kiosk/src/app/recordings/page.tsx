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
    <div className="container mx-auto px-4 py-8">
      <div className="flex justify-between items-center mb-6">
        <h1 className="text-2xl font-bold">EEG Recordings</h1>
        <Link href="/" className="text-blue-500 hover:text-blue-700">
          Back to Dashboard
        </Link>
      </div>

      {loading ? (
        <div className="text-center py-10">Loading recordings...</div>
      ) : error ? (
        <div className="text-red-500 text-center py-10">{error}</div>
      ) : recordings.length === 0 ? (
        <div className="text-center py-10">No recordings found.</div>
      ) : (
        <div className="bg-white shadow-md rounded-lg overflow-y-auto max-h-[calc(100vh-10rem)]">
          <table className="min-w-full divide-y divide-gray-200 ">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                  File Name
                </th>
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                  Size
                </th>
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                  Created
                </th>
                <th className="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">
                  Actions
                </th>
              </tr>
            </thead>
            <tbody className="bg-white divide-y divide-gray-200">
              {recordings.map((recording, index) => (
                <tr key={index} className={index % 2 === 0 ? 'bg-white' : 'bg-gray-50'}>
                  <td className="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                    {recording.name}
                  </td>
                  <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                    {formatFileSize(recording.size)}
                  </td>
                  <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                    {formatDate(recording.created)}
                  </td>
                  <td className="px-6 py-4 whitespace-nowrap text-right text-sm font-medium">
                    <div className="flex items-center space-x-2">
                      <a
                        href={recording.path}
                        className="text-blue-600 hover:text-blue-900"
                        download
                      >
                        Download
                      </a>
                      <button
                        onClick={() => {
                          navigator.clipboard.writeText(
                            `${window.location.origin}${recording.path}`
                          );
                          alert('Link copied to clipboard!');
                        }}
                        className="text-gray-600 hover:text-gray-900"
                      >
                        <svg
                          xmlns="http://www.w3.org/2000/svg"
                          className="h-5 w-5"
                          viewBox="0 0 20 20"
                          fill="currentColor"
                        >
                          <path d="M8 2a1 1 0 000 2h2a1 1 0 110 2H8a1 1 0 01-1-1V3a1 1 0 011-1zm8 9a1 1 0 00-2 0v2a1 1 0 01-1 1H9a1 1 0 110-2h6a1 1 0 001-1v-2zM2 8a1 1 0 011-1h2a1 1 0 000-2H3a1 1 0 01-1 1v2zm0 4a1 1 0 011-1h2a1 1 0 000-2H3a1 1 0 01-1 1v2zm5 4a1 1 0 000 2h2a1 1 0 110 2H8a1 1 0 01-1-1v-3a1 1 0 011-1zm8-4a1 1 0 00-2 0v2a1 1 0 01-1 1h-2a1 1 0 110-2h2a1 1 0 001-1v-2z" />
                        </svg>
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}