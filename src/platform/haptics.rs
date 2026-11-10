use std::sync::atomic::{AtomicBool, Ordering};

static HAPTICS_ENABLED: AtomicBool = AtomicBool::new(true);

#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(psz_sound: *const u16, hmod: usize, fdw_sound: u32) -> i32;
}

const SND_ASYNC: u32 = 0x0001;
const SND_MEMORY: u32 = 0x0004;

pub fn set_haptics_enabled(enabled: bool) {
    HAPTICS_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_haptics_enabled() -> bool {
    HAPTICS_ENABLED.load(Ordering::Relaxed)
}

/// Generate an in-memory WAV buffer for a short synthesized haptic tick.
fn generate_click_wav() -> Vec<u8> {
    generate_sine_wav(2200.0, 16, 0.45)
}

fn generate_scroll_wav() -> Vec<u8> {
    generate_sine_wav(1200.0, 10, 0.28)
}

fn generate_sine_wav(freq_hz: f32, duration_ms: usize, peak_vol: f32) -> Vec<u8> {
    let sample_rate: u32 = 22050;
    let num_samples = (sample_rate as usize * duration_ms) / 1000;
    let mut pcm: Vec<i16> = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let progress = i as f32 / num_samples.max(1) as f32;
        // Exponential fade envelope for crisp, tactile haptic click
        let env = (1.0 - progress).powf(2.5) * peak_vol;
        let sample = (t * freq_hz * 2.0 * std::f32::consts::PI).sin() * env;
        pcm.push((sample * 32767.0) as i16);
    }

    // Build standard RIFF / WAVE header
    let data_len = (pcm.len() * 2) as u32;
    let riff_chunk_size = 36 + data_len;
    let mut wav = Vec::with_capacity(44 + data_len as usize);

    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_chunk_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size (16 for PCM)
    wav.extend_from_slice(&1u16.to_le_bytes());  // AudioFormat (1 = PCM)
    wav.extend_from_slice(&1u16.to_le_bytes());  // NumChannels (1 = Mono)
    wav.extend_from_slice(&sample_rate.to_le_bytes()); // SampleRate
    let byte_rate = sample_rate * 2;
    wav.extend_from_slice(&byte_rate.to_le_bytes()); // ByteRate
    wav.extend_from_slice(&2u16.to_le_bytes());  // BlockAlign
    wav.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());

    for s in pcm {
        wav.extend_from_slice(&s.to_le_bytes());
    }

    wav
}

pub fn play_click_haptic() {
    if !is_haptics_enabled() {
        return;
    }
    std::thread::spawn(|| {
        static CLICK_WAV: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        let wav = CLICK_WAV.get_or_init(generate_click_wav);
        unsafe {
            PlaySoundW(wav.as_ptr() as *const u16, 0, SND_ASYNC | SND_MEMORY);
        }
    });
}

pub fn play_scroll_haptic() {
    if !is_haptics_enabled() {
        return;
    }
    std::thread::spawn(|| {
        static SCROLL_WAV: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        let wav = SCROLL_WAV.get_or_init(generate_scroll_wav);
        unsafe {
            PlaySoundW(wav.as_ptr() as *const u16, 0, SND_ASYNC | SND_MEMORY);
        }
    });
}
