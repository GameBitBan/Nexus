//! Jam audio, both ends.
//!
//! Host: the engine's tap (every block that's played, before volume) → a ring → the encoder thread, which
//! every 20 ms resamples to 48 kHz and encodes one Opus frame (320 kbps, music mode — the same as Nexus 1)
//! for the WebRTC track. When nothing plays it sends silence, so listeners stay in step.
//!
//! Guest: Opus packets → FFmpeg's decoder → resampled to the output device's rate → a ring → its own output
//! stream, which waits for 0.3 s of sound before starting (and again after a hiccup) and drops sound when it
//! falls more than a second behind. Volume and "pause just for me" are the listener's own.

use crate::audio::{Tap, open_stereo};
use ffmpeg::format::{Sample, sample::Type as SampleType};
use ffmpeg::{ChannelLayout, frame};
use ffmpeg_next as ffmpeg;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

pub const RATE: u32 = 48_000;
pub const FRAME: usize = 960;
const BITRATE: usize = 320_000;

// ---------------------------------------------------------------- host

/// What the engine's tap fills; the encoder thread drains it.
pub struct Feed {
    cons: rtrb::Consumer<f32>,
    rate: Arc<AtomicU32>,
}

/// The tap to give the engine, and the other end of it.
pub fn tap() -> (Tap, Feed) {
    let (mut prod, cons) = rtrb::RingBuffer::<f32>::new(1 << 19);
    let rate = Arc::new(AtomicU32::new(44_100));
    let r2 = rate.clone();
    let tap: Tap = Box::new(move |data: &[f32], sr: u32| {
        r2.store(sr, Ordering::Relaxed);
        let n = data.len().min(prod.slots()) & !1;
        if n > 0
            && let Ok(chunk) = prod.write_chunk_uninit(n) {
                chunk.fill_from_iter(data[..n].iter().copied());
            }
    });
    (tap, Feed { cons, rate })
}

/// Opus via FFmpeg: libopus when the build has it (every build Nexus ships with), else FFmpeg's own.
pub struct OpusEncoder {
    enc: ffmpeg::encoder::Audio,
    fmt: Sample,
    pts: i64,
}

impl OpusEncoder {
    pub fn new() -> Result<Self, String> {
        ffmpeg::init().map_err(|e| e.to_string())?;
        let (codec, native) = match ffmpeg::encoder::find_by_name("libopus") {
            Some(c) => (c, false),
            None => (ffmpeg::encoder::find(ffmpeg::codec::Id::OPUS).ok_or("this FFmpeg has no Opus encoder")?, true),
        };
        let mut enc = ffmpeg::codec::context::Context::new_with_codec(codec).encoder().audio().map_err(|e| e.to_string())?;
        let fmt = if native { Sample::F32(SampleType::Planar) } else { Sample::I16(SampleType::Packed) };
        enc.set_rate(RATE as i32);
        enc.set_channel_layout(ChannelLayout::STEREO);
        enc.set_format(fmt);
        enc.set_bit_rate(BITRATE);
        enc.set_time_base((1, RATE as i32));
        let mut opts = ffmpeg::Dictionary::new();
        if native {
            opts.set("strict", "experimental");
        } else {
            opts.set("application", "audio");
            opts.set("frame_duration", "20");
        }
        let enc = enc.open_as_with(codec, opts).map_err(|e| format!("opus encoder: {e}"))?;
        Ok(OpusEncoder { enc, fmt, pts: 0 })
    }

    /// One 20 ms frame (960 interleaved stereo f32 frames) → the Opus packets that came out.
    pub fn encode(&mut self, pcm: &[f32]) -> Vec<Vec<u8>> {
        let mut f = frame::Audio::new(self.fmt, FRAME, ChannelLayout::STEREO);
        f.set_rate(RATE);
        f.set_pts(Some(self.pts));
        self.pts += FRAME as i64;
        match self.fmt {
            Sample::I16(_) => {
                let d = f.data_mut(0);
                for (i, s) in pcm.iter().take(FRAME * 2).enumerate() {
                    let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                    d[i * 2..i * 2 + 2].copy_from_slice(&v.to_ne_bytes());
                }
            }
            _ => {
                for ch in 0..2 {
                    let d = f.data_mut(ch);
                    for i in 0..FRAME {
                        let v = pcm.get(i * 2 + ch).copied().unwrap_or(0.0);
                        d[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
                    }
                }
            }
        }
        let mut out = Vec::new();
        if self.enc.send_frame(&f).is_ok() {
            let mut p = ffmpeg::Packet::empty();
            while self.enc.receive_packet(&mut p).is_ok() {
                if let Some(d) = p.data() {
                    out.push(d.to_vec());
                }
            }
        }
        out
    }
}

impl Drop for OpusEncoder {
    fn drop(&mut self) {
        // drain the encoder's lookahead quietly (otherwise libopus complains on close)
        if self.enc.send_eof().is_ok() {
            let mut p = ffmpeg::Packet::empty();
            while self.enc.receive_packet(&mut p).is_ok() {}
        }
    }
}

/// Interleaved stereo f32 at any rate → interleaved stereo f32 at `to` Hz (FFmpeg's resampler).
pub struct Resampler {
    from: u32,
    to: u32,
    swr: Option<ffmpeg::software::resampling::Context>,
    in_fmt: Sample,
}

impl Resampler {
    pub fn new(to: u32) -> Self {
        Resampler { from: 0, to, swr: None, in_fmt: Sample::F32(SampleType::Packed) }
    }

