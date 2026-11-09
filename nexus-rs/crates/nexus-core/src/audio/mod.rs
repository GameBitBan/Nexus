//! The Nexus audio engine.
//!
//! decode (FFmpeg) → ReplayGain → EQ (linear-phase FIR) → ring buffer → output callback
//! (volume / boost, peak limiter, visualizer + Jam taps) → device (cpal; WASAPI exclusive on Windows).
//!
//! Gapless: when a file ends the decoder rolls straight into the queued next file inside the same stream.
//! The output callback never blocks and never allocates.

mod decoder;
pub mod eq;
mod output;
#[cfg(windows)]
mod wasapi_exclusive;

pub use eq::EqSettings;
pub use output::{list_devices, OutputInfo};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

/// Seconds of audio the decoder keeps ready ahead of the device.
pub(crate) const BUFFER_SEC: f64 = 0.30;
/// Ring capacity in samples (interleaved stereo): enough for BUFFER_SEC at 384 kHz plus a FIR block.
pub(crate) const RING_SAMPLES: usize = 1 << 19;
const VIS_LEN: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// a file was opened; its length in seconds
    Loaded { path: String, duration: f64 },
    /// gapless rollover: the device is now playing this (queued) file
    Advanced { path: String },
    /// the last file finished playing
    Ended,
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayGain {
    Off,
    Track,
    Album,
}

/// Settings the decoder reads at the start of each file.
#[derive(Clone, Debug)]
pub struct PlaybackOptions {
    pub replaygain: ReplayGain,
    pub replaygain_preamp: f32,
    pub gapless: bool,
    pub device: String,
    pub exclusive: bool,
}

