# Current Modular Rust Architecture

This diagram illustrates the existing system architecture, highlighting its modular and extensible nature.

```mermaid
graph TD
    subgraph Hardware Layer
        A[EEG Hardware - ADS1299]
    end

    subgraph Rust Daemon
        B[Sensor Driver]
        C[Data Acquisition]
        D[Pipeline Executor]
        E[WebSocket Server]

        subgraph Pipeline Stages
            F[To Voltage]
            G[Basic Voltage Filter]
            H[Brain Waves FFT]
            I[CSV Recorder]
        end
    end

    subgraph Client
        J[UI/WebSocket Client]
    end

    A --> B
    B --> C
    C --> D
    D -- Manages & Executes --> F
    D -- Manages & Executes --> G
    D -- Manages & Executes --> H
    D -- Manages & Executes --> I
    
    F --> G
    G --> H
    H --> E
    I -- Optional --> E

    E -- Streams Data --> J