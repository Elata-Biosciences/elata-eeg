use rppal::spi::{Spi, Bus, SlaveSelect, Mode};
use rppal::gpio::{Gpio, InputPin, Level};
use std::error::Error;
use std::thread::sleep;
use std::time::Duration;

// Register addresses
const ID_REG_ADDR: u8 = 0x00;
const CONFIG1_ADDR: u8 = 0x01;
const CONFIG2_ADDR: u8 = 0x02;
const CONFIG3_ADDR: u8 = 0x03;
const CONFIG4_ADDR: u8 = 0x04;
const CH1SET_ADDR: u8 = 0x05;
const CH2SET_ADDR: u8 = 0x06;
const LOFF_SENSP_ADDR: u8 = 0x0F;
const MISC1_ADDR: u8 = 0x15;

// Register base values
const CONFIG1: u8 = 0x90;        // Base configuration
const DR_250SPS: u8 = 0x06;      // 250 SPS data rate
const CONFIG2: u8 = 0xD3;        // Internal reference, test signal enabled
const CONFIG3: u8 = 0x60;        // Required base value
const CONFIG4: u8 = 0x00;        // Disable lead-off comparators
const MISC1: u8 = 0x00;          // Base value
const LOFF_SENSP: u8 = 0x0;

// Channel settings
const CHNSET: u8 = 0;
const POWER_DOWN: u8 = 1 << 7;
const GAIN_1: u8 = 0 << 4;
const GAIN_8: u8 = 4 << 4;
const MUX_NORMAL: u8 = 0;        // Normal electrode input
const MUX_INPUT_SHORTED: u8 = 1; // Debugging
const MUX_BIAS_MEASURE: u8 = 2;  // BIAS measurements
const MUX_MVDD_SUPPLY: u8 = 3;   // Debugging
const MUX_TEMP_SENSOR: u8 = 4;   // Debugging
const MUX_TEST_SIGNAL: u8 = 5;   // Debugging
const MUX_BIAS_DRP: u8 = 6;      // BIAS_DRP (positive electrode is the driver)
const MUX_BIAS_DRN: u8 = 7;      // BIAS_DRN (negative electrode is the driver)

// Bias settings
const PD_REFBUF_ON: u8 = 1 << 7;  // bit 7
const BIAS_MEAS_ON: u8 = 1 << 4;
const BIASREF_INT_ON: u8 = 1 << 3;
const PD_BIAS_ON: u8 = 1 << 2;
const BIAS_LOFF_SENS_ON: u8 = 1 << 1;
const BIASSTAT_ON: u8 = 1 << 0;
const SRB1_ON: u8 = 1 << 5;

// SPI commands
const RREG_CMD: u8 = 0x20;    // Read register command
const WREG_CMD: u8 = 0x40;    // Write register command
const RESET_CMD: u8 = 0x06;   // Reset command
const SDATAC_CMD: u8 = 0x11;  // Stop data continuous mode
const START_CMD: u8 = 0x08;   // Start conversion
const RDATAC_CMD: u8 = 0x10;  // Read data continuous mode
const STOP_CMD: u8 = 0x0A;    // Stop conversion

// Status byte bits (first byte in data frame)
const STATUS_BYTE_DRDY_ALL: u8 = 0x80;  // DRDY bit for all channels (1 = not ready, 0 = data ready)
const STATUS_BYTE_LOFF_STATP: u8 = 0x00; // Lead-off detection positive side status (disabled)
const STATUS_BYTE_LOFF_STATN: u8 = 0x20; // Lead-off detection negative side status
const STATUS_BYTE_GPIO_BITS: u8 = 0x0F;  // GPIO data bits [3:0]

// Computed register values
const CH1_REG: u8 = CHNSET | GAIN_1 | MUX_NORMAL;
const CH2_REG: u8 = CHNSET | GAIN_1 | MUX_NORMAL;
const POWER_DOWN_CHANNEL: u8 = CHNSET | POWER_DOWN;
const CONFIG1_REG: u8 = CONFIG1 | DR_250SPS;
const CONFIG2_REG: u8 = CONFIG2;  // Already contains all needed bits
const CONFIG3_REG: u8 = CONFIG3 | PD_REFBUF_ON | PD_BIAS_ON | BIASREF_INT_ON;//CONFIG3 | BIAS_MEAS_ON | BIASREF_INT_ON;
const CONFIG4_REG: u8 = CONFIG4;  // Already contains all needed bits
const MISC1_REG: u8 = MISC1 | SRB1_ON;
const LOFF_SENSP_REG: u8 = LOFF_SENSP & 0x0;


fn main() -> Result<(), Box<dyn Error>> {
    read_channel_data_test()?;
    id_register_test()
}

/// Convert 24-bit SPI data to a signed 32-bit integer (sign-extended)
fn ch_sample_to_raw(msb: u8, mid: u8, lsb: u8) -> i32 {
    let raw_value = ((msb as u32) << 16) | ((mid as u32) << 8) | (lsb as u32);
    ((raw_value as i32) << 8) >> 8
}

