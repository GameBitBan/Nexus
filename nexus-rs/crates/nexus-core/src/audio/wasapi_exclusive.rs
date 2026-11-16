//! WASAPI exclusive mode (Windows): Nexus takes the device, skipping the Windows mixer, so the output is
//! bit-perfect with EQ off and volume at 100 %. Same idea as Nexus 1's sounddevice `WasapiSettings(exclusive)`.
//!
//! The device runs at the file's own rate when it can (else the nearest common rate, and the decoder
//! resamples), in the best integer format it takes (32, 24-in-32, 24, 16 bit; float as a last resort).
//! Everything COM happens on one thread; the returned guard stops it when dropped. Any failure to open is
//! an `Err`, and the caller falls back to shared mode.

use super::{OutputInfo, Renderer, Shared};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

/// HRESULT AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED
const NOT_ALIGNED: u32 = 0x8889_0019;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fmt {
    I32,
    I24In32,
    I24,
    I16,
    F32,
}

impl Fmt {
    fn wave(self, rate: u32) -> WaveFormat {
        let (store, valid, kind) = match self {
            Fmt::I32 => (32, 32, SampleType::Int),
            Fmt::I24In32 => (32, 24, SampleType::Int),
            Fmt::I24 => (24, 24, SampleType::Int),
            Fmt::I16 => (16, 16, SampleType::Int),
            Fmt::F32 => (32, 32, SampleType::Float),
        };
        WaveFormat::new(store, valid, kind, rate as usize, 2, None)
    }

    fn bytes(self) -> usize {
        match self {
            Fmt::I24 => 3,
            Fmt::I16 => 2,
            _ => 4,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Fmt::I32 => "i32",
            Fmt::I24In32 => "i24 (in 32)",
            Fmt::I24 => "i24",
            Fmt::I16 => "i16",
            Fmt::F32 => "f32",
        }
    }

