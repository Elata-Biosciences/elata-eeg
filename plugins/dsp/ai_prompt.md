this is an architecture doc for the ai to understand the context of this directory rapidly

# DSP Application Architecture

## 1. Introduction
... THIS FILE IS INCOMPELETE
## 2. Applet DSP Pipeline Lifecycle

A key design principle for DSP modules that serve Kiosk applets (e.g., `dsp_brain_waves_fft`) is resource efficiency. The DSP processing for a specific applet should only be active when the corresponding applet is visually displayed and actively used by the user in the Kiosk UI.

This is achieved through the following mechanism:

*   **Client-Managed Connection:** The Kiosk UI applet component (e.g., `AppletFftRenderer.tsx` for the Brain Waves applet) is responsible for managing the lifecycle of its WebSocket connection to the DSP service.
    *   When the applet becomes active (e.g., mounted and visible), it establishes a WebSocket connection to its dedicated endpoint on the DSP service (e.g., `/applet/brain_waves/data`).
    *   When the applet becomes inactive (e.g., unmounted, hidden, or the user navigates away), it **must explicitly close** its WebSocket connection.
*   **Server-Side Termination:** The DSP service (e.g., `dsp_brain_waves_fft/src/lib.rs`) is designed to detect WebSocket disconnections. Upon a client disconnecting:
    *   The specific processing loop and any resources (like data buffers) associated with that client's connection are terminated and cleaned up.
    *   This ensures that no unnecessary computations are performed for inactive applets.

This approach ensures that DSP resources are only consumed when an applet is actively providing value to the user, contributing to overall system performance and stability.