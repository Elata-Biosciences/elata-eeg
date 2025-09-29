# Electrode Placement for Imagined Tongue Movements

This guide outlines a strategic 8-channel EEG placement strategy to detect imagined tongue movements by targeting the primary motor cortex.

## Target Brain Region: Primary Motor Cortex (M1)

For imagined tongue movements, the target is the **primary motor cortex (M1)**. Specifically, the area controlling the tongue and face, which is located on the lower, lateral (side) part of the motor strip. With a limited 8-channel headband, a targeted placement is crucial to capture a clear signal.

---

## Recommended Placement Strategy

The optimal approach is a **bilateral placement** focused over the motor cortex, centered around the **C3** and **C4** positions of the standard 10-20 system, and extending towards the ears.

### Channel Layout (8 Channels)

#### 1. Core Motor Channels (4 channels)
Place two channels on each side of the head to bracket the motor area for the face/tongue.

- **Left Hemisphere:**
  - **C3**: Over the motor cortex (controls the right side of the body).
  - **T7**: Over the temporal lobe, just above the ear.
- **Right Hemisphere:**
  - **C4**: Over the motor cortex (controls the left side of the body).
  - **T8**: Over the temporal lobe, just above the ear.

> **Rationale:** The area between **C3/T7** and **C4/T8** directly covers the region controlling facial and tongue muscles. This allows for analyzing activity across this specific zone.

#### 2. Supplementary Motor/Premotor Channels (2 channels)
Place two channels slightly forward of C3 and C4.

- **Left Hemisphere:** **FC3**
- **Right Hemisphere:** **FC4**

> **Rationale:** The premotor and supplementary motor areas are involved in movement *planning*. For imagined tasks, the signal often originates here.

#### 3. Reference and Ground Channels (2 channels)

- **Reference:** Place on a non-active location, like the mastoid bone behind one ear (**A1** or **A2**), to provide a stable baseline.
- **Ground:** Place on the forehead near the center (**Fpz**) to help reduce electrical noise.

---

### Visual Placement Guide

Imagine a line drawn over the top of your head from ear to ear. **C3** and **C4** are on this line, roughly halfway between the center and your ears.

```
      (Front of Head)
           (Fpz - Ground)

      FC3           FC4
      (Channel 5)   (Channel 6)

      C3             C4
      (Channel 1)   (Channel 2)

      T7             T8
      (Channel 3)   (Channel 4)

(Left Ear - A1 Ref)   (Right Ear)
```

---

### Why This Setup is Effective

- **Targets the Right Brain Area:** Focuses limited sensors directly over the most relevant cortical real estate.
- **Bilateral Coverage:** Imagined movement activates both hemispheres. This setup captures data from both sides, allowing for analysis of differences between them to improve classification.
- **Captures Planning and Execution Signals:** Covering both premotor (**FC3/FC4**) and motor (**C3/C4/T7/T8**) areas increases the chance of capturing the full neural signature of the imagined movement.

### Final Tip

When training your model, pay close attention to the signals from the **C3/T7** and **C4/T8** pairs. This is where you are most likely to find the distinguishing features for imagined tongue movement.