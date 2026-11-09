//! The decoder thread: FFmpeg → stereo f32 at the device rate → ReplayGain → EQ → ring buffer.

use super::eq::{Fir, impulse};
use super::output::OutputHandle;
use super::{BUFFER_SEC, Cmd, Event, Mark, RING_SAMPLES, ReplayGain, Shared, State};
use crossbeam_channel::Receiver;
use ffmpeg_next as ffmpeg;
use ffmpeg::format::Sample;
use ffmpeg::format::sample::Type as SampleType;
use ffmpeg::{ChannelLayout, codec, frame, media};
use lofty::prelude::*;
use lofty::tag::ItemKey;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// What the thread is doing between files.
struct Chain {
    out: OutputHandle,
    producer: Option<rtrb::Producer<f32>>,
    /// frames pushed into the ring since the stream opened
    pushed: u64,
    /// frames fed into the EQ since the stream opened (ahead of `pushed` by what the EQ holds)
    fed: u64,
    fir: Option<Fir>,
    fir_ver: u64,
    rate: u32,
    scratch: Vec<f32>,
}

enum Outcome {
    /// the file played to its end
    Eof,
    /// a new command interrupted it
    Interrupted(Cmd),
}

pub(crate) fn run(shared: Arc<Shared>, cmds: Receiver<Cmd>) {
    if let Err(e) = ffmpeg::init() {
        let _ = shared.events.send(Event::Error(format!("FFmpeg didn't start: {e}")));
        return;
    }
    ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Fatal);
    let mut chain = Chain {
        out: OutputHandle::spawn(shared.clone()),
        producer: None,
        pushed: 0,
        fed: 0,
        fir: None,
        fir_ver: 0,
        rate: 0,
        scratch: Vec::with_capacity(65536),
    };
    let mut pending: Option<Cmd> = None;
    loop {
        let cmd = match pending.take() {
            Some(c) => c,
            None => match cmds.recv() {
                Ok(c) => c,
                Err(_) => break,
            },
        };
        match cmd {
            Cmd::Quit => break,
            Cmd::Stop => stop(&shared, &mut chain),
            Cmd::Seek(_) => {}
            Cmd::Reopen => {
                chain.out.close();
                chain.rate = 0;
            }
            Cmd::Load { path, start, play } => {
                pending = play_from(&shared, &cmds, &mut chain, path, start, play);
            }
        }
    }
    chain.out.close();
}

fn stop(shared: &Shared, chain: &mut Chain) {
    shared.set_state(State::Stopped);
    drain(shared, chain);
    shared.marks.lock().clear();
    shared.end_at.store(u64::MAX, Ordering::SeqCst);
}

/// Throws away everything buffered (seek / stop) and lines the counters up again.
fn drain(shared: &Shared, chain: &mut Chain) {
    let mut guard = shared.consumer.lock();
    if let Some(c) = guard.as_mut() {
        let n = c.slots();
        if let Ok(ch) = c.read_chunk(n) {
            ch.commit_all();
        }
        shared.played.fetch_add((n / 2) as u64, Ordering::SeqCst);
    }
    drop(guard);
    if let Some(f) = chain.fir.as_mut() {
        f.reset();
    }
    let played = shared.played.load(Ordering::SeqCst);
    chain.pushed = played;
    chain.fed = played;
}

fn wait_drained(shared: &Shared, chain: &Chain, cmds: &Receiver<Cmd>) -> Option<Cmd> {
    loop {
        if shared.played.load(Ordering::SeqCst) >= chain.pushed || shared.state() == State::Stopped {
            return None;
        }
        if let Ok(c) = cmds.recv_timeout(Duration::from_millis(10)) {
            if !matches!(c, Cmd::Seek(_)) {
                return Some(c);
            }
        }
    }
}

