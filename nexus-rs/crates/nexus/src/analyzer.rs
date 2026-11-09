//! Turns what's playing right now into 96 spectrum bands, peaks, a level and a waveform for the
//! visualizers (and the dancing ring on the record). Only runs while something shows it.

use realfft::RealFftPlanner;
use realfft::num_complex::Complex32;
use std::sync::Arc;

pub const N: usize = 96;
const FFT: usize = 2048;

pub struct Analyzer {
    pub bands: Vec<f32>,
    pub peaks: Vec<f32>,
    pub wave: Vec<f32>,
    pub level: f32,
    idle: bool,
    edges: Option<(u32, Vec<usize>)>,
    fft: Arc<dyn realfft::RealToComplex<f32>>,
    window: Vec<f32>,
    scratch: Vec<f32>,
    spec: Vec<Complex32>,
    last: f64,
}

impl Default for Analyzer {
    fn default() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT);
        let window = (0..FFT).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT - 1) as f32).cos()).collect();
        Analyzer {
            bands: vec![0.0; N],
            peaks: vec![0.0; N],
            wave: vec![0.0; 512],
            level: 0.0,
            idle: false,
            edges: None,
            scratch: fft.make_input_vec(),
            spec: fft.make_output_vec(),
            fft,
            window,
            last: 0.0,
        }
    }
}

impl Analyzer {
    /// One step (call at ~60 fps while a visualizer is visible). `samples` = the last 2048 mono
    /// samples played, or None when nothing is playing. Returns false once everything has settled.
    pub fn tick(&mut self, now: f64, samples: Option<(Vec<f32>, u32)>, sensitivity: f32) -> bool {
        if now - self.last < 0.014 {
            return !self.idle;
        }
        self.last = now;
        let sens = sensitivity.max(0.3);
        let playing = samples.is_some();
        let mut v = vec![0.0f32; N];
        if let Some((x, sr)) = samples.filter(|(x, _)| x.len() >= FFT) {
            let sr = if sr == 0 { 44100 } else { sr };
            if self.edges.as_ref().map(|e| e.0) != Some(sr) {
                let hi = 16000f32.min(sr as f32 / 2.0 * 0.95);
                let (lo_l, hi_l) = (32f32.ln(), hi.ln());
                let bins = (0..=N)
                    .map(|i| {
                        let f = (lo_l + (hi_l - lo_l) * i as f32 / N as f32).exp();
                        ((f * FFT as f32 / sr as f32) as usize).clamp(1, FFT / 2)
                    })
                    .collect();
                self.edges = Some((sr, bins));
            }
            let bins = &self.edges.as_ref().unwrap().1;
            let x = &x[x.len() - FFT..];
            for i in 0..FFT {
                self.scratch[i] = x[i] * self.window[i];
            }
            let _ = self.fft.process(&mut self.scratch, &mut self.spec);
            let norm = FFT as f32 / 4.0;
            for i in 0..N {
                let (a, b) = (bins[i], bins[i + 1].max(bins[i] + 1));
                let m = self.spec[a..b.min(self.spec.len())].iter().fold(0.0f32, |m, c| m.max(c.norm())) / norm;
                let tilt = 9.0 * i as f32 / (N - 1) as f32;
                let db = 20.0 * (m + 1e-7).log10() + tilt;
                v[i] = ((db + 57.0) / 54.0 * sens).clamp(0.0, 1.0);
            }
            let tail = &x[FFT - 1024..];
            self.level = (tail.iter().map(|s| s * s).sum::<f32>() / 1024.0).sqrt() * 3.0 * sens;
            self.wave = tail.iter().step_by(2).map(|s| (s * 2.2 * sens).clamp(-1.0, 1.0)).collect();
        } else {
            self.wave.iter_mut().for_each(|w| *w *= 0.8);
            self.level *= 0.8;
        }
        for i in 0..N {
            let b = &mut self.bands[i];
            *b = if v[i] > *b { *b + (v[i] - *b) * 0.65 } else { *b * 0.88 + v[i] * 0.12 };
            self.peaks[i] = (self.peaks[i] - 0.012).max(*b);
        }
        let quiet = !playing && self.peaks.iter().cloned().fold(0.0, f32::max) < 0.002 && self.level < 0.002;
        let was_idle = self.idle;
        self.idle = quiet;
        !(quiet && was_idle)
    }

    pub fn active(&self) -> bool {
        !self.idle
    }
}

/// np.interp-style resample of `v` to `n` points.
pub fn resample(v: &[f32], n: usize) -> Vec<f32> {
    if n == 0 || v.is_empty() {
        return vec![0.0; n];
    }
    if n == 1 {
        return vec![v[0]];
    }
    (0..n)
        .map(|i| {
            let x = i as f32 * (v.len() - 1) as f32 / (n - 1) as f32;
            let i0 = x.floor() as usize;
            let i1 = (i0 + 1).min(v.len() - 1);
            let t = x - i0 as f32;
            v[i0] * (1.0 - t) + v[i1] * t
        })
        .collect()
}