/// Convert signed raw ADC value to voltage using VREF and gain
/// Formula: voltage = (raw * (VREF / Gain)) / 2^23
fn ch_raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
    ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32
}

/// Helper function to write to a register
fn write_register(spi: &mut Spi, reg_addr: u8, val: u8) -> Result<(), Box<dyn Error>> {
    spi.write(&[WREG_CMD | reg_addr, 0x00, val])?;
    Ok(())
}

/// Helper function to read from a register
fn read_register(spi: &mut Spi, reg_addr: u8) -> Result<u8, Box<dyn Error>> {
    let mut buffer = [0u8];
    spi.write(&[RREG_CMD | reg_addr, 0x00])?;
    spi.transfer(&mut buffer, &[0u8])?;
    Ok(buffer[0])
}

fn verify_register(spi: &mut Spi, name: &str, reg_addr: u8, exp: u8) -> Result<(), Box<dyn Error>>  {
    let act = read_register(spi, reg_addr)?;
    if (act != exp) {
        println!("!!!!REGISTER MISMATCH: {}=0x{:02X}, expected 0x{:02X}", name, act, exp);
    }
    Ok(())
}

/// Test to verify the ADS1299 ID register
fn id_register_test() -> Result<(), Box<dyn Error>> {
    println!("Starting minimal ADS1299 ID register test...");
    
    // Initialize SPI with the same settings as the working Python script
    let mut spi = Spi::new(
        Bus::Spi0,
        SlaveSelect::Ss0,
        500_000,  // 500kHz
        Mode::Mode1  // CPOL=0, CPHA=1
    )?;
    
    println!("SPI initialized");
    
    // Send RESET command
    spi.write(&[RESET_CMD])?;
    println!("RESET command sent");
    
    // Send zeros (as in the Python script)
    spi.write(&[0x00, 0x00, 0x00])?;
    println!("Zeros sent");
    
    // Send SDATAC command
    spi.write(&[SDATAC_CMD])?;
    println!("SDATAC command sent");
    
    // Read ID register
    let id_value = read_register(&mut spi, ID_REG_ADDR)?;
    println!("ID register value: 0x{:02X}", id_value);
    
    if id_value == 0x3E {
        println!("SUCCESS: Correct ID value (0x3E) read from ADS1299!");
    } else {
        println!("ERROR: Incorrect ID value. Expected 0x3E, got 0x{:02X}", id_value);
    }
    
    Ok(())
}

