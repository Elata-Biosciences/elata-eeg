# ADS1299 Driver Architecture

This document provides a visual representation of the ADS1299 driver architecture and its integration with the EEG system.

## System Architecture

```mermaid
graph TD
    User[User Application] --> EegSystem
    
    subgraph "EEG System"
        EegSystem[EegSystem] --> Driver
        EegSystem --> SignalProcessor[Signal Processor]
        Driver --> EventChannel[Driver Event Channel]
        EventChannel --> ProcessingTask[Processing Task]
        ProcessingTask --> SignalProcessor
        SignalProcessor --> OutputChannel[Processed Data Channel]
        OutputChannel --> User
    end
    
    subgraph "ADS1299 Driver"
        Driver[Ads1299Driver] --> Inner[Ads1299Inner]
        Driver --> SPI[SPI Communication]
        Driver --> GPIO[GPIO (DRDY Pin)]
        Driver --> AcquisitionTask[Acquisition Task]
        AcquisitionTask --> SPI
        AcquisitionTask --> GPIO
        AcquisitionTask --> EventChannel
    end
    
    subgraph "Hardware"
        SPI --> ADS1299[ADS1299 Chip]
        GPIO --> ADS1299
        ADS1299 --> Electrodes[Electrodes]
    end
```

## Data Flow

```mermaid
sequenceDiagram
    participant User as User Application
    participant EegSys as EEG System
    participant Driver as ADS1299 Driver
    participant Chip as ADS1299 Chip
    participant Proc as Signal Processor
    
    User->>EegSys: start(config)
    EegSys->>Driver: start_acquisition()
    Driver->>Chip: initialize_chip()
    Driver->>Chip: start_conversion()
    Driver->>Driver: spawn acquisition task
    
    loop Acquisition Loop
        Chip-->>Driver: DRDY signal (GPIO)
        Driver->>Chip: read_data()
        Chip-->>Driver: raw data
        Driver->>Driver: convert to AdcData
        Driver->>EegSys: DriverEvent::Data
        EegSys->>Proc: process_sample()
        Proc-->>EegSys: processed data
        EegSys->>User: ProcessedData
    end
    
    User->>EegSys: shutdown()
    EegSys->>Driver: stop_acquisition()
    Driver->>Chip: stop_conversion()
    Driver->>Driver: terminate acquisition task
    EegSys->>Driver: shutdown()
```

## Component Structure

```mermaid
classDiagram
    class AdcDriver {
        <<trait>>
        +start_acquisition() Result
        +stop_acquisition() Result
        +shutdown() Result
        +get_status() DriverStatus
        +get_config() Result~AdcConfig~
    }
    
    class Ads1299Driver {
        -inner: Arc~Mutex~Ads1299Inner~~
        -task_handle: Option~JoinHandle~
        -tx: Sender~DriverEvent~
        -spi: Option~Spi~
        -drdy_pin: Option~InputPin~
        +new(config, buffering) Result
        +start_acquisition() Result
        +stop_acquisition() Result
        +shutdown() Result
        +get_status() DriverStatus
        +get_config() Result~AdcConfig~
        -init_spi() Result
        -init_drdy_pin() Result
        -send_command(command) Result
        -read_register(register) Result
        -write_register(register, value) Result
        -read_data() Result
        -reset_chip() Result
        -start_conversion() Result
        -stop_conversion() Result
        -configure_single_ended() Result
        -configure_sample_rate(rate) Result
        -initialize_chip() Result
        -notify_status_change() Result
    }
    
    class Ads1299Inner {
        -config: AdcConfig
        -running: bool
        -status: DriverStatus
        -base_timestamp: Option~u64~
        -sample_count: u64
        -registers: [u8; 24]
    }
    
    class EegSystem {
        -driver: Box~dyn AdcDriver~
        -processor: Arc~Mutex~SignalProcessor~~
        -processing_task: Option~JoinHandle~
        -tx: Sender~ProcessedData~
        -event_rx: Option~Receiver~DriverEvent~~
        +new(config) Result
        +start(config) Result
        +stop() Result
        +reconfigure(config) Result
        +shutdown() Result
        +driver_status() DriverStatus
        +driver_config() Result~AdcConfig~
    }
    
    AdcDriver <|.. Ads1299Driver : implements
    Ads1299Driver *-- Ads1299Inner : contains
    EegSystem *-- AdcDriver : contains
```

