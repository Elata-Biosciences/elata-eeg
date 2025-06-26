//! DSP filtering module for EEG signal processing
//! 
//! This module provides signal processing capabilities including
//! high-pass, low-pass, and powerline filtering for EEG data.

use std::f32::consts::PI;

// Simple biquad filter coefficients
#[derive(Clone, Debug)]
struct FilterCoefficients {
    b0: f32, b1: f32, b2: f32,  // numerator coefficients
    a1: f32, a2: f32,           // denominator coefficients (a0 is normalized to 1)
}

// Direct Form II Transposed biquad filter implementation
#[derive(Clone, Debug)]
struct DigitalFilter {
    coeffs: FilterCoefficients,
    z1: f32,  // delay line 1
    z2: f32,  // delay line 2
}

impl DigitalFilter {
    fn new(coeffs: FilterCoefficients) -> Self {
        Self {
            coeffs,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        // Clamp input to prevent extreme values
        let x = x.clamp(-8192.0, 8191.0);
        
        // Direct Form II Transposed implementation
        let y = self.coeffs.b0 * x + self.z1;
        self.z1 = self.coeffs.b1 * x - self.coeffs.a1 * y + self.z2;
        self.z2 = self.coeffs.b2 * x - self.coeffs.a2 * y;
        
        // Clamp output to prevent instability
        y.clamp(-8192.0, 8191.0)
    }
}

// Add struct definitions for each filter type
#[derive(Clone, Debug)]
struct NotchFilter(DigitalFilter);

#[derive(Clone, Debug)]
struct HighpassFilter(DigitalFilter);

#[derive(Clone, Debug)]
struct LowpassFilter(DigitalFilter);

impl NotchFilter {
    fn new(sample_rate: f32, notch_freq: f32) -> Self {
        let q_factor = 30.0;  // High Q for narrow notch
        let coeffs = Self::notch_coefficients(sample_rate, notch_freq, q_factor);
        NotchFilter(DigitalFilter::new(coeffs))
    }
    
