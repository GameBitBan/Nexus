//! Output devices. A dedicated thread owns the device stream (cpal streams aren't `Send` on every platform);
//! the decoder asks it to open at a sample rate and gets back the rate the device actually runs at.

use super::{Renderer, Shared};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use crossbeam_channel::{Sender, bounded, unbounded};
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Default)]
pub struct OutputInfo {
    pub device: String,
    pub rate: u32,
    pub channels: u16,
    pub format: String,
    pub exclusive: bool,
    /// true when the device runs at the file's own rate (no resampling)
    pub native_rate: bool,
}

/// Output device names, for settings.
pub fn list_devices() -> Vec<String> {
    let host = cpal::default_host();
    let Ok(devs) = host.output_devices() else { return Vec::new() };
    devs.filter_map(|d| d.description().ok().map(|x| x.name().to_string())).collect()
}

pub(crate) enum OutCmd {
    Open { rate: u32, device: String, exclusive: bool, reply: Sender<Result<u32, String>> },
    Close,
    Quit,
}

pub(crate) struct OutputHandle {
    tx: Sender<OutCmd>,
    pub rate: u32,
    pub open: bool,
}

impl OutputHandle {
    pub fn spawn(shared: Arc<Shared>) -> Self {
        let (tx, rx) = unbounded::<OutCmd>();
        std::thread::Builder::new()
            .name("nexus-output".into())
            .spawn(move || {
                let mut stream: Option<Box<dyn std::any::Any>> = None;
                for cmd in rx {
                    match cmd {
                        OutCmd::Open { rate, device, exclusive, reply } => {
                            stream = None;
                            let res = open(&shared, rate, &device, exclusive);
                            match res {
                                Ok((s, actual)) => {
                                    stream = Some(s);
                                    let _ = reply.send(Ok(actual));
                                }
                                Err(e) => {
                                    let _ = reply.send(Err(e));
                                }
                            }
                        }
                        OutCmd::Close => stream = None,
                        OutCmd::Quit => break,
                    }
                }
                drop(stream);
            })
            .expect("spawn output thread");
        OutputHandle { tx, rate: 0, open: false }
    }

    pub fn open(&mut self, rate: u32, device: &str, exclusive: bool) -> Result<u32, String> {
        let (rtx, rrx) = bounded(1);
        let _ = self.tx.send(OutCmd::Open { rate, device: device.to_string(), exclusive, reply: rtx });
        let r = rrx.recv().map_err(|e| e.to_string())?;
        if let Ok(actual) = r {
            self.rate = actual;
            self.open = true;
        }
        r
    }

    pub fn close(&mut self) {
        let _ = self.tx.send(OutCmd::Close);
        self.open = false;
        self.rate = 0;
    }
}

impl Drop for OutputHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(OutCmd::Quit);
    }
}

fn open(shared: &Arc<Shared>, rate: u32, device: &str, exclusive: bool) -> Result<(Box<dyn std::any::Any>, u32), String> {
    #[cfg(windows)]
    if exclusive {
        match super::wasapi_exclusive::open(shared.clone(), rate, device) {
            Ok(r) => return Ok(r),
            Err(e) => log::warn!("exclusive mode failed ({e}), using shared mode"),
        }
    }
    let _ = exclusive;
    if let Some(path) = device.strip_prefix("file:") {
        return open_file(shared, rate, path);
    }
    open_cpal(shared, rate, device)
}

/// Test output: renders in real time (512-frame blocks) and appends what was played to a raw f32 file.
fn open_file(shared: &Arc<Shared>, rate: u32, path: &str) -> Result<(Box<dyn std::any::Any>, u32), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (s2, stop2) = (shared.clone(), stop.clone());
    std::thread::spawn(move || {
        let mut r = Renderer::new(s2);
        let mut buf = vec![0.0f32; 1024];
        let block = std::time::Duration::from_secs_f64(512.0 / f64::from(rate));
        let mut next = std::time::Instant::now();
        while !stop2.load(Ordering::SeqCst) {
            let n = r.render(&mut buf);
            if n > 0 {
                let bytes: Vec<u8> = buf[..n * 2].iter().flat_map(|x| x.to_le_bytes()).collect();
                let _ = file.write_all(&bytes);
            }
            next += block;
            if let Some(d) = next.checked_duration_since(std::time::Instant::now()) {
                std::thread::sleep(d);
            }
        }
    });
    struct StopOnDrop(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for StopOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    *shared.out_info.lock() = OutputInfo { device: format!("file {path}"), rate, channels: 2, format: "f32".into(), exclusive: false, native_rate: true };
    shared.rate.store(rate, Ordering::SeqCst);
    Ok((Box::new(StopOnDrop(stop)), rate))
}

