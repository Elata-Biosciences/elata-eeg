# ADS1299EEG_FE Board Driver

This document provides information on setting up and using the ADS1299EEG_FE board with the Elata EEG system.

### ADS1299 Configuration Reference Table

| Help Guide | Used? | Jumpers | Register Flag | Explanation |
|------------|-------|---------|--------------|-------------|
| 5V Unipolar Power | ✅ Yes | JP2 [2–3] shorted, JP20 [1–2] shorted | – | Operating from a single 5 V supply (AVDD = 5 V, AVSS = 0 V). The ADS1299 is powered unipolarly; the "negative" rail is ground (0 V). Make sure all supplies and grounds are stable and well-filtered for low-noise EEG. |
| Reference Electrode | ✅ Yes | JP25 Pin 6 is ref electrode | MISC1.SRB1 = 1 | The single reference electrode to which all "positive" measurement electrodes are compared. Setting SRB1=1 routes this reference to the negative input of all channels (or a subset, depending on your board). |
| Bias Electrode | ✅ Yes | JP25 Pin 4 is bias electrode | CONFIG3.PD_BIAS = 1 | The bias electrode drives a mid-supply voltage (often VDD/2) back into the patient/electrode circuit to suppress common-mode interference and keep input signals within a suitable range. Enabling PD_BIAS=1 powers the internal bias amplifier. |
| BIAS_SHD (Shield) | ❌ No | JP17 open | – | The shield drive signal is the same as the bias signal but is used to drive a cable shield in high-impedance or noisy environments. It reduces capacitive coupling. If not using shielded cables or no need for active shielding, leave it open. |
| BIAS_DRV | ✅ Yes | JP1 [1–2] | CONFIG3.PD_BIAS = 1 | The output of the on-chip bias amplifier (the active drive). This is usually routed to the bias electrode. By driving an active bias, the system better cancels common-mode noise. |
| Fixed Bias | ✅ Yes | – | CONFIG3.BIASREF_INT = 1 | With BIASREF_INT=1, the ADS1299 internally generates a ~mid-supply reference for the bias amplifier. This "fixed" bias is simpler but does not adapt to changing electrode offsets. |
| Closed-Loop Bias | ❌ No | – | BIAS_SENSP/N = 0x00 | A feedback loop that senses actual electrode voltages and adjusts the bias in real time. Improves common-mode rejection (CMR) but can saturate the bias amp if too many channels or high offsets are present. Usually recommended when you have a well-characterized electrode setup and want maximum noise rejection. |
| Ref Buffer | ✅ Yes | JP7 [1–2], JP8 [2–3] | CONFIG3.PD_REFBUF = 1 | Powers the internal reference buffer. This buffer provides a stable, low-impedance reference voltage to the inputs (especially when using SRB1 for many channels). It helps reduce loading and keep the measured signals accurate. |
| Ref Unbuffered | ❌ No | JP8 [1–2], JP7 open | – | If you disable the reference buffer, you save the buffer's noise contribution but risk heavier loading on the external reference. Typically not used in EEG unless you have a robust external reference setup. |
| Reference Voltage Internal | ✅ Yes | – | CONFIG3.BIASREF_INT = 1 | This overlaps conceptually with "Fixed Bias," because BIASREF_INT selects the internal mid-supply as the bias reference. If you also use the ADS1299's built-in 2.4 V reference for ADC conversions (CONFIG3.PD_REFBUF=1 + default reference routing), then the device is fully self-contained with no external reference source. |
| Bias Buffer | ✅ Yes | – | CONFIG3.PD_BIAS = 1 | Enables the on-chip buffer for the bias amplifier (the same setting under BIAS Electrode). Ensuring this is on if you're driving a bias electrode. |
| Bias UnBuffer | ❌ No | – | – | If you prefer not to use the on-chip bias buffer (for instance, if you have an external amplifier driving your bias), you'd disable it. Not used here. |
| SRB1 | ✅ Yes | – | MISC1.SRB1 = 1 | Connects the inverting inputs of all channels to SRB1 (the "reference electrode"). Commonly used in EEG to share one reference. If you're using SRB2 or separate references, you'd handle it differently. |

