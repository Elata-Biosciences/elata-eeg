# WebSocket Refactor Plan

This document outlines the steps to implement the optimized hybrid binary/JSON protocol as defined in `binary_protocol.md`.

## Backend Implementation

1.  **Define Message Structs:**
    *   In a new `crates/daemon/src/protocol.rs` module, define two new structs that can be serialized to JSON:
        *   `MetaUpdateMsg`: For the `meta_update` message.
        *   `DataPacketHeader`: For the minimal JSON header of the `data_packet` message.

2.  **Update `websocket_sink` Stage:**
    *   Add state to the `WebsocketSink` struct to track the last sent `meta_rev` for each topic: `last_meta_rev: HashMap<String, u32>`.
    *   In the `process` loop:
        *   Check if the incoming packet's `meta_rev` is new for its topic.
        *   If it's new, create and send a `meta_update` JSON message to the broker. Update `last_meta_rev`.
        *   Create the minimal `DataPacketHeader`.
        *   Serialize the header to JSON.
        *   Get the raw bytes from the packet's samples.
        *   Construct the final hybrid binary message: `[json_len | json_bytes | sample_bytes]`.
        *   Send the binary message to the broker.

3.  **Update `websocket_broker`:**
    *   The broker needs to handle two types of messages from the sink's channel: the JSON string for `meta_update` and the binary `Vec<u8>` for `data_packet`.
    *   Define a `BrokerMessage` enum that can be either `Meta(String)` or `Data(Vec<u8>)`.
    *   The `websocket_sink` will send this enum.
    *   The `websocket_broker` will receive this enum and send the appropriate message type (Text for `Meta`, Binary for `Data`) to the connected WebSocket clients.

## Frontend Implementation

1.  **Define TypeScript Types:**
    *   In `kiosk/src/types/eeg.ts`, create interfaces that match the `MetaUpdateMsg` and `DataPacketHeader` JSON structures.

2.  **Update `EegDataContext`:**
    *   Add state to manage the received metadata: `const [metadata, setMetadata] = useState<Map<string, SensorMeta>>(new Map());`.
    *   In the `useWebSocket` hook:
        *   Modify the `onmessage` handler to distinguish between string and binary messages.
        *   **On String Message:** Parse the JSON. If it's a `meta_update`, update the `metadata` state map.
        *   **On Binary Message (`ArrayBuffer`):**
            *   Read the 4-byte header length.
            *   Decode the JSON header.
            *   Use the `topic` from the header to look up the full `SensorMeta` from the `metadata` state map.
            *   Create a zero-copy `Float32Array` view on the rest of the buffer.
            *   Combine the retrieved `SensorMeta` with the data from the `data_packet` to form the `SampleChunk` that the rest of the app expects.

## Testing

1.  **End-to-End Verification:**
    *   Start the backend and connect with the frontend.
    *   Verify that the `meta_update` message is received first.
    *   Verify that subsequent `data_packet` messages are received and parsed correctly.
    *   Confirm that the `EegRenderer` displays the data correctly.
    *   (Optional) Add a mechanism in the UI to trigger a metadata change on the backend and confirm a new `meta_update` message is sent and handled.
