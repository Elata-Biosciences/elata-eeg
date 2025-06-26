use std::f32::consts::PI;
use rand::Rng;
use log::{debug, trace};
use lazy_static::lazy_static;
use super::super::types::{AdcConfig, AdcData, DriverError};

/// Helper function to get current timestamp in microseconds
///
/// This is used to get the initial base timestamp when acquisition starts.
/// For individual samples, we use a calculated timestamp based on the sample number
/// and sample rate to ensure precise and consistent timing.
pub fn current_timestamp_micros() -> Result<u64, DriverError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .map_err(|e| DriverError::Other(format!("Failed to get timestamp: {}", e)))
}

// Same one as the ADS1299
fn convert_sample_to_voltage(sample_value: i32, gain: u8, use_4_5v_ref: bool) -> f32 {
    // Factor for converting to voltage: 2^23 (full scale of 24-bit ADC with sign bit)
    const FACTOR: f64 = 8_388_608.0; // 2^23
    
    // Get the full-scale voltage based on gain and reference voltage
    let v_fs = if use_4_5v_ref {
        match gain {
            1 => 4.5,
            2 => 2.25,
            4 => 1.125,
            6 => 0.75,
            8 => 0.563,
            12 => 0.375,
            24 => 0.188,
            _ => 4.5, // Default to gain=1 value if invalid gain provided
        }
    } else {
        match gain {
            1 => 2.4,
            2 => 1.2,
            4 => 0.6,
            6 => 0.4,
            8 => 0.3,
            12 => 0.2,
            24 => 0.1,
            _ => 2.4, // Default to gain=1 value if invalid gain provided
        }
    };
    
    // Convert to voltage and return as f32
    ((sample_value as f64 * v_fs) / FACTOR) as f32
}

/// Helper function to generate dummy ADC data with sine waves for each channel.
/// Each channel's sine wave frequency is defined by:
///     channel 0: 2 Hz, channel 1: 6 Hz, channel 2: 10 Hz, etc.
/// (i.e., channel i gets 2 + 4*i Hz).
pub fn gen_eeg_sinusoid_data(config: &AdcConfig, relative_micros: u64) -> Vec<AdcData> {
    let t_secs = relative_micros as f32 / 1_000_000.0;
    trace!("Generating sample at t={} secs", t_secs);

    // Scale factor for converting sine wave (-1.0 to 1.0) to 24-bit range
    const AMPLITUDE: f32 = 2000.0 * 256.0; // Scale for 24-bit range
    let timestamp = relative_micros;

    config.channels.iter().enumerate().map(|(i, &channel)| {
        let freq = 2.0 + (i as f32) * 4.0; // 2 Hz for ch0, 6 Hz for ch1, etc.
        let angle = 2.0 * PI * freq * t_secs;
        let waveform = angle.sin();
        let raw_value = (waveform * AMPLITUDE) as i32;
        let voltage = convert_sample_to_voltage(raw_value, config.gain as u8, (config.vref - 4.5).abs() < f32::EPSILON);
        
        AdcData {
            channel,
            raw_value,
            voltage,
            timestamp,
        }
    }).collect()
}

