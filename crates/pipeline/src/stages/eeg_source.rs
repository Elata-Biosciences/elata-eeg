//! # EEG Source Stage
//!
//! This stage is responsible for interfacing with EEG hardware drivers,
//! acquiring raw data in batches, and forwarding it into the pipeline.

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use serde::Deserialize;

use sensors::types::{AdcConfig, AdcDriver};

use crate::config::StageConfig;
use crate::control::{ControlCommand, PipelineEvent};
use crate::data::{PacketData, PacketHeader, RtPacket};
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext, StageInitCtx};
use eeg_types::data::SensorMeta;
use flume::{Receiver, Sender};

/// Factory for creating `EegSource` stages.
#[derive(Default)]
pub struct EegSourceFactory;

impl StageFactory for EegSourceFactory {
    fn create(
        &self,
        config: &StageConfig,
        init_ctx: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        let params: EegSourceParams = serde_json::from_value(serde_json::to_value(&config.params)?)
            .map_err(|e| {
                StageError::BadConfig(format!("Failed to parse EegSource params: {}", e))
            })?;

        let driver = init_ctx
            .driver
            .clone()
            .ok_or_else(|| StageError::BadConfig("Driver not available in context".to_string()))?;

        let stage = EegSource::new(
            config.name.clone(),
            driver,
            params.batch_size,
            config.outputs.clone(),
            init_ctx.event_tx.clone(),
        )?;

        // Create a channel for this producer stage
        let (_tx, rx) = flume::unbounded();
        Ok((Box::new(stage), Some(rx)))
    }
}

#[derive(Debug, Deserialize)]
struct EegSourceParams {
    batch_size: usize,
    #[serde(default)]
    outputs: Vec<String>,
}

/// The `EegSource` stage.
pub struct EegSource {
    id: String,
    driver: Arc<Mutex<Box<dyn AdcDriver + Send>>>,
    batch_size: usize,
    outputs: Vec<String>,
    event_tx: Sender<PipelineEvent>,
    meta_rev_counter: Arc<AtomicUsize>,
    frame_id_counter: u64,
    stop_flag: Arc<AtomicBool>, // Used to signal driver to stop blocking acquire
}

impl EegSource {
    pub fn new(
        id: String,
        driver: Arc<Mutex<Box<dyn AdcDriver + Send>>>,
        batch_size: usize,
        outputs: Vec<String>,
        event_tx: Sender<PipelineEvent>,
    ) -> Result<Self, StageError> {
        let mut driver_guard = driver.lock().unwrap();
        if let Err(e) = driver_guard.initialize() {
            log::error!("Failed to initialize driver: {}", e);
            return Err(StageError::DriverError(e.to_string()));
        }
        let initial_config = driver_guard.get_config().unwrap();
        let initial_num_channels: usize =
            initial_config.chips.iter().map(|chip| chip.channels.len()).sum();
        if initial_num_channels == 0 {
            return Err(StageError::BadConfig("No channels configured for driver".to_string()));
        }
        log::info!("Driver configured with {} total channels", initial_num_channels);

        let meta_rev_counter = Arc::new(AtomicUsize::new(1));
        let initial_sensor_meta =
            Arc::new(create_sensor_meta_from_config(&initial_config, &meta_rev_counter));
        if event_tx
            .send(PipelineEvent::SourceReady {
                meta: (*initial_sensor_meta).clone(),
            })
            .is_err()
        {
            log::error!("Failed to send initial source ready event, stopping.");
            return Err(StageError::SendError("Failed to send initial SourceReady".to_string()));
        }

        Ok(Self {
            id,
            driver: driver.clone(),
            batch_size,
            outputs,
            event_tx,
            meta_rev_counter,
            frame_id_counter: 0,
            stop_flag: Arc::new(AtomicBool::new(false)),
        })
    }
}

fn flatten_global_channel_indices(config: &AdcConfig) -> Vec<u8> {
    let mut globals: Vec<u8> = Vec::new();
    for (chip_idx, chip) in config.chips.iter().enumerate() {
        let base = (chip_idx as u8) * 8;
        for &c in &chip.channels {
            globals.push(base + (c as u8));
        }
    }
    globals
}

fn create_sensor_meta_from_config(
    config: &AdcConfig,
    meta_rev: &Arc<AtomicUsize>,
) -> SensorMeta {
    let channel_names: Vec<String> = flatten_global_channel_indices(config)
        .into_iter()
        .map(|g| format!("CH{}", g))
        .collect();

    SensorMeta {
        sensor_id: 1, // Set appropriate sensor ID
        meta_rev: meta_rev.load(Ordering::Relaxed) as u32,
        schema_ver: 1,
        source_type: "eeg_source".to_string(),
        v_ref: config.vref,
        adc_bits: 24,
        gain: config.gain,
        sample_rate: config.sample_rate,
        offset_code: 0,
        is_twos_complement: true,
        channel_names,
        #[cfg(feature = "meta-tags")]
        tags: std::collections::HashMap::new(),
    }
}

