//! Spek-style check: decode a file once, take evenly spaced FFT windows across it, and judge whether it's
//! really lossless (an MP3/AAC encoder leaves a hard ceiling at 16–20 kHz), true hi-res, or padded 24-bit.

use ffmpeg::format::Sample;
use ffmpeg::format::sample::Type as SampleType;
use ffmpeg::{ChannelLayout, codec, frame, media};
use ffmpeg_next as ffmpeg;
use realfft::RealFftPlanner;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const DB_FLOOR: f32 = -120.0;
const SPEK_STOPS: [(f32, [f32; 3]); 7] = [
    (0.00, [0.0, 0.0, 0.0]),
    (0.12, [0.0, 0.0, 70.0]),
    (0.30, [120.0, 0.0, 150.0]),
    (0.50, [230.0, 20.0, 40.0]),
    (0.68, [255.0, 130.0, 0.0]),
    (0.85, [255.0, 230.0, 40.0]),
    (1.00, [255.0, 255.0, 255.0]),
];

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Good,
    Warn,
    Bad,
}

#[derive(Clone, Debug)]
pub struct Verdict {
    pub cutoff: f32,
    pub drop: f32,
    pub kind: Kind,
    pub text: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Analysis {
    /// dB, `bins` rows × `cols` columns, stored column by column
    pub db: Vec<f32>,
    pub bins: usize,
    pub cols: usize,
    pub bin_hz: f32,
    pub sr: u32,
    pub channels: u32,
    pub bits: u32,
    pub duration: f64,
    pub n: usize,
    pub peak: f32,
    pub verdict: Verdict,
}

impl Analysis {
    pub fn at(&self, bin: usize, col: usize) -> f32 {
        self.db[col * self.bins + bin]
    }
}

/// 256-entry colour map: "spek" (black → purple → red → yellow → white) or "mono".
pub fn palette(kind: &str) -> Vec<[u8; 3]> {
    (0..256)
        .map(|i| {
            let x = i as f32 / 255.0;
            if kind == "mono" {
                let g = (x.powf(0.9) * 255.0) as u8;
                return [g, g, g];
            }
            let j = SPEK_STOPS.iter().position(|(p, _)| *p >= x).unwrap_or(SPEK_STOPS.len() - 1).max(1);
            let (p0, c0) = SPEK_STOPS[j - 1];
            let (p1, c1) = SPEK_STOPS[j];
            let t = if p1 > p0 { ((x - p0) / (p1 - p0)).clamp(0.0, 1.0) } else { 0.0 };
            [0, 1, 2].map(|k| (c0[k] + (c1[k] - c0[k]) * t) as u8)
        })
        .collect()
}

/// Decodes `path` and builds the spectrogram. `progress` gets 0…1000; set `cancel` to stop early.
pub fn analyze(path: &str, columns: usize, progress: &AtomicU32, cancel: &AtomicBool) -> Result<Analysis, String> {
    ffmpeg::init().map_err(|e| e.to_string())?;
    let mut ictx = ffmpeg::format::input(&path).map_err(|e| format!("can't open: {e}"))?;
    let st = ictx.streams().best(media::Type::Audio).ok_or("no audio in this file")?;
    let idx = st.index();
    let tb = f64::from(st.time_base());
    let dur = if st.duration() > 0 {
        st.duration() as f64 * tb
    } else if ictx.duration() > 0 {
        ictx.duration() as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)
    } else {
        0.0
    };
    let ctx = codec::context::Context::from_parameters(st.parameters()).map_err(|e| e.to_string())?;
    let mut dec = ctx.decoder().audio().map_err(|e| format!("no decoder: {e}"))?;
    let sr = dec.rate().max(1);
    let channels = u32::from(dec.channels());
    // SAFETY: reading a plain int field of the open codec context
    let raw_bits = unsafe { (*dec.as_ptr()).bits_per_raw_sample }.max(0) as u32;
    let n = if sr > 50_000 { 8192 } else { 4096 };
    let total = ((dur * f64::from(sr)) as usize).max(n);
    let cols = columns.max(2);
    let pos: Vec<usize> = (0..cols).map(|i| (i as f64 * (total - n) as f64 / (cols - 1) as f64) as usize).collect();
    let win: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos()).collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
    let bins = n / 2 + 1;
    let mut spec = fft.make_output_vec();
    let mut scratch = fft.make_input_vec();
    let mut mags: Vec<f32> = Vec::with_capacity(bins * cols);
    let mut buf: Vec<f32> = Vec::new();
    let (mut base, mut k) = (0usize, 0usize);
    let mut lowbits_used = false;
    let mut int_frames = 0u32;
    let mut peak = 0f32;
    let mut resampler: Option<ffmpeg::software::resampling::Context> = None;
    let mut decoded = frame::Audio::empty();
    let mut mono = frame::Audio::empty();

    let mut take_windows = |buf: &mut Vec<f32>, base: &mut usize, k: &mut usize, mags: &mut Vec<f32>, tail: bool| {
        while *k < cols && (pos[*k] + n <= *base + buf.len() || (tail && !buf.is_empty())) {
            let st = pos[*k].saturating_sub(*base);
            for i in 0..n {
                scratch[i] = buf.get(st + i).copied().unwrap_or(0.0) * win[i];
            }
            let _ = fft.process(&mut scratch, &mut spec);
            mags.extend(spec.iter().map(|c| c.norm()));
            *k += 1;
            if *k % 50 == 0 {
                progress.store((*k * 1000 / cols) as u32, Ordering::Relaxed);
            }
        }
        if *k < cols {
            let drop = pos[*k].saturating_sub(*base).min(buf.len());
            if drop > 0 {
                buf.drain(..drop);
                *base += drop;
            }
        }
    };

    let mut handle = |decoded: &frame::Audio, buf: &mut Vec<f32>| -> Result<(), String> {
        if decoded.format() == Sample::I32(SampleType::Packed) || decoded.format() == Sample::I32(SampleType::Planar) {
            if raw_bits > 16 && !lowbits_used {
                int_frames += 1;
                let planes = if decoded.is_planar() { decoded.planes() } else { 1 };
                let per_plane = decoded.samples() * if decoded.is_planar() { 1 } else { usize::from(decoded.channels()) };
                'p: for p in 0..planes {
                    // data() is the whole (padded) line: only the real samples count
                    let data = decoded.data(p);
                    for c in data.chunks_exact(4).take(per_plane) {
                        let v = i32::from_ne_bytes([c[0], c[1], c[2], c[3]]);
                        if v & 0xFFFF != 0 {
                            lowbits_used = true;
                            break 'p;
                        }
                    }
                }
            }
        }
        let rs = match &mut resampler {
            Some(r) => r,
            None => {
                let layout = if decoded.channel_layout().is_empty() { ChannelLayout::default(i32::from(decoded.channels())) } else { decoded.channel_layout() };
                resampler = Some(
                    ffmpeg::software::resampling::Context::get(decoded.format(), layout, decoded.rate(), Sample::F32(SampleType::Packed), ChannelLayout::MONO, decoded.rate())
                        .map_err(|e| e.to_string())?,
                );
                resampler.as_mut().unwrap()
            }
        };
        rs.run(decoded, &mut mono).map_err(|e| e.to_string())?;
        let samples = mono.samples();
        let data = mono.data(0);
        for c in data.chunks_exact(4).take(samples) {
            let v = f32::from_ne_bytes([c[0], c[1], c[2], c[3]]);
            peak = peak.max(v.abs());
            buf.push(v);
        }
        Ok(())
    };

    'outer: for (s, packet) in ictx.packets() {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if s.index() != idx {
            continue;
        }
        if dec.send_packet(&packet).is_err() {
            continue;
        }
        while dec.receive_frame(&mut decoded).is_ok() {
            handle(&decoded, &mut buf)?;
            take_windows(&mut buf, &mut base, &mut k, &mut mags, false);
            if k >= cols {
                break 'outer;
            }
        }
    }
    if k < cols {
        let _ = dec.send_eof();
        while dec.receive_frame(&mut decoded).is_ok() {
            handle(&decoded, &mut buf)?;
        }
        take_windows(&mut buf, &mut base, &mut k, &mut mags, true);
    }
    let cols = mags.len() / bins;
    if cols == 0 {
        return Err("no audio decoded".into());
    }
    let wmean: f32 = win.iter().sum::<f32>() / n as f32;
    let reference = n as f32 * wmean / 2.0;
    let db: Vec<f32> = mags.iter().map(|m| (20.0 * (m / reference).max(1e-9).log10()).max(DB_FLOOR)).collect();
    let bin_hz = sr as f32 / n as f32;
    let mut an = Analysis {
        db,
        bins,
        cols,
        bin_hz,
        sr,
        channels,
        bits: raw_bits,
        duration: dur,
        n,
        peak,
        verdict: Verdict { cutoff: 0.0, drop: 0.0, kind: Kind::Good, text: String::new(), notes: Vec::new() },
    };
    an.verdict = verdict(&an, if int_frames > 0 { Some(lowbits_used) } else { None });
    progress.store(1000, Ordering::Relaxed);
    Ok(an)
}

