//! 10-band graphic equalizer as a linear-phase FIR filter (no phase smearing), run with FFT overlap-add.
//! Same design as the Python version: the dB curve is interpolated on log-frequency between the band centres,
//! turned into an impulse response, centred and Hann-windowed.

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use std::sync::Arc;

pub const BANDS: [f32; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

pub const PRESETS: &[(&str, [f32; 10])] = &[
    ("flat", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("bass boost", [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("bass + treble", [5.0, 4.0, 2.0, 0.0, -1.0, -1.0, 0.0, 2.0, 4.0, 5.0]),
    ("treble boost", [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 4.0, 5.0, 6.0]),
    ("vocal", [-2.0, -2.0, -1.0, 1.0, 3.0, 4.0, 3.0, 1.0, 0.0, -1.0]),
    ("loudness", [5.0, 4.0, 1.0, 0.0, -1.0, 0.0, 1.0, 2.0, 4.0, 4.0]),
    ("hip-hop", [5.0, 4.0, 2.0, 3.0, -1.0, -1.0, 1.0, 0.0, 2.0, 3.0]),
    ("electronic", [4.0, 4.0, 1.0, 0.0, -2.0, 1.0, 0.0, 1.0, 3.0, 4.0]),
    ("rock", [4.0, 3.0, -1.0, -2.0, -1.0, 1.0, 3.0, 4.0, 4.0, 4.0]),
    ("acoustic", [3.0, 3.0, 2.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 1.0]),
    ("late night", [-3.0, -2.0, 0.0, 1.0, 2.0, 2.0, 1.0, 0.0, -2.0, -3.0]),
];

#[derive(Clone, Debug, PartialEq)]
pub struct EqSettings {
    pub on: bool,
    pub bands: [f32; 10],
    pub preamp: f32,
}

impl Default for EqSettings {
    fn default() -> Self {
        EqSettings { on: false, bands: [0.0; 10], preamp: 0.0 }
    }
}

impl EqSettings {
    /// From the `eq` object in config.json: `{"on": bool, "bands": [..10], "preamp": dB}`.
    pub fn from_json(v: &serde_json::Value) -> Self {
        let mut s = EqSettings { on: v.get("on").and_then(|x| x.as_bool()).unwrap_or(false), ..Default::default() };
        if let Some(a) = v.get("bands").and_then(|x| x.as_array()) {
            for (i, g) in a.iter().take(10).enumerate() {
                s.bands[i] = g.as_f64().unwrap_or(0.0) as f32;
            }
        }
        s.preamp = v.get("preamp").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        s
    }

    /// The EQ curve in dB at `freq` (what the EQ page draws).
    pub fn db_at(&self, freq: f32) -> f32 {
        interp_db(freq, &self.bands) + self.preamp
    }
}

/// np.interp on log10(frequency), clamped at both ends.
fn interp_db(freq: f32, gains: &[f32; 10]) -> f32 {
    let x = freq.max(1.0).log10();
    let xs: Vec<f32> = BANDS.iter().map(|b| b.log10()).collect();
    if x <= xs[0] {
        return gains[0];
    }
    if x >= xs[9] {
        return gains[9];
    }
    let i = xs.iter().rposition(|&b| b <= x).unwrap_or(0).min(8);
    let t = (x - xs[i]) / (xs[i + 1] - xs[i]);
    gains[i] + t * (gains[i + 1] - gains[i])
}

/// Linear-phase impulse response for the curve (4096 taps, 8192 above 50 kHz).
pub fn impulse(sample_rate: u32, gains: &[f32; 10], preamp: f32) -> Vec<f32> {
    let n = if sample_rate > 50_000 { 8192 } else { 4096 };
    let mut planner = RealFftPlanner::<f32>::new();
    let inv = planner.plan_fft_inverse(n);
    let mut spec: Vec<Complex32> = (0..=n / 2)
        .map(|k| {
            let f = k as f32 * sample_rate as f32 / n as f32;
            let db = interp_db(f, gains) + preamp;
            Complex32::new(10f32.powf(db / 20.0), 0.0)
        })
        .collect();
    let mut h = vec![0.0f32; n];
    inv.process(&mut spec, &mut h).expect("fft sizes match");
    // irfft is unnormalised in realfft; numpy's is divided by n
    let scale = 1.0 / n as f32;
    let mut out = vec![0.0f32; n];
    for i in 0..n {
        let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos(); // np.hanning
        out[i] = h[(i + n - n / 2) % n] * scale * w; // np.roll(h, n/2)
    }
    out
}

/// Streaming stereo FIR convolution (FFT overlap-add). Any block size in; output comes in whole blocks.
pub struct Fir {
    pub taps: usize,
    m: usize,
    l: usize,
    h: Vec<Complex32>,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    tail: [Vec<f32>; 2],
    inbuf: Vec<f32>, // interleaved stereo waiting for a full block
    scratch_in: Vec<f32>,
    scratch_spec: Vec<Complex32>,
    scratch_out: Vec<f32>,
}

impl Fir {
    pub fn new(h: &[f32]) -> Self {
        let taps = h.len();
        let m = (2 * taps).next_power_of_two();
        let l = m - taps + 1;
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(m);
        let inv = planner.plan_fft_inverse(m);
        let mut padded = vec![0.0f32; m];
        padded[..taps].copy_from_slice(h);
        let mut hs = fwd.make_output_vec();
        fwd.process(&mut padded, &mut hs).expect("fft sizes match");
        let scale = 1.0 / m as f32;
        for c in &mut hs {
            *c *= scale;
        }
        Fir {
            taps,
            m,
            l,
            h: hs,
            tail: [vec![0.0; taps - 1], vec![0.0; taps - 1]],
            inbuf: Vec::new(),
            scratch_in: fwd.make_input_vec(),
            scratch_spec: fwd.make_output_vec(),
            scratch_out: inv.make_output_vec(),
            fwd,
            inv,
        }
    }

    /// Keeps the running state of an older filter of the same length, so changing the EQ doesn't click.
    pub fn take_state(&mut self, old: &mut Fir) {
        if old.taps == self.taps {
            self.tail = std::mem::take(&mut old.tail);
            self.inbuf = std::mem::take(&mut old.inbuf);
        }
    }

    pub fn reset(&mut self) {
        for t in &mut self.tail {
            t.iter_mut().for_each(|x| *x = 0.0);
        }
        self.inbuf.clear();
    }

    /// Group delay in frames (the output lags the input by this much).
    pub fn delay(&self) -> usize {
        self.taps / 2
    }

    /// Feeds interleaved stereo `x`, appends whatever full blocks are ready to `out`.
    pub fn process(&mut self, x: &[f32], out: &mut Vec<f32>) {
        self.inbuf.extend_from_slice(x);
        let (l, m, n) = (self.l, self.m, self.taps);
        while self.inbuf.len() >= l * 2 {
            let start = out.len();
            out.resize(start + l * 2, 0.0);
            for ch in 0..2 {
                for i in 0..m {
                    self.scratch_in[i] = if i < l { self.inbuf[i * 2 + ch] } else { 0.0 };
                }
                self.fwd.process(&mut self.scratch_in, &mut self.scratch_spec).expect("fft");
                for (a, b) in self.scratch_spec.iter_mut().zip(&self.h) {
                    *a *= *b;
                }
                self.inv.process(&mut self.scratch_spec, &mut self.scratch_out).expect("fft");
                let y = &mut self.scratch_out;
                for i in 0..n - 1 {
                    y[i] += self.tail[ch][i];
                }
                for i in 0..l {
                    out[start + i * 2 + ch] = y[i];
                }
                self.tail[ch].copy_from_slice(&y[l..l + n - 1]);
            }
            self.inbuf.drain(..l * 2);
        }
    }

    /// Pushes the last partial block through (end of the stream).
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let pad = vec![0.0f32; self.l * 2];
        self.process(&pad, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_eq_passes_signal_through_with_delay() {
        let h = impulse(44100, &[0.0; 10], 0.0);
        let mut fir = Fir::new(&h);
        let d = fir.delay();
        let mut x = vec![0.0f32; 20000 * 2];
        for i in 0..20000 {
            let v = (i as f32 * 0.05).sin() * 0.5;
            x[i * 2] = v;
            x[i * 2 + 1] = -v;
        }
        let mut y = Vec::new();
        fir.process(&x, &mut y);
        fir.flush(&mut y);
        let mut err = 0.0f32;
        for i in 2000..15000 {
            err = err.max((y[(i + d) * 2] - x[i * 2]).abs()).max((y[(i + d) * 2 + 1] - x[i * 2 + 1]).abs());
        }
        assert!(err < 0.01, "max error {err}");
    }

    #[test]
    fn boost_raises_level() {
        let h = impulse(48000, &[0.0, 0.0, 0.0, 0.0, 0.0, 6.0, 6.0, 6.0, 0.0, 0.0], 0.0);
        let mut fir = Fir::new(&h);
        let x: Vec<f32> = (0..48000).flat_map(|i| {
            let v = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin() * 0.25;
            [v, v]
        }).collect();
        let mut y = Vec::new();
        fir.process(&x, &mut y);
        let peak = y[20000..40000].iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!((peak / 0.25 - 2.0).abs() < 0.1, "gain {}", peak / 0.25); // +6 dB ≈ ×2
    }
}
