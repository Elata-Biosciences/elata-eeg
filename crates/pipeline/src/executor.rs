//! The multi-threaded pipeline executor.

use crate::control::ControlCommand;
use crate::data::RtPacket;
use crate::error::FatalError;
use crate::graph::{PipelineGraph, StageId, StageMode};
use crate::stage::{Stage, StageContext, StageState};
use flume::{Receiver, Selector, Sender};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
};
use std::thread;
use tracing::{debug, error, info, warn};

/// A handle to a running stage in the executor.
struct StageHandle {
    thread_handle: thread::JoinHandle<()>,
}

/// An enum to unify the different message types that a stage thread can receive.
enum StageMessage {
    Data(Arc<RtPacket>),
    Control(ControlCommand),
    Stop,
}

/// The main executor for the pipeline.
///
/// This struct manages the thread pool and the communication channels between stages.
pub struct Executor {
    handles: Arc<Mutex<HashMap<StageId, StageHandle>>>,
    stop_txs: Arc<Mutex<Vec<Sender<()>>>>,
    control_txs: Arc<Mutex<Vec<Sender<ControlCommand>>>>,
    _graph: PipelineGraph,
}

/// A bus for sending control commands to all stages.
pub struct ControlBus {
    transmitters: Arc<Mutex<Vec<Sender<ControlCommand>>>>,
}

impl ControlBus {
    /// Sends a command to all stages.
    pub fn send_all(&self, cmd: ControlCommand) {
        for tx in self.transmitters.lock().unwrap().iter() {
            if let Err(e) = tx.send(cmd.clone()) {
                error!("Failed to send control command to a stage: {}", e);
            }
        }
    }
}

impl Executor {
    /// Creates a new executor from a pipeline graph.
    pub fn new(
        mut graph: PipelineGraph,
    ) -> (
        Self,
        Receiver<FatalError>,
        ControlBus,
        HashMap<StageId, Sender<Arc<RtPacket>>>,
    ) {
        let (fatal_error_tx, fatal_error_rx) = flume::unbounded();
        let mut producer_txs = HashMap::new();

        let mut executor = Self {
            handles: Arc::new(Mutex::new(HashMap::new())),
            stop_txs: Arc::new(Mutex::new(Vec::new())),
            control_txs: Arc::new(Mutex::new(Vec::new())),
            _graph: graph.clone(),
        };

        let mut producer_rxs = HashMap::new();
        for (name, node) in graph.nodes.iter_mut() {
            if node.mode == StageMode::Producer {
                let (tx, rx) = flume::unbounded();
                producer_txs.insert(name.clone(), tx);
                producer_rxs.insert(name.clone(), rx);
            }
        }

        for (name, rx) in producer_rxs {
            if let Some(node) = graph.nodes.get_mut(&name) {
                node.producer_rx = Some(rx);
            }
        }

        let control_bus = ControlBus {
            transmitters: Arc::clone(&executor.control_txs),
        };

        executor.wire_and_start(&mut graph, fatal_error_tx);
        executor.validate_wiring(&graph);
        (executor, fatal_error_rx, control_bus, producer_txs)
    }

