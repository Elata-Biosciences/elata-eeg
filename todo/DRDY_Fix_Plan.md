# DRDY Dispatcher and GPIO Refactoring Plan

## 1. Current Situation

The `elata-eeg` project is facing persistent runtime errors when using the dual-chip `ElataV2Driver`. The primary symptoms are:
- **`DRDY dispatcher timed out`**: The system does not receive the "Data Ready" signal from the ADS1299 chips.
- **`GpioError("Pin X is already in use")`**: The application panics because multiple parts of the code are trying to claim exclusive ownership of the same GPIO hardware pins.

These issues indicate a fundamental architectural problem in how GPIO resources and hardware interrupts are managed, especially in a multi-chip environment.

## 2. History & What We've Done So Far

- **Initial Compilation Fix**: Corrected a `E0277` compilation error by implementing `From<rppal::gpio::Error> for sensors::DriverError`.
- **Attempted DRDY Fix**: Modified the `CONFIG1_REG` in `ads1299/registers.rs` to enable the DRDY pin and corrected clock configuration in `elata_v2/driver.rs`. This was a correct step but insufficient on its own.
- **In-depth Analysis**: Through a collaborative process and detailed feedback, we've diagnosed the root causes:
    1.  **GPIO Resource Conflict**: Multiple `Gpio::new()` calls create conflicting handles to the same hardware, leading to panics.
    2.  **Flawed Interrupt Handling**: Each chip driver thread polling its own DRDY pin is unreliable and prone to race conditions.
    3.  **Insufficient Reset Timing**: The ADS1299 chips may not have enough time to stabilize after a reset command, causing subsequent configuration to fail silently.

## 3. The Final Implementation Plan

Based on our analysis, we have an approved architectural plan to resolve these issues definitively.

### TODO List

- [ ] **Step 1: Centralize GPIO and DRDY Pin Management**
- [ ] **Step 2: Decouple `Ads1299Driver` from GPIO**
- [ ] **Step 3: Implement the Centralized DRDY Dispatcher**
- [ ] **Step 4: Ensure Correct Reset Timing**

---

### Step 1: Centralize GPIO and DRDY Pin Management

**Goal**: `ElataV2Driver` will become the sole owner of all GPIO resources.

**Actions**:
- In `ElataV2Driver::new()`, create a single `Arc<Gpio>` instance.
- Acquire `InputPin` handles for each DRDY pin *once* and store them.
- Create `flume` channels for each chip to signal DRDY events.

**Example Code (`crates/boards/src/elata_v2/driver.rs`):**
```rust
// In ElataV2Driver struct
pub struct ElataV2Driver {
    chip_drivers: Vec<Ads1299Driver>,
    gpio: Arc<Gpio>,
    // drdy_pins: Vec<InputPin>, // Will be owned by the dispatcher thread
    status: Arc<Mutex<DriverStatus>>,
    config: AdcConfig,
}

// In ElataV2Driver::new()
pub fn new(config: AdcConfig) -> Result<Self, Box<dyn Error>> {
    // ...
    let gpio = Arc::new(Gpio::new()?);
    // ...
    let mut drdy_txs = Vec::new();
    let mut drdy_rxs = Vec::new();
    for _ in 0..NUM_CHIPS {
        let (tx, rx) = flume::bounded(1);
        drdy_txs.push(tx);
        drdy_rxs.push(rx);
    }

    let mut drdy_rxs_iter = drdy_rxs.into_iter();
    for chip_config in config.chips.iter() {
        let cs_pin = gpio.get(chip_config.cs_pin)?.into_output();
        let drdy_rx = drdy_rxs_iter.next().unwrap();
        let driver = Ads1299Driver::new(/*...,*/ drdy_rx, /*...*/)?;
        chip_drivers.push(driver);
    }
    // ...
}
```

### Step 2: Decouple `Ads1299Driver` from GPIO

**Goal**: Make `Ads1299Driver` unaware of the underlying GPIO hardware.

**Actions**:
- Modify `Ads1299Driver::new()` to accept a `Receiver<()>` for DRDY events.
- Remove all direct GPIO-related code from `Ads1299Driver`.
- Update the acquisition loop to block on the channel receiver.