/// Helper function to generate more realistic EEG-like data with multiple frequency bands.
/// This implementation creates synthetic EEG data with delta, theta, alpha, beta, and gamma
/// components, as well as simulated line noise at 50Hz and 60Hz.
pub fn gen_realistic_eeg_data(config: &AdcConfig, relative_micros: u64) -> Vec<AdcData> {
    use rand::Rng;
    use std::f32::consts::PI;
    
    // Define constants
    const BYTES_PER_SAMPLE: usize = 3; // Assuming 24-bit samples (i24)
    
    let t_secs = relative_micros as f32 / 1_000_000.0;
    trace!("Generating EEG sample at t={} secs", t_secs);
    
    // Create or get the EEG generator
    // We use a static mutex to ensure thread safety and preserve state between calls
    lazy_static! {
        static ref EEG_GENERATORS: std::sync::Mutex<std::collections::HashMap<u32, EegGenerator>> =
            std::sync::Mutex::new(std::collections::HashMap::new());
    }
    
    // Get or create an EEG generator for this sample rate and channel count
    let mut generators = EEG_GENERATORS.lock().unwrap();
    let generator_key = config.sample_rate;
    
    if !generators.contains_key(&generator_key) {
        debug!("Creating new EEG generator for sample rate {} Hz", config.sample_rate);
        generators.insert(generator_key, EegGenerator::new(config.sample_rate, config.channels.len()));
    }
    
    // Get a mutable reference to the generator
    let gen = generators.get_mut(&generator_key).unwrap();
    
    // Update the time for all channels
    for chan_idx in 0..gen.num_channels {
        gen.t[chan_idx] = t_secs;
    }
    
    // Generate samples for each channel
    let timestamp = relative_micros;
    
    config.channels.iter().enumerate().map(|(i, &channel)| {
        let raw_value = gen.generate_sample(i);
        let voltage = convert_sample_to_voltage(raw_value, config.gain as u8, (config.vref - 4.5).abs() < f32::EPSILON);
        
        AdcData {
            channel,
            raw_value,
            voltage,
            timestamp,
        }
    }).collect()
}

/// A generator for realistic EEG-like data with multiple frequency bands.
#[derive(Debug, Clone)]
pub struct EegGenerator {
    pub sample_rate: u32,
    pub num_channels: usize,
    pub t: Vec<f32>,
    pub line_noise_amplitude: Vec<f32>,
    // Phase accumulators for each frequency band and channel
    delta_phase: Vec<f32>,
    theta_phase: Vec<f32>,
    alpha_phase: Vec<f32>,
    beta_phase: Vec<f32>,
    gamma_phase: Vec<f32>,
    // Frequencies for each band (in Hz)
    delta_freq: f32,
    theta_freq: f32,
    alpha_freq: f32,
    beta_freq: f32,
    gamma_freq: f32,
    // Band amplitudes vary by channel
    channel_weights: Vec<[f32; 5]>,
    // For alpha bursts
    alpha_burst_counter: Vec<i32>,
    // Add phase accumulators for line noise
    line_noise_50hz_phase: Vec<f32>,
    line_noise_60hz_phase: Vec<f32>,
}