/// Test to read channel data from the ADS1299
fn read_channel_data_test() -> Result<(), Box<dyn Error>> {
    println!("Starting ADS1299 channel data reading test...");
    
    // Initialize SPI
    let mut spi = Spi::new(
        Bus::Spi0,
        SlaveSelect::Ss0,
        500_000,  // 500kHz
        Mode::Mode1  // CPOL=0, CPHA=1
    )?;
    
    // Initialize DRDY pin (GPIO25)
    let gpio = Gpio::new()?;
    let drdy_pin = gpio.get(25)?.into_input_pullup();
    
    println!("SPI and GPIO initialized");
    
    // Power-up sequence
    // 1. Send RESET command
    spi.write(&[RESET_CMD])?;
    sleep(Duration::from_millis(10));
    
    // 2. Send zeros
    spi.write(&[0x00, 0x00, 0x00])?;
    sleep(Duration::from_millis(10));
    
    // 3. Send SDATAC command
    spi.write(&[SDATAC_CMD])?;
    sleep(Duration::from_millis(10));
    
    // 4. Check device ID
    let id_value = read_register(&mut spi, ID_REG_ADDR)?;
    
    if id_value != 0x3E {
        return Err(format!("Invalid device ID: 0x{:02X}, expected 0x3E", id_value).into());
    }
    println!("Device ID verified: 0x3E");
    
    // 5. Configure registers
    sleep(Duration::from_millis(100));

    println!("Configuring ADS1299 registers...");
    write_register(&mut spi, CONFIG1_ADDR, CONFIG1_REG)?;
    write_register(&mut spi, CONFIG2_ADDR, CONFIG2_REG)?;
    write_register(&mut spi, CONFIG3_ADDR, CONFIG3_REG)?;
    write_register(&mut spi, CONFIG4_ADDR, CONFIG4_REG)?;
    write_register(&mut spi, LOFF_SENSP_ADDR, LOFF_SENSP_REG)?;
    write_register(&mut spi, MISC1_ADDR, MISC1_REG)?;
    write_register(&mut spi, CH1SET_ADDR, CH1_REG)?;
    write_register(&mut spi, CH2SET_ADDR, CH2_REG)?;
    for ch in 3..=8 {
        write_register(&mut spi, CH1SET_ADDR + (ch - 1), POWER_DOWN_CHANNEL)?;
    }
    const BIAS_SENSP_ADDR: u8 = 0xd;
    const BIAS_SENSN_ADDR: u8 = 0xe;
    write_register(&mut spi, BIAS_SENSP_ADDR, 3);
    write_register(&mut spi, BIAS_SENSN_ADDR, 3);

    verify_register(&mut spi, "CONFIG1", CONFIG1_ADDR, CONFIG1_REG);
    verify_register(&mut spi, "CONFIG2", CONFIG2_ADDR, CONFIG2_REG);
    verify_register(&mut spi, "CONFIG3", CONFIG3_ADDR, CONFIG3_REG);
    verify_register(&mut spi, "CH1", CH1SET_ADDR, CH1_REG);
    verify_register(&mut spi, "CH2", CH2SET_ADDR, CH2_REG);
    verify_register(&mut spi, "CONFIG4", CONFIG4_ADDR, CONFIG4_REG);
    verify_register(&mut spi, "MISC1", MISC1_ADDR, MISC1_REG);


    println!("----Register Dump----");
    let names = ["ID", "CONFIG1", "CONFIG2", "CONFIG3", "LOFF", "CH1SET", "CH2SET", "CH3SET", "CH4SET", "CH5SET", "CH6SET", "CH7SET", "CH8SET", "BIAS_SENSP", "BIAS_SENSN", "LOFF_SENSP", "LOFF_SENSN", "LOFF_FLIP", "LOFF_STATP", "LOFF_STATN", "GPIO", "MISC1", "MISC2", "CONFIG4"];
    for reg in 0..=0x17 {println!("0x{:02X} - 0x{:02X} {}", reg, read_register(&mut spi, reg as u8)?, names[reg]);}
    println!("----Register Dump----");

    // Add a longer delay after configuration
    println!("Waiting for configuration to settle...");
    sleep(Duration::from_millis(100));
    
    // Make sure we're in SDATAC mode before starting
    spi.write(&[SDATAC_CMD])?;
    sleep(Duration::from_millis(10));
    
    // 6. Start data conversion
    spi.write(&[START_CMD])?;
    sleep(Duration::from_millis(10));
    
    // 7. Enter continuous data mode (RDATAC)
    spi.write(&[RDATAC_CMD])?;
    sleep(Duration::from_millis(10));
    // sleep(Duration::from_millis(10*1000));
    
    println!("Data conversion started in continuous mode");
    
    // 8. Read data samples
    println!("Reading 20 samples from channels...");
    for i in 0..3 {
        // Wait for DRDY to go low
        let mut timeout = 1000;
        let drdy_start = std::time::Instant::now();
        let initial_drdy_state = drdy_pin.read();
        println!("Initial DRDY state: {:?}", initial_drdy_state);
        
        while drdy_pin.read() == Level::High && timeout > 0 {
            sleep(Duration::from_micros(10));
            timeout -= 1;
        }
        
        let drdy_duration = drdy_start.elapsed();
        
        if timeout == 0 {
            println!("DRDY timeout - pin never went low");
            continue;
        } else {
            println!("DRDY went low after {:?} (timeout count: {})", drdy_duration, 1000 - timeout);
        }
        
        // Read data directly without sending commands each time
        // 3 status bytes + (3 bytes per channel * 2 channels) = 9 bytes
        let mut read_buffer = [0u8; 9];
        let write_buffer = [0u8; 9];
        
        println!("Reading data frame directly...");
        match spi.transfer(&mut read_buffer, &write_buffer) {
            Ok(_) => println!("SPI transfer successful"),
            Err(e) => println!("SPI transfer error: {}", e),
        }
        
        println!("All bytes: [{:02X}, {:02X}, {:02X}, {:02X}, {:02X}, {:02X}, {:02X}, {:02X}, {:02X}]",
                 read_buffer[0], read_buffer[1], read_buffer[2],
                 read_buffer[3], read_buffer[4], read_buffer[5],
                 read_buffer[6], read_buffer[7], read_buffer[8]);
        
        // Status byte interpretation:
        // For example, if status byte is 0xC0 = 1100 0000 in binary, which means:
        // - Bit 7 (0x80) is set: DRDY_ALL = 1, indicating not all channels are ready
        // - Bit 6 (0x40) is set: LOFF_STATP = 1, indicating lead-off detected on positive side
        // - Bits 5-0 are clear: No other status flags are active
        println!("Status byte: 0x{:02X}", read_buffer[0]);
        
        // First 3 are status bytes
        let raw_value = ch_sample_to_raw(read_buffer[3], read_buffer[4], read_buffer[5]);
        let voltage = ch_raw_to_voltage(raw_value, 4.5, 1.0);
        // let ch2_value = ch_spi_data_to_i32(read_buffer[6], read_buffer[7], read_buffer[8]);
        
        println!("Sample {}: Channel 1 raw bytes [{:02X} {:02X} {:02X}] = {}, v={}",
                 i, read_buffer[3], read_buffer[4], read_buffer[5], raw_value, voltage);
        // println!("Sample {}: Channel 2 raw bytes [{:02X} {:02X} {:02X}] = {}",
        //          i, read_buffer[6], read_buffer[7], read_buffer[8], ch2_value);

    }
    
    // 9. Exit continuous data mode (SDATAC)
    spi.write(&[SDATAC_CMD])?;
    sleep(Duration::from_millis(10));
    
    // 10. Stop data conversion
    spi.write(&[STOP_CMD])?;
    println!("Data conversion stopped");
    
    Ok(())
}