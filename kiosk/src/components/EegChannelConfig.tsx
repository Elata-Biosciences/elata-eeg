'use client';

import { useState, useEffect, useRef } from 'react';
import { useEegConfig } from './EegConfig';

interface ChannelConfigProps {
  className?: string;
}

export default function EegChannelConfig({ className = '' }: ChannelConfigProps) {
  const { config, status } = useEegConfig();
  const [selectedChannels, setSelectedChannels] = useState<number[]>([]);
  const [maxChannels, setMaxChannels] = useState<number>(8);
  const [isUpdating, setIsUpdating] = useState(false);
  const [message, setMessage] = useState<{ text: string; type: 'success' | 'error' | 'info' | null }>({
    text: '',
    type: null,
  });
  const wsRef = useRef<WebSocket | null>(null);
  const isProduction = process.env.NODE_ENV === 'production';

  // Initialize selected channels from config
  useEffect(() => {
    if (config && config.channels) {
      setSelectedChannels(config.channels);
      // Set max channels to at least the current number of channels
      setMaxChannels(Math.max(8, Math.max(...config.channels) + 1));
    }
  }, [config]);

  // Connect to the config WebSocket
  useEffect(() => {
    const ws = new WebSocket('ws://localhost:8080/config');
    wsRef.current = ws;

    ws.onopen = () => {
      if (!isProduction) console.log('Channel Config WebSocket: Connection opened');
    };

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        
        // Check if this is a response message
        if (data.status) {
          if (data.status === 'ok') {
            setMessage({ text: data.message, type: 'success' });
            setIsUpdating(false);
          } else if (data.status === 'error') {
            setMessage({ text: data.message, type: 'error' });
            setIsUpdating(false);
          }
        } 
        // If it's a config update, the EegConfig provider will handle it
      } catch (error) {
        console.error('Error parsing config data:', error);
      }
    };

    ws.onclose = () => {
      if (!isProduction) console.log('Channel Config WebSocket: Connection closed');
      wsRef.current = null;
    };

    ws.onerror = (error) => {
      console.error('Channel Config WebSocket error:', error);
      setMessage({ 
        text: 'Connection error. Please try again later.', 
        type: 'error' 
      });
    };

    return () => {
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [isProduction]);

  // Handle channel selection change
  const handleChannelChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = parseInt(e.target.value, 10);
    
    if (e.target.checked) {
      // Add channel
      setSelectedChannels(prev => [...prev, value].sort((a, b) => a - b));
    } else {
      // Remove channel
      setSelectedChannels(prev => prev.filter(ch => ch !== value));
    }
  };

  // Handle number of channels change
  const handleMaxChannelsChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const value = parseInt(e.target.value, 10);
    setMaxChannels(value);
    
    // Ensure selected channels are within the new max
    setSelectedChannels(prev => prev.filter(ch => ch < value));
  };

  // Apply channel configuration
  const applyChannelConfig = () => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) {
      setMessage({ 
        text: 'WebSocket connection not available. Please try again later.', 
        type: 'error' 
      });
      return;
    }

    if (selectedChannels.length === 0) {
      setMessage({ 
        text: 'Please select at least one channel.', 
        type: 'error' 
      });
      return;
    }

    setIsUpdating(true);
    setMessage({ text: 'Updating configuration...', type: 'info' });

    const configMessage = {
      channels: selectedChannels
    };

    wsRef.current.send(JSON.stringify(configMessage));
  };

  // Generate checkboxes for channel selection
  const renderChannelCheckboxes = () => {
    const checkboxes = [];
    for (let i = 0; i < maxChannels; i++) {
      checkboxes.push(
        <div key={i} className="flex items-center mr-4 mb-2">
          <input
            type="checkbox"
            id={`channel-${i}`}
            value={i}
            checked={selectedChannels.includes(i)}
            onChange={handleChannelChange}
            className="mr-2"
            disabled={isUpdating}
          />
          <label htmlFor={`channel-${i}`} className="text-sm">
            Channel {i}
          </label>
        </div>
      );
    }
    return checkboxes;
  };

  return (
    <div className={`p-4 bg-gray-900 text-white rounded-lg mb-4 ${className}`}>
      <h2 className="text-xl font-bold mb-2">EEG Channel Configuration</h2>
      <div className="mb-4">
        <label className="block mb-2">Number of Available Channels:</label>
        <select 
          value={maxChannels} 
          onChange={handleMaxChannelsChange}
          className="bg-gray-800 text-white p-2 rounded w-full mb-4"
          disabled={isUpdating}
        >
          <option value="2">2 Channels</option>
          <option value="4">4 Channels</option>
          <option value="8">8 Channels</option>
          <option value="16">16 Channels</option>
          <option value="32">32 Channels</option>
        </select>
        
        <div className="mb-4">
          <label className="block mb-2">Select Active Channels:</label>
          <div className="flex flex-wrap">
            {renderChannelCheckboxes()}
          </div>
        </div>
        
        <div className="flex items-center justify-between">
          <button
            onClick={applyChannelConfig}
            disabled={isUpdating || selectedChannels.length === 0}
            className={`px-4 py-2 rounded ${
              isUpdating || selectedChannels.length === 0
                ? 'bg-gray-600 cursor-not-allowed'
                : 'bg-blue-600 hover:bg-blue-700'
            }`}
          >
            {isUpdating ? 'Updating...' : 'Apply Channel Configuration'}
          </button>
          
          <div className="ml-4">
            {message.text && (
              <span
                className={`text-sm ${
                  message.type === 'success'
                    ? 'text-green-400'
                    : message.type === 'error'
                    ? 'text-red-400'
                    : 'text-blue-400'
                }`}
              >
                {message.text}
              </span>
            )}
          </div>
        </div>
      </div>
      
      <div className="mt-4 p-3 bg-gray-800 rounded">
        <h3 className="font-semibold mb-2">Current Configuration:</h3>
        <div className="text-sm">
          <div>Status: {status}</div>
          {config && (
            <div>
              Active Channels: {config.channels.join(', ')}
            </div>
          )}
        </div>
      </div>
      
      <div className="mt-4 text-xs text-gray-400">
        <p>Note: Configuration changes are blocked while recording is in progress.</p>
      </div>
    </div>
  );
}