use std::sync::Arc;
use serde::{Deserialize, Serialize};
use biquad::{Biquad, DirectForm2Transposed as DF2T, Coefficients, Type, Q_BUTTERWORTH_F32, ToHertz};

use crate::stage::{Stage, StageContext, StageInitCtx};
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::data::{RtPacket, PacketView, PacketData};
use crate::RecycledF32Vec;
use crate::config::StageConfig;
use eeg_types::comms::pipeline::BrokerMessage;
use flume::Receiver;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TripleIirConfig {
    /// Optional override for channels in interleaved stream.
    /// If not provided, infer from packet header.
    #[serde(default)]
    pub channels: Option<usize>,
    /// High-pass cutoff (Hz)
    pub high_pass: f32,
    /// Low-pass cutoff (Hz)
    pub low_pass: f32,
    /// Optional powerline notch (50 or 60)
    pub notch: Option<f32>,
    /// Output name (default: "out")
    #[serde(default = "default_out")]
    pub output: String,
}
fn default_out() -> String { "out".to_string() }

#[derive(Default)]
pub struct TripleIirFactory;
impl StageFactory for TripleIirFactory {
    fn create(
        &self,
        config: &StageConfig,
        _init: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        let cfg: TripleIirConfig = serde_json::from_value(serde_json::Value::Object(config.params.clone().into_iter().collect()))
            .map_err(|e| StageError::BadConfig(format!("TripleIir config error: {:?}", e)))?;
        if let Some(c) = cfg.channels { if c == 0 { return Err(StageError::BadConfig("channels must be >=1 if provided".into())); } }
        if !(cfg.high_pass > 0.0 && cfg.low_pass > cfg.high_pass) {
            return Err(StageError::BadConfig("0 < high_pass < low_pass required".into()));
        }
        if let Some(n) = cfg.notch { if n != 50.0 && n != 60.0 { return Err(StageError::BadConfig("notch must be 50.0 or 60.0".into())); } }

        Ok((Box::new(TripleIirStage::new(config.name.clone(), cfg, config.outputs.clone())), None))
    }
}

struct ChannelChain {
    hp: DF2T<f32>,
    notch: Option<DF2T<f32>>,
    lp: DF2T<f32>,
}
impl ChannelChain {
    fn run(&mut self, x: f32) -> f32 {
        let y1 = self.hp.run(x + 1e-20);     // tiny offset avoids denormals on some CPUs
        let y2 = if let Some(n) = &mut self.notch { n.run(y1) } else { y1 };
        self.lp.run(y2)
    }
}

pub struct TripleIirStage {
    id: String,
    out_name: String,
    cfg: TripleIirConfig,
    fs_last: Option<f32>,
    chains: Vec<ChannelChain>, // one chain per channel
    scratch: Vec<f32>,
}

impl TripleIirStage {
    pub fn new(id: String, cfg: TripleIirConfig, outputs: Vec<String>) -> Self {
        let out_name = outputs.get(0).cloned().unwrap_or_else(|| cfg.output.clone());
        Self { id, out_name, cfg, fs_last: None, chains: Vec::new(), scratch: Vec::new() }
    }