    /// `planar`: the input is FFmpeg's planar float (what decoders hand out) instead of interleaved.
    fn ensure(&mut self, from: u32, in_fmt: Sample) {
        if self.swr.is_some() && self.from == from && self.in_fmt == in_fmt {
            return;
        }
        self.from = from;
        self.in_fmt = in_fmt;
        self.swr = ffmpeg::software::resampling::Context::get(in_fmt, ChannelLayout::STEREO, from, Sample::F32(SampleType::Packed), ChannelLayout::STEREO, self.to).ok();
    }

    /// Interleaved stereo in, appended to `out`.
    pub fn push(&mut self, pcm: &[f32], from: u32, out: &mut Vec<f32>) {
        let n = pcm.len() / 2;
        if n == 0 {
            return;
        }
        if from == self.to {
            out.extend_from_slice(&pcm[..n * 2]);
            return;
        }
        let mut f = frame::Audio::new(Sample::F32(SampleType::Packed), n, ChannelLayout::STEREO);
        f.set_rate(from);
        let d = f.data_mut(0);
        for (i, s) in pcm[..n * 2].iter().enumerate() {
            d[i * 4..i * 4 + 4].copy_from_slice(&s.to_ne_bytes());
        }
        self.run(&f, from, out);
    }

    /// A decoded frame (any of FFmpeg's float layouts, stereo), appended to `out` as interleaved stereo.
    pub fn push_frame(&mut self, f: &frame::Audio, out: &mut Vec<f32>) {
        if f.samples() == 0 {
            return;
        }
        let rate = f.rate();
        if rate == self.to && f.format() == Sample::F32(SampleType::Packed) {
            let b = &f.data(0)[..f.samples() * 8];
            out.extend(b.chunks_exact(4).map(|x| f32::from_ne_bytes([x[0], x[1], x[2], x[3]])));
            return;
        }
        self.run(f, rate, out);
    }

    fn run(&mut self, f: &frame::Audio, from: u32, out: &mut Vec<f32>) {
        self.ensure(from, f.format());
        let Some(swr) = self.swr.as_mut() else { return };
        let cap = f.samples() * self.to as usize / from.max(1) as usize + 256;
        let mut o = frame::Audio::new(Sample::F32(SampleType::Packed), cap, ChannelLayout::STEREO);
        if swr.run(f, &mut o).is_err() {
            self.swr = None;
            return;
        }
        let got = o.samples();
        let b = &o.data(0)[..got * 8];
        out.extend(b.chunks_exact(4).map(|x| f32::from_ne_bytes([x[0], x[1], x[2], x[3]])));
    }
}

/// The host's encoder thread: every 20 ms, one Opus frame to `send` (unless `idle()` says nobody's
/// listening). Stops when `stop` is set.
pub fn run_encoder(mut feed: Feed, stop: Arc<AtomicBool>, idle: impl Fn() -> bool, mut send: impl FnMut(Vec<u8>)) {
    let mut enc = match OpusEncoder::new() {
        Ok(e) => e,
        Err(e) => {
            log::warn!("jam: {e}");
            return;
        }
    };
    let mut res = Resampler::new(RATE);
    let mut raw: Vec<f32> = Vec::with_capacity(1 << 16);
    let mut buf: Vec<f32> = Vec::with_capacity(RATE as usize * 2);
    let step = Duration::from_millis(20);
    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        // what was played since last time
        raw.clear();
        let avail = feed.cons.slots() & !1;
        if avail > 0
            && let Ok(chunk) = feed.cons.read_chunk(avail) {
                let (a, b) = chunk.as_slices();
                raw.extend_from_slice(a);
                raw.extend_from_slice(b);
                chunk.commit_all();
            }
        res.push(&raw, feed.rate.load(Ordering::Relaxed), &mut buf);
        // running behind (a hiccup on this end): drop the backlog so the delay stays short
        if buf.len() > RATE as usize * 2 * 6 / 10 {
            let keep = RATE as usize * 2 * 2 / 10;
            buf.drain(..buf.len() - keep);
        }
        let mut pcm = [0f32; FRAME * 2];
        let n = buf.len().min(FRAME * 2);
        pcm[..n].copy_from_slice(&buf[..n]);
        buf.drain(..n);
        if !idle() {
            for p in enc.encode(&pcm) {
                send(p);
            }
        }
        next += step;
        let now = Instant::now();
        match next.checked_duration_since(now) {
            Some(d) => std::thread::sleep(d),
            None if now - next > Duration::from_millis(200) => next = now, // asleep for a while: don't burst
            None => {}
        }
    }
}