/// Makes sure the device runs at `want` Hz (or the nearest it can), reopening it when the rate changes.
fn ensure_output(shared: &Shared, chain: &mut Chain, want: u32, cmds: &Receiver<Cmd>) -> Result<Option<Cmd>, String> {
    if chain.out.open && chain.rate == want {
        return Ok(None);
    }
    if chain.out.open {
        if let Some(c) = wait_drained(shared, chain, cmds) {
            return Ok(Some(c));
        }
        chain.out.close();
    }
    let (prod, cons) = rtrb::RingBuffer::<f32>::new(RING_SAMPLES);
    *shared.consumer.lock() = Some(cons);
    chain.producer = Some(prod);
    shared.played.store(0, Ordering::SeqCst);
    shared.marks.lock().clear();
    chain.pushed = 0;
    chain.fed = 0;
    chain.fir = None;
    chain.fir_ver = 0;
    let (device, exclusive) = {
        let o = shared.opts.lock();
        (o.device.clone(), o.exclusive)
    };
    let actual = chain.out.open(want, &device, exclusive)?;
    chain.rate = actual;
    shared.rate.store(actual, Ordering::SeqCst);
    Ok(None)
}

fn replaygain(shared: &Shared, path: &str) -> f32 {
    let (mode, pre) = {
        let o = shared.opts.lock();
        (o.replaygain, o.replaygain_preamp)
    };
    if mode == ReplayGain::Off {
        return 1.0;
    }
    let Ok(tagged) = lofty::read_from_path(path) else { return 1.0 };
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else { return 1.0 };
    let key = if mode == ReplayGain::Album { ItemKey::ReplayGainAlbumGain } else { ItemKey::ReplayGainTrackGain };
    let v = tag.get_string(key).or_else(|| tag.get_string(ItemKey::ReplayGainTrackGain));
    let db: f32 = v
        .map(|s| s.to_lowercase().replace("db", "").trim().parse().unwrap_or(0.0))
        .unwrap_or(0.0);
    super::db_to_gain(db + pre)
}

struct Source {
    ictx: ffmpeg::format::context::Input,
    dec: codec::decoder::Audio,
    stream: usize,
    tb: f64,
    duration: f64,
    rate: u32,
}

fn open(path: &str) -> Result<Source, String> {
    let ictx = ffmpeg::format::input(&path).map_err(|e| format!("can't open: {e}"))?;
    let st = ictx.streams().best(media::Type::Audio).ok_or("no audio in this file")?;
    let stream = st.index();
    let tb = f64::from(st.time_base());
    let duration = if st.duration() > 0 {
        st.duration() as f64 * tb
    } else if ictx.duration() > 0 {
        ictx.duration() as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)
    } else {
        0.0
    };
    let ctx = codec::context::Context::from_parameters(st.parameters()).map_err(|e| e.to_string())?;
    let dec = ctx.decoder().audio().map_err(|e| format!("no decoder: {e}"))?;
    let rate = dec.rate().max(1);
    Ok(Source { ictx, dec, stream, tb, duration, rate })
}

fn choose_rate(shared: &Shared, file_rate: u32) -> u32 {
    // shared mode on Windows: the mixer runs at its own rate, ask for that (it's what the device default is);
    // everywhere else ask for the file's rate and let the output fall back if the device can't
    let _ = shared;
    file_rate
}

/// Plays `path` from `start`, then rolls into queued files (gapless). Returns a command that interrupted it.
fn play_from(shared: &Arc<Shared>, cmds: &Receiver<Cmd>, chain: &mut Chain, path: String, start: f64, play: bool) -> Option<Cmd> {
    // a new file: drop what was buffered for the old one
    shared.set_state(State::Paused);
    drain(shared, chain);
    shared.marks.lock().clear();
    shared.end_at.store(u64::MAX, Ordering::SeqCst);
    let mut path = path;
    let mut start = start;
    let mut first = true;
    loop {
        let mut src = match open(&path) {
            Ok(s) => s,
            Err(e) => {
                let _ = shared.events.send(Event::Error(format!("{}: {e}", file_name(&path))));
                shared.set_state(State::Stopped);
                return None;
            }
        };
        if first {
            let _ = shared.events.send(Event::Loaded { path: path.clone(), duration: src.duration });
        }
        match ensure_output(shared, chain, choose_rate(shared, src.rate), cmds) {
            Ok(Some(c)) => return Some(c),
            Ok(None) => {}
            Err(e) => {
                let _ = shared.events.send(Event::Error(format!("audio output: {e}")));
                shared.set_state(State::Stopped);
                return None;
            }
        }
        if first {
            shared.set_state(if play { State::Playing } else { State::Paused });
            first = false;
        }
        let outcome = decode_file(shared, cmds, chain, &mut src, &path, start);
        match outcome {
            Outcome::Interrupted(c) => return Some(c),
            Outcome::Eof => {}
        }
        let gapless = shared.opts.lock().gapless;
        let next = shared.next.lock().take();
        match next {
            Some(n) if gapless => {
                path = n;
                start = 0.0;
            }
            Some(n) => {
                // not gapless: let this one finish, then start the next one fresh
                end_stream(shared, chain);
                if let Some(c) = wait_drained(shared, chain, cmds) {
                    return Some(c);
                }
                return Some(Cmd::Load { path: n, start: 0.0, play: true });
            }
            None => {
                end_stream(shared, chain);
                return None;
            }
        }
    }
}

