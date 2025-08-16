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
 * Build a validated driver payload from current config and desired settings.
 * Throws an Error with a user-friendly message if validation fails.
 */
export function buildDriverPayload(current: CurrentConfigLike, settings: BuildSettings): DriverPayload {
  const desiredChannels = Math.max(0, Math.floor(settings.channels || 0));
  if (desiredChannels < 1) throw new Error('Channel count must be at least 1');

  const boardDriver = current.board_driver || 'default';
  const isTwoChipBoard = boardDriver === 'ElataV2' || (current.chips?.length === 2);

  const channels = makeRange(desiredChannels);

  // Build chips layout, but only include channels in the outgoing payload
  let chips: OutputChipPayload[];
  if (isTwoChipBoard) {
    if (desiredChannels > 16) throw new Error('ElataV2 supports up to 16 channels');
    const chip0Channels = channels.filter((ch) => ch >= 0 && ch <= 7);
    const chip1Channels = channels.filter((ch) => ch >= 8 && ch <= 15).map((ch)=> ch - 8);
    chips = [
      { channels: chip0Channels },
      { channels: chip1Channels },
    ];
  } else {
    chips = [{ channels }];
  }

  const payload: DriverPayload = {
    type: isTwoChipBoard ? 'ElataV2' : boardDriver,
    sample_rate: settings.sample_rate,
    vref: current.vref ?? 4.5,
    gain: (settings.gain ?? current.gain ?? 1.0),
    chips,
  };

  if (isTwoChipBoard && payload.chips.length !== 2) {
    throw new Error('Two-chip board requires exactly 2 chip configurations');
  }

  const totalChannels = payload.chips.reduce((acc, c) => acc + (c.channels?.length || 0), 0);
  if (totalChannels !== desiredChannels) {
    throw new Error('Internal error: channel layout mismatch');
  }

  return payload;
}
