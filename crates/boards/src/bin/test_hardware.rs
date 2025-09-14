/*!
# Hardware Test Utility for EEG Boards

This program performs comprehensive hardware testing for EEG acquisition boards using the ADS1299 family of chips.

## Purpose
The test_hardware utility verifies that:
- GPIO pins are accessible and functioning
- SPI communication works with the ADS1299 chip
- The ADS1299 chip can be detected and identified
- DRDY (Data Ready) pin shows activity
- Power and reset control pins work

## Hardware Requirements
- Raspberry Pi with GPIO and SPI interfaces
- ADS1299-based EEG board connected via SPI
- Proper pin connections:
  - DRDY: GPIO 25
  - CS: GPIO 8 (primary) or GPIO 7 (alternative)
  - SPI: Bus 0 (MOSI, MISO, SCLK on GPIO 10, 9, 11)

## Running the Test

### Prerequisites
Ensure you have the necessary permissions to access GPIO and SPI:
```bash
# Add user to gpio and spi groups
sudo usermod -a -G gpio,spi $USER

# Or run with sudo (not recommended for development)
sudo ./test_hardware
```

### Build and Run
```bash
# From the project root
cd crates/boards
cargo build --bin test_hardware
cargo run --bin test_hardware

# Or from project root
cargo run --bin test_hardware
```

### Expected Output
The test will output detailed information about:
- GPIO pin states
- SPI communication attempts
- ADS1299 chip detection (looks for ID 0x3E for ADS1299)
- DRDY pin monitoring for 10 seconds

### Troubleshooting
- **Permission denied**: Run with `sudo` or add user to gpio/spi groups
- **No ADS1299 detected**: Check SPI wiring and power connections
- **DRDY never changes**: Hardware may not be powered or configured correctly
- **SPI errors**: Verify SPI bus and chip select pin connections

## Test Sequence
1. **GPIO Access Test**: Verifies GPIO pins can be controlled
2. **SPI Communication Test**: Tests different SPI speeds and modes
3. **ADS1299 Detection**: Attempts to read chip ID register
4. **Power/Reset Test**: Activates control pins and re-tests SPI
5. **DRDY Monitoring**: Watches for data ready signal activity

## Exit Codes
- 0: Test completed successfully
- Non-zero: Hardware or communication error encountered
*/

use rppal::gpio::Gpio;
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use std::time::Duration;
use std::thread;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== EEG Hardware Test ===");
    
    // Test 1: GPIO Access
    println!("1. Testing GPIO access...");
    let gpio = Gpio::new()?;
    
    // Test DRDY pin (GPIO25)
    let drdy_pin = gpio.get(25)?.into_input_pullup();
    println!("   DRDY pin 25 state: {}", 
             if drdy_pin.is_high() { "HIGH" } else { "LOW" });
    
    // Test CS pin (GPIO8) - try different CS pins
    println!("   Testing CS pins...");
    let mut cs_pin8 = gpio.get(8)?.into_output_high();
    println!("   CS pin 8 set to HIGH");
    
    // Also try GPIO7 (CE1) as alternative CS
    let mut cs_pin7 = gpio.get(7)?.into_output_high();
    println!("   CS pin 7 set to HIGH");
    
    // Test 2: SPI Communication with detailed debugging
    println!("2. Testing SPI communication...");
    
    // Try different SPI configurations
    let spi_configs = [
        (1_000_000, Mode::Mode1, "1MHz Mode1"),
        (500_000, Mode::Mode1, "500kHz Mode1"),
        (1_000_000, Mode::Mode0, "1MHz Mode0"),
        (2_000_000, Mode::Mode1, "2MHz Mode1"),
    ];
    
    for (speed, mode, desc) in spi_configs {
        println!("   Trying SPI config: {}", desc);
        let mut spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, speed, mode)?;
        
        // Test with GPIO8 as CS
        println!("     Using GPIO8 as CS:");
        test_ads1299_id(&mut spi, &mut cs_pin8)?;
        
        // Test with GPIO7 as CS
        println!("     Using GPIO7 as CS:");
        test_ads1299_id(&mut spi, &mut cs_pin7)?;
        
        // Test with hardware CS (no manual CS control)
        println!("     Using hardware CS:");
        test_ads1299_id_hw_cs(&mut spi)?;
    }
    
    // Test 3: Try to activate power and reset pins
    println!("3. Attempting to activate power and reset pins...");
    
    // Try setting potential power/reset pins HIGH
    let power_reset_pins = [
        (17, "GPIO17 (RESET)"),
        (18, "GPIO18 (PWDN)"), 
        (23, "GPIO23 (RESET)"),
        (24, "GPIO24 (PWDN)"),
        (27, "GPIO27 (START)"),
    ];
    
    let mut control_pins = Vec::new();
    
    for (pin_num, desc) in power_reset_pins {
        match gpio.get(pin_num) {
            Ok(pin) => {
                let mut output_pin = pin.into_output_high();
                println!("   {} set to HIGH", desc);
                control_pins.push(output_pin);
            }
            Err(e) => println!("   {} - Error: {}", desc, e),
        }
    }
    
    // Wait for chip to initialize
    println!("   Waiting 100ms for chip initialization...");
    thread::sleep(Duration::from_millis(100));
    
    // Test SPI again after power-up
    println!("4. Re-testing SPI after power-up...");
    let mut spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 1_000_000, Mode::Mode1)?;
    
    println!("   Using GPIO8 as CS:");
    test_ads1299_id(&mut spi, &mut cs_pin8)?;
    
    println!("   Using hardware CS:");
    test_ads1299_id_hw_cs(&mut spi)?;
    
    // Test 4: Monitor DRDY for activity with longer duration
    println!("5. Monitoring DRDY for 10 seconds...");
    let start_state = drdy_pin.is_high();
    let mut changes = 0;
    let mut last_state = start_state;
    
    for i in 0..100 {
        let current_state = drdy_pin.is_high();
        if current_state != last_state {
            changes += 1;
            println!("   DRDY changed to {} at {}ms", 
                     if current_state { "HIGH" } else { "LOW" }, i * 100);
            last_state = current_state;
        }
        
        if i % 20 == 0 {
            println!("   DRDY: {} (total changes: {})", 
                     if current_state { "HIGH" } else { "LOW" }, changes);
        }
        
        thread::sleep(Duration::from_millis(100));
    }
    
    if changes > 0 {
        println!("   ✅ DRDY is active - hardware is working");
    } else {
        println!("   ❌ DRDY never changed - hardware not responding");
    }
    
    Ok(())
}