fn end_stream(shared: &Shared, chain: &mut Chain) {
    if let Some(mut f) = chain.fir.take() {
        let mut out = Vec::new();
        f.flush(&mut out);
        let _ = push(shared, chain, &out, None);
    }
    shared.end_at.store(chain.pushed, Ordering::SeqCst);
}

fn file_name(p: &str) -> String {
    std::path::Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string())
}

/// Writes interleaved stereo into the ring, waiting for room. Returns false if the stream went away.
fn push(shared: &Shared, chain: &mut Chain, samples: &[f32], cmds: Option<&Receiver<Cmd>>) -> bool {
    let _ = cmds;
    let mut off = 0;
    while off < samples.len() {
        let Some(prod) = chain.producer.as_mut() else { return false };
        let free = prod.slots() & !1;
        if free == 0 {
            if shared.state() == State::Stopped {
                return false;
            }
            std::thread::sleep(Duration::from_millis(3));
            continue;
        }
        let n = free.min(samples.len() - off);
        if let Ok(mut chunk) = prod.write_chunk(n) {
            let (a, b) = chunk.as_mut_slices();
            a.copy_from_slice(&samples[off..off + a.len()]);
            let al = a.len();
            b.copy_from_slice(&samples[off + al..off + al + b.len()]);
            chunk.commit_all();
        }
        off += n;
        chain.pushed += (n / 2) as u64;
    }
    true
}

fn filled_frames(chain: &Chain) -> usize {
    chain.producer.as_ref().map(|p| (RING_SAMPLES - p.slots()) / 2).unwrap_or(0)
}

fn add_mark(shared: &Shared, chain: &Chain, path: &Arc<str>, duration: f64, offset: f64) {
    let delay = chain.fir.as_ref().map(|f| f.delay() as u64).unwrap_or(0);
    shared.marks.lock().push_back(Mark { at: chain.fed + delay, path: path.clone(), duration, offset });
}