    /// Wires the graph and starts the threads.
    fn wire_and_start(
        &mut self,
        graph: &mut PipelineGraph,
        fatal_error_tx: Sender<FatalError>,
    ) {
        let core_ids = core_affinity::get_core_ids().unwrap_or_default();
        if core_ids.is_empty() {
            warn!("Could not get core IDs. Cannot set thread affinity.");
        }

        let mut stage_inputs: HashMap<(StageId, String), Sender<Arc<RtPacket>>> = HashMap::new();
        let mut stage_rxs: HashMap<StageId, HashMap<String, Receiver<Arc<RtPacket>>>> =
            HashMap::new();

        for stage_config in &graph.config.stages {
            let mut input_rxs = HashMap::new();
            for input_port_spec in &stage_config.inputs {
                let capacity = stage_config.channel_capacity.unwrap_or(1024);
                let (tx, rx) = flume::bounded(capacity);
                let downstream_port_name = if let Some(parts) = input_port_spec.split_once("->") {
                    parts.1.to_string()
                } else {
                    "in".to_string()
                };
                stage_inputs.insert((stage_config.name.clone(), downstream_port_name.clone()), tx);
                input_rxs.insert(downstream_port_name, rx);
            }
            stage_rxs.insert(stage_config.name.clone(), input_rxs);
        }

        let mut stage_outputs: HashMap<(StageId, String), Vec<Sender<Arc<RtPacket>>>> =
            HashMap::new();
        for stage_config in &graph.config.stages {
            for output_port in &stage_config.outputs {
                stage_outputs.insert(
                    (stage_config.name.clone(), output_port.clone()),
                    Vec::new(),
                );
            }
            if stage_config.outputs.is_empty() {
                stage_outputs.insert((stage_config.name.clone(), "out".to_string()), Vec::new());
            }
        }

        for downstream_config in &graph.config.stages {
                  for input_port_spec in &downstream_config.inputs {
                      let (upstream_spec, downstream_port_name) =
                          if let Some(parts) = input_port_spec.split_once("->") {
                              (parts.0, parts.1.to_string())
                          } else {
                              (input_port_spec.as_str(), "in".to_string())
                          };
  
                      let parts: Vec<&str> = upstream_spec.split('.').collect();
                      if parts.len() != 2 {
                          warn!(
                              "Skipping invalid input specifier '{}' for stage '{}'. Format must be 'stage_name.port_name'.",
                              upstream_spec, downstream_config.name
                          );
                          continue;
                      }
                      let upstream_stage_name = parts[0].to_string();
                      let upstream_port_name = parts[1].to_string();
  
                      if let Some(output_senders) =
                          stage_outputs.get_mut(&(upstream_stage_name.clone(), upstream_port_name.clone()))
                      {
                          if let Some(input_sender) =
                              stage_inputs.get(&(downstream_config.name.clone(), downstream_port_name.clone()))
                          {
                              output_senders.push(input_sender.clone());
                              info!("Wired output '{}.{}' to input '{}' on stage '{}'", upstream_stage_name, upstream_port_name, downstream_port_name, downstream_config.name);
                          } else {
                              warn!("Wiring Error: Could not find input port '{}' for spec '{}' on downstream stage '{}'", downstream_port_name, input_port_spec, downstream_config.name);
                          }
                      } else {
                          warn!("Wiring Error: Could not find output port '{}.{}' for downstream stage '{}'", upstream_stage_name, upstream_port_name, downstream_config.name);
                      }
                  }
              }

        let stage_ids: Vec<_> = graph.nodes.keys().cloned().collect();
        for (i, stage_id) in stage_ids.iter().enumerate() {
            let (stop_tx, stop_rx) = flume::bounded(1);
            self.stop_txs.lock().unwrap().push(stop_tx);

            let (control_tx, control_rx) = flume::unbounded();
            self.control_txs.lock().unwrap().push(control_tx);

            let mut node = graph.nodes.remove(stage_id).unwrap();
            let mut context = graph.context.clone();
            let fatal_error_tx = fatal_error_tx.clone();

            let input_rxs = stage_rxs.remove(stage_id).unwrap_or_default();
            let output_txs_by_port = stage_outputs
                .iter()
                .filter(|((id, _), _)| id == stage_id)
                .map(|((_, port), senders)| (port.clone(), senders.clone()))
                .collect::<HashMap<_, _>>();


            let thread_name = node.name.clone();
            let builder = thread::Builder::new().name(thread_name);
            let core_ids_clone = core_ids.clone();

            let thread_handle = builder
                .spawn(move || {
                    if !core_ids_clone.is_empty() {
                        let core_id = core_ids_clone[i % core_ids_clone.len()];
                        if core_affinity::set_for_current(core_id) {
                            debug!("Set affinity for stage '{}' to core {:?}", node.name, core_id);
                        } else {
                            warn!("Failed to set affinity for stage '{}' to core {:?}", node.name, core_id);
                        }
                    }

                    info!("Stage thread '{}' started.", node.name);

                    match node.mode {
                        StageMode::Producer => {
                            let mut draining = false;
                            loop {
                                // 0) Stop / Control (non-blocking, drain the control queue)
                                if stop_rx.try_recv().is_ok() {
                                    node.state = StageState::Halted;
                                }
                                while let Ok(cmd) = control_rx.try_recv() {
                                    match cmd {
                                        ControlCommand::Drain => {
                                            info!("Draining producer '{}'", node.name);
                                            node.state = StageState::Draining;
                                            draining = true;
                                        }
                                        other => {
                                            if let Err(e) = node.stage.lock().unwrap().control(&other, &mut context) {
                                                error!("Control error on '{}': {}", node.name, e);
                                            }
                                        }
                                    }
                                }

                                if node.state == StageState::Halted { break; }

                                // 1) Produce only if not draining
                                let produced = if draining {
                                    // In drain mode, we could allow the producer to flush internal buffers.
                                    // For now, we just stop producing and let it halt on the next check.
                                    Ok(None)
                                } else {
                                    node.stage.lock().unwrap().produce(&mut context)
                                };

                                match produced {
                                    Ok(Some(outputs)) => {
                                        for (port, pkt) in outputs {
                                            if let Some(senders) = output_txs_by_port.get(&port) {
                                                for tx in senders { let _ = tx.send(pkt.clone()); }
                                            } else {
                                                warn!("'{}' produced to unwired port '{}'", node.name, port);
                                            }
                                        }
                                    }
                                    Ok(None) => {
                                        // idle path; avoid hot spin
                                        std::thread::yield_now();
                                    }
                                    Err(e) => {
                                        error!("Producer '{}' error: {}", node.name, e);
                                        let _ = fatal_error_tx.send(FatalError { stage_id: node.name.clone(), error: Box::new(e) });
                                        node.state = StageState::Halted;
                                    }
                                }

                                if node.state == StageState::Draining {
                                    // Producers drain instantly unless they have internal buffers to flush.
                                    node.state = StageState::Halted;
                                }
                            }
                        }
                        _ => { // Consumer / Fanout
                            loop {
                                if node.state == StageState::Halted {
                                    break;
                                }

                                // First, check for control messages non-blockingly to prevent starvation
                                if let Ok(cmd) = control_rx.try_recv() {
                                    let mut stage = node.stage.lock().unwrap();
                                    match cmd {
                                        ControlCommand::Drain => {
                                            info!("Draining stage '{}'", stage.id());
                                            node.state = StageState::Draining;
                                        }
                                        _ => {
                                            if let Err(e) = stage.control(&cmd, &mut context) {
                                                error!("Error handling control command: {}", e);
                                            }
                                        }
                                    }
                                }
                                if stop_rx.try_recv().is_ok() {
                                    node.state = StageState::Halted;
                                    continue;
                                }


                                if node.state == StageState::Draining {
                                    let all_inputs_disconnected =
                                        input_rxs.values().all(|rx| rx.is_disconnected());
                                    if all_inputs_disconnected {
                                        info!("Stage '{}' drained. Halting.", node.name);
                                        node.state = StageState::Halted;
                                        continue;
                                    }
                                }

                                let mut selector = Selector::new();
                                for (_, rx) in &input_rxs {
                                    selector = selector.recv(rx, |msg| msg.map(StageMessage::Data));
                                }
                                selector = selector.recv(&stop_rx, |msg| msg.map(|_| StageMessage::Stop).map_err(|e| e.into()));
                                selector = selector.recv(&control_rx, |msg| msg.map(StageMessage::Control).map_err(|e| e.into()));

                                // Use a timeout to prevent blocking indefinitely, allowing control messages to be checked.
                                let result = selector.wait_timeout(std::time::Duration::from_millis(5));
                                match result {
                                    Ok(Ok(StageMessage::Data(packet))) => {
                                        if process_packet(packet, &mut node, &mut context, &output_txs_by_port, &fatal_error_tx) {
                                            break;
                                        }
                                    }
                                    Ok(Ok(StageMessage::Control(cmd))) => {
                                        let mut stage = node.stage.lock().unwrap();
                                        match cmd {
                                            ControlCommand::Drain => {
                                                info!("Draining stage '{}'", stage.id());
                                                node.state = StageState::Draining;
                                            }
                                            _ => {
                                                if let Err(e) = stage.control(&cmd, &mut context) {
                                                    error!("Error handling control command: {}", e);
                                                }
                                            }
                                        }
                                    }
                                    Ok(Ok(StageMessage::Stop)) => {
                                        node.state = StageState::Halted;
                                    }
                                    Ok(Err(_)) => {
                                        info!("A channel for stage '{}' disconnected. Halting.", node.name);
                                        node.state = StageState::Halted;
                                    }
                                    Err(_) => {
                                        // Timeout, loop again to check control channels
                                    }
                                }
                            }
                        }
                    }
                    info!("Stage thread '{}' finished.", node.name);
                })
                .unwrap();

            self.handles
                .lock()
                .unwrap()
                .insert(stage_id.clone(), StageHandle { thread_handle });
        }
    }