## Hardware Setup

    /*
DEBUGGING
    P71
    - avdd, avdd1 = 5v
    - dvdd = 3v
    - avss = gnd?
    - gnd
    */
### Connections

The ADS1299EEG_FE board connects to the Raspberry Pi 5 via SPI with the following pinout:

| Power Line | ADS1299 Pin (J4) | Raspberry Pi 5 Pin |
|------------|------------------|-------------------|
| DVDD 5V | Pin JP4.10 | Pin 2 |
| DVDD 3V | Pin JP4.19 | Pin 1 |
| Ground | Pin JP4.5 | Pin 9 |


| SPI Signal | ADS1299 Pin (J3) | Raspberry Pi 5 Pin |
|------------|------------------|-------------------|
| SCLK (Clock) | Pin 3 | Pin 23 |
| MOSI (DIN) | Pin 11 | Pin 19 |
| MISO (DOUT) | Pin 13 | Pin 21 |
| CS (Chip Select) | Pin 1 | Pin 24 (CE0) |
| DRDYB (Data Ready) | Pin 15 | Pin 22 (GPIO25) |
| GND (DGND) | Pin 4, 10, 18 | Pin 6 |

### Board Configuration for Single-Ended Operation

Follow these steps to configure the ADS1299EEG_FE board for single-ended operation:

1. **Return All Jumpers to Factory Defaults**
   - Ensure all jumpers are in their default positions (typically configured for "differential" mode)
   - This includes JP7, JP8, JP6, JP17, etc.

2. **Remove Differential Jumpers on J6**
   - Remove jumpers from pins 5-36 of J6
   - In single-ended mode, you don't use the negative inputs directly

3. **Connect SRB1 to the Negative Inputs**
   - Open pin 1 and pin 2 on JP25
   - Open pin 3 and pin 4 on JP25
   - Short pin 5 and pin 6 on JP25
   - This ties SRB1 to the on-board mid-supply (BIAS_ELEC)
   - The driver will set the SRB1 bit in the MISC1 register to route SRB1 to all negative inputs

4. **Provide Single-Ended Signals on J6**
   - Connect signals to the following pins on J6:
     - Ch1 → pin 36
     - Ch2 → pin 32
     - Ch3 → pin 28
     - Ch4 → pin 24
     - Ch5 → pin 20
     - Ch6 → pin 16
     - Ch7 → pin 12
     - Ch8 → pin 8

5. **Reference and Bias Setup**
   - Reference Electrode (REF_ELEC): Connect to the reference point that defines your "0 V"
   - Bias Electrode (BIAS_ELEC or BIAS_DRV): Connect to the subject to provide mid-supply voltage

6. **Optional: Buffered vs. Unbuffered Reference**
   - Unbuffered: JP8 = (1–2)
   - Buffered: JP7 = (1–2) and JP8 = (2–3)

7. Change JP23 to use internal clock CLKSEL=1... TODO double check this

## Software Configuration

The driver automatically configures the ADS1299 chip based on the provided `AdcConfig` parameters:

```rust
let config = AdcConfig {
    sample_rate: 250,  // Sample rate in Hz (supported: 250, 500, 1000, 2000)
    gain: 24.0,        // Gain setting (supported: 1, 2, 4, 6, 8, 12, 24)
    channels: vec![0, 1, 2, 3, 4, 5, 6, 7],  // Channels to enable
    board_driver: DriverType::Ads1299,
    batch_size: 32,    // Number of samples to collect in a batch
    Vref: 4.5,         // Reference voltage (4.5V for ADS1299)
    dsp_high_pass_cutoff_hz: 0.1,  // High-pass filter cutoff (Hz)
    dsp_low_pass_cutoff_hz: 100.0, // Low-pass filter cutoff (Hz)
};
```

