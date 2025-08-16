# Debugging Summary: `PinUsed(0)` and Empty CSV

This document summarizes the debugging process for the `eeg_daemon` application, which initially crashed with a `PinUsed(0)` error and subsequently produced an empty CSV file.

## 1. Initial Problem: `PinUsed(0)` Error

The application was crashing on startup with the following panic:

```
thread 'main' panicked at crates/pipeline/src/stages/eeg_source.rs:193:79:
called `Result::unwrap()` on an `Err` value: PinUsed(0)
```

This indicated that a GPIO pin was being initialized twice.

### Investigation and Fix

- **Added Logging:** We added logging to `eeg_source.rs` to trace the driver initialization process.
- **Diagnosis:** The logs revealed that the `ElataV2Driver` was being initialized twice: once by the pipeline's `eeg_source` stage and a second time by a redundant "Sensor Thread" in `main.rs`.
- **Solution:** We removed the redundant "Sensor Thread" from `main.rs`, making the `eeg_source` stage the sole owner of the hardware.

## 2. Second Problem: Empty CSV File

After fixing the crash, the application ran without errors, but the output CSV file (`e2e_test_output.csv`) was empty, containing only the header row. This indicated that no data was reaching the `csv_sink` stage.

### Investigation and Fixes

- **Packet Type Mismatch:** We discovered that the `csv_sink` stage expects `RtPacket::RawAndVoltage` packets, but the `to_voltage` stage was producing a different packet type. We modified `to_voltage.rs` to produce the correct packet.
- **Unimplemented `acquire_batched`:** We found that the `acquire_batched` function in `elata_v2/driver.rs` was not implemented and was returning an error immediately.
- **DRDY Timeouts:** After implementing a basic `acquire_batched` loop, the application began logging `DRDY timeout` warnings, indicating that the ADC was not sending data-ready signals.
- **Missing Start Commands:** We determined that the ADS1299 chip was not being commanded to start acquiring data. We added the `CMD_WAKEUP`, `CMD_START`, and `CMD_RDATAC` commands to the `ElataV2Driver`'s initialization sequence.

## 3. Current Situation & Next Hypothesis

The `DRDY` timeouts are now resolved, and the application appears to run correctly, but the CSV file remains empty.

**Hypothesis:** The `DRDY` handler thread in `elata_v2/driver.rs` is not functioning as expected. While the main acquisition loop is no longer timing out, the `drdy_tx.send(())` call might not be successfully signaling the main thread, or there could be a logical error in the `acquire_batched` loop that prevents the `batch_buffer` from being populated. The `is_low()` check might be too simplistic, and a more robust edge-detection mechanism might be required.

## 4. Relevant Files

- [`crates/daemon/src/main.rs`](crates/daemon/src/main.rs)
- [`crates/pipeline/src/stages/eeg_source.rs`](crates/pipeline/src/stages/eeg_source.rs)
- [`crates/pipeline/src/stages/to_voltage.rs`](crates/pipeline/src/stages/to_voltage.rs)
- [`crates/pipeline/src/stages/csv_sink.rs`](crates/pipeline/src/stages/csv_sink.rs)
- [`crates/boards/src/elata_v2/driver.rs`](crates/boards/src/elata_v2/driver.rs)
- [`crates/sensors/src/ads1299/driver.rs`](crates/sensors/src/ads1299/driver.rs)
- [`crates/daemon/e2e_test_pipeline.yaml`](crates/daemon/e2e_test_pipeline.yaml)