    /// Stops the executor, shutting down all stage threads.
    pub fn stop(self) {
        info!("Stopping multi-threaded executor gracefully...");

        // 1. Send a Drain command to all stages to let them finish processing.
        let control_bus = ControlBus {
            transmitters: Arc::clone(&self.control_txs),
        };
        info!("Requesting all stages to drain...");
        control_bus.send_all(ControlCommand::Drain);

        // Give stages a moment to start draining. This is a simple approach.
        // A more robust solution might involve waiting for acknowledgment.
        thread::sleep(std::time::Duration::from_millis(100));

        // 2. Send the final stop signal.
        info!("Sending stop signal to all stages...");
        for tx in self.stop_txs.lock().unwrap().iter() {
            let _ = tx.send(());
        }

        // 3. Wait for all threads to join.
        let mut handles = self.handles.lock().unwrap();
        for (stage_id, handle) in handles.drain() {
            info!("Waiting for stage '{}' to shut down...", stage_id);
            if let Err(e) = handle.thread_handle.join() {
                error!("Stage '{}' panicked during shutdown: {:?}", stage_id, e);
            }
        }
    }

    pub fn get_current_config(&self) -> crate::config::SystemConfig {
        self._graph.get_current_config()
    }

    /// Validates the wiring of the graph, logging warnings for common issues.
    fn validate_wiring(&self, graph: &PipelineGraph) {
        info!("Starting post-wiring validation...");

        let mut output_connection_counts: HashMap<(StageId, String), usize> = HashMap::new();
        for downstream_config in &graph.config.stages {
            for input_port_spec in &downstream_config.inputs {
                let parts: Vec<&str> = input_port_spec.split('.').collect();
                if parts.len() == 2 {
                    let upstream_stage_name = parts[0].to_string();
                    let upstream_port_name = parts[1].to_string();
                    *output_connection_counts
                        .entry((upstream_stage_name, upstream_port_name))
                        .or_insert(0) += 1;
                }
            }
        }

        for stage_config in &graph.config.stages {
            // Check for outputs that are declared but not consumed by any downstream stage.
            for output_port in &stage_config.outputs {
                let key = (stage_config.name.clone(), output_port.clone());
                if !output_connection_counts.contains_key(&key) {
                    warn!(
                        "Output Validation: Output port '{}.{}' is declared but not connected to any input.",
                        stage_config.name, output_port
                    );
                }
            }
            if stage_config.outputs.is_empty() && !stage_config.inputs.is_empty() {
                 let key = (stage_config.name.clone(), "out".to_string());
                 if !output_connection_counts.contains_key(&key) {
                    warn!(
                        "Output Validation: Default output port '{}.out' is not connected to any input.",
                        stage_config.name
                    );
                }
            }


            // Check for inputs that are not connected.
            for input_port_spec in &stage_config.inputs {
                let (_upstream_spec, downstream_port_name) =
                    if let Some(parts) = input_port_spec.split_once("->") {
                        (parts.0, parts.1.to_string())
                    } else {
                        (input_port_spec.as_str(), "in".to_string())
                    };

                let is_connected = graph.config.stages.iter().any(|upstream_config| {
                    upstream_config.outputs.iter().any(|output_port| {
                        let expected_input_spec = format!("{}.{}", upstream_config.name, output_port);
                        input_port_spec.starts_with(&expected_input_spec)
                    }) || (upstream_config.outputs.is_empty() && input_port_spec == &format!("{}.out", upstream_config.name))
                });

                if !is_connected {
                    warn!(
                        "Input Validation: Input '{}' on stage '{}' is not connected to any output.",
                        downstream_port_name, stage_config.name
                    );
                }
            }
        }

        info!("Post-wiring validation complete.");
    }
}