impl Default for PlaybackOptions {
    fn default() -> Self {
        PlaybackOptions { replaygain: ReplayGain::Off, replaygain_preamp: 0.0, gapless: true, device: String::new(), exclusive: false }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Mark {
    /// played-frame count at which this file (or this seek position) starts sounding
    pub at: u64,
    pub path: Arc<str>,
    pub duration: f64,
    pub offset: f64,
}

/// Something that receives every block that is played (Jam). Called on the audio thread: keep it cheap.
pub type Tap = Box<dyn FnMut(&[f32], u32) + Send>;

pub(crate) struct Shared {
    pub state: AtomicU8,
    pub volume: AtomicU32,
    pub muted: AtomicBool,
    pub played: AtomicU64,
    pub end_at: AtomicU64,
    pub rate: AtomicU32,
    pub marks: Mutex<VecDeque<Mark>>,
    pub consumer: Mutex<Option<rtrb::Consumer<f32>>>,
    pub vis: Mutex<(Vec<f32>, usize)>,
    pub tap: Mutex<Option<Tap>>,
    pub eq: Mutex<EqSettings>,
    pub eq_version: AtomicU64,
    pub next: Mutex<Option<String>>,
    pub opts: Mutex<PlaybackOptions>,
    pub device_lost: AtomicBool,
    pub out_info: Mutex<OutputInfo>,
    pub events: Sender<Event>,
}

impl Shared {
    pub fn set_state(&self, s: State) {
        self.state.store(s as u8, Ordering::SeqCst);
    }

    pub fn state(&self) -> State {
        match self.state.load(Ordering::SeqCst) {
            1 => State::Playing,
            2 => State::Paused,
            _ => State::Stopped,
        }
    }

    fn current_mark(&self) -> Option<Mark> {
        let played = self.played.load(Ordering::SeqCst);
        let mut marks = self.marks.lock();
        while marks.len() > 1 && marks[1].at <= played {
            marks.pop_front();
        }
        marks.front().filter(|m| m.at <= played).cloned().or_else(|| marks.front().cloned())
    }
}

pub(crate) enum Cmd {
    Load { path: String, start: f64, play: bool },
    Seek(f64),
    Stop,
    Reopen,
    Quit,
}

/// Handle to the engine. Cheap to share between threads (all methods take `&self`).
pub struct Engine {
    shared: Arc<Shared>,
    cmd: Sender<Cmd>,
    events: Receiver<Event>,
    last_path: Mutex<Option<Arc<str>>>,
    current_duration: Mutex<f64>,
}

impl Engine {
    pub fn new(opts: PlaybackOptions) -> Self {
        let (ev_tx, ev_rx) = unbounded();
        let shared = Arc::new(Shared {
            state: AtomicU8::new(State::Stopped as u8),
            volume: AtomicU32::new(0.64f32.to_bits()),
            muted: AtomicBool::new(false),
            played: AtomicU64::new(0),
            end_at: AtomicU64::new(u64::MAX),
            rate: AtomicU32::new(0),
            marks: Mutex::new(VecDeque::new()),
            consumer: Mutex::new(None),
            vis: Mutex::new((vec![0.0; VIS_LEN], 0)),
            tap: Mutex::new(None),
            eq: Mutex::new(EqSettings::default()),
            eq_version: AtomicU64::new(1),
            next: Mutex::new(None),
            opts: Mutex::new(opts),
            device_lost: AtomicBool::new(false),
            out_info: Mutex::new(OutputInfo::default()),
            events: ev_tx,
        });
        let (cmd_tx, cmd_rx) = unbounded();
        let s = shared.clone();
        std::thread::Builder::new()
            .name("nexus-decoder".into())
            .spawn(move || decoder::run(s, cmd_rx))
            .expect("spawn decoder thread");
        Engine { shared, cmd: cmd_tx, events: ev_rx, last_path: Mutex::new(None), current_duration: Mutex::new(0.0) }
    }

    /// Opens `path` and (optionally) starts playing from `start` seconds.
    pub fn load(&self, path: &str, start: f64, play: bool) {
        *self.last_path.lock() = Some(Arc::from(path));
        let _ = self.cmd.send(Cmd::Load { path: path.to_string(), start, play });
    }

    pub fn play(&self) {
        if self.shared.state() == State::Paused {
            self.shared.set_state(State::Playing);
        }
    }

    pub fn pause(&self) {
        if self.shared.state() == State::Playing {
            self.shared.set_state(State::Paused);
        }
    }

    pub fn stop(&self) {
        let _ = self.cmd.send(Cmd::Stop);
    }

    pub fn seek(&self, secs: f64) {
        let _ = self.cmd.send(Cmd::Seek(secs.max(0.0)));
    }

    pub fn state(&self) -> State {
        self.shared.state()
    }

    /// Linear gain (1.0 = 100 %; above boosts, the limiter keeps it clean).
    pub fn set_volume(&self, gain: f32) {
        self.shared.volume.store(gain.max(0.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_muted(&self, m: bool) {
        self.shared.muted.store(m, Ordering::Relaxed);
    }

    pub fn muted(&self) -> bool {
        self.shared.muted.load(Ordering::Relaxed)
    }

    pub fn set_eq(&self, eq: EqSettings) {
        *self.shared.eq.lock() = eq;
        self.shared.eq_version.fetch_add(1, Ordering::SeqCst);
    }

    /// The file to roll into when the current one ends (gapless). `None` = stop at the end.
    pub fn set_next(&self, path: Option<String>) {
        *self.shared.next.lock() = path;
    }

    pub fn set_options(&self, opts: PlaybackOptions) {
        let reopen = {
            let mut o = self.shared.opts.lock();
            let changed = o.device != opts.device || o.exclusive != opts.exclusive;
            *o = opts;
            changed
        };
        if reopen {
            let _ = self.cmd.send(Cmd::Reopen);
        }
    }

    /// Position in seconds within the file that is sounding now.
    pub fn position(&self) -> f64 {
        let rate = self.shared.rate.load(Ordering::Relaxed).max(1);
        match self.shared.current_mark() {
            Some(m) => {
                let played = self.shared.played.load(Ordering::SeqCst);
                let p = m.offset + played.saturating_sub(m.at) as f64 / f64::from(rate);
                if m.duration > 0.0 { p.min(m.duration) } else { p }
            }
            None => 0.0,
        }
    }

    pub fn duration(&self) -> f64 {
        *self.current_duration.lock()
    }

    /// The file the device is playing now (differs from the last `load` after a gapless rollover).
    pub fn current_path(&self) -> Option<String> {
        self.shared.current_mark().map(|m| m.path.to_string())
    }

    pub fn output_info(&self) -> OutputInfo {
        self.shared.out_info.lock().clone()
    }

    /// The last `n` mono samples that were played (for visualizers).
    pub fn vis_samples(&self, n: usize) -> Vec<f32> {
        let v = self.shared.vis.lock();
        let (ring, w) = (&v.0, v.1);
        let n = n.min(ring.len());
        (0..n).map(|i| ring[(w + ring.len() - n + i) % ring.len()]).collect()
    }

    pub fn sample_rate(&self) -> u32 {
        self.shared.rate.load(Ordering::Relaxed)
    }

    /// Every played block also goes here (Jam). `None` removes it.
    pub fn set_tap(&self, tap: Option<Tap>) {
        *self.shared.tap.lock() = tap;
    }

    /// Call regularly from the UI: returns what happened since the last call.
    pub fn poll(&self) -> Vec<Event> {
        let mut out: Vec<Event> = self.events.try_iter().collect();
        for e in &out {
            if let Event::Loaded { duration, .. } = e {
                *self.current_duration.lock() = *duration;
            }
        }
        if let Some(m) = self.shared.current_mark() {
            let mut last = self.last_path.lock();
            if last.as_deref() != Some(&*m.path) && m.at <= self.shared.played.load(Ordering::SeqCst) {
                *last = Some(m.path.clone());
                *self.current_duration.lock() = m.duration;
                out.push(Event::Advanced { path: m.path.to_string() });
            }
        }
        let end = self.shared.end_at.load(Ordering::SeqCst);
        if end != u64::MAX && self.shared.played.load(Ordering::SeqCst) >= end && self.state() == State::Playing {
            self.shared.end_at.store(u64::MAX, Ordering::SeqCst);
            self.shared.set_state(State::Stopped);
            out.push(Event::Ended);
        }
        if self.shared.device_lost.swap(false, Ordering::SeqCst) {
            out.push(Event::Error("the audio device went away — trying again".into()));
            let _ = self.cmd.send(Cmd::Reopen);
        }
        out
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Quit);
    }
}

/// The output callback's work, shared by every output backend: pull from the ring, volume with a short
/// ramp, peak limiter (fast attack, slow release), visualizer + Jam taps. `out` is interleaved stereo.
pub(crate) struct Renderer {
    shared: Arc<Shared>,
    gain_cur: f32,
    lim: f32,
}

impl Renderer {
    pub fn new(shared: Arc<Shared>) -> Self {
        Renderer { shared, gain_cur: -1.0, lim: 1.0 }
    }

    /// Fills `out` (interleaved stereo). Returns the number of frames that came from the stream.
    pub fn render(&mut self, out: &mut [f32]) -> usize {
        let frames = out.len() / 2;
        if self.shared.state() != State::Playing {
            out.fill(0.0);
            return 0;
        }
        let Some(mut guard) = self.shared.consumer.try_lock() else {
            out.fill(0.0); // the decoder is flushing for a seek
            return 0;
        };
        let Some(cons) = guard.as_mut() else {
            out.fill(0.0);
            return 0;
        };
        let avail = cons.slots().min(frames * 2) & !1;
        let n = avail / 2;
        if let Ok(chunk) = cons.read_chunk(avail) {
            let (a, b) = chunk.as_slices();
            out[..a.len()].copy_from_slice(a);
            out[a.len()..a.len() + b.len()].copy_from_slice(b);
            chunk.commit_all();
        }
        drop(guard);
        out[n * 2..].fill(0.0);
        self.shared.played.fetch_add(n as u64, Ordering::SeqCst);
        if n == 0 {
            return 0;
        }
        let rate = self.shared.rate.load(Ordering::Relaxed);
        if let Some(mut tap) = self.shared.tap.try_lock() {
            if let Some(f) = tap.as_mut() {
                f(&out[..n * 2], rate);
            }
        }
        if let Some(mut vis) = self.shared.vis.try_lock() {
            let (ring, w) = &mut *vis;
            let len = ring.len();
            for i in 0..n {
                ring[*w] = (out[i * 2] + out[i * 2 + 1]) * 0.5;
                *w = (*w + 1) % len;
            }
        }
        let target = if self.shared.muted.load(Ordering::Relaxed) {
            0.0
        } else {
            f32::from_bits(self.shared.volume.load(Ordering::Relaxed))
        };
        if self.gain_cur < 0.0 {
            self.gain_cur = target;
        }
        let g0 = self.gain_cur;
        let mut peak = 0.0f32;
        for i in 0..n {
            let g = g0 + (target - g0) * (i as f32 / n as f32);
            out[i * 2] *= g;
            out[i * 2 + 1] *= g;
            peak = peak.max(out[i * 2].abs()).max(out[i * 2 + 1].abs());
        }
        self.gain_cur = target;
        let want = if peak > 0.0 { (0.97 / peak).min(1.0) } else { 1.0 };
        let new = if want < self.lim { want } else { want.min(self.lim + 0.0015 * n as f32 / 256.0) };
        if new != 1.0 || self.lim != 1.0 {
            for i in 0..n {
                let l = self.lim + (new - self.lim) * (i as f32 / n as f32);
                out[i * 2] *= l;
                out[i * 2 + 1] *= l;
            }
        }
        self.lim = new;
        for s in &mut out[..n * 2] {
            *s = s.clamp(-1.0, 1.0);
        }
        n
    }
}

/// `-6.5 dB` → 0.47
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// The UI volume (0..=max, 1.0 = 100 %) to a linear gain: perceptual up to 100 %, then straight boost.
pub fn volume_to_gain(v: f32) -> f32 {
    if v <= 1.0 { v * v } else { v }
}
