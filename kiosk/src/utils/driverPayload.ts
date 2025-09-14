// Internal helper type for building channel layout
export interface ChipConfig {
  channels: number[];
  spi_bus: number;
  cs_pin: number;
}

export interface CurrentConfigLike {
  board_driver?: string;
  chips?: ChipConfig[];
  vref?: number;
  gain?: number;
  // drdy_pin intentionally omitted from outgoing payloads
}

export interface BuildSettings {
  channels: number; // desired total channel count starting from 0
  sample_rate: number;
  gain?: number; // optional UI override
}

// Minimal chip description we send to the daemon (no hardware pins)
export interface OutputChipPayload {
  channels: number[];
}

export interface DriverPayload {
  type: string;
  sample_rate: number;
  vref: number;
  gain: number;
  chips: OutputChipPayload[];
}

function makeRange(n: number): number[] {
  return Array.from({ length: n }, (_, i) => i);
}

/**
 * Build a validated driver payload from the current configuration and desired UI settings.
 *
 * This function is critical for ensuring that the frontend and backend configurations
 * remain synchronized. It dynamically constructs the payload based on the number of
 * chips reported in the `current` config object, which is received from the daemon.
 *
 * Key Logic:
 * 1.  It is agnostic to board names like 'ElataV1' or 'ElataV2'.
 * 2.  The number of chips is determined solely by `current.chips.length`.
 * 3.  The maximum number of channels is calculated as `numChips * 8`.
 * 4.  It generates a `chips` array in the payload that precisely matches the
 *     hardware, only including chip objects that have active channels. This
 *     prevents errors on the backend from empty or superfluous chip configurations.
 *
 * Throws an Error with a user-friendly message if validation fails.
 */
export function buildDriverPayload(current: CurrentConfigLike, settings: BuildSettings): DriverPayload {
  const desiredChannels = Math.max(0, Math.floor(settings.channels || 0));
  if (desiredChannels < 1) throw new Error('Channel count must be at least 1');

  const boardDriver = current.board_driver || 'default';
  const numChips = current.chips?.length || 1;
  const maxChannels = numChips * 8;

  if (desiredChannels > maxChannels) {
    throw new Error(`This board supports a maximum of ${maxChannels} channels.`);
  }

  const channels = makeRange(desiredChannels);

  // Dynamically build chip configurations
  const chips: OutputChipPayload[] = [];
  for (let i = 0; i < numChips; i++) {
    const chipChannels = channels
      .filter(ch => ch >= i * 8 && ch < (i + 1) * 8)
      .map(ch => ch - i * 8); // Convert to chip-local channel index (0-7)
    
    // CRITICAL FIX: Only add a chip object to the payload if it will contain
    // channels. This prevents sending empty chip configurations for unused chips
    // on multi-chip boards, which was the root cause of the reconfiguration error.
    if (chipChannels.length > 0) {
        chips.push({ channels: chipChannels });
    }
  }

  // If no channels were assigned (e.g., desiredChannels was 0, which is invalid but
  // we safeguard here), ensure at least one chip config is present to match driver expectations.
  if (chips.length === 0) {
    chips.push({ channels: [] });
  }

  const payload: DriverPayload = {
    type: boardDriver,
    sample_rate: settings.sample_rate,
    vref: current.vref ?? 4.5,
    gain: (settings.gain ?? current.gain ?? 1.0),
    chips,
  };

  const totalChannels = payload.chips.reduce((acc, c) => acc + (c.channels?.length || 0), 0);
  if (totalChannels !== desiredChannels) {
    throw new Error('Internal error: channel layout mismatch');
  }

  return payload;
}