impl EegGenerator {
    pub fn new(sample_rate: u32, num_channels: usize) -> Self {
        let mut rng = rand::thread_rng();
        
        debug!("Initializing EEG generator with {} Hz sample rate", sample_rate);
        
        // Create different weights for each channel
        // Format: [delta, theta, alpha, beta, gamma]
        let base_channel_weights = [
            // ch1 - ch8
            [3.0, 1.5, 0.8, 0.4, 0.1],  // Frontal left (Fp1) - more delta/theta
            [3.0, 1.5, 0.8, 0.4, 0.1],  // Frontal right (Fp2) - similar to Fp1
            [2.0, 1.2, 1.5, 0.6, 0.1],  // Central left (C3) - mix
            [2.0, 1.2, 1.5, 0.6, 0.1],  // Central right (C4) - mix
            [1.5, 1.0, 2.5, 0.7, 0.1],  // Parietal left (P3) - stronger alpha
            [1.5, 1.0, 2.5, 0.7, 0.1],  // Parietal right (P4) - stronger alpha
            [1.2, 0.8, 3.0, 0.5, 0.1],  // Occipital left (O1) - strongest alpha
            [1.2, 0.8, 3.0, 0.5, 0.1],  // Occipital right (O2) - strongest alpha
            // ch9 - ch16
            [3.0, 1.5, 0.8, 0.4, 0.1],  // Frontal left (Fp1) - more delta/theta
            [3.0, 1.5, 0.8, 0.4, 0.1],  // Frontal right (Fp2) - similar to Fp1
            [2.0, 1.2, 1.5, 0.6, 0.1],  // Central left (C3) - mix
            [2.0, 1.2, 1.5, 0.6, 0.1],  // Central right (C4) - mix
            [1.5, 1.0, 2.5, 0.7, 0.1],  // Parietal left (P3) - stronger alpha
            [1.5, 1.0, 2.5, 0.7, 0.1],  // Parietal right (P4) - stronger alpha
            [1.2, 0.8, 3.0, 0.5, 0.1],  // Occipital left (O1) - strongest alpha
            [1.2, 0.8, 3.0, 0.5, 0.1],  // Occipital right (O2) - strongest alpha            
            // ch17 - ch24
            [3.0, 1.5, 0.8, 0.4, 0.1],  // Frontal left (Fp1) - more delta/theta
            [3.0, 1.5, 0.8, 0.4, 0.1],  // Frontal right (Fp2) - similar to Fp1
            [2.0, 1.2, 1.5, 0.6, 0.1],  // Central left (C3) - mix
            [2.0, 1.2, 1.5, 0.6, 0.1],  // Central right (C4) - mix
            [1.5, 1.0, 2.5, 0.7, 0.1],  // Parietal left (P3) - stronger alpha
            [1.5, 1.0, 2.5, 0.7, 0.1],  // Parietal right (P4) - stronger alpha
            [1.2, 0.8, 3.0, 0.5, 0.1],  // Occipital left (O1) - strongest alpha
            [1.2, 0.8, 3.0, 0.5, 0.1],  // Occipital right (O2) - strongest alpha
            // ch25 - ch32
            [3.0, 1.5, 0.8, 0.4, 0.1],  // Frontal left (Fp1) - more delta/theta
            [3.0, 1.5, 0.8, 0.4, 0.1],  // Frontal right (Fp2) - similar to Fp1
            [2.0, 1.2, 1.5, 0.6, 0.1],  // Central left (C3) - mix
            [2.0, 1.2, 1.5, 0.6, 0.1],  // Central right (C4) - mix
            [1.5, 1.0, 2.5, 0.7, 0.1],  // Parietal left (P3) - stronger alpha
            [1.5, 1.0, 2.5, 0.7, 0.1],  // Parietal right (P4) - stronger alpha
            [1.2, 0.8, 3.0, 0.5, 0.1],  // Occipital left (O1) - strongest alpha
            [1.2, 0.8, 3.0, 0.5, 0.1],  // Occipital right (O2) - strongest alpha  
        ];
        
        // Create channel weights for the requested number of channels
        let mut channel_weights = Vec::with_capacity(num_channels);
        for i in 0..num_channels {
            // Reuse the base weights if we have more than 8 channels
            channel_weights.push(base_channel_weights[i % 8]);
        }
        
        // Initialize random starting phases
        let mut delta_phase = vec![0.0; num_channels];
        let mut theta_phase = vec![0.0; num_channels];
        let mut alpha_phase = vec![0.0; num_channels];
        let mut beta_phase = vec![0.0; num_channels];
        let mut gamma_phase = vec![0.0; num_channels];
        let mut line_noise_50hz_phase = vec![0.0; num_channels];
        let mut line_noise_60hz_phase = vec![0.0; num_channels];
        let mut line_noise_amplitude = vec![0.0; num_channels];
        let mut alpha_burst_counter = vec![0; num_channels];
        
        for i in 0..num_channels {
            delta_phase[i] = rng.gen::<f32>() * 2.0 * PI;
            theta_phase[i] = rng.gen::<f32>() * 2.0 * PI;
            alpha_phase[i] = rng.gen::<f32>() * 2.0 * PI;
            beta_phase[i] = rng.gen::<f32>() * 2.0 * PI;
            gamma_phase[i] = rng.gen::<f32>() * 2.0 * PI;
            line_noise_50hz_phase[i] = rng.gen::<f32>() * 2.0 * PI;
            line_noise_60hz_phase[i] = rng.gen::<f32>() * 2.0 * PI;
            
            // Different channels pick up different amounts of line noise
            line_noise_amplitude[i] = rng.gen_range(0.2..0.7);
        }
        
        Self {
            sample_rate,
            num_channels,
            t: vec![0.0; num_channels],
            line_noise_amplitude,
            delta_phase,
            theta_phase,
            alpha_phase,
            beta_phase,
            gamma_phase,
            delta_freq: 2.5,    // Center of delta band
            theta_freq: 6.0,    // Center of theta band
            alpha_freq: 10.0,   // Center of alpha band
            beta_freq: 20.0,    // Center of beta band
            gamma_freq: 40.0,   // Lower gamma
            channel_weights,
            alpha_burst_counter,
            line_noise_50hz_phase,
            line_noise_60hz_phase,
        }
    }