// ---------------------------------------------------------------- guest

/// The listener's own controls, shared with the output callback.
pub struct GuestCtl {
    pub volume: AtomicU32,
    pub paused: AtomicBool,
    pub muted: AtomicBool,
    pub level: AtomicU32,
    vis: Mutex<(Vec<f32>, usize)>,
}

impl GuestCtl {
    pub fn new(volume: f32) -> Arc<Self> {
        Arc::new(GuestCtl {
            volume: AtomicU32::new(volume.to_bits()),
            paused: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            level: AtomicU32::new(0),
            vis: Mutex::new((vec![0.0; 8192], 0)),
        })
    }

    pub fn set_volume(&self, gain: f32) {
        self.volume.store(gain.to_bits(), Ordering::Relaxed);
    }

    /// The last `n` mono samples heard (visualizer).
    pub fn vis_samples(&self, n: usize) -> Vec<f32> {
        let v = self.vis.lock();
        let (ring, w) = (&v.0, v.1);
        let n = n.min(ring.len());
        (0..n).map(|i| ring[(w + ring.len() - n + i) % ring.len()]).collect()
    }
}

/// The listener's output: owns its device on its own thread (cpal streams aren't `Send` everywhere).
pub struct GuestOutput {
    prod: rtrb::Producer<f32>,
    pub rate: u32,
    stop: Arc<AtomicBool>,
}

impl GuestOutput {
    pub fn open(device: &str, ctl: Arc<GuestCtl>) -> Result<Self, String> {
        let (prod, mut cons) = rtrb::RingBuffer::<f32>::new(1 << 19);
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = crossbeam_channel::bounded::<Result<u32, String>>(1);
        let dev = device.to_string();
        let stop2 = stop.clone();
        std::thread::Builder::new()
            .name("nexus-jam-out".into())
            .spawn(move || {
                let mut primed = false;
                let mut rate = RATE;
                let rate_cell = Arc::new(AtomicU32::new(RATE));
                let rc = rate_cell.clone();
                let render = move |out: &mut [f32]| -> usize {
                    let frames = out.len() / 2;
                    let rate = rc.load(Ordering::Relaxed) as usize;
                    let have = cons.slots() / 2;
                    // a second behind (clock drift, a burst after a stall): skip ahead
                    if have > rate {
                        let skip = (have - rate * 3 / 10) * 2;
                        if let Ok(c) = cons.read_chunk(skip) {
                            c.commit_all();
                        }
                    }
                    let have = cons.slots() / 2;
                    if !primed {
                        primed = have >= rate * 3 / 10;
                    }
                    if !primed {
                        out.fill(0.0);
                        ctl.level.store(0, Ordering::Relaxed);
                        return frames;
                    }
                    let n = have.min(frames);
                    if let Ok(c) = cons.read_chunk(n * 2) {
                        let (a, b) = c.as_slices();
                        out[..a.len()].copy_from_slice(a);
                        out[a.len()..a.len() + b.len()].copy_from_slice(b);
                        c.commit_all();
                    }
                    out[n * 2..].fill(0.0);
                    if n < frames {
                        primed = false; // ran dry: build up a little again
                    }
                    if let Some(mut vis) = ctl.vis.try_lock() {
                        let (ring, w) = &mut *vis;
                        let len = ring.len();
                        for i in 0..n {
                            ring[*w] = (out[i * 2] + out[i * 2 + 1]) * 0.5;
                            *w = (*w + 1) % len;
                        }
                    }
                    if ctl.paused.load(Ordering::Relaxed) || ctl.muted.load(Ordering::Relaxed) {
                        out.fill(0.0);
                        ctl.level.store(0, Ordering::Relaxed);
                        return frames;
                    }
                    let g = f32::from_bits(ctl.volume.load(Ordering::Relaxed));
                    let mut peak = 0f32;
                    for s in &mut out[..n * 2] {
                        *s = (*s * g).clamp(-1.0, 1.0);
                        peak = peak.max(s.abs());
                    }
                    ctl.level.store(peak.to_bits(), Ordering::Relaxed);
                    frames
                };
                match open_stereo(&dev, RATE, render, || {}) {
                    Ok((stream, actual, _info)) => {
                        rate = actual;
                        rate_cell.store(actual, Ordering::Relaxed);
                        let _ = tx.send(Ok(actual));
                        while !stop2.load(Ordering::Relaxed) {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                    }
                }
                let _ = rate;
            })
            .map_err(|e| e.to_string())?;
        let rate = rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "the audio device didn't open".to_string())??;
        Ok(GuestOutput { prod, rate, stop })
    }