impl Stage for EegSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn control(
        &mut self,
        cmd: &ControlCommand,
        _ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        if let ControlCommand::SetParameter {
            target_stage,
            parameters,
        } = cmd
        {
            if target_stage == self.id() {
                log::info!(
                    "EegSource received SetParameter with parameters: {:?}",
                    parameters
                );

                if let Some(driver_params) = parameters.get("driver") {
                    match serde_json::from_value::<AdcConfig>(driver_params.clone()) {
                        Ok(new_config) => {
                            log::info!("Reconfiguring driver...");
                            // Capture old shape for comparison
                            let (shape_before, reconfig_result) = {
                                let mut driver_guard = self.driver.lock().unwrap();
                                let old_cfg = driver_guard.get_config().unwrap_or_else(|_| new_config.clone());
                                let before = (
                                    old_cfg.sample_rate,
                                    old_cfg.vref,
                                    old_cfg.gain,
                                    flatten_global_channel_indices(&old_cfg),
                                );
                                let res = driver_guard.reconfigure(&new_config);
                                (before, res)
                            };

                            if let Err(e) = reconfig_result {
                                log::error!("Failed to reconfigure driver: {}", e);
                            } else {
                                // Determine if data shape changed (channels/rate/gain/vref)
                                let shape_after = (
                                    new_config.sample_rate,
                                    new_config.vref,
                                    new_config.gain,
                                    flatten_global_channel_indices(&new_config),
                                );
                                let shape_changed = shape_before != shape_after;

                                if shape_changed {
                                    self.meta_rev_counter.fetch_add(1, Ordering::Relaxed);
                                    let new_meta = Arc::new(create_sensor_meta_from_config(
                                        &new_config,
                                        &self.meta_rev_counter,
                                    ));
                                    log::info!(
                                        "Sending SourceReady event with new metadata (rev: {})",
                                        new_meta.meta_rev
                                    );
                                    let _ = self.event_tx.send(PipelineEvent::SourceReady {
                                        meta: (*new_meta).clone(),
                                    });
                                } else {
                                    log::info!("Driver reconfigured with no data shape change; not emitting SourceReady");
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to deserialize driver parameters for reconfiguration: {}",
                                e
                            );
                            return Err(StageError::BadConfig(
                                "Invalid driver parameters".to_string(),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn process(
        &mut self,
        _packet: Arc<RtPacket>, // Ignored, this is a source stage
        _ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        unreachable!("process should not be called on a producer stage");
    }

    fn produce(
        &mut self,
        _ctx: &mut StageContext,
    ) -> Result<Option<Vec<(String, Arc<RtPacket>)>>, StageError> {
        let (samples, timestamp, num_channels, sensor_meta) = {
            let mut driver_guard = self.driver.lock().unwrap();
            let config = driver_guard.get_config().unwrap();
            let num_channels = config.chips.iter().map(|c| c.channels.len()).sum();
            let sensor_meta = Arc::new(create_sensor_meta_from_config(&config, &self.meta_rev_counter));
            match driver_guard.acquire_batched(self.batch_size, &self.stop_flag) {
                Ok((s, t, _)) => (s, t, num_channels, sensor_meta),
                Err(e) => {
                    log::error!("Driver error in acquire_batched: {}", e);
                    self.stop_flag.store(true, Ordering::Relaxed);
                    (Vec::new(), 0, 0, sensor_meta)
                }
            }
        };

        if self.stop_flag.load(Ordering::Relaxed) || samples.is_empty() {
            // Use a short sleep to avoid spinning on the CPU when idle
            std::thread::sleep(Duration::from_millis(1));
            return Ok(None);
        }

        let num_samples_in_batch = if num_channels > 0 {
            samples.len() / num_channels
        } else {
            0
        };

        let output_name = self.outputs.iter().find(|&s| s == "raw_data").cloned().unwrap_or_else(|| "out".to_string());
        let packet = Arc::new(RtPacket::RawI32(PacketData {
            header: PacketHeader {
                source_id: format!("{}.{}", self.id, output_name),
                packet_type: "RawI32".to_string(),
                frame_id: {
                    let prev = self.frame_id_counter;
                    self.frame_id_counter += 1;
                    prev
                },
                ts_ns: timestamp,
                batch_size: num_samples_in_batch as u32,
                num_channels: num_channels as u32,
                meta: sensor_meta,
            },
            samples,
        }));

        Ok(Some(vec![(output_name, packet)]))
    }

    fn shutdown(&mut self, _ctx: &mut StageContext) -> Result<(), StageError> {
        self.stop_flag.store(true, Ordering::Relaxed);
        Ok(())
    }
}
