// This file defines the shared data structures between the backend and frontend.

export interface StageConfig {
  name: string;
  type: string;
  params: any;
  inputs: string[];
  outputs: string[];
}

export interface SystemConfig {
  stages: StageConfig[];
}

export interface EegSample {
  value: number;
  timestamp: bigint;
  channelIndex: number;
}

export interface SensorMeta {
  // Stable identity
  sensor_id: number;
  meta_rev: number;

  schema_ver: number;
  source_type: string;
  v_ref: number;
  adc_bits: number;
  gain: number;
  sample_rate: number;

  // v2 additions
  offset_code: number;
  is_twos_complement: boolean;
  channel_names: string[];
  batch_size: number;
  data_type: 'f32' | 'i32';
}

export interface MetaUpdateMsg {
  message_type: 'meta_update';
  topic: string;
  meta: SensorMeta;
}

export interface DataPacketHeader {
  topic: string;
  packet_type: string;
  ts_ns: number;
  batch_size: number;
  num_channels: number;
  meta_rev: number;
}

export interface SampleChunk {
  meta: SensorMeta;
  samples: Float32Array | Int32Array; // Can be either f32 or i32
  timestamp: number; // The timestamp of the first sample in the chunk
}