fn find_device(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    if !name.is_empty() {
        if let Ok(mut devs) = host.output_devices() {
            if let Some(d) = devs.find(|d| d.description().map(|x| x.name() == name).unwrap_or(false)) {
                return Some(d);
            }
        }
    }
    host.default_output_device()
}

fn open_cpal(shared: &Arc<Shared>, want: u32, device: &str) -> Result<(Box<dyn std::any::Any>, u32), String> {
    let host = cpal::default_host();
    let dev = find_device(&host, device).ok_or("no audio output device")?;
    let name = dev.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "default".into());
    // the file's own rate when the device can do it (no resampling), else the device default
    let pref = [SampleFormat::F32, SampleFormat::I32, SampleFormat::I16, SampleFormat::I24];
    let mut chosen = None;
    if let Ok(ranges) = dev.supported_output_configs() {
        let ranges: Vec<_> = ranges.collect();
        'outer: for fmt in pref {
            for ch in [2u16, 1, 4, 6, 8] {
                if let Some(r) = ranges.iter().find(|r| {
                    r.sample_format() == fmt && r.channels() == ch && r.min_sample_rate() <= want && want <= r.max_sample_rate()
                }) {
                    chosen = r.clone().try_with_sample_rate(want);
                    if chosen.is_some() {
                        break 'outer;
                    }
                }
            }
        }
    }
    let supported = match chosen {
        Some(c) => c,
        None => dev.default_output_config().map_err(|e| format!("{name}: {e}"))?,
    };
    let fmt = supported.sample_format();
    let config: StreamConfig = supported.config();
    let actual = config.sample_rate;
    let stream = match fmt {
        SampleFormat::F32 => build::<f32>(&dev, &config, shared.clone()),
        SampleFormat::I32 => build::<i32>(&dev, &config, shared.clone()),
        SampleFormat::I16 => build::<i16>(&dev, &config, shared.clone()),
        SampleFormat::I24 => build::<cpal::I24>(&dev, &config, shared.clone()),
        SampleFormat::U16 => build::<u16>(&dev, &config, shared.clone()),
        SampleFormat::F64 => build::<f64>(&dev, &config, shared.clone()),
        other => return Err(format!("unsupported sample format {other}")),
    }
    .map_err(|e| format!("{name}: {e}"))?;
    stream.play().map_err(|e| format!("{name}: {e}"))?;
    *shared.out_info.lock() = OutputInfo {
        device: name,
        rate: actual,
        channels: config.channels,
        format: format!("{fmt}"),
        exclusive: false,
        native_rate: actual == want,
    };
    shared.rate.store(actual, Ordering::SeqCst);
    Ok((Box::new(stream), actual))
}

fn build<T>(dev: &cpal::Device, config: &StreamConfig, shared: Arc<Shared>) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let mut renderer = Renderer::new(shared.clone());
    let mut stereo: Vec<f32> = vec![0.0; 16384 * 2];
    let lost = shared.clone();
    dev.build_output_stream(
        config.clone(),
        move |data: &mut [T], _| {
            let frames = data.len() / channels;
            if stereo.len() < frames * 2 {
                stereo.resize(frames * 2, 0.0);
            }
            let buf = &mut stereo[..frames * 2];
            renderer.render(buf);
            for (i, frame) in data.chunks_mut(channels).enumerate() {
                let (l, r) = (buf[i * 2], buf[i * 2 + 1]);
                match channels {
                    1 => frame[0] = T::from_sample((l + r) * 0.5),
                    _ => {
                        frame[0] = T::from_sample(l);
                        frame[1] = T::from_sample(r);
                        for s in &mut frame[2..] {
                            *s = T::from_sample(0.0f32);
                        }
                    }
                }
            }
        },
        move |err: cpal::Error| {
            log::warn!("audio device: {err}");
            if matches!(err.kind(), cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::DeviceChanged) {
                lost.device_lost.store(true, Ordering::SeqCst);
            }
        },
        None,
    )
}
