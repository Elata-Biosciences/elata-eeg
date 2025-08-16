use pipeline::data::RtPacket;
use pipeline::stage::{Stage, StageContext};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;
use uuid::Uuid;

const FFT_SIZE: usize = 1 << 12;
const VOLTS_TO_MICROVOLTS: f32 = 1_000_000.0;

#[derive(Clone)]
pub struct BrainWavesFftPlugin {
    id: String,
    channel_buffers: Vec<Vec<f32>>,
    fft_planner: Arc<dyn Fft<f32>>,
    num_channels: usize,
    #[allow(dead_code)]
    sample_rate: f32,
}

impl BrainWavesFftPlugin {
    pub fn new(num_channels: usize, sample_rate: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        Self {
            id: Uuid::new_v4().to_string(),
            channel_buffers: vec![Vec::with_capacity(FFT_SIZE); num_channels],
            fft_planner: fft,
            num_channels,
            sample_rate,
        }
    }
}

impl Stage for BrainWavesFftPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, pipeline::error::StageError> {
        if let RtPacket::Voltage(packet_data) = &*packet {
            let samples_per_channel = packet_data.samples.len() / self.num_channels;
            if samples_per_channel == 0 {
                return Ok(vec![]);
            }

            for ch in 0..self.num_channels {
                let start = ch * samples_per_channel;
                let end = start + samples_per_channel;
                if let Some(channel_samples) = packet_data.samples.get(start..end) {
                    self.channel_buffers[ch].extend_from_slice(channel_samples);
                }
            }

            if self.channel_buffers.iter().any(|b| b.len() >= FFT_SIZE) {
                for ch in 0..self.num_channels {
                    if self.channel_buffers[ch].len() >= FFT_SIZE {
                        let mut samples: Vec<f32> = self.channel_buffers[ch][..FFT_SIZE]
                            .iter()
                            .map(|&v| v * VOLTS_TO_MICROVOLTS)
                            .collect();

                        let window: Vec<f32> =
                            apodize::hanning_iter(FFT_SIZE).map(|v| v as f32).collect();
                        for (i, sample) in samples.iter_mut().enumerate() {
                            *sample *= window[i];
                        }

                        let mut buffer: Vec<Complex<f32>> = samples
                            .into_iter()
                            .map(|v| Complex::new(v, 0.0))
                            .collect();

                        self.fft_planner.process(&mut buffer);
                        self.channel_buffers[ch].drain(..FFT_SIZE / 2);
                    }
                }
            }

            Ok(vec![("out".to_string(), packet)])
        } else {
            Ok(vec![])
        }
    }
}