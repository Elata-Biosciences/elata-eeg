# ADS1299EEG_FE Board Driver

This document outlines the configuration and setup of the [TI ADS1299 EVM](https://www.ti.com/tool/ADS1299EEGFE-PDK) board for integration with the Elata EEG system. It provides the necessary steps to initialize the board, configure the driver, and bring up the hardware for data acquisition.

The setup uses a Raspberry Pi 5 connected to the ADS1299 EVM, serving as the main controller for EEG signal acquisition. Custom drivers on the Pi handle SPI communication, device configuration, and system initialization, forming the core of the Elata EEG hardware stack.

```
[EEG Electrodes]
       │
       ▼
[ADS1299 EVM Board]
       │   (SPI + GPIO)
       ▼
[Raspberry Pi 5]
       │
       ▼
[UI / Logging / Processing]
```
## ADS1299 Configuration Summary
TL;DR: The ADS1299 board is configured for basic EEG acquisition using a unipolar 5V supply, with a buffered reference electrode (SRB1) and a fixed internal bias drive to stabilize signals and reduce common-mode noise. For the prototype, we aren't using closed-loop bias or shield driving.

| Feature / Setting          | Used? | Hardware (Jumpers)                    | Software (Register Flag) | Purpose (Basic Explanation)                                                                                                |
| :------------------------- | :---- | :------------------------------------ | :----------------------- | :------------------------------------------------------------------------------------------------------------------------- |
| **Power**                  |       |                                       |                          |                                                                                                                            |
| 5V Unipolar Power          | ✅ Yes | JP2 [2–3] shorted,<br>JP20 [1–2] shorted | –                        | Powers the chip using a single +5V supply relative to Ground (0V). Ensures stable power is crucial for clean signals.         |
| **Reference Signal Setup** |       |                                       |                          | *How the 'zero point' for measurements is established and shared.*                                                          |
| Reference Electrode        | ✅ Yes | JP25 Pin 6 connected                  | `MISC1.SRB1 = 1`         | The main 'negative' or 'zero volt' point that all active EEG electrodes are measured against.                               |
| SRB1 Usage                 | ✅ Yes | –                                     | `MISC1.SRB1 = 1`         | Connects the negative input of all measurement channels internally to the single Reference Electrode pin (SRB1).              |
| Reference Buffer           | ✅ Yes | JP7 [1–2] shorted,<br>JP8 [2–3] shorted | `CONFIG3.PD_REFBUF = 1`  | Powers an internal amplifier (buffer) for the reference signal. Prevents the signal from weakening when used by many channels. |
| *Ref Unbuffered*           | ❌ No  | *JP8 [1–2], JP7 open*                 | *–*                      | *(Alternative)* Uses the reference signal directly without the internal buffer. Not used here to ensure stability.         |
| **Bias Circuit Setup**     |       |                                       |                          | *How the system actively counteracts noise and keeps signals centered.*                                                    |
| Bias Electrode Output      | ✅ Yes | JP25 Pin 4 connected                  | `CONFIG3.PD_BIAS = 1`    | Pin where the bias signal is sent out (usually connected to a dedicated bias electrode on the subject).                    |
| Bias Drive (BIAS_DRV)      | ✅ Yes | JP1 [1–2] shorted                     | `CONFIG3.PD_BIAS = 1`    | Connects the internal bias amplifier output to the Bias Electrode pin.                                                     |
| Bias Amplifier & Buffer    | ✅ Yes | –                                     | `CONFIG3.PD_BIAS = 1`    | Enables the internal circuitry that generates and strengthens the bias signal.                                           |
| Fixed Bias Reference       | ✅ Yes | –                                     | `CONFIG3.BIASREF_INT = 1`| Tells the bias circuit to use a fixed internal voltage (usually half the supply voltage) as its target. Simple and stable. |
| *Closed-Loop Bias*         | ❌ No  | *–*                                   | *BIAS_SENSP/N != 0x00*   | *(Alternative)* Adjusts bias based on measured signals for potentially better noise reduction, but more complex. Not used. |
| *Bias UnBuffered*          | ❌ No  | *–*                                   | *–*                      | *(Alternative)* Disables the internal bias buffer (if using an external one). Not used here.                             |
| **Shield Drive**           |       |                                       |                          |                                                                                                                            |
| BIAS_SHD (Shield)        | ❌ No  | JP17 open                             | –                        | An optional output mirroring the bias signal, used to drive cable shields for extra noise immunity. Not used here.       |

## Bring Up

### Jumper Configuration

1) Jumpers Factory Settings (see table 1 https://www.ti.com/lit/ug/slau443b/slau443b.pdf)
2) JP8 - 2/3
3) JP23 - 2/3  (TODO, is CLKSEL=1 necessary?)
4) JP25 - Open
4) JP4 - Open (connects to VCC 5V)
4) JP24 - Open (connects to VCC 3.3V)

### Voltage Values
 - avdd, avdd1 = 5v
 - dvdd = 3v
 - avss = gnd?
 - gnd

### Pi 5 and ADS1299 Board Connection

The ADS1299EEG_FE board connects to the Raspberry Pi 5 via SPI with the following pinout:


