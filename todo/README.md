# TODO and Implementation Notes

This directory contains task tracking and implementation documentation for the EEG system.

## Completed Tasks

- [FFT Plugin Subscriber Awareness Implementation](./fft_plugin_subscriber_awareness_implementation.md) - Updated the brain_waves_fft plugin to skip expensive FFT calculations when no subscribers are present, providing significant CPU savings.

## Planned Tasks

- [Plugin Subscriber Awareness Detailed Guide](./plugin_subscriber_awareness_detailed_guide.md) - Comprehensive guide for implementing subscriber awareness across all plugins
- [Plugin Subscriber Awareness Implementation Plan](./plugin_subscriber_awareness_implementation_plan.md) - Step-by-step plan for rolling out subscriber awareness to other plugins

## Notes

When adding new tasks or implementation notes:
1. Create a new `.md` file in this directory
2. Add a corresponding entry to this README.md file
3. Use descriptive filenames that clearly indicate the task or feature