    fn notch_coefficients(sample_rate: f32, freq: f32, q: f32) -> FilterCoefficients {
        let omega = 2.0 * PI * freq / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cos_omega = omega.cos();
        
        // Notch filter coefficients
        let b0 = 1.0;
        let b1 = -2.0 * cos_omega;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;
        
        // Normalize by a0
        FilterCoefficients {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
    
    fn process(&mut self, x: f32) -> f32 {
        self.0.process(x)
    }
}

impl HighpassFilter {
    fn new(sample_rate: f32, cutoff_freq: f32) -> Self {
        let q = 0.7071067811865476; // Butterworth Q factor (1/sqrt(2))
        let coeffs = Self::highpass_coefficients(sample_rate, cutoff_freq, q);
        Self(DigitalFilter::new(coeffs))
    }
    
    fn highpass_coefficients(sample_rate: f32, freq: f32, q: f32) -> FilterCoefficients {
        let omega = 2.0 * PI * freq / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cos_omega = omega.cos();
        
        // High-pass filter coefficients
        let b0 = (1.0 + cos_omega) / 2.0;
        let b1 = -(1.0 + cos_omega);
        let b2 = (1.0 + cos_omega) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;
        
        // Normalize by a0
        FilterCoefficients {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
    
    fn process(&mut self, x: f32) -> f32 {
        self.0.process(x)
    }
}

impl LowpassFilter {
    fn new(sample_rate: f32, cutoff_freq: f32) -> Self {
        let q = 0.7071067811865476; // Butterworth Q factor (1/sqrt(2))
        let coeffs = Self::lowpass_coefficients(sample_rate, cutoff_freq, q);
        Self(DigitalFilter::new(coeffs))
    }
    
    fn lowpass_coefficients(sample_rate: f32, freq: f32, q: f32) -> FilterCoefficients {
        let omega = 2.0 * PI * freq / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cos_omega = omega.cos();
        
        // Low-pass filter coefficients
        let b0 = (1.0 - cos_omega) / 2.0;
        let b1 = 1.0 - cos_omega;
        let b2 = (1.0 - cos_omega) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;
        
        // Normalize by a0
        FilterCoefficients {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
    
    fn process(&mut self, x: f32) -> f32 {
        self.0.process(x)
    }
}

/// Signal processor for applying various filters to EEG data
#[derive(Clone)]
pub struct SignalProcessor {
    _sample_rate: u32,
    num_channels: usize,
    powerline_notch_filters: Option<Vec<NotchFilter>>, // Holds 50Hz OR 60Hz filters, or None
    highpass_filters: Vec<HighpassFilter>,
    lowpass_filters: Vec<LowpassFilter>,
}

impl SignalProcessor {
    /// Create a new signal processor with the specified parameters
    pub fn new(
        sample_rate: u32,
        num_channels: usize,
        dsp_high_pass_cutoff: f32,
        dsp_low_pass_cutoff: f32,
        powerline_filter_hz: Option<u32>,
    ) -> Self {
        println!("[SignalProcessor::new] Initializing with sample_rate: {}, num_channels: {}, HP_cutoff: {}, LP_cutoff: {}, powerline_filter_hz: {:?}",
                 sample_rate, num_channels, dsp_high_pass_cutoff, dsp_low_pass_cutoff, powerline_filter_hz);
        // Add validation for sample rate
        assert!(sample_rate > 0, "Sample rate must be positive");
        assert!(sample_rate >= 200, "Sample rate should be at least 200Hz for proper filter operation");
        
        let sample_rate_f32 = sample_rate as f32;
        
        // Create powerline notch filters based on the configuration
        let powerline_notch_filters = match powerline_filter_hz {
            Some(freq) if freq == 50 || freq == 60 => {
                Some((0..num_channels)
                    .map(|_| NotchFilter::new(sample_rate_f32, freq as f32))
                    .collect())
            },
            _ => {
                println!("[SignalProcessor::new] Powerline filter is OFF or invalid value: {:?}", powerline_filter_hz);
                None // No powerline filter
            }
        };
        if powerline_notch_filters.is_some() {
            println!("[SignalProcessor::new] Powerline notch filters CREATED for {:?} Hz", powerline_filter_hz.unwrap());
        } else {
            println!("[SignalProcessor::new] Powerline notch filters are NONE");
        }
        
        Self {
            _sample_rate: sample_rate,
            num_channels,
            powerline_notch_filters,
            highpass_filters: (0..num_channels)
                .map(|_| HighpassFilter::new(sample_rate_f32, dsp_high_pass_cutoff))
                .collect(),
            lowpass_filters: (0..num_channels)
                .map(|_| LowpassFilter::new(sample_rate_f32, dsp_low_pass_cutoff))
                .collect(),
        }
    }

    /// Process a single sample for a specific channel
    pub fn process_sample(&mut self, channel: usize, sample: f32) -> f32 {
        // Add channel bounds check
        assert!(channel < self.num_channels, "Channel index out of bounds");
        
        let mut processed = sample;
        processed = self.highpass_filters[channel].process(processed);  // Move highpass first
        
        // Apply powerline notch filter if configured
        if let Some(notch_filters) = &mut self.powerline_notch_filters {
            if channel < notch_filters.len() {
                processed = notch_filters[channel].process(processed);
            }
        }
        
        processed = self.lowpass_filters[channel].process(processed);
        processed
    }
    
    /// Process a chunk of samples for a specific channel
    ///
    /// This is more efficient than processing samples individually when working with batches
    ///
    /// # Arguments
    /// * `channel` - The channel index
    /// * `samples` - The input samples to process
    /// * `output` - The buffer to store processed samples (must be pre-allocated with same length as samples)
    ///
    /// # Returns
    /// * `Result<(), &'static str>` - Ok if successful, Err with message if failed
    pub fn process_chunk(&mut self, channel: usize, samples: &[f32], output: &mut [f32]) -> Result<(), &'static str> {
        // Validate inputs
        if channel >= self.num_channels {
            return Err("Channel index out of bounds");
        }
        
        if output.len() < samples.len() {
            return Err("Output buffer too small");
        }
        
        // Process each sample through the filter chain
        for (i, &sample) in samples.iter().enumerate() {
            let mut processed = sample;
            processed = self.highpass_filters[channel].process(processed);
            
            // Apply powerline notch filter if configured
            if let Some(notch_filters) = &mut self.powerline_notch_filters {
                if channel < notch_filters.len() {
                    processed = notch_filters[channel].process(processed);
                }
            }
            
            processed = self.lowpass_filters[channel].process(processed);
            output[i] = processed;
        }
        
        Ok(())
    }

    /// Reset the signal processor with new parameters
    pub fn reset(&mut self, new_sample_rate: u32, new_num_channels: usize, dsp_high_pass_cutoff: f32, dsp_low_pass_cutoff: f32, powerline_filter_hz: Option<u32>) {
        println!("[SignalProcessor::reset] Resetting with sample_rate: {}, num_channels: {}, HP_cutoff: {}, LP_cutoff: {}, powerline_filter_hz: {:?}",
                 new_sample_rate, new_num_channels, dsp_high_pass_cutoff, dsp_low_pass_cutoff, powerline_filter_hz);
        self._sample_rate = new_sample_rate;
        self.num_channels = new_num_channels;
        // Recreate all filters
        let sample_rate = self._sample_rate as f32;
        
        // Create powerline notch filters based on the configuration
        self.powerline_notch_filters = match powerline_filter_hz {
            Some(freq) if freq == 50 || freq == 60 => {
                Some((0..self.num_channels)
                    .map(|_| NotchFilter::new(sample_rate, freq as f32))
                    .collect())
            },
            _ => {
                println!("[SignalProcessor::reset] Powerline filter is OFF or invalid value: {:?}", powerline_filter_hz);
                None // No powerline filter
            }
        };
        if self.powerline_notch_filters.is_some() {
            println!("[SignalProcessor::reset] Powerline notch filters RE-CREATED for {:?} Hz", powerline_filter_hz.unwrap());
        } else {
            println!("[SignalProcessor::reset] Powerline notch filters are NOW NONE");
        }
        
        self.highpass_filters = (0..self.num_channels)
            .map(|_| HighpassFilter::new(sample_rate, dsp_high_pass_cutoff))
            .collect();
        self.lowpass_filters = (0..self.num_channels)
            .map(|_| LowpassFilter::new(sample_rate, dsp_low_pass_cutoff))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_processor_creation() {
        let processor = SignalProcessor::new(500, 8, 1.0, 50.0, Some(60));
        assert_eq!(processor.sample_rate, 500);
        assert_eq!(processor.num_channels, 8);
    }

    #[test]
    fn test_process_chunk() {
        let mut processor = SignalProcessor::new(500, 2, 1.0, 50.0, Some(60));
        let input = vec![1.0, 2.0, 3.0, 4.0];
        let mut output = vec![0.0; 4];

        let result = processor.process_chunk(0, &input, &mut output);
        assert!(result.is_ok());
        assert_eq!(output.len(), 4);
    }

    #[test]
    fn test_channel_bounds_check() {
        let mut processor = SignalProcessor::new(500, 2, 1.0, 50.0, Some(60));
        let input = vec![1.0, 2.0];
        let mut output = vec![0.0; 2];

        let result = processor.process_chunk(5, &input, &mut output);
        assert!(result.is_err());
    }

    #[test]
    fn test_process_sample() {
        let mut processor = SignalProcessor::new(500, 1, 1.0, 50.0, Some(60));
        let result = processor.process_sample(0, 1.0);
        // Should return a filtered value, not exactly 1.0
        assert!(result.is_finite());
    }

    #[test]
    fn test_powerline_filter_disabled() {
        let processor = SignalProcessor::new(500, 2, 1.0, 50.0, None);
        assert!(processor.powerline_notch_filters.is_none());
    }

    #[test]
    fn test_powerline_filter_enabled() {
        let processor = SignalProcessor::new(500, 2, 1.0, 50.0, Some(50));
        assert!(processor.powerline_notch_filters.is_some());
        assert_eq!(processor.powerline_notch_filters.as_ref().unwrap().len(), 2);
    }
}