## Register Map

The ADS1299 has several registers that need to be configured for proper operation. Here's a visual representation of the key registers used in single-ended mode:

```mermaid
graph TD
    subgraph "ADS1299 Registers"
        CONFIG1[CONFIG1 0x01]
        CONFIG2[CONFIG2 0x02]
        CONFIG3[CONFIG3 0x03]
        LOFF[LOFF 0x04]
        CH1SET[CH1SET 0x05]
        CH2SET[CH2SET 0x06]
        CH3SET[CH3SET 0x07]
        CH4SET[CH4SET 0x08]
        CH5SET[CH5SET 0x09]
        CH6SET[CH6SET 0x0A]
        CH7SET[CH7SET 0x0B]
        CH8SET[CH8SET 0x0C]
        MISC1[MISC1 0x15]
    end
    
    CONFIG1 -->|Sample Rate| SampleRate[Sample Rate Settings]
    CONFIG3 -->|Bias| BiasSettings[Bias Settings]
    MISC1 -->|SRB1 = 1| SingleEnded[Single-Ended Mode]
    CH1SET -->|Gain| GainSettings[Gain Settings]
    CH2SET -->|Gain| GainSettings
    CH3SET -->|Gain| GainSettings
    CH4SET -->|Gain| GainSettings
    CH5SET -->|Gain| GainSettings
    CH6SET -->|Gain| GainSettings
    CH7SET -->|Gain| GainSettings
    CH8SET -->|Gain| GainSettings
```

## SPI Communication

The ADS1299 communicates with the Raspberry Pi via SPI. Here's a diagram of the SPI communication:

```mermaid
sequenceDiagram
    participant Pi as Raspberry Pi
    participant ADS as ADS1299
    
    Note over Pi,ADS: Command: SDATAC (Stop Read Data Continuous)
    Pi->>ADS: 0x11
    
    Note over Pi,ADS: Command: WREG (Write Register)
    Pi->>ADS: 0x40 | register
    Pi->>ADS: 0x00 (number of registers - 1)
    Pi->>ADS: value
    
    Note over Pi,ADS: Command: RREG (Read Register)
    Pi->>ADS: 0x20 | register
    Pi->>ADS: 0x00 (number of registers - 1)
    ADS->>Pi: value
    
    Note over Pi,ADS: Command: START (Start Conversion)
    Pi->>ADS: 0x08
    
    Note over Pi,ADS: Command: RDATAC (Read Data Continuous)
    Pi->>ADS: 0x10
    
    loop Data Acquisition
        ADS-->>Pi: DRDY goes low
        Pi->>ADS: Send dummy bytes
        ADS->>Pi: Status (3 bytes)
        ADS->>Pi: Channel 1 data (3 bytes)
        ADS->>Pi: Channel 2 data (3 bytes)
        ADS->>Pi: ... (remaining channels)
    end
    
    Note over Pi,ADS: Command: STOP (Stop Conversion)
    Pi->>ADS: 0x0A
```

## Hardware Connection

```mermaid
graph LR
    subgraph "Raspberry Pi 5"
        MOSI[MOSI Pin 19]
        MISO[MISO Pin 21]
        SCLK[SCLK Pin 23]
        CS[CS Pin 24]
        DRDY[GPIO25 Pin 22]
        GND[GND Pin 6]
    end
    
    subgraph "ADS1299EEG_FE"
        ADS_MOSI[DIN Pin 11]
        ADS_MISO[DOUT Pin 13]
        ADS_SCLK[SCLK Pin 3]
        ADS_CS[CS Pin 7]
        ADS_DRDY[DRDY Pin 15]
        ADS_GND[DGND Pins 4,10,18]
        
        subgraph "Electrodes"
            CH1[CH1 Pin 36]
            CH2[CH2 Pin 32]
            CH3[CH3 Pin 28]
            CH4[CH4 Pin 24]
            CH5[CH5 Pin 20]
            CH6[CH6 Pin 16]
            CH7[CH7 Pin 12]
            CH8[CH8 Pin 8]
            BIAS[BIAS_ELEC]
            REF[REF_ELEC]
        end
    end
    
    MOSI --> ADS_MOSI
    MISO <-- ADS_MISO
    SCLK --> ADS_SCLK
    CS --> ADS_CS
    DRDY <-- ADS_DRDY
    GND --- ADS_GND