    /// f32 → device bytes. Scales by 2^(bits-1) like FFmpeg's int → float, so integer sources come back
    /// out unchanged.
    fn write(self, x: f32, out: &mut Vec<u8>) {
        let x = f64::from(x);
        match self {
            Fmt::I32 => out.extend_from_slice(&((x * 2_147_483_648.0).round().clamp(-2_147_483_648.0, 2_147_483_647.0) as i32).to_le_bytes()),
            Fmt::I24In32 => {
                let v = (x * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
                out.extend_from_slice(&(v << 8).to_le_bytes());
            }
            Fmt::I24 => {
                let v = (x * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
                out.extend_from_slice(&v.to_le_bytes()[..3]);
            }
            Fmt::I16 => out.extend_from_slice(&((x * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16).to_le_bytes()),
            Fmt::F32 => out.extend_from_slice(&(x as f32).to_le_bytes()),
        }
    }
}

struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

pub(crate) fn open(shared: Arc<Shared>, rate: u32, device: &str) -> Result<(Box<dyn std::any::Any>, u32), String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = crossbeam_channel::bounded::<Result<u32, String>>(1);
    let (stop2, dev) = (stop.clone(), device.to_string());
    std::thread::Builder::new()
        .name("nexus-wasapi".into())
        .spawn(move || run(shared, rate, &dev, stop2, tx))
        .map_err(|e| e.to_string())?;
    let actual = rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "the device didn't answer".to_string())??;
    Ok((Box::new(StopOnDrop(stop)), actual))
}

fn run(shared: Arc<Shared>, want: u32, device: &str, stop: Arc<AtomicBool>, tx: crossbeam_channel::Sender<Result<u32, String>>) {
    let _ = wasapi::initialize_mta();
    let fail = |e: String| {
        let _ = tx.send(Err(e));
    };
    let en = match DeviceEnumerator::new() {
        Ok(e) => e,
        Err(e) => return fail(e.to_string()),
    };
    // the device by its name (as settings shows it), else the default
    let mut dev = None;
    if !device.is_empty() {
        if let Ok(col) = en.get_device_collection(Direction::Render) {
            for d in (&col).into_iter().flatten() {
                if d.get_friendlyname().map(|n| n == device).unwrap_or(false) {
                    dev = Some(d);
                    break;
                }
            }
        }
    }
    let dev = match dev.map(Ok).unwrap_or_else(|| en.get_default_device(Direction::Render)) {
        Ok(d) => d,
        Err(e) => return fail(e.to_string()),
    };
    let name = dev.get_friendlyname().unwrap_or_else(|_| "default".into());
    let mut client = match dev.get_iaudioclient() {
        Ok(c) => c,
        Err(e) => return fail(e.to_string()),
    };
    // the file's rate first, then the common ones nearest to it
    let mut rates = vec![want];
    let mut common = [44_100u32, 48_000, 88_200, 96_000, 176_400, 192_000];
    common.sort_by_key(|r| (i64::from(*r) - i64::from(want)).abs() + if r % 44_100 == want % 44_100 { 0 } else { 1_000_000 });
    rates.extend(common.iter().filter(|r| **r != want));
    let order = [Fmt::I32, Fmt::I24In32, Fmt::I24, Fmt::I16, Fmt::F32];
    let mut found = None;
    'outer: for r in rates {
        for f in order {
            if let Ok(w) = client.is_supported_exclusive_with_quirks(&f.wave(r)) {
                found = Some((f, r, w));
                break 'outer;
            }
        }
    }
    let Some((fmt, rate, wave)) = found else { return fail(format!("{name} doesn't take any format in exclusive mode")) };
    let period = match client.calculate_aligned_period_near(100_000, Some(128), &wave) {
        Ok(p) => p,
        Err(e) => return fail(e.to_string()),
    };
    if let Err(e) = client.initialize_client(&wave, Direction::Render, StreamMode::EventsExclusive { period_hns: period }, None) {
        let unaligned = matches!(&e, wasapi::WasapiError::Windows(w) if w.code().0 as u32 == NOT_ALIGNED);
        if !unaligned {
            return fail(format!("{name}: {e}"));
        }
        // the documented recovery: use the aligned buffer size the device suggests, on a new client
        let frames = client.get_buffer_size().unwrap_or(0);
        let aligned = wasapi::calculate_period_100ns(i64::from(frames), i64::from(rate));
        client = match dev.get_iaudioclient() {
            Ok(c) => c,
            Err(e) => return fail(e.to_string()),
        };
        if let Err(e) = client.initialize_client(&wave, Direction::Render, StreamMode::EventsExclusive { period_hns: aligned }, None) {
            return fail(format!("{name}: {e}"));
        }
    }
    let (event, render) = match (client.set_get_eventhandle(), client.get_audiorenderclient()) {
        (Ok(h), Ok(r)) => (h, r),
        (Err(e), _) | (_, Err(e)) => return fail(e.to_string()),
    };
    *shared.out_info.lock() = OutputInfo { device: name.clone(), rate, channels: 2, format: fmt.name().into(), exclusive: true, native_rate: rate == want };
    shared.rate.store(rate, Ordering::SeqCst);
    let _ = tx.send(Ok(rate));

    let mut renderer = Renderer::new(shared.clone());
    let mut stereo: Vec<f32> = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut fill = |frames: usize| -> bool {
        stereo.resize(frames * 2, 0.0);
        renderer.render(&mut stereo);
        bytes.clear();
        bytes.reserve(frames * 2 * fmt.bytes());
        for &s in &stereo {
            fmt.write(s, &mut bytes);
        }
        render.write_to_device(&bytes, None).is_ok()
    };
    // a full buffer before starting, so it starts with sound rather than a gap
    let frames = client.get_available_space_in_frames().unwrap_or(0) as usize;
    if frames > 0 && !fill(frames) {
        shared.device_lost.store(true, Ordering::SeqCst);
        return;
    }
    if client.start_stream().is_err() {
        shared.device_lost.store(true, Ordering::SeqCst);
        return;
    }
    while !stop.load(Ordering::SeqCst) {
        if event.wait_for_event(1000).is_err() {
            // unplugged, or another app took it
            log::warn!("exclusive output stopped answering");
            shared.device_lost.store(true, Ordering::SeqCst);
            break;
        }
        let frames = match client.get_available_space_in_frames() {
            Ok(f) => f as usize,
            Err(_) => {
                shared.device_lost.store(true, Ordering::SeqCst);
                break;
            }
        };
        if frames > 0 && !fill(frames) {
            shared.device_lost.store(true, Ordering::SeqCst);
            break;
        }
    }
    let _ = client.stop_stream();
}
