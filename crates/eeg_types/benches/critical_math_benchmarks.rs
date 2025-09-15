//! Performance benchmarks for critical EEG mathematical operations
//!
//! These benchmarks ensure that core mathematical functions meet performance
//! requirements for real-time EEG processing.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

/// Benchmark 24-bit ADC sign extension
fn bench_adc_sign_extension(c: &mut Criterion) {
    c.bench_function("24bit_adc_sign_extension", |b| {
        b.iter(|| {
            let msb = black_box(0x7F);
            let mid = black_box(0xFF);
            let lsb = black_box(0xFF);

            let raw_value = ((msb as u32) << 16) | ((mid as u32) << 8) | (lsb as u32);
            black_box(((raw_value as i32) << 8) >> 8)
        })
    });
}

/// Benchmark voltage conversion calculation
fn bench_voltage_conversion(c: &mut Criterion) {
    c.bench_function("voltage_conversion", |b| {
        b.iter(|| {
            let raw = black_box(4194304i32);
            let vref = black_box(4.5f32);
            let gain = black_box(24.0f32);

            black_box(((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32)
        })
    });
}

/// Benchmark biquad filter processing
fn bench_biquad_filter(c: &mut Criterion) {
    struct BiquadFilter {
        b0: f32,
        b1: f32,
        b2: f32,
        a1: f32,
        a2: f32,
        z1: f32,
        z2: f32,
    }

    impl BiquadFilter {
        fn new() -> Self {
            // Typical lowpass filter coefficients
            Self {
                b0: 0.067455273,
                b1: 0.134910546,
                b2: 0.067455273,
                a1: -1.142980502,
                a2: 0.412801595,
                z1: 0.0,
                z2: 0.0,
            }
        }

        fn process(&mut self, x: f32) -> f32 {
            let y = self.b0 * x + self.z1;
            self.z1 = self.b1 * x - self.a1 * y + self.z2;
            self.z2 = self.b2 * x - self.a2 * y;
            y
        }
    }

    let mut filter = BiquadFilter::new();

    c.bench_function("biquad_filter_process", |b| {
        b.iter(|| {
            let input = black_box(100.0f32);
            black_box(filter.process(input))
        })
    });
}

/// Benchmark batch voltage conversion (realistic workload)
fn bench_batch_voltage_conversion(c: &mut Criterion) {
    let raw_samples: Vec<i32> = (0..1000).map(|i| i * 1000).collect();
    let vref = 4.5f32;
    let gain = 24.0f32;

    c.bench_function("batch_voltage_conversion_1000_samples", |b| {
        b.iter(|| {
            let samples = black_box(&raw_samples);
            let mut voltages = Vec::with_capacity(samples.len());

            for &raw in samples {
                let voltage = ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32;
                voltages.push(voltage);
            }

            black_box(voltages)
        })
    });
}

/// Benchmark multi-channel filter processing
fn bench_multichannel_filtering(c: &mut Criterion) {
    struct BiquadFilter {
        b0: f32,
        b1: f32,
        b2: f32,
        a1: f32,
        a2: f32,
        z1: f32,
        z2: f32,
    }

    impl BiquadFilter {
        fn new() -> Self {
            Self {
                b0: 0.067455273,
                b1: 0.134910546,
                b2: 0.067455273,
                a1: -1.142980502,
                a2: 0.412801595,
                z1: 0.0,
                z2: 0.0,
            }
        }

        fn process(&mut self, x: f32) -> f32 {
            let y = self.b0 * x + self.z1;
            self.z1 = self.b1 * x - self.a1 * y + self.z2;
            self.z2 = self.b2 * x - self.a2 * y;
            y
        }
    }

    let mut filters: Vec<BiquadFilter> = (0..8).map(|_| BiquadFilter::new()).collect();
    let samples: Vec<f32> = (0..128).map(|i| (i as f32) * 0.001).collect();

    c.bench_function("8_channel_filter_128_samples", |b| {
        b.iter(|| {
            let input_samples = black_box(&samples);
            let mut output = Vec::with_capacity(input_samples.len() * 8);

            for &sample in input_samples {
                for filter in &mut filters {
                    output.push(filter.process(sample));
                }
            }

            black_box(output)
        })
    });
}

criterion_group!(
    benches,
    bench_adc_sign_extension,
    bench_voltage_conversion,
    bench_biquad_filter,
    bench_batch_voltage_conversion,
    bench_multichannel_filtering
);
criterion_main!(benches);