fn percentile(v: &mut [f32], q: f32) -> f32 {
    v.sort_by(f32::total_cmp);
    let pos = q * (v.len() - 1) as f32;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    v[lo] + (v[hi] - v[lo]) * (pos - lo as f32)
}

fn searchsorted(freq_of: impl Fn(usize) -> f32, len: usize, x: f32) -> usize {
    (0..len).find(|&i| freq_of(i) >= x).unwrap_or(len)
}

/// Finds the lossy-encoder lowpass cliff; judges true lossless / fake hi-res / padded 24-bit.
pub fn verdict(an: &Analysis, lowbits_used: Option<bool>) -> Verdict {
    let (bins, cols) = (an.bins, an.cols);
    let nyq = an.sr as f32 / 2.0;
    let step = an.bin_hz;
    let f = |i: usize| i as f32 * step;
    let loud: Vec<usize> = (0..cols).filter(|&c| (0..bins).any(|b| an.at(b, c) > -60.0)).collect();
    let use_cols: Vec<usize> = if loud.len() > 10 { loud } else { (0..cols).collect() };
    let mut l = vec![0f32; bins];
    let mut tmp = Vec::with_capacity(use_cols.len());
    for (b, lv) in l.iter_mut().enumerate() {
        tmp.clear();
        tmp.extend(use_cols.iter().map(|&c| an.at(b, c)));
        *lv = percentile(&mut tmp, 0.97);
    }
    // moving average over ~150 Hz (edge-padded), same alignment as numpy's convolve(..., "same")
    let k = ((150.0 / step).round() as usize).max(1);
    let half = (k - 1) / 2;
    let ls: Vec<f32> = (0..bins)
        .map(|i| {
            let mut s = 0.0;
            for j in 0..k {
                let q = (i + half) as isize - j as isize;
                s += l[q.clamp(0, bins as isize - 1) as usize];
            }
            s / k as f32
        })
        .collect();
    let span = ((800.0 / step).round() as usize).max(2);
    let lo = searchsorted(f, bins, 11000.0);
    let top_margin = searchsorted(f, bins, nyq - 400.0);
    let mean = |a: usize, b: usize| {
        let (a, b) = (a.min(bins), b.min(bins));
        if b > a { ls[a..b].iter().sum::<f32>() / (b - a) as f32 } else { f32::NAN }
    };
    let (mut best_drop, mut best_i) = (0f32, None::<usize>);
    for i in lo..(lo + 1).max(top_margin.saturating_sub(span)) {
        let below = mean(i.saturating_sub(span), i);
        let above = mean(i, i + span);
        let drop = below - above;
        if drop > best_drop {
            best_drop = drop;
            best_i = Some(i);
        }
    }
    let above_lvl = best_i.map(|i| mean(i, i + span)).unwrap_or(0.0);
    let cutoff;
    if best_i.is_some() && ((best_drop > 22.0 && above_lvl < -95.0) || (best_drop > 14.0 && above_lvl < -108.0)) {
        cutoff = f(best_i.unwrap());
    } else {
        cutoff = ls.iter().rposition(|&v| v > -100.0).map(f).unwrap_or(nyq);
        best_drop = 0.0;
    }
    let khz = f64::from(cutoff) / 1000.0;
    let has_drop = best_drop > 0.0;
    let (mut kind, text) = if an.sr > 50_000 {
        if khz <= 20.5 && has_drop {
            (Kind::Bad, format!("lossy source — cuts off at {khz:.1} kHz (upsampled from MP3/AAC)"))
        } else if khz <= 27.0 {
            (Kind::Warn, format!("not true hi-res — nothing above {khz:.1} kHz (upsampled from 44.1/48 kHz)"))
        } else {
            (Kind::Good, format!("true hi-res — content up to {khz:.1} kHz"))
        }
    } else if has_drop && khz < 17.0 {
        (Kind::Bad, format!("lossy transcode — hard cutoff at {khz:.1} kHz (≈128 kbps MP3 or lower)"))
    } else if has_drop && khz < 19.6 {
        (Kind::Bad, format!("lossy transcode — hard cutoff at {khz:.1} kHz (≈160–256 kbps MP3/AAC)"))
    } else if has_drop && khz < 21.0 {
        (Kind::Warn, format!("probably lossy — cutoff at {khz:.1} kHz (≈320 kbps MP3 / 256 kbps AAC)"))
    } else {
        (Kind::Good, format!("looks lossless — content up to {khz:.1} kHz"))
    };
    let mut notes = Vec::new();
    if !has_drop && khz < 19.0 && an.sr <= 50_000 {
        notes.push(format!("little content above {khz:.0} kHz but no encoder cliff — normal for older or lo-fi recordings"));
    }
    if an.bits > 16 && lowbits_used == Some(false) {
        notes.push("24-bit container but only 16 bits used (padded)".into());
        if kind == Kind::Good {
            kind = Kind::Warn;
        }
    }
    Verdict { cutoff, drop: best_drop, kind, text, notes }
}

/// The spectrogram as RGB pixels, `cols` × `height`, low frequencies at the bottom.
pub fn spectrum_rgb(an: &Analysis, palette_kind: &str, height: usize) -> (usize, usize, Vec<u8>) {
    let pal = palette(palette_kind);
    let (w, h) = (an.cols, height.max(1));
    let mut out = vec![0u8; w * h * 3];
    let edges: Vec<usize> = (0..=h).map(|i| i * an.bins / h).collect();
    for row in 0..h {
        let (a, b) = (edges[row], edges[row + 1].max(edges[row] + 1).min(an.bins));
        let y = h - 1 - row;
        for c in 0..w {
            let mut m = DB_FLOOR;
            for bin in a..b {
                m = m.max(an.at(bin, c));
            }
            let v = ((m - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0);
            let px = pal[(v * 255.0) as usize];
            let o = (y * w + c) * 3;
            out[o..o + 3].copy_from_slice(&px);
        }
    }
    (w, h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_ends() {
        let p = palette("spek");
        assert_eq!(p[0], [0, 0, 0]);
        assert_eq!(p[255], [255, 255, 255]);
        assert_eq!(palette("mono")[255], [255, 255, 255]);
    }
}
