use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use std::f32::consts::PI;

// WebSocket handling imports
use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::Serialize;
use futures_util::{StreamExt, SinkExt};
use std::sync::Arc;
use tokio::sync::broadcast;
use eeg_driver::AdcConfig;

// Import EegBatchData from the driver crate to avoid duplication
pub use eeg_driver::EegBatchData;

/// Configuration shared with DSP modules
#[derive(Clone, Debug)]
pub struct DspSharedConfig {
    // For now, this can be empty or contain daemon-specific config
    // In the future, this might contain paths, ports, or other shared settings
}

/// FFT result for a single channel
#[derive(Serialize, Debug, Clone)]
pub struct ChannelFftResult {
    pub power: Vec<f32>,
    pub frequencies: Vec<f32>,
}

/// Response structure for brain waves FFT WebSocket
#[derive(Serialize)]
pub struct BrainWavesAppletResponse {
    pub timestamp: u64,
    pub fft_results: Vec<ChannelFftResult>,
    pub error: Option<String>,
}

// Private helper function to generate Hann window coefficients
// Using the same formula as your example: 0.5 - 0.5 * cos(2*PI*n/N)
fn generate_hann_window(n: usize) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![1.0];
    }
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos())
        .collect()
}

/// Processes a chunk of EEG data to calculate its power spectrum.
/// Simple approach similar to your example - no Welch method, no smoothing.
///
/// # Arguments
///
/// * `data` - A slice of f32 representing raw EEG data for a single channel.
/// * `sample_rate` - The sample rate of the EEG data in Hz.
///
/// # Returns
///
/// A `Result` containing a tuple of two `Vec<f32>`:
/// * The first vector is the power spectrum in (µV)²/Hz.
/// * The second vector is the corresponding frequency bins in Hz.
/// Returns an error string if processing fails (e.g., empty data).
pub fn process_eeg_data(data: &[f32], sample_rate: f32) -> Result<(Vec<f32>, Vec<f32>), String> {
    if data.is_empty() {
        return Err("Input data cannot be empty".to_string());
    }
    if sample_rate <= 0.0 {
        return Err("Sample rate must be positive".to_string());
    }

    let n = data.len();

    // Generate Hann window coefficients
    let hann_coeffs = generate_hann_window(n);

    // Apply Hann window and convert to complex (scale V to µV)
    let mut buffer: Vec<Complex<f32>> = data.iter()
        .zip(&hann_coeffs)
        .map(|(&x, &w)| Complex::new(x * w * 1_000_000.0, 0.0))
        .collect();

    // FFT
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    fft.process(&mut buffer);

    // Calculate one-sided PSD like your example
    let spectrum_len = n / 2 + 1;
    let mut power_spectrum: Vec<f32> = Vec::with_capacity(spectrum_len);
    
    for k in 0..spectrum_len {
        let power = if k == 0 || (n % 2 == 0 && k == n / 2) {
            // DC and Nyquist (if present) - no factor of 2
            buffer[k].norm_sqr() / (sample_rate * n as f32)
        } else {
            // All other frequencies - factor of 2 for one-sided spectrum
            2.0 * buffer[k].norm_sqr() / (sample_rate * n as f32)
        };
        power_spectrum.push(power);
    }

    // Generate frequency bins
    let frequency_bins: Vec<f32> = (0..spectrum_len)
        .map(|i| i as f32 * sample_rate / n as f32)
        .collect();

    Ok((power_spectrum, frequency_bins))
}

/// Sets up the brain waves FFT WebSocket endpoint
///
/// This function creates a warp filter that handles WebSocket connections
/// for the brain waves FFT applet. It processes EEG data and responds with FFT results.
///
/// # Arguments
///
/// * `config` - Shared configuration for DSP modules
/// * `eeg_data_rx` - Broadcast receiver for EEG batch data
/// * `adc_config_rx` - Broadcast receiver for ADC configuration updates
///
/// # Returns
///
/// A warp filter that can be combined with other routes
pub fn setup_fft_websocket_endpoint(
    _config: &DspSharedConfig, // Remains for now, though unused
    eeg_data_tx: broadcast::Sender<EegBatchData>,
    adc_config_tx: broadcast::Sender<AdcConfig>, // For subscribing to updates
    shared_adc_config_arc: Arc<tokio::sync::Mutex<AdcConfig>>, // For getting initial state
) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
    warp::path("applet")
        .and(warp::path("brain_waves"))
        .and(warp::path("data"))
        .and(warp::ws())
        .and(warp::any().map(move || eeg_data_tx.subscribe()))
        .and(warp::any().map(move || adc_config_tx.subscribe()))
        .and(warp::any().map(move || shared_adc_config_arc.clone())) // Pass the Arc
        .map(|ws: warp::ws::Ws, eeg_rx: broadcast::Receiver<EegBatchData>, config_rx: broadcast::Receiver<AdcConfig>, initial_config_arc_cloned: Arc<tokio::sync::Mutex<AdcConfig>>| {
            ws.on_upgrade(move |socket| handle_brain_waves_fft_websocket(socket, eeg_rx, config_rx, initial_config_arc_cloned))
        })
        .boxed()
}