**Example Code (`crates/sensors/src/ads1299/driver.rs`):**
```rust
// In Ads1299Driver struct
pub struct Ads1299Driver {
    // ...
    drdy_rx: Receiver<()>,
}

// In Ads1299Driver::new()
pub fn new(
    config: ChipConfig,
    bus: Arc<SpiBus>,
    cs_pin: OutputPin,
    drdy_rx: Receiver<()>, // New parameter
    sensor_meta: Arc<SensorMeta>,
) -> Result<Self, DriverError> {
    // ...
}

// In Ads1299Driver::acquire_raw()
pub fn acquire_raw(
    &mut self,
    tx: Sender<(u8, PacketOwned)>,
    stop_flag: &Arc<AtomicBool>,
    chip_id: u8,
    // drdy_rx is now a struct field
) -> Result<(), SensorError> {
    // ...
    while !stop_flag.load(Ordering::Relaxed) {
        match self.drdy_rx.recv() { // Block on the channel
            Ok(_) => {
                self.read_frame(&mut buffer)?;
                // ... process and send packet
            }
            Err(_) => {
                // Dispatcher has shut down
                break;
            }
        }
    }
    // ...
}
```

### Step 3: Implement the Centralized DRDY Dispatcher

**Goal**: Create a single, reliable thread to handle all hardware interrupts.

**Actions**:
- In `ElataV2Driver::acquire()`, spawn a new thread dedicated to polling.
- This thread will own the `InputPin` handles.
- It will use `gpio.poll_interrupts()` to wait for an edge on any DRDY pin.
- When an interrupt fires, it will send a message on the corresponding chip's channel.

**Example Code (`crates/boards/src/elata_v2/driver.rs`):**
```rust
// In ElataV2Driver::acquire()
fn acquire(
    &mut self,
    tx: Sender<BridgeMsg>,
    stop_flag: &AtomicBool,
) -> Result<(), SensorError> {
    // ...
    // --- Spawn DRDY interrupt dispatcher thread ---
    let dispatcher_stop_flag = thread_stop_flag.clone();
    let mut drdy_pins: Vec<InputPin> = self.config.chips.iter()
        .map(|c| self.gpio.get(c.drdy_pin).unwrap().into_input())
        .collect();

    // THIS IS THE CRITICAL MISSING STEP:
    for pin in &mut drdy_pins {
        pin.set_async_interrupt(rppal::gpio::Trigger::FallingEdge)
            .map_err(|e| SensorError::DriverError(e.to_string()))?;
    }

    let gpio_clone = self.gpio.clone();
    let drdy_txs_clone = drdy_txs.clone(); // Assuming drdy_txs is available
    let dispatcher_handle = thread::Builder::new()
        .name("drdy_dispatcher".to_string())
        .spawn(move || {
            let poll_timeout = Duration::from_millis(200);
            let drdy_pins_refs: Vec<&InputPin> = drdy_pins.iter().collect();
            while !dispatcher_stop_flag.load(Ordering::Relaxed) {
                match gpio_clone.poll_interrupts(&drdy_pins_refs, true, Some(poll_timeout)) {
                    Ok(Some((pin, _level))) => {
                        if let Some(chip_index) = drdy_pins.iter().position(|p| p.pin() == pin.pin()) {
                            if drdy_txs_clone[chip_index].send(()).is_err() {
                                break; // Channel closed
                            }
                        }
                    }
                    // ... error handling
                }
            }
        }).unwrap();
    // ...
}
```

### Step 4: Ensure Correct Reset Timing

**Goal**: Give the ADS1299 chips enough time to stabilize after a reset.

**Actions**:
- In `ElataV2Driver::initialize()`, ensure the `thread::sleep` duration after the `CMD_RESET` command is at least `10ms`.

**Example Code (`crates/boards/src/elata_v2/driver.rs`):**
```rust
// In ElataV2Driver::initialize()
fn initialize(&mut self) -> Result<(), DriverError> {
    // ...
    for chip in self.chip_drivers.iter_mut() {
        chip.send_command(CMD_RESET)?;
    }
    // Wait for reset to complete
    thread::sleep(Duration::from_millis(10)); // Ensure this is 10ms
    // ...
}