| Power Line | ADS1299 Pin | Raspberry Pi 5 Pin |
|------------|------------------|-------------------|
| DVDD 5V | JP4, pin next to "JP4" text | Pin 2 |
| DVDD 3V | JP24, middle pin  | Pin 1 |
| Ground | JP5, pin next to arrow (ground) symbol | Pin 6 |


| SPI Signal | ADS1299 Pin (J3) | Raspberry Pi 5 Pin |
|------------|------------------|-------------------|
| CS (Chip Select) | Pin 1 | Pin 24 (CE0) |
| SCLK (Clock) | Pin 3 | Pin 23 |
| MOSI (DIN) | Pin 11 | Pin 19 |
| MISO (DOUT) | Pin 13 | Pin 21 |
| DRDYB (Data Ready) | Pin 15 | Pin 22 (GPIO25) |

### Channel 1 Electrode Connection
- Bias Electrode -> JP25 Pin 4
- Reference Electrode -> JP25 Pin 6
- Channel 1 Electrode -> CH1 Positive

### Register Map Reference (Channel 1 on, Gain=24)
| Address | Value | Register Name          | Description                                      |
|---------|--------|------------------------|--------------------------------------------------|
| 0x00    | 0x3E   | ID                     | Chip ID (Read-only)                              |
| 0x01    | 0x96   | CONFIG1                | Configuration Register 1                         |
| 0x02    | 0xD3   | CONFIG2                | Configuration Register 2                         |
| 0x03    | 0xEC   | CONFIG3                | Configuration Register 3                         |
| 0x04    | 0x00   | LOFF                   | Lead-Off Control Register                        |
| 0x05    | 0x00   | CH1SET                 | Channel 1 Settings                               |
| 0x06    | 0x81   | CH2SET                 | Channel 2 Settings                               |
| 0x07    | 0x81   | CH3SET                 | Channel 3 Settings                               |
| 0x08    | 0x81   | CH4SET                 | Channel 4 Settings                               |
| 0x09    | 0x81   | CH5SET                 | Channel 5 Settings                               |
| 0x0A    | 0x81   | CH6SET                 | Channel 6 Settings                               |
| 0x0B    | 0x81   | CH7SET                 | Channel 7 Settings                               |
| 0x0C    | 0x81   | CH8SET                 | Channel 8 Settings                               |
| 0x0D    | 0x00   | BIAS_SENSP             | Bias Drive Positive Derivation Register          |
| 0x0E    | 0x00   | BIAS_SENSN             | Bias Drive Negative Derivation Register          |
| 0x0F    | 0x00   | LOFF_SENSP             | Positive Lead-Off Detection Register             |
| 0x10    | 0x00   | LOFF_SENSN             | Negative Lead-Off Detection Register             |
| 0x11    | 0x00   | LOFF_FLIP              | Lead-Off Flip Register                           |
| 0x12    | 0x00   | LOFF_STATP             | Lead-Off Positive Status Register (Read-only)    |
| 0x13    | 0x00   | LOFF_STATN             | Lead-Off Negative Status Register (Read-only)    |
| 0x14    | 0x0F   | GPIO                   | General-Purpose I/O Register                     |
| 0x15    | 0x20   | MISC1                  | Miscellaneous 1 Register                         |
| 0x16    | 0x00   | MISC2                  | Miscellaneous 2 Register                         |
| 0x17    | 0x00   | CONFIG4                | Configuration Register 4                         |


## Implementation Notes

- The ADS1299 communicates via SPI with the following settings:
  - Mode 1 (CPOL=0, CPHA=1)
  - Maximum clock speed of 5MHz (we use 4MHz to be safe)
  - MSB first
  - 8-bit word size

- The DRDY pin is used to detect when new data is available:
  - It is active low (goes low when data is ready)
  - We use GPIO25 (Pin 22) on the Raspberry Pi 5

- The ADS1299 has a 24-bit ADC, so each sample is 3 bytes
  - We need to convert these to i32 values

- The ADS1299 can operate in continuous or single-shot mode
  - We use continuous mode for EEG applications
  - We use the RDATAC command to start continuous data acquisition

## Usage Example

```rust
use eeg_driver::{AdcConfig, EegSystem, DriverType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create configuration for ADS1299
    let config = AdcConfig {
        sample_rate: 250,
        channels: vec![0, 1, 2, 3],
        gain: 24.0,
        board_driver: DriverType::Ads1299,
        batch_size: 32,
        Vref: 4.5,
        dsp_high_pass_cutoff_hz: 0.1,
        dsp_low_pass_cutoff_hz: 100.0,
    };

    // Initialize the EEG system
    let (mut eeg_system, mut data_rx) = EegSystem::new(config.clone()).await?;
    
    // Start acquisition
    eeg_system.start(config).await?;

    // Process data
    while let Some(processed_data_batch) = data_rx.recv().await {
        println!("Received data with timestamp: {}", processed_data_batch.timestamp);
        // Process your data here
    }

    // Always shut down properly when done
    eeg_system.shutdown().await?;
    
    Ok(())
}
```

