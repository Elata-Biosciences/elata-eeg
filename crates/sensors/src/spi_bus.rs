//! # Shared SPI Bus
//!
//! Pi 5 only has 2 chip selects. This overcomes that limitation w/ GPIO CS's
//! This module provides a thread-safe, shared SPI bus implementation that uses
//! manual GPIO toggling for Chip Select (CS). This allows any GPIO pin to be
//! used for CS, rather than being restricted to the hardware CE0/CE1 pins.

use rppal::gpio::OutputPin;
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::types::DriverError;

// A small delay is often required between CS toggle and clock, and after the transaction.
const CS_DELAY_US: u64 = 2;

/// A thread-safe wrapper for an SPI bus that allows sharing across multiple drivers.
/// Chip Select is handled manually by this struct's methods.
#[derive(Clone)]
pub struct SpiBus {
    spi: Arc<Mutex<Spi>>,
}

impl SpiBus {
    /// Creates a new shared SPI bus.
    ///
    /// A dummy `SlaveSelect` is provided to satisfy the `rppal` driver, but it is
    /// ignored. The hardware CE pin will toggle but should not be connected to anything.
    pub fn new(bus: Bus, clock_speed: u32, mode: Mode) -> Result<Self, DriverError> {
        let spi = Spi::new(bus, SlaveSelect::Ss0, clock_speed, mode)?;
        Ok(Self {
            spi: Arc::new(Mutex::new(spi)),
        })
    }

    /// Writes data to the SPI bus, wrapping the transaction in a manual CS toggle.
    pub fn write(&self, cs_pin: &mut OutputPin, data: &[u8]) -> Result<(), DriverError> {
        let mut spi = self.spi.lock().unwrap();
        
        // Ensure CS is high before we start
        cs_pin.set_high();

        // Start transaction
        cs_pin.set_low();
        thread::sleep(Duration::from_micros(CS_DELAY_US));

        let result = spi.write(data).map_err(|e| DriverError::from(e));

        // End transaction
        thread::sleep(Duration::from_micros(CS_DELAY_US));
        cs_pin.set_high();

        result.map(|_| ())
    }

    /// Performs a transfer on the SPI bus, wrapping the transaction in a manual CS toggle.
    pub fn transfer(
        &self,
        cs_pin: &mut OutputPin,
        read_buffer: &mut [u8],
        write_buffer: &[u8],
    ) -> Result<(), DriverError> {
        let spi = self.spi.lock().unwrap();

        cs_pin.set_high();

        cs_pin.set_low();
        thread::sleep(Duration::from_micros(CS_DELAY_US));

        let result = spi
            .transfer(read_buffer, write_buffer)
            .map_err(|e| DriverError::from(e));

        thread::sleep(Duration::from_micros(CS_DELAY_US));
        cs_pin.set_high();

        result.map(|_| ())
    }
}