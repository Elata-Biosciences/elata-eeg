
## Next Steps

1. Implement the constructor (`new` method)
2. Implement SPI communication functions
3. Implement register configuration functions
4. Implement data acquisition functions
5. Test with hardware

## Implementation Notes

- The ADS1299 communicates via SPI with the following settings:
  - Mode 1 (CPOL=0, CPHA=1)
  - Maximum clock speed of 5MHz (we use 4MHz to be safe)
  - MSB first
  - 8-bit word size

- The DRDY pin is used to detect when new data is available:
  - It is active low (goes low when data is ready)
  - We use GPIO25 (Pin 22) on the Raspberry Pi 5

- For single-ended operation, we need to:
  - Set the SRB1 bit in the MISC1 register
  - Configure the channel settings registers (CHnSET)
  - Configure the bias and reference settings

- The ADS1299 has a 24-bit ADC, so each sample is 3 bytes
  - We need to convert these to i32 values
  - For bipolar mode, we need to handle sign extension for negative values
  - For unipolar mode, we should treat the 24-bit value as unsigned (0x000000 to 0xFFFFFF)

- The ADS1299 can operate in continuous or single-shot mode
  - We use continuous mode for EEG applications
  - We use the RDATAC command to start continuous data acquisition