fn test_ads1299_id(spi: &mut Spi, cs_pin: &mut rppal::gpio::OutputPin) -> Result<(), Box<dyn std::error::Error>> {
    // Proper ADS1299 initialization sequence
    println!("       Sending RESET command...");
    cs_pin.set_low();
    thread::sleep(Duration::from_micros(10));
    spi.write(&[0x06])?; // RESET command
    thread::sleep(Duration::from_micros(10));
    cs_pin.set_high();
    thread::sleep(Duration::from_millis(10)); // Wait for reset
    
    println!("       Sending SDATAC command...");
    cs_pin.set_low();
    thread::sleep(Duration::from_micros(10));
    spi.write(&[0x11])?; // SDATAC command (stop data continuous)
    thread::sleep(Duration::from_micros(10));
    cs_pin.set_high();
    thread::sleep(Duration::from_millis(1));
    
    // Now try to read ID register
    println!("       Reading ID register...");
    let tx_buf = [0x20, 0x00]; // RREG ID command
    let mut rx_buf = [0x00, 0x00];
    
    cs_pin.set_low();
    thread::sleep(Duration::from_micros(10));
    spi.transfer(&mut rx_buf, &tx_buf)?;
    thread::sleep(Duration::from_micros(10));
    cs_pin.set_high();
    
    println!("       SPI Response: 0x{:02X} 0x{:02X}", rx_buf[0], rx_buf[1]);
    
    if rx_buf[1] == 0x3E {
        println!("       ✅ ADS1299 detected!");
    } else if rx_buf[1] == 0x92 {
        println!("       ✅ ADS1298 detected!");
    } else if rx_buf[0] == 0x00 && rx_buf[1] == 0x00 {
        println!("       ❌ No response");
    } else {
        println!("       ❓ Unexpected response - ID: 0x{:02X}", rx_buf[1]);
    }
    
    Ok(())
}

fn test_ads1299_id_hw_cs(spi: &mut Spi) -> Result<(), Box<dyn std::error::Error>> {
    // Send RESET command
    spi.write(&[0x06])?;
    thread::sleep(Duration::from_millis(18)); // ADS1299 needs 18 tCLK after reset
    
    // Send SDATAC command  
    spi.write(&[0x11])?;
    thread::sleep(Duration::from_millis(1));
    
    // Read ID register - for ADS1299EEG-FE, we need 3 bytes total
    let tx_buf = [0x20, 0x00, 0x00]; // RREG ID, 1 byte, dummy
    let mut rx_buf = [0x00, 0x00, 0x00];
    
    spi.transfer(&mut rx_buf, &tx_buf)?;
    
    println!("       SPI Response: 0x{:02X} 0x{:02X} 0x{:02X}", rx_buf[0], rx_buf[1], rx_buf[2]);
    
    // Check all positions for the device ID
    for (i, &byte) in rx_buf.iter().enumerate() {
        if byte == 0x3E {
            println!("       ✅ ADS1299 detected at position {}!", i);
            return Ok(());
        }
    }
    
    println!("       ❓ Unexpected response - no 0x3E found");
    Ok(())
}
