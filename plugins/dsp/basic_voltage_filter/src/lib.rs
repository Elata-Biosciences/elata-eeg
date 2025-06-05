use biquad::{Biquad, DirectForm2Transposed, Coefficients, Type, Q_BUTTERWORTH_F32, ToHertz};

// Update the FilterCoefficients to use biquad's Coefficients
#[derive(Clone, Debug)]
struct FilterCoefficients {
    coeffs: Coefficients<f32>
}

// Simplify DigitalFilter to use biquad's DirectForm2Transposed
#[derive(Debug)]
struct DigitalFilter {
    filter: DirectForm2Transposed<f32>
}

impl DigitalFilter {
    fn new(coeffs: FilterCoefficients) -> Self {
        Self {
            filter: biquad::DirectForm2Transposed::<f32>::new(coeffs.coeffs)
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        // Clamp input to prevent extreme values
        let x = x.clamp(-8192.0, 8191.0);
        let y = self.filter.run(x);
        // Clamp output to prevent instability
        y.clamp(-8192.0, 8191.0)
    }
}

// Add struct definitions for each filter type
#[derive(Debug)]
struct NotchFilter(DigitalFilter);

#[derive(Debug)]
struct HighpassFilter(DigitalFilter);

#[derive(Debug)]
struct LowpassFilter(DigitalFilter);

// Update NotchFilter implementation to use BandPass instead of NotchFilter
impl NotchFilter {
    fn new(sample_rate: f32, notch_freq: f32) -> Self {
        let q_factor = 30.0;  // High Q for narrow notch
        let coeffs = Coefficients::<f32>::from_params(
            Type::Notch,  // <-- Changed from BandPass to Notch
            sample_rate.hz(),
            notch_freq.hz(),
            q_factor
        ).unwrap();
        
        NotchFilter(DigitalFilter::new(FilterCoefficients { coeffs }))
    }
    
    fn process(&mut self, x: f32) -> f32 {
        self.0.process(x)
    }
}

// Similar updates for HighpassFilter
impl HighpassFilter {
    fn new(sample_rate: f32, cutoff_freq: f32) -> Self {
        let coeffs = Coefficients::<f32>::from_params(
            Type::HighPass,
            sample_rate.hz(),
            cutoff_freq.hz(),
            Q_BUTTERWORTH_F32
        ).unwrap();
        
        Self(DigitalFilter::new(FilterCoefficients { coeffs }))
    }
    
    fn process(&mut self, x: f32) -> f32 {
        self.0.process(x)
    }
}

// And LowpassFilter
impl LowpassFilter {
    fn new(sample_rate: f32, cutoff_freq: f32) -> Self {
        let coeffs = Coefficients::<f32>::from_params(
            Type::LowPass,
            sample_rate.hz(),
            cutoff_freq.hz(),
            Q_BUTTERWORTH_F32
        ).unwrap();
        
        Self(DigitalFilter::new(FilterCoefficients { coeffs }))
    }
    
    fn process(&mut self, x: f32) -> f32 {
        self.0.process(x)
    }
}

pub struct SignalProcessor {
    sample_rate: u32,
    num_channels: usize,
    powerline_notch_filters: Option<Vec<NotchFilter>>, // Holds 50Hz OR 60Hz filters, or None
    highpass_filters: Vec<HighpassFilter>,
    lowpass_filters: Vec<LowpassFilter>,
}

impl SignalProcessor {
    pub fn new(sample_rate: u32, num_channels: usize, dsp_high_pass_cutoff: f32, dsp_low_pass_cutoff: f32, powerline_filter_hz: Option<u32>) -> Self {
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
            sample_rate,
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

    pub fn reset(&mut self, new_sample_rate: u32, new_num_channels: usize, dsp_high_pass_cutoff: f32, dsp_low_pass_cutoff: f32, powerline_filter_hz: Option<u32>) {
        println!("[SignalProcessor::reset] Resetting with sample_rate: {}, num_channels: {}, HP_cutoff: {}, LP_cutoff: {}, powerline_filter_hz: {:?}",
                 new_sample_rate, new_num_channels, dsp_high_pass_cutoff, dsp_low_pass_cutoff, powerline_filter_hz);
        self.sample_rate = new_sample_rate;
        self.num_channels = new_num_channels;
        // Recreate all filters
        let sample_rate = self.sample_rate as f32;
        
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