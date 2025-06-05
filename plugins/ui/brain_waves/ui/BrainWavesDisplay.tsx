import React from 'react';
import { AppletFftRenderer } from './AppletFftRenderer';

interface BrainWavesDisplayProps {
    containerWidth?: number;
    containerHeight?: number;
    eegConfig: {
        sample_rate: number;
        channels: number[] | { channel_number: number;
                                is_active: boolean
                              }[];
        // Add other properties of eegConfig if known and used
    };
}

const BrainWavesDisplay: React.FC<BrainWavesDisplayProps> = ({
    containerWidth = 600, // Default width
    containerHeight = 400, // Default height
    eegConfig
}) => {
    return (
        <div style={{ width: containerWidth, height: containerHeight, display: 'flex', flexDirection: 'column' }}>
            <h3>Brain Waves FFT Display</h3>
            
            {eegConfig && eegConfig.channels && eegConfig.sample_rate ? (
                <AppletFftRenderer
                    config={eegConfig} // Pass the EEG config
                    containerWidth={containerWidth}
                    containerHeight={containerHeight - 50} // Minimal adjustment for title
                />
            ) : (
                <p>EEG Config not available or incomplete. Cannot render FFT.</p>
            )}
        </div>
    );
};

export default BrainWavesDisplay;