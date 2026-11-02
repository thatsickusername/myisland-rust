use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    IAudioCaptureClient, IAudioClient, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

pub struct AudioLevels {
    pub levels: [f32; 6],
    pub peak: f32,
    pub is_hearing: bool,
}

impl Default for AudioLevels {
    fn default() -> Self {
        Self {
            levels: [0.15; 6],
            peak: 0.0,
            is_hearing: false,
        }
    }
}

pub struct AudioCaptureService {
    levels_state: Arc<Mutex<AudioLevels>>,
    running: Arc<AtomicBool>,
}

impl AudioCaptureService {
    pub fn start() -> Self {
        let levels_state = Arc::new(Mutex::new(AudioLevels::default()));
        let running = Arc::new(AtomicBool::new(true));

        let levels_clone = levels_state.clone();
        let running_clone = running.clone();

        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }

            let mut loud_peak = 0.12f32;
            let target_freqs = [60.0f32, 150.0, 400.0, 1000.0, 2500.0, 6000.0];

            'outer: while running_clone.load(Ordering::Relaxed) {
                // Initialize default render endpoint
                let (device, meter_opt) = unsafe {
                    let Ok(enumerator): Result<IMMDeviceEnumerator, _> =
                        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    else {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        continue;
                    };
                    let Ok(dev) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        continue;
                    };
                    let meter: Option<IAudioMeterInformation> = dev.Activate(CLSCTX_ALL, None).ok();
                    (dev, meter)
                };

                // Try WASAPI loopback client
                let loopback_setup = unsafe {
                    (|| -> Option<(IAudioClient, IAudioCaptureClient, f32, usize, bool)> {
                        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).ok()?;
                        let pwfx = client.GetMixFormat().ok()?;
                        let sample_rate = (*pwfx).nSamplesPerSec as f32;
                        let channels = (*pwfx).nChannels as usize;
                        let is_float = (*pwfx).wBitsPerSample == 32;

                        let init_res = client.Initialize(
                            AUDCLNT_SHAREMODE_SHARED,
                            AUDCLNT_STREAMFLAGS_LOOPBACK,
                            2000000, // 200ms buffer
                            0,
                            pwfx,
                            None,
                        );

                        if init_res.is_ok() {
                            let capture = client.GetService::<IAudioCaptureClient>().ok()?;
                            let _ = client.Start();
                            Some((client, capture, sample_rate, channels, is_float))
                        } else {
                            None
                        }
                    })()
                };

                if let Some((client, capture, sample_rate, channels, is_float)) = loopback_setup {
                    let mut sample_window = Vec::with_capacity(1024);
                    let mut is_hearing = false;

                    while running_clone.load(Ordering::Relaxed) {
                        let mut packets_read = 0;
                        loop {
                            match unsafe { capture.GetNextPacketSize() } {
                                Ok(size) if size > 0 => {}
                                _ => break,
                            }
                            let mut p_data: *mut u8 = std::ptr::null_mut();
                            let mut num_frames: u32 = 0;
                            let mut flags: u32 = 0;

                            let hr = unsafe {
                                capture.GetBuffer(
                                    &mut p_data,
                                    &mut num_frames,
                                    &mut flags,
                                    None,
                                    None,
                                )
                            };

                            if hr.is_ok() && !p_data.is_null() && num_frames > 0 {
                                packets_read += 1;
                                if (flags & 1) == 0 { // AUDCLNT_BUFFERFLAGS_SILENT == 1
                                    if is_float {
                                        let float_slice = unsafe {
                                            std::slice::from_raw_parts(p_data as *const f32, (num_frames as usize) * channels)
                                        };
                                        // Take mono downmix
                                        for frame in float_slice.chunks_exact(channels) {
                                            let mono = frame.iter().sum::<f32>() / (channels as f32);
                                            sample_window.push(mono);
                                        }
                                    } else {
                                        let pcm_slice = unsafe {
                                            std::slice::from_raw_parts(p_data as *const i16, (num_frames as usize) * channels)
                                        };
                                        for frame in pcm_slice.chunks_exact(channels) {
                                            let mono = (frame.iter().map(|&s| s as f32).sum::<f32>() / (channels as f32)) / 32768.0;
                                            sample_window.push(mono);
                                        }
                                    }
                                }
                                unsafe {
                                    let _ = capture.ReleaseBuffer(num_frames);
                                }
                            } else {
                                break;
                            }
                        }

                        // Process when we have at least 512 samples
                        if sample_window.len() >= 512 {
                            let chunk = &sample_window[sample_window.len() - 512..];
                            let mut raw = [0.0f32; 6];

                            for (i, &f0) in target_freqs.iter().enumerate() {
                                raw[i] = goertzel_mag(chunk, f0, sample_rate);
                            }

                            sample_window.clear();

                            let loud = raw.iter().sum::<f32>() / 6.0;

                            let mut meter_peak = 0.0f32;
                            if let Some(ref m) = meter_opt {
                                if let Ok(p) = unsafe { m.GetPeakValue() } {
                                    meter_peak = p;
                                }
                            } else {
                                meter_peak = loud * 4.0;
                            }

                            loud_peak = loud_peak.max(meter_peak * 0.99).max(0.06);
                            is_hearing = meter_peak > 0.012 || loud > 0.005;

                            let mut current_levels = [0.06f32; 6];
                            if is_hearing {
                                let norm = (meter_peak / loud_peak).clamp(0.2, 1.0);
                                for i in 0..6 {
                                    let freq_mod = (raw[i] * 25.0).clamp(0.4, 1.6);
                                    current_levels[i] = (norm * freq_mod).clamp(0.18, 1.0);
                                }
                            }

                            if let Ok(mut state) = levels_clone.lock() {
                                state.levels = current_levels;
                                state.peak = meter_peak.clamp(0.0, 1.0);
                                state.is_hearing = is_hearing;
                            }
                        }

                        let sleep_dur = if is_hearing || packets_read > 0 { 12 } else { 35 };
                        std::thread::sleep(std::time::Duration::from_millis(sleep_dur));
                    }
                    let _ = unsafe { client.Stop() };
                } else {
                    // Fallback to IAudioMeterInformation polling if loopback failed
                    while running_clone.load(Ordering::Relaxed) {
                        let mut peak = 0.0f32;
                        let mut got_peak = false;
                        if let Some(ref m) = meter_opt {
                            if let Ok(p) = unsafe { m.GetPeakValue() } {
                                peak = p;
                                got_peak = true;
                            }
                        }

                        if !got_peak {
                            std::thread::sleep(std::time::Duration::from_millis(200));
                            continue 'outer;
                        }

                        let is_hearing = peak > 0.012;
                        if let Ok(mut state) = levels_clone.lock() {
                            state.peak = peak;
                            state.is_hearing = is_hearing;
                            if is_hearing {
                                for i in 0..6 {
                                    let factor = 1.0 + 0.3 * ((i as f32) * 1.3).sin();
                                    state.levels[i] = (peak * factor).clamp(0.18, 1.0);
                                }
                            } else {
                                state.levels = [0.06; 6];
                            }
                        }

                        std::thread::sleep(std::time::Duration::from_millis(16));
                    }
                }
            }
        });

        Self {
            levels_state,
            running,
        }
    }

    pub fn get_levels(&self) -> AudioLevels {
        if let Ok(state) = self.levels_state.lock() {
            AudioLevels {
                levels: state.levels,
                peak: state.peak,
                is_hearing: state.is_hearing,
            }
        } else {
            AudioLevels::default()
        }
    }
}

impl Drop for AudioCaptureService {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/// Goertzel algorithm to compute magnitude of a target frequency in O(N)
fn goertzel_mag(samples: &[f32], target_freq: f32, sample_rate: f32) -> f32 {
    let n = samples.len() as f32;
    let k = (0.5 + (n * target_freq / sample_rate)).floor();
    let omega = (2.0 * std::f32::consts::PI * k) / n;
    let coeff = 2.0 * omega.cos();

    let mut q0: f32;
    let mut q1: f32 = 0.0;
    let mut q2: f32 = 0.0;

    for &sample in samples {
        q0 = coeff * q1 - q2 + sample;
        q2 = q1;
        q1 = q0;
    }

    let power = q1 * q1 + q2 * q2 - q1 * q2 * coeff;
    (power.max(0.0).sqrt() / (n / 2.0)).clamp(0.0, 1.0)
}