### Register Configuration

The driver sets the following key registers:

1. **CONFIG1**: Controls sample rate and analog-to-digital converter mode
2. **CONFIG2**: Controls test signal configuration
3. **CONFIG3**: Controls bias and reference buffer operation
4. **LOFF**: Controls lead-off detection settings
5. **CHnSET**: Controls channel settings (gain, mux, PGA)
6. **MISC1**: Controls SRB1 routing (set for single-ended operation)

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

## Troubleshooting

### Common Issues

1. **No Data Received**
   - Check physical connections, especially the Data Ready pin
   - Verify SPI bus is properly configured
   - Ensure the board is powered correctly

2. **Noisy Signals**
   - Check that the bias electrode is properly connected
   - Verify reference electrode connection
   - Consider using shielded cables for electrode connections

3. **Incorrect Voltage Readings**
   - Verify the Vref setting matches the actual reference voltage
   - Check gain settings
   - Ensure proper grounding

## Advanced Configuration

For advanced users, the driver exposes methods to directly access and modify ADS1299 registers. Refer to the ADS1299 datasheet for detailed register descriptions.

## ADS1299 Register Map

Here's a brief overview of the key registers used in the driver:

| Register | Address | Description |
|----------|---------|-------------|
| ID       | 0x00    | Chip ID (Read-only) |
| CONFIG1  | 0x01    | Configuration Register 1 |
| CONFIG2  | 0x02    | Configuration Register 2 |
| CONFIG3  | 0x03    | Configuration Register 3 |
| LOFF     | 0x04    | Lead-Off Control Register |
| CH1SET   | 0x05    | Channel 1 Settings |
| CH2SET   | 0x06    | Channel 2 Settings |
| CH3SET   | 0x07    | Channel 3 Settings |
| CH4SET   | 0x08    | Channel 4 Settings |
| CH5SET   | 0x09    | Channel 5 Settings |
| CH6SET   | 0x0A    | Channel 6 Settings |
| CH7SET   | 0x0B    | Channel 7 Settings |
| CH8SET   | 0x0C    | Channel 8 Settings |
| BIAS_SENSP | 0x0D  | Bias Drive Positive Derivation Register |
| BIAS_SENSN | 0x0E  | Bias Drive Negative Derivation Register |
| LOFF_SENSP | 0x0F  | Positive Lead-Off Detection Register |
| LOFF_SENSN | 0x10  | Negative Lead-Off Detection Register |
| LOFF_FLIP  | 0x11  | Lead-Off Flip Register |
| LOFF_STATP | 0x12  | Lead-Off Positive Status Register (Read-only) |
| LOFF_STATN | 0x13  | Lead-Off Negative Status Register (Read-only) |
| GPIO      | 0x14   | General-Purpose I/O Register |
| MISC1     | 0x15   | Miscellaneous 1 Register |
| MISC2     | 0x16   | Miscellaneous 2 Register |
| CONFIG4   | 0x17   | Configuration Register 4 |

### Key Register Settings for Single-Ended Operation

For single-ended operation, the following register settings are crucial:

1. **MISC1 Register (0x15)**
   - Set bit 5 (SRB1) to 1 to connect SRB1 to all negative inputs
   - Default value after setting SRB1: 0x20

2. **CHnSET Registers (0x05-0x0C)**
   - For each enabled channel, set the appropriate gain
   - Set bits 5-4 (MUX) to 0b00 for normal electrode input
   - Example for gain=24: 0x60 (0b01100000)

3. **CONFIG3 Register (0x03)**
   - Set bit 7 (BIAS_STAT) to 1 to enable bias buffer
   - Set bit 3 (BIAS_MEAS) to 0 for normal operation
   - Set bit 2 (BIASREF_INT) to 1 to use internal reference for bias
   - Default value: 0x96 (0b10010110)