/// Processes a single data packet through a stage.
fn process_packet(
    packet: Arc<RtPacket>,
    node: &mut crate::graph::PipelineNode,
    context: &mut StageContext,
    output_txs_by_port: &HashMap<String, Vec<Sender<Arc<RtPacket>>>>,
    fatal_error_tx: &Sender<FatalError>,
) -> bool {
    let mut stage_guard = node.stage.lock().unwrap();
    let stage_id = stage_guard.id().to_string();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        stage_guard.process(packet, context)
    }));

    // Drop the guard as soon as we're done with the stage
    drop(stage_guard);

    match result {
        Ok(Ok(output_packets)) => {
            if output_packets.is_empty() {
                return false;
            }
            for (port_name, packet) in output_packets {
                if let Some(senders) = output_txs_by_port.get(&port_name) {
                    for sender in senders {
                        if sender.send(packet.clone()).is_err() {
                            debug!("Downstream channel for port '{}' on stage '{}' disconnected.", port_name, stage_id);
                        }
                    }
                } else {
                    warn!("Stage '{}' produced output for un-wired port '{}'", stage_id, port_name);
                }
            }
        }
        Ok(Err(e)) => {
            error!("Stage '{}' returned an error: {}", stage_id, e);
            // Note: We are not sending non-panic errors as fatal.
            // The original code did, but this might be better handled
            // by a different mechanism.
        }
        Err(error) => {
            error!("Stage '{}' panicked.", stage_id);
            let _ = fatal_error_tx.send(FatalError {
                stage_id: stage_id.clone(),
                error,
            });
            node.state = StageState::Halted;
            return true; // Fatal error
        }
    }
    false
}