    pub fn generate_sample(&mut self, channel: usize) -> i32 {
        let mut rng = rand::thread_rng();
        
        // Phase increments for each oscillator
        let delta_phase_inc = 2.0 * PI * self.delta_freq / self.sample_rate as f32;
        let theta_phase_inc = 2.0 * PI * self.theta_freq / self.sample_rate as f32;
        let alpha_phase_inc = 2.0 * PI * self.alpha_freq / self.sample_rate as f32;
        let beta_phase_inc = 2.0 * PI * self.beta_freq / self.sample_rate as f32;
        let gamma_phase_inc = 2.0 * PI * self.gamma_freq / self.sample_rate as f32;
        
        // Line noise phase increments
        let line_50hz_inc = 2.0 * PI * 50.0 / self.sample_rate as f32;
        let line_60hz_inc = 2.0 * PI * 60.0 / self.sample_rate as f32;
        
        // Update phases
        self.delta_phase[channel] += delta_phase_inc;
        self.theta_phase[channel] += theta_phase_inc;
        self.alpha_phase[channel] += alpha_phase_inc;
        self.beta_phase[channel] += beta_phase_inc;
        self.gamma_phase[channel] += gamma_phase_inc;
        self.line_noise_50hz_phase[channel] += line_50hz_inc;
        self.line_noise_60hz_phase[channel] += line_60hz_inc;
        
        // Wrap phases to avoid floating point precision issues
        if self.delta_phase[channel] > 2.0 * PI { self.delta_phase[channel] -= 2.0 * PI; }
        if self.theta_phase[channel] > 2.0 * PI { self.theta_phase[channel] -= 2.0 * PI; }
        if self.alpha_phase[channel] > 2.0 * PI { self.alpha_phase[channel] -= 2.0 * PI; }
        if self.beta_phase[channel] > 2.0 * PI { self.beta_phase[channel] -= 2.0 * PI; }
        if self.gamma_phase[channel] > 2.0 * PI { self.gamma_phase[channel] -= 2.0 * PI; }
        if self.line_noise_50hz_phase[channel] > 2.0 * PI { self.line_noise_50hz_phase[channel] -= 2.0 * PI; }
        if self.line_noise_60hz_phase[channel] > 2.0 * PI { self.line_noise_60hz_phase[channel] -= 2.0 * PI; }
        
        // Generate signals for each frequency band
        let delta = self.delta_phase[channel].sin() * self.channel_weights[channel][0];
        let theta = self.theta_phase[channel].sin() * self.channel_weights[channel][1];
        let alpha = self.alpha_phase[channel].sin() * self.channel_weights[channel][2];
        let beta = self.beta_phase[channel].sin() * self.channel_weights[channel][3];
        let gamma = self.gamma_phase[channel].sin() * self.channel_weights[channel][4];
        
        // Add line noise
        let line_noise_50 = self.line_noise_50hz_phase[channel].sin() * self.line_noise_amplitude[channel] * 0.7;
        let line_noise_60 = self.line_noise_60hz_phase[channel].sin() * self.line_noise_amplitude[channel] * 0.3;
        
        // Add some random noise (1/f noise approximation)
        let noise = (rng.gen::<f32>() - 0.5) * 0.2;
        
        // Combine all components
        let signal = delta + theta + alpha + beta + gamma + line_noise_50 + line_noise_60 + noise;
        
        // Scale to 24-bit range and convert to i32
        let amplitude = 2000.0 * 256.0; // Scale up by 2^8 for 24-bit vs 16-bit
        (signal * amplitude) as i32
    }
}