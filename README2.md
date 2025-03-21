TL;DR: This..

## Simple Description
This..

## Electro Placement Guide TODO clean this up
- Check Electrode Contact: Good contact (gel, minimal motion) helps reduce large offset drifts. Scotch tape can help hold electrodes in place but can also allow some micro motion.
- Give the System Time to Settle: Right after you place electrodes, the offset is often larger. Let the system stabilize for ~30 s or so before recording.

  Active Electrodes (4 sites): Whatever scalp locations you want to record from (e.g., Fp1, Fp2, O1, O2, or T3, T4, etc., depending on your experiment).
    Reference Electrode:
        Linked-Ears Reference: A common practice is to link the left and right earlobes (or mastoids) together via a y‐cable, then feed that combined signal into SRB1 (the reference). This helps balance left/right ear impedances and can reduce some artifacts.
        Or simply pick one ear/mastoid to use as reference for all channels if linking ears is not convenient.
    Bias (Ground) Electrode: A single bias electrode can still be placed on (for example) an earlobe, mastoid, or an Fpz location (low on the forehead, near the hairline).

Rationale:

    All channels can share the same reference electrode (or the same “linked” pair) so signals are measured consistently.
    Using a single bias electrode for all channels simplifies the setup and is standard practice.

## Setup Guide
After the product is assembled with the touch screen, clone then start the kiosk application.
```bash
git clone https://github.com/Elata-Biosciences/elata-eeg
cd elata-eeg
chmod +x install.sh
bash install.sh
```

## Behavior
- 
- 

## Dev Usage
#### Change Code
```bash
# Stop kiosk mode
bash stop.sh

# Term 1, driver
cd driver; cargo build
# Term 2, daemon
cd daemon; cargo build; cargo run
# Term 3, kiosk
cd kiosk; npm run dev
```

#### Rebuild Production
```bash
# Stop
bash stop.sh

# ...<Change code here> ...

# Rebuild code base and run kiosk mode
bash rebuild.sh
```

## TODO
- Interent security check review
- Production ready task items
  1) Mock Driver Issue: Update the daemon's main.rs to use a real hardware driver instead of the mock driver. Currently, it's using DriverType::Mock on line 29, but the real hardware driver (DriverType::Ads1299) is not fully implemented yet.
  2) Error Handling: Add more robust error handling for critical operations in the script, beyond just using set -e.
  3) Health Checks: Add mechanisms to verify that services are running correctly after they're started.
  4) Network Configuration: Add support for configuring network settings, especially if a static IP is needed.
  5) Security Hardening: Consider additional security measures for a production system.
  6) add screen rotate... wlr-randr --output HDMI-A-2 --transform 270
- X11 (LDXE-pi) to Wayland (labwc) was rocky
- After kiosk is solid, make sure all the files in install.sh (e.g. /home/elata/.config/labwc/autostart) mathc what we got working in the acutal files. 
- Realtime hardware update? pros and cons
- Firmware versioning in the filename to account for expirimenetal errors from firmware
- We're losing 1 bit of precision using float32. 24bit ADC and float32 is 23bit
- Incoroporate positive channel lead-off detection? Maybe useful in the csv file for data analysis. (see LOFF_STATP register)