    fn rebuild_if_needed(&mut self, fs_hz: f32, chans: usize) -> Result<(), StageError> {
        if self.fs_last == Some(fs_hz) && self.chains.len() == chans && !self.chains.is_empty() { return Ok(()); }
        if fs_hz <= 0.0 { return Err(StageError::BadConfig("sample_rate must be > 0".into())); }
        let nyq = fs_hz * 0.5;
        if !(self.cfg.high_pass > 0.0 && self.cfg.high_pass < nyq) { return Err(StageError::BadConfig("bad high_pass vs Nyquist".into())); }
        if !(self.cfg.low_pass > 0.0 && self.cfg.low_pass < nyq) { return Err(StageError::BadConfig("bad low_pass vs Nyquist".into())); }
        if let Some(n) = self.cfg.notch { if n >= nyq { return Err(StageError::BadConfig("notch must be < Nyquist".into())); } }

        // Build coefficients (Butterworth Q is fine for HP/LP)
        let hp = Coefficients::<f32>::from_params(Type::HighPass, fs_hz.hz(), self.cfg.high_pass.hz(), Q_BUTTERWORTH_F32)
            .map_err(|e| StageError::BadConfig(format!("HP coeffs: {:?}", e)))?;
        let lp = Coefficients::<f32>::from_params(Type::LowPass,  fs_hz.hz(), self.cfg.low_pass.hz(),  Q_BUTTERWORTH_F32)
            .map_err(|e| StageError::BadConfig(format!("LP coeffs: {:?}", e)))?;
        let notch = if let Some(n) = self.cfg.notch {
            // Narrow notch
            let q = 30.0;
            Some(Coefficients::<f32>::from_params(Type::Notch, fs_hz.hz(), n.hz(), q)
                .map_err(|e| StageError::BadConfig(format!("Notch coeffs: {:?}", e)))?)
        } else { None };

        self.chains = (0..chans).map(|_| ChannelChain {
            hp: DF2T::<f32>::new(hp),
            notch: notch.map(|n| DF2T::<f32>::new(n)),
            lp: DF2T::<f32>::new(lp),
        }).collect();

        self.fs_last = Some(fs_hz);
        Ok(())
    }
}

impl Stage for TripleIirStage {
    fn id(&self) -> &str { &self.id }

    fn process(
        &mut self,
        pkt: Arc<RtPacket>,
        ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        let out_pkt = match PacketView::from(&*pkt) {
            PacketView::Voltage { header, data } => {
                let fs = header.meta.sample_rate;
                // Determine channel count: explicit override or infer from header
                let mut chans = self.cfg.channels.unwrap_or_else(|| header.num_channels as usize);
                if chans == 0 {
                    // Fallback: infer from batch_size if available
                    let batch = header.batch_size as usize;
                    if batch > 0 && data.len() % batch == 0 {
                        chans = data.len() / batch;
                    }
                }
                if chans == 0 { return Err(StageError::BadConfig("Unable to determine channel count".into())); }

                self.rebuild_if_needed(fs as f32, chans)?;

                let frames = data.len() / chans;
                self.scratch.clear();
                self.scratch.reserve(frames * chans);

                for f in 0..frames {
                    for ch in 0..chans {
                        let idx = f * chans + ch;
                        let x = data[idx];
                        let y = self.chains[ch].run(x);
                        self.scratch.push(y);
                    }
                }

                let mut out_samples = RecycledF32Vec::with_capacity(ctx.allocator.clone(), self.scratch.len());
                out_samples.extend_from_slice(&self.scratch);

                let mut out_header = header.clone();
                out_header.source_id = format!("{}.{}", self.id, self.out_name);
                out_header.packet_type = "Voltage".to_string();

                Arc::new(RtPacket::Voltage(PacketData { header: out_header, samples: out_samples }))
            }
            _ => pkt, // pass through non-Voltage types
        };
        Ok(vec![(self.out_name.clone(), out_pkt)])
    }

    fn reconfigure(&mut self, cfg: &serde_json::Value, _ctx: &mut StageContext) -> Result<(), StageError> {
        let new_cfg: TripleIirConfig = serde_json::from_value(cfg.clone())
            .map_err(|e| StageError::BadConfig(format!("TripleIir reconfig error: {:?}", e)))?;
        if let Some(c) = new_cfg.channels { if c == 0 { return Err(StageError::BadConfig("channels must be >=1 if provided".into())); } }
        if !(new_cfg.high_pass > 0.0 && new_cfg.low_pass > new_cfg.high_pass) {
            return Err(StageError::BadConfig("0 < high_pass < low_pass required".into()));
        }
        if let Some(n) = new_cfg.notch { if n != 50.0 && n != 60.0 { return Err(StageError::BadConfig("notch must be 50.0 or 60.0".into())); } }
        self.cfg = new_cfg;
        self.chains.clear();   // force rebuild on next packet
        Ok(())
    }
}
