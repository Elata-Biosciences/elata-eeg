use std::error::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use log::{info, warn, error, debug};

use eeg_sensor::{
    create_driver, AdcConfig, AdcData, AdcDriver, DriverError, DriverEvent, DriverStatus,
};

pub struct EegSystem {
    driver: Box<dyn AdcDriver>,
    processing_task: Option<JoinHandle<()>>,
    tx: mpsc::Sender<AdcData>,
    event_rx: Option<mpsc::Receiver<DriverEvent>>,
    cancel_token: CancellationToken,
}

impl EegSystem {
    /// Creates an EEG processing system without starting it
    pub async fn new(
        config: AdcConfig
    ) -> Result<(Self, mpsc::Receiver<AdcData>), Box<dyn Error>> {
        info!("Creating new EegSystem with config");
        let (driver, event_rx) = create_driver(config).await?;
        let (tx, rx) = mpsc::channel(100);
        let cancel_token = CancellationToken::new();

        let system = Self {
            driver,
            processing_task: None,
            tx,
            event_rx: Some(event_rx),
            cancel_token,
        };

        Ok((system, rx))
    }

    /// Starts the processing system with the given configuration
    pub async fn start(&mut self, config: AdcConfig) -> Result<(), Box<dyn Error>> {
        self.initialize_processing(config).await
    }

    /// Internal helper to initialize or reinitialize the driver and processing task
    async fn initialize_processing(&mut self, config: AdcConfig) -> Result<(), Box<dyn Error>> {
        info!("Initializing EEG processing with config");
        
        // Validate configuration
        if config.channels.is_empty() {
            error!("Cannot initialize with zero channels");
            return Err(Box::new(DriverError::ConfigurationError(
                "Cannot initialize with zero channels".into()
            )));
        }

        if config.sample_rate == 0 {
            error!("Invalid sample rate: 0");
            return Err(Box::new(DriverError::ConfigurationError(
                "Sample rate must be greater than 0".into()
            )));
        }

        // Cancel any existing processing task gracefully
        if self.processing_task.is_some() {
            debug!("Cancelling existing processing task");
            self.cancel_token.cancel();
            self.cancel_token = CancellationToken::new();
        }

        // Start acquisition
        self.driver.start_acquisition().await?;

        // Take ownership of the event receiver
        let mut event_rx = self.event_rx.take().expect("Event receiver should exist");

        // Start the processing task
        let tx = self.tx.clone();
        // Clone the cancellation token for the task
        let cancel_token = self.cancel_token.clone();

        self.processing_task = Some(tokio::spawn(async move {
            // Create a select future that will complete when either:
            // 1. We receive an event from the driver
            // 2. The cancellation token is triggered
            loop {
                tokio::select! {
                    // Check if cancellation was requested
                    _ = cancel_token.cancelled() => {
                        break;
                    }
                    // Process events from the driver
                    event_opt = event_rx.recv() => {
                        match event_opt {
                            Some(event) => {
                                match event {
                                    DriverEvent::Data(data_batch) => {
                                        // Forward each AdcData directly to the output channel
                                        for data in data_batch {
                                            if let Err(e) = tx.send(data).await {
                                                eprintln!("Error sending data: {}", e);
                                                // Continue processing - don't break on send errors
                                            }
                                        }
                                    }
                                    DriverEvent::StatusChange(status) => {
                                        if status == DriverStatus::Stopped {
                                            break;
                                        }
                                        
                                        // Log status changes but don't forward them
                                        println!("Driver status changed: {:?}", status);
                                    }
                                    DriverEvent::Error(err_msg) => {
                                        eprintln!("Driver error: {}", err_msg);
                                        // Note: We can't send errors through AdcData channel
                                        // Errors will be handled by the device daemon
                                    }
                                }
                            }
                            None => {
                                // Channel closed, exit the task
                                break;
                            }
                        }
                    }
                }
            }
        }));

        Ok(())
    }


    /// Stop the data acquisition & gracefully cancel the background task
    pub async fn stop(&mut self) -> Result<(), Box<dyn Error>> {
        debug!("Stopping EEG system");
        self.driver.stop_acquisition().await?;
        
        if let Some(task) = self.processing_task.take() {
            debug!("Cancelling processing task");
            self.cancel_token.cancel();
            
            match tokio::time::timeout(Duration::from_millis(500), task).await {
                Ok(_) => debug!("Processing task stopped gracefully"),
                Err(_) => warn!("Processing task didn't complete in time, forcing abort"),
            }
            
            self.cancel_token = CancellationToken::new();
        }
        
        Ok(())
    }

    /// Reconfigure the driver with new settings
    pub async fn reconfigure(&mut self, config: AdcConfig) -> Result<(), Box<dyn Error>> {
        info!("Reconfiguring EEG system with new settings");
        self.stop().await?;
        
        // Create new driver with updated configuration
        let (new_driver, new_event_rx) = create_driver(config.clone()).await?;
        self.driver = new_driver;
        self.event_rx = Some(new_event_rx);
        
        self.initialize_processing(config).await
    }

    /// Retrieve the current driver status
    pub async fn driver_status(&self) -> DriverStatus {
        self.driver.get_status().await
    }

    /// Retrieve the driver's configuration
    pub async fn driver_config(&self) -> Result<AdcConfig, DriverError> {
        self.driver.get_config().await
    }

    /// Completely shut down the EEG system and clean up resources
    pub async fn shutdown(&mut self) -> Result<(), Box<dyn Error>> {
        const SHUTDOWN_TIMEOUT_MS: u64 = 1000;
        
        let shutdown_future = async {
            self.stop().await?;
            self.driver.shutdown().await.map_err(|e| Box::new(e) as Box<dyn Error>)
        };
        
        match tokio::time::timeout(
            Duration::from_millis(SHUTDOWN_TIMEOUT_MS),
            shutdown_future
        ).await {
            Ok(result) => result,
            Err(_) => {
                error!("Shutdown timed out");
                Err(Box::new(DriverError::Other("Shutdown timed out".into())))
            }
        }
    }
}

impl Drop for EegSystem {
    fn drop(&mut self) {
        // Since we can't use .await in Drop, we'll just log a warning
        eprintln!("Warning: EegSystem dropped without calling shutdown() first");
        eprintln!("Always call system.shutdown().await before dropping the system");
    }
}