fn decode_file(shared: &Arc<Shared>, cmds: &Receiver<Cmd>, chain: &mut Chain, src: &mut Source, path: &str, start: f64) -> Outcome {
    let path_arc: Arc<str> = Arc::from(path);
    let rg = replaygain(shared, path);
    if start > 0.0 {
        let ts = (start * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
        let _ = src.ictx.seek(ts, ..ts);
        src.dec.flush();
    }
    let mut resampler: Option<ffmpeg::software::resampling::Context> = None;
    let mut need_mark: Option<f64> = Some(start);
    let mut frame = frame::Audio::empty();
    let mut out_frame = frame::Audio::empty();
    let mut eof_sent = false;
    loop {
        // commands between packets
        while let Ok(c) = cmds.try_recv() {
            match c {
                Cmd::Seek(t) => {
                    drain(shared, chain);
                    let ts = (t * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
                    let _ = src.ictx.seek(ts, ..ts);
                    src.dec.flush();
                    resampler = None;
                    eof_sent = false;
                    shared.end_at.store(u64::MAX, Ordering::SeqCst);
                    // drop marks ahead of the play position, then mark the new spot
                    let played = shared.played.load(Ordering::SeqCst);
                    shared.marks.lock().retain(|m| m.at <= played);
                    need_mark = Some(t);
                }
                Cmd::Stop => {
                    stop(shared, chain);
                    return Outcome::Interrupted(Cmd::Seek(-1.0)).normalize();
                }
                other => return Outcome::Interrupted(other),
            }
        }
        if shared.state() == State::Stopped {
            return Outcome::Interrupted(Cmd::Seek(-1.0)).normalize();
        }
        let target = (f64::from(chain.rate) * BUFFER_SEC) as usize;
        if filled_frames(chain) > target {
            std::thread::sleep(Duration::from_millis(4));
            continue;
        }
        // EQ changes take effect within one block
        let ver = shared.eq_version.load(Ordering::SeqCst);
        if ver != chain.fir_ver {
            chain.fir_ver = ver;
            let eq = shared.eq.lock().clone();
            if eq.on {
                let mut f = Fir::new(&impulse(chain.rate, &eq.bands, eq.preamp));
                if let Some(old) = chain.fir.as_mut() {
                    f.take_state(old);
                } else {
                    // the EQ adds a fixed delay: line the position marks up with it
                    chain.fed = chain.pushed;
                }
                chain.fir = Some(f);
            } else if let Some(mut old) = chain.fir.take() {
                let mut tail = Vec::new();
                old.flush(&mut tail);
                let keep = tail.len().min(old.delay() * 2);
                if !push(shared, chain, &tail[..keep], None) {
                    return Outcome::Eof;
                }
                chain.fed = chain.pushed;
            }
        }
        // one packet → frames
        let mut got_frames = false;
        if !eof_sent {
            let mut packet = ffmpeg::Packet::empty();
            match packet.read(&mut src.ictx) {
                Ok(()) => {
                    if packet.stream() != src.stream {
                        continue;
                    }
                    if src.dec.send_packet(&packet).is_err() {
                        continue; // a damaged packet: skip it
                    }
                }
                Err(ffmpeg::Error::Eof) => {
                    let _ = src.dec.send_eof();
                    eof_sent = true;
                }
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => continue,
                Err(_) => {
                    let _ = src.dec.send_eof();
                    eof_sent = true;
                }
            }
        }
        while src.dec.receive_frame(&mut frame).is_ok() {
            got_frames = true;
            if frame.channel_layout().is_empty() || frame.channel_layout().channels() == 0 {
                let ch = i32::from(frame.channels().max(1));
                frame.set_channel_layout(ChannelLayout::default(ch));
            }
            if let Some(t) = need_mark.take() {
                let at = frame.timestamp().map(|ts| ts as f64 * src.tb).filter(|&x| x >= 0.0 && t > 0.0).unwrap_or(t);
                add_mark(shared, chain, &path_arc, src.duration, at);
            }
            if resampler.is_none() {
                match frame.resampler(Sample::F32(SampleType::Packed), ChannelLayout::STEREO, chain.rate) {
                    Ok(r) => resampler = Some(r),
                    Err(e) => {
                        let _ = shared.events.send(Event::Error(format!("{}: resampler: {e}", file_name(path))));
                        return Outcome::Eof;
                    }
                }
            }
            let res = resampler.as_mut().expect("just made");
            if res.run(&frame, &mut out_frame).is_err() {
                continue;
            }
            if !emit(shared, chain, &out_frame, rg) {
                return Outcome::Eof;
            }
        }
        if eof_sent && !got_frames {
            break;
        }
    }
    // what the resampler still holds
    if let Some(res) = resampler.as_mut() {
        while let Ok(Some(_)) = res.flush(&mut out_frame) {
            if out_frame.samples() == 0 || !emit(shared, chain, &out_frame, rg) {
                break;
            }
        }
    }
    if need_mark.is_some() {
        add_mark(shared, chain, &path_arc, src.duration, start);
    }
    Outcome::Eof
}

impl Outcome {
    /// `Interrupted(Seek(-1))` means "stopped, nothing more to do".
    fn normalize(self) -> Outcome {
        match self {
            Outcome::Interrupted(Cmd::Seek(t)) if t < 0.0 => Outcome::Interrupted(Cmd::Stop),
            o => o,
        }
    }
}

fn emit(shared: &Shared, chain: &mut Chain, f: &frame::Audio, rg: f32) -> bool {
    let n = f.samples();
    if n == 0 {
        return true;
    }
    let bytes = &f.data(0)[..n * 2 * 4];
    let mut samples: Vec<f32> = bytes.chunks_exact(4).map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])).collect();
    if rg != 1.0 {
        for s in &mut samples {
            *s *= rg;
        }
    }
    chain.fed += n as u64;
    match chain.fir.as_mut() {
        Some(fir) => {
            let mut out = std::mem::take(&mut chain.scratch);
            out.clear();
            fir.process(&samples, &mut out);
            let ok = push(shared, chain, &out, None);
            chain.scratch = out;
            ok
        }
        None => push(shared, chain, &samples, None),
    }
}