    /// Interleaved stereo at `self.rate`.
    pub fn push(&mut self, pcm: &[f32]) {
        let n = pcm.len().min(self.prod.slots()) & !1;
        if n > 0
            && let Ok(chunk) = self.prod.write_chunk_uninit(n) {
                chunk.fill_from_iter(pcm[..n].iter().copied());
            }
    }
}

impl Drop for GuestOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Opus packets → PCM for the output.
pub struct OpusDecoder {
    dec: ffmpeg::decoder::Audio,
    res: Resampler,
    frame: frame::Audio,
}

impl OpusDecoder {
    pub fn new(out_rate: u32) -> Result<Self, String> {
        ffmpeg::init().map_err(|e| e.to_string())?;
        let codec = ffmpeg::decoder::find(ffmpeg::codec::Id::OPUS).ok_or("this FFmpeg has no Opus decoder")?;
        let mut ctx = ffmpeg::codec::context::Context::new_with_codec(codec);
        // no OpusHead from WebRTC: tell the decoder it's 48 kHz stereo
        unsafe {
            let p = ctx.as_mut_ptr();
            (*p).sample_rate = RATE as i32;
            ffmpeg::ffi::av_channel_layout_default(&mut (*p).ch_layout, 2);
        }
        let dec = ctx.decoder().audio().map_err(|e| format!("opus decoder: {e}"))?;
        Ok(OpusDecoder { dec, res: Resampler::new(out_rate), frame: frame::Audio::empty() })
    }

    /// One RTP payload → interleaved stereo at the output rate, appended to `out`.
    pub fn decode(&mut self, payload: &[u8], out: &mut Vec<f32>) {
        if payload.is_empty() {
            return;
        }
        let pkt = ffmpeg::Packet::copy(payload);
        if self.dec.send_packet(&pkt).is_err() {
            return;
        }
        while self.dec.receive_frame(&mut self.frame).is_ok() {
            if self.frame.channel_layout().channels() == 1 {
                // mono stream: duplicate to both sides
                let n = self.frame.samples();
                let m: Vec<f32> = match self.frame.format() {
                    Sample::F32(_) => self.frame.data(0)[..n * 4].chunks_exact(4).map(|x| f32::from_ne_bytes([x[0], x[1], x[2], x[3]])).collect(),
                    _ => continue,
                };
                let st: Vec<f32> = m.iter().flat_map(|&x| [x, x]).collect();
                let rate = self.frame.rate();
                self.res.push(&st, rate, out);
                continue;
            }
            self.res.push_frame(&self.frame, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_round_trip() {
        let Ok(mut enc) = OpusEncoder::new() else { return };
        let mut dec = OpusDecoder::new(44_100).expect("decoder");
        let mut out = Vec::new();
        for k in 0..50 {
            let pcm: Vec<f32> = (0..FRAME)
                .flat_map(|i| {
                    let t = (k * FRAME + i) as f32 / RATE as f32;
                    let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
                    [v, v]
                })
                .collect();
            for p in enc.encode(&pcm) {
                dec.decode(&p, &mut out);
            }
        }
        // a second of sound at 48 kHz → about 44100 frames, with the codec delay and resampler tail
        let frames = out.len() / 2;
        assert!(frames > 40_000 && frames < 46_000, "{frames}");
        let peak = out.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.4 && peak < 0.7, "{peak}");
    }
}
