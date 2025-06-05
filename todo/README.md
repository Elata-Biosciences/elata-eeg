# TODO Priorities
- Remove printlns?
- Fix readmes
- MVP push. add /boot/firmware/config.txt line? dtparam=spi=on
- Manual - include the http://raspberrypi.local:3000/ thing
 - Blink test doc

# TODO Docs
- security.md... security hardening
- [EEG Performance Optimization Plan](./eeg_performance_optimization_plan.md) - **COMPLETE** - Plan to reduce CPU usage from ~16% to ~3% by eliminating multiple daemon processes and optimizing DSP pipeline.
- [Part 2 Implementation Plan](./part2_implementation_plan.md) - **COMPLETE** - Detailed implementation plan for process consolidation and DSP coordinator integration.
- [Part 2 Implementation Status](./part2_implementation_status.md) - **COMPLETE** - Phase 1, 2 & 3 complete: Major performance optimization achieved (61% CPU improvement).
- [CPU Leak Fix Implementation Plan](./cpu_leak_fix_implementation_plan.md) - **IN PROGRESS** - **CRITICAL** - Fix for escalating CPU usage (4.9% → 6.7%). Multi-pipeline demand-based processing to achieve 0% CPU when idle.

- [Real-Time Filter Investigation (ADS1299 & DSP)](./realtime_filter_investigation.md) - Analysis of current filter behavior and plan for dynamic UI control.

- [Kiosk Boot Failure Investigation](./boot_failures.md) - Diagnosing "site can't be reached" error after reboot.

- [Next.js Build Error Session Notes](./next_js_build_error_session_notes.md) - Investigation of React dependency resolution issues with applet files outside kiosk project.

### TODO MVP
- 3-d printed case
- BOM... (e.g. header to lead cables)
- Install Instructions
  - BOM, assembly, Software setup, (possible keyboard for wifi), configure browser to detect local network

Lowest Priority
- rename the board? Animal name (elk, emu, eel, ezo)?
- low power type mode (update graph in circular motion. Increase batch size)
- Change board name elata-e1 and add firmware versioning in the filename
- Incoroporate positive channel lead-off detection? Maybe useful in the csv file for data analysis. (see LOFF_STATP register)
- Clean up driver object. may be better if we can use the board as the obj and pass the filtering into it or something
- Dry vs wet electrodes- make py notebook?
- Signal quality and filtering. AVSS vs GND vs DGND vs AGND. 5v powering AVDD
 - AVSS=AGND (JP25). DGND=AGND (Figure 58)... SPI uses AGND... AGND/AVSS/DGND - Analog ground
 - Filtering adds delays and possible phase distortions. Could impact BCI/Nuerofeedback
- UI can change session name too (need to have keyboard popup then). Batch size configurable in browser?
- What's a good way to do study/session naming in the file? How do we key subject/their-data to the file recorded?
- multiple eegs, makes it hard to do over WiFi
- increment session in fname during the day. so expirimeent number can be keyed to notes. or add keyboard.