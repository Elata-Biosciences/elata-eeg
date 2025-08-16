//! Register definitions and low-level register helpers for the ADS1299 chip.

// ADS1299 Register Value Constants
pub const MUX_NORMAL: u8 = 0 << 0;
pub const PD_REFBUF: u8 = 1 << 7;      // 1 : Enable internal reference buffer
pub const BIAS_MEAS: u8 = 1 << 4;      // 1 : BIAS_IN signal is routed to the channel that has the MUX_Setting 010 (VREF)
pub const BIASREF_INT: u8 = 1 << 3;    // 1 : BIASREF signal (AVDD + AVSS) / 2 generated internally
pub const PD_BIAS: u8 = 1 << 2;        // 1 : BIAS buffer is enabled
pub const BIAS_LOFF_SENS: u8 = 1 << 1; // 1 : BIAS sense is enabled
pub const SRB1: u8 = 1 << 5;           // 1 : Switches closed.. This bit connects the SRB1 to all 4, 6, or 8 channels inverting inputs
pub const DC_TEST: u8 = 3 << 0;
pub const POWER_OFF_CH: u8 = 0x81;
pub const BIAS_SENS_OFF_MASK: u8 = 0x00;
pub const DAISY_DISABLE: u8 = 1 << 6;

// Register Addresses
pub const REG_ID_ADDR    : u8 = 0x00;
pub const CONFIG1_ADDR   : u8 = 0x01; pub const CONFIG1_REG: u8 = 0x90;
pub const CONFIG2_ADDR   : u8 = 0x02; pub const CONFIG2_REG: u8 = 0xC0;
pub const CONFIG3_ADDR   : u8 = 0x03; pub const CONFIG3_REG: u8 = 0x60;
pub const LOFF_ADDR      : u8 = 0x04;
pub const CH1SET_ADDR    : u8 = 0x05; pub const CHN_REG    : u8 = 0x00;
                                      pub const CHN_OFF    : u8 = 0x00 | POWER_OFF_CH;
pub const BIAS_SENSP_ADDR: u8 = 0x0D; pub const BIAS_SENSP_REG : u8 = BIAS_SENS_OFF_MASK;
pub const BIAS_SENSN_ADDR: u8 = 0x0E; pub const BIAS_SENSN_REG : u8 = 0x00;
pub const LOFF_SENSP_ADDR: u8 = 0x0F; pub const LOFF_SESP_REG: u8 = 0x00;
pub const MISC1_ADDR     : u8 = 0x15; pub const MISC1_REG   : u8 = 0x00;
pub const CONFIG4_ADDR   : u8 = 0x17; pub const CONFIG4_REG : u8 = 0x00;

// ADS1299 Commands
pub const CMD_WAKEUP: u8 = 0x02;
pub const CMD_STANDBY: u8 = 0x04;
pub const CMD_RESET: u8 = 0x06;
pub const CMD_START: u8 = 0x08;
pub const CMD_STOP: u8 = 0x0A;
pub const CMD_RDATAC: u8 = 0x10;
pub const CMD_SDATAC: u8 = 0x11;
pub const CMD_RDATA: u8 = 0x12;
pub const CMD_NOP: u8 = 0x00;

/// Convert gain value to register mask.
pub fn gain_to_reg_mask(gain: f32) -> Result<u8, crate::types::DriverError> {
    match gain as u8 {
        1 => Ok(0 << 4),
        2 => Ok(1 << 4),
        4 => Ok(2 << 4),
        6 => Ok(3 << 4),
        8 => Ok(4 << 4),
        12 => Ok(5 << 4),
        24 => Ok(6 << 4),
        _ => Err(crate::types::DriverError::ConfigurationError(
            format!("Unsupported gain: {}. Supported gains: 1, 2, 4, 6, 8, 12, 24", gain)
        )),
    }
}

/// Convert samples per second value to register mask.
pub fn sps_to_reg_mask(sps: u32) -> Result<u8, crate::types::DriverError> {
    match sps {
        250 => Ok(6 << 0),
        500 => Ok(5 << 0),
        1000 => Ok(4 << 0),
        2000 => Ok(3 << 0),
        4000 => Ok(2 << 0),
        8000 => Ok(1 << 0),
        16_000 => Ok(0 << 0),
        _ => Err(crate::types::DriverError::ConfigurationError(
            format!("Unsupported samples per second: {}. Supported sps: 250, 500, 1000, 2000, 4000, 8000, 16000", sps)
        )),
    }
}