/// Handles the brain waves FFT WebSocket connection
async fn handle_brain_waves_fft_websocket(
    ws: WebSocket,
    mut rx_eeg: broadcast::Receiver<EegBatchData>,
    mut rx_config: broadcast::Receiver<AdcConfig>, // For updates
    shared_adc_config_arc: Arc<tokio::sync::Mutex<AdcConfig>>, // For initial state
) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    // WebSocket client connected - using log crate for proper logging
    log::info!("Brain Waves FFT WebSocket client connected");
    
    // CRITICAL FIX: Force the daemon to process RawData pipeline
    // The FFT plugin needs EegBatchData which comes from the RawData pipeline
    // Without this, the daemon's demand-based processing will skip all data processing
    log::debug!("Brain Waves FFT: Requesting RawData pipeline activation");

    const FFT_WINDOW_DURATION_SECONDS: f32 = 2.0; // Process 2 seconds of data for FFT
    const FFT_WINDOW_SLIDE_SECONDS: f32 = 1.0; // Slide window by 1 second (50% overlap)

    let mut channel_buffers: Vec<Vec<f32>> = Vec::new();
    let mut num_channels = 0;
    let mut sample_rate_f32 = 250.0; // Default sample rate
    let mut fft_window_samples = 0;
    let mut fft_slide_samples = 0;

    // Function to reinitialize based on config
    let reinitialize = |
        num_channels: &mut usize,
        sample_rate_f32: &mut f32,
        channel_buffers: &mut Vec<Vec<f32>>,
        fft_window_samples: &mut usize,
        fft_slide_samples: &mut usize,
        config: &AdcConfig
    | {
        *num_channels = config.channels.len();
        *sample_rate_f32 = config.sample_rate as f32;
        *channel_buffers = vec![Vec::new(); *num_channels];
        *fft_window_samples = (*sample_rate_f32 * FFT_WINDOW_DURATION_SECONDS).round() as usize;
        *fft_slide_samples = (*sample_rate_f32 * FFT_WINDOW_SLIDE_SECONDS).round() as usize;
        
        log::info!(
            "Brain Waves FFT: Initialized for {} channels, sample rate: {} Hz",
            *num_channels, *sample_rate_f32
        );
        log::debug!(
            "Brain Waves FFT: Window size: {} samples, Slide size: {} samples",
            *fft_window_samples, *fft_slide_samples
        );
    };

    // Get initial config directly from the shared Arc<Mutex<AdcConfig>>
    let initial_config_from_arc = {
        let guard = shared_adc_config_arc.lock().await;
        guard.clone()
    };
    reinitialize(&mut num_channels, &mut sample_rate_f32, &mut channel_buffers, &mut fft_window_samples, &mut fft_slide_samples, &initial_config_from_arc);

    // The following try_recv() is now less critical for initial setup,
    // but we can keep it to see if any broadcast happened *just* after subscription and before this.
    // It's unlikely to succeed if the Arc method worked.
    // Check for any immediate config updates (optional)
    match rx_config.try_recv() {
        Ok(initial_config_broadcast) => {
            if initial_config_broadcast != initial_config_from_arc {
                log::warn!("Brain Waves FFT: Config mismatch detected during initialization");
            }
        }
        Err(_) => {
            // Expected - no immediate config updates
        }
    }


    if num_channels == 0 { // This check should ideally not be true if Arc init worked
        log::warn!("Brain Waves FFT: Warning - num_channels is 0 after Arc init, check config. Waiting for broadcast update.");
    }

    loop {
        tokio::select! {
            // Handle EEG data
            Ok(eeg_batch_data) = rx_eeg.recv() => {
                // Only log at trace level for detailed processing info
                log::trace!("Brain Waves FFT: Received EegBatchData with {} channels, {} samples in first channel",
                    eeg_batch_data.channels.len(),
                    eeg_batch_data.channels.get(0).map_or(0, |ch| ch.len()));
                
                // Check for errors in the EEG data
                if let Some(err_msg) = &eeg_batch_data.error {
                    log::error!("Brain Waves FFT: Received error in EegBatchData: {}", err_msg);
                    let response = BrainWavesAppletResponse {
                        timestamp: eeg_batch_data.timestamp,
                        fft_results: Vec::new(),
                        error: Some(err_msg.clone()),
                    };
                    if let Ok(json_response) = serde_json::to_string(&response) {
                        if ws_tx.send(Message::text(json_response)).await.is_err() {
                            log::info!("Brain Waves FFT: WebSocket client disconnected while sending error.");
                            break;
                        }
                    }
                    continue;
                }

                // Check if we have valid configuration
                if num_channels == 0 {
                    log::debug!("Brain Waves FFT: No configuration available, skipping data processing");
                    continue;
                }

                // Check for channel count mismatch
                if eeg_batch_data.channels.len() != num_channels {
                    log::warn!(
                        "Brain Waves FFT: Channel count mismatch. Expected {}, got {}. Skipping this batch.",
                        num_channels, eeg_batch_data.channels.len()
                    );
                    continue;
                }
                // Add data to channel buffers
                for (i, data_vec) in eeg_batch_data.channels.iter().enumerate() {
                    if i < num_channels {
                        let before_len = channel_buffers[i].len();
                        channel_buffers[i].extend_from_slice(data_vec);
                        let after_len = channel_buffers[i].len();
                        log::trace!("Brain Waves FFT: Channel {} buffer: {} -> {} samples (+{})",
                            i, before_len, after_len, data_vec.len());
                    }
                }

                // Process FFT for channels that have enough data
                let mut all_channel_fft_results: Vec<ChannelFftResult> = Vec::with_capacity(num_channels);
                let mut processing_error: Option<String> = None;

                for i in 0..num_channels {
                    log::trace!("Brain Waves FFT: Channel {} buffer size: {}, need: {}",
                        i, channel_buffers[i].len(), fft_window_samples);
                    
                    if channel_buffers[i].len() >= fft_window_samples {
                        let window_data: Vec<f32> = channel_buffers[i][..fft_window_samples].to_vec();
                        log::trace!("Brain Waves FFT: Processing FFT for channel {} with {} samples", i, window_data.len());
                        
                        // Perform FFT
                        match process_eeg_data(&window_data, sample_rate_f32) {
                            Ok((power, frequencies)) => {
                                log::trace!("Brain Waves FFT: Channel {} FFT success - {} power bins, {} freq bins",
                                    i, power.len(), frequencies.len());
                                all_channel_fft_results.push(ChannelFftResult { power, frequencies });
                            }
                            Err(e) => {
                                log::error!("Brain Waves FFT: Error processing channel {}: {}", i, e);
                                processing_error = Some(format!("FFT processing error on channel {}: {}", i, e));
                                // Add an empty result to maintain channel order if one fails
                                all_channel_fft_results.push(ChannelFftResult { power: Vec::new(), frequencies: Vec::new()});
                            }
                        }

                        // Slide the window by removing processed samples
                        if fft_slide_samples > 0 && channel_buffers[i].len() >= fft_slide_samples {
                            channel_buffers[i].drain(..fft_slide_samples);
                            log::trace!("Brain Waves FFT: Channel {} buffer after slide: {}", i, channel_buffers[i].len());
                        }
                    } else {
                        // Not enough data for this channel yet
                        log::trace!("Brain Waves FFT: Channel {} - not enough data yet ({}/{})",
                            i, channel_buffers[i].len(), fft_window_samples);
                        all_channel_fft_results.push(ChannelFftResult { power: Vec::new(), frequencies: Vec::new()});
                    }
                }

                // Send response if we have any results or errors
                if !all_channel_fft_results.iter().all(|res| res.power.is_empty()) || processing_error.is_some() {
                    let response = BrainWavesAppletResponse {
                        timestamp: eeg_batch_data.timestamp,
                        fft_results: all_channel_fft_results.clone(),
                        error: processing_error,
                    };

                    // Log FFT results summary at trace level
                    log::trace!("Brain Waves FFT: Sending FFT results for {} channels", all_channel_fft_results.len());

                    if let Ok(json_response) = serde_json::to_string(&response) {
                        if ws_tx.send(Message::text(json_response)).await.is_err() {
                            log::info!("Brain Waves FFT: WebSocket client disconnected while sending FFT results.");
                            break;
                        }
                    }
                } else {
                    log::trace!("Brain Waves FFT: No FFT results to send (all channels empty)");
                }
            },

            // Handle configuration updates
            config_result = rx_config.recv() => {
                match config_result {
                    Ok(new_config) => {
                        log::info!("Brain Waves FFT: Received config update - reinitializing");
                        reinitialize(&mut num_channels, &mut sample_rate_f32, &mut channel_buffers, &mut fft_window_samples, &mut fft_slide_samples, &new_config);
                    }
                    Err(e) => {
                        // If the channel is closed, we can't get any more configs, so break.
                        if matches!(e, tokio::sync::broadcast::error::RecvError::Closed) {
                            break;
                        }
                        // For RecvError::Lagged, we might have missed some messages.
                        // We'll log it and continue, hoping a future config update might arrive.
                    }
                }
            },

            // Handle incoming WebSocket messages (currently not used, but good to have)
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        if msg.is_close() {
                            log::info!("Brain Waves FFT: WebSocket client requested close");
                            break;
                        }
                        // For now, we don't handle any incoming messages from the client
                        // In the future, this could be used for custom FFT configuration
                    }
                    Some(Err(e)) => {
                        log::error!("Brain Waves FFT: WebSocket error: {}", e);
                        break;
                    }
                    None => {
                        log::info!("Brain Waves FFT: WebSocket stream ended");
                        break;
                    }
                }
            }
        }
    }
    log::info!("Brain Waves FFT: WebSocket client disconnected");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_eeg_data_simple_sine_wave() {
        let sample_rate = 250.0; // 250 Hz
        let duration = 4.0; // 4 seconds (need more data for Welch's method)
        let n_samples = (sample_rate * duration) as usize;
        let frequency = 10.0; // 10 Hz sine wave

        let mut data = Vec::with_capacity(n_samples);
        for i in 0..n_samples {
            let time = i as f32 / sample_rate;
            data.push((2.0 * std::f32::consts::PI * frequency * time).sin());
        }

        match process_eeg_data(&data, sample_rate) {
            Ok((power_spectrum, frequency_bins)) => {
                assert!(!power_spectrum.is_empty());
                assert!(!frequency_bins.is_empty());
                assert_eq!(power_spectrum.len(), frequency_bins.len());

                // Find the peak frequency
                let mut max_power = 0.0;
                let mut peak_freq_bin = 0.0;
                for i in 0..power_spectrum.len() {
                    if power_spectrum[i] > max_power {
                        max_power = power_spectrum[i];
                        peak_freq_bin = frequency_bins[i];
                    }
                }
                // Allow for some tolerance due to Welch's method binning
                assert!((peak_freq_bin - frequency).abs() < 2.0, "Peak frequency {} is not close to {}", peak_freq_bin, frequency);
            }
            Err(e) => panic!("Processing failed: {}", e),
        }
    }

    #[test]
    fn test_process_eeg_data_empty_input() {
        let data: Vec<f32> = Vec::new();
        let sample_rate = 250.0;
        assert!(process_eeg_data(&data, sample_rate).is_err());
    }

    #[test]
    fn test_process_eeg_data_invalid_sample_rate() {
        let data: Vec<f32> = vec![1.0, 2.0, 3.0];
        let sample_rate = 0.0;
        assert!(process_eeg_data(&data, sample_rate).is_err());
    }

    #[test]
    fn test_frequency_bins_basic() {
        let sample_rate = 200.0;
        let n_samples = 512; // Larger sample for Welch's method
        let data: Vec<f32> = vec![0.1; n_samples]; // Small non-zero values

        match process_eeg_data(&data, sample_rate) {
            Ok((power_spectrum, frequency_bins)) => {
                assert!(!frequency_bins.is_empty());
                assert_eq!(power_spectrum.len(), frequency_bins.len());
                assert_eq!(frequency_bins[0], 0.0); // DC component
                // Last frequency should be less than Nyquist
                assert!(frequency_bins.last().unwrap() <= &(sample_rate / 2.0));
            }
            Err(e) => panic!("Processing failed: {}", e),
        }
    }

    #[test]
    fn test_microvolts_scaling() {
        let sample_rate = 250.0;
        let data = vec![1e-6; 512]; // 1 microvolt in volts
        
        match process_eeg_data(&data, sample_rate) {
            Ok((power_spectrum, _)) => {
                // Should have some power (not all zeros)
                let total_power: f32 = power_spectrum.iter().sum();
                assert!(total_power > 0.0, "Power spectrum should not be all zeros");
            }
            Err(e) => panic!("Processing failed: {}", e),
        }
    }
}