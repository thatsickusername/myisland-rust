use serde::Deserialize;
use std::collections::HashMap;
use std::time::Instant;

use crate::config::{COMPACT_LYRICS_MAX_W, COMPACT_LYRICS_MIN_W};

#[derive(Debug, Clone)]
pub struct LyricLine {
    pub t: u64, // milliseconds
    pub text: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ActiveLyric {
    pub text: Option<String>,
    pub previous: Option<String>,
    pub k: f32, // 0.0 to 1.0
}

#[derive(Debug, Clone)]
pub struct LyricsState {
    pub current_key: String,
    pub lines: Vec<LyricLine>,
    pub current_index: i32,
    pub is_gap: bool,
    pub changed_at: Instant,
    pub previous_text: Option<String>,
    pub widest_px: f32,
    pub is_loading: bool,
    pub hold_until: Instant,
    cache: HashMap<String, Vec<LyricLine>>,
}

#[derive(Deserialize)]
struct LrclibResponse {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
}

#[derive(Deserialize)]
struct LrclibSearchItem {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    duration: Option<f64>,
}

#[derive(Deserialize)]
struct ItunesResponse {
    results: Vec<ItunesTrack>,
}

#[derive(Deserialize)]
struct ItunesTrack {
    #[serde(rename = "artworkUrl100")]
    artwork_url_100: Option<String>,
}

pub fn clean_title_and_artist(raw_title: &str, raw_artist: &str) -> (String, String) {
    let mut title = raw_title.trim().to_string();
    let mut artist = raw_artist.trim().to_string();

    // 1. Strip - Topic and VEVO from artist (e.g. "Joji - Topic" -> "Joji", "TheWeekndVEVO" -> "The Weeknd")
    if let Some(pos) = artist.to_lowercase().find(" - topic") {
        artist = artist[..pos].trim().to_string();
    }
    if artist.to_lowercase().ends_with("vevo") && artist.len() > 4 {
        artist = artist[..artist.len() - 4].trim().to_string();
    }

    // 2. Strip bracketed noise like (Official Music Video), [Audio], (Lyrics), etc.
    let patterns_to_strip = [
        "(visualizer)", "(official visualizer)", "(official music video)",
        "(official video)", "(official audio)", "(lyric video)", "(lyrics)",
        "(audio)", "[official video]", "[official audio]", "[official music video]",
        "[mv]", "(mv)", "[hd]", "(hd)", "[4k]", "(4k)", "(remastered)", "[remastered]",
        "(remaster)", "[remaster]",
    ];
    let mut lower = title.to_lowercase();
    for pat in &patterns_to_strip {
        if let Some(pos) = lower.find(pat) {
            title.replace_range(pos..pos + pat.len(), "");
            lower = title.to_lowercase();
        }
    }

    // Also strip generic trailing brackets if they contain official/video/audio/remaster
    for open_ch in ['(', '['] {
        if let Some(idx) = title.find(open_ch) {
            let inside = title[idx..].to_lowercase();
            if inside.contains("official") || inside.contains("video") || inside.contains("audio") || inside.contains("lyric") || inside.contains("remaster") {
                title = title[..idx].trim().to_string();
            }
        }
    }

    // 3. Handle dash in title (Artist - Title) - same rule as MyIsland lines 1253-1257
    for sep in [" - ", " \u{2013} ", " \u{2014} "] {
        if let Some((part1, part2)) = title.split_once(sep) {
            let p1 = part1.trim();
            let p2 = part2.trim();
            let p1_lower = p1.to_lowercase();
            let p2_lower = p2.to_lowercase();
            let artist_lower = artist.to_lowercase();

            if !artist.is_empty() && p1_lower == artist_lower {
                title = p2.to_string();
            } else if !artist.is_empty() && p2_lower == artist_lower {
                title = p1.to_string();
            } else if artist.is_empty() || !title.to_lowercase().contains(&artist_lower) || artist_lower == "youtube" || artist_lower == "brave" || artist_lower == "chrome" {
                artist = p1.to_string();
                title = p2.to_string();
            }
            break;
        }
    }

    title = title.trim().to_string();
    artist = artist.trim().to_string();

    (title, artist)
}

pub fn fetch_online_artwork(raw_title: &str, raw_artist: &str) -> Option<Vec<u8>> {
    let (clean_title, clean_artist) = clean_title_and_artist(raw_title, raw_artist);
    // Require BOTH clean title and artist so random YouTube videos don't get replaced with music covers
    if clean_title.is_empty() || clean_artist.is_empty() {
        return None;
    }

    let query = format!("{} {}", clean_artist, clean_title);
    let enc_query = url_encode(query.trim());
    let itunes_url = format!("https://itunes.apple.com/search?term={}&entity=song&limit=1", enc_query);
    query_itunes_artwork(&itunes_url)
}

fn query_itunes_artwork(url: &str) -> Option<Vec<u8>> {
    if let Ok(resp) = ureq::get(url)
        .timeout(std::time::Duration::from_millis(3500))
        .call()
    {
        if let Ok(data) = resp.into_json::<ItunesResponse>() {
            if let Some(track) = data.results.first() {
                if let Some(ref art_url) = track.artwork_url_100 {
                    let hires_url = art_url.replace("100x100bb.jpg", "300x300bb.jpg");
                    if let Ok(art_resp) = ureq::get(&hires_url)
                        .timeout(std::time::Duration::from_millis(4000))
                        .call()
                    {
                        let mut bytes = Vec::new();
                        use std::io::Read;
                        if art_resp.into_reader().read_to_end(&mut bytes).is_ok() && !bytes.is_empty() {
                            return Some(bytes);
                        }
                    }
                }
            }
        }
    }
    None
}

fn url_encode(input: &str) -> String {
    let mut out = String::new();
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

impl LyricsState {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            current_key: String::new(),
            lines: Vec::new(),
            current_index: -1,
            is_gap: false,
            changed_at: now,
            previous_text: None,
            widest_px: 0.0,
            is_loading: false,
            hold_until: now,
            cache: HashMap::new(),
        }
    }

    pub fn has_lyrics(&self) -> bool {
        !self.lines.is_empty() || (self.is_loading && Instant::now() < self.hold_until)
    }

    /// Width for the compact music pill driven by the lyric currently on screen.
    ///
    /// Earlier this used the widest line in the whole song, so the pill locked to
    /// one width for the entire track. Using the active line (and the outgoing
    /// line during a crossfade) lets the pill breathe between the min and max
    /// compact widths as each sentence arrives.
    pub fn get_compact_width(&self) -> f32 {
        const CHROME_PX: f32 = 120.0;

        let active = self.get_now_lyric();
        let mut text_px = 0.0f32;
        for text in [active.text.as_deref(), active.previous.as_deref()]
            .into_iter()
            .flatten()
        {
            let px: f32 = text
                .chars()
                .map(|c| if c.is_ascii() { 8.0 } else { 13.5 })
                .sum();
            text_px = text_px.max(px);
        }

        (text_px + CHROME_PX).clamp(COMPACT_LYRICS_MIN_W, COMPACT_LYRICS_MAX_W)
    }

    pub fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
        let mut lines = Vec::new();
        for raw_line in lrc.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }

            // Find [mm:ss.xx]
            if let Some(open_idx) = line.find('[') {
                if let Some(close_idx) = line.find(']') {
                    let stamp = &line[open_idx + 1..close_idx];
                    let text = line[close_idx + 1..].trim();

                    // Filter out credit lines
                    let lower_text = text.to_lowercase();
                    if lower_text.starts_with("作词")
                        || lower_text.starts_with("作曲")
                        || lower_text.starts_with("producer")
                        || lower_text.starts_with("lyricist")
                        || lower_text.starts_with("composer")
                    {
                        continue;
                    }

                    if let Some(colon_idx) = stamp.find(':') {
                        let mins_str = &stamp[..colon_idx];
                        let secs_str = &stamp[colon_idx + 1..];
                        if let (Ok(mins), Ok(secs)) =
                            (mins_str.parse::<u64>(), secs_str.parse::<f64>())
                        {
                            let ms = mins * 60_000 + (secs * 1000.0) as u64;
                            lines.push(LyricLine {
                                t: ms,
                                text: text.to_string(),
                            });
                        }
                    }
                }
            }
        }
        lines.sort_by_key(|l| l.t);
        lines
    }

    fn search_lrclib(url: &str, duration_sec: u64) -> Option<Vec<LyricLine>> {
        if let Ok(s_resp) = ureq::get(url)
            .timeout(std::time::Duration::from_millis(3500))
            .set("User-Agent", "MyIsland/0.1.0")
            .call()
        {
            if let Ok(items) = s_resp.into_json::<Vec<LrclibSearchItem>>() {
                let mut best_lrc: Option<String> = None;

                // 1. Try finding a synced hit within 4 seconds tolerance
                for item in &items {
                    if let Some(ref lrc) = item.synced_lyrics {
                        if duration_sec > 0 {
                            let dur = item.duration.unwrap_or(0.0);
                            let diff = (dur - duration_sec as f64).abs();
                            if diff <= 4.0 {
                                best_lrc = Some(lrc.clone());
                                break;
                            }
                        }
                    }
                }

                // 2. If no hit within 4s, fallback to first synced result (same as MyIsland line 75: hit || synced[0])
                // Since this search query already includes both artist_name and track_name, this allows
                // tracks with intro/outro video padding (YouTube/browser) to still receive synced lyrics!
                if best_lrc.is_none() {
                    for item in items {
                        if let Some(lrc) = item.synced_lyrics {
                            best_lrc = Some(lrc);
                            break;
                        }
                    }
                }

                if let Some(lrc) = best_lrc {
                    let parsed = Self::parse_lrc(&lrc);
                    if !parsed.is_empty() {
                        return Some(parsed);
                    }
                }
            }
        }
        None
    }

    pub fn fetch_lyrics(title: &str, artist: &str, duration_sec: u64) -> Option<Vec<LyricLine>> {
        let (clean_title, clean_artist) = clean_title_and_artist(title, artist);
        if clean_title.is_empty() {
            return None;
        }

        let enc_title = url_encode(&clean_title);
        let enc_artist = url_encode(&clean_artist);

        // 1. Exact get (track_name + artist_name + optional duration)
        let get_url = if duration_sec > 0 && !clean_artist.is_empty() {
            format!(
                "https://lrclib.net/api/get?track_name={}&artist_name={}&duration={}",
                enc_title, enc_artist, duration_sec
            )
        } else if !clean_artist.is_empty() {
            format!(
                "https://lrclib.net/api/get?track_name={}&artist_name={}",
                enc_title, enc_artist
            )
        } else {
            format!("https://lrclib.net/api/get?track_name={}", enc_title)
        };

        if let Ok(resp) = ureq::get(&get_url)
            .timeout(std::time::Duration::from_millis(3500))
            .set("User-Agent", "MyIsland/0.1.0")
            .call()
        {
            if let Ok(data) = resp.into_json::<LrclibResponse>() {
                if let Some(lrc) = data.synced_lyrics {
                    let parsed = Self::parse_lrc(&lrc);
                    if !parsed.is_empty() {
                        return Some(parsed);
                    }
                }
            }
        }

        // 2. Fallback to search with clean title & artist (strictly requires artist match, within 4s duration)
        if !clean_artist.is_empty() {
            let search_url = format!(
                "https://lrclib.net/api/search?track_name={}&artist_name={}",
                enc_title, enc_artist
            );
            if let Some(lines) = Self::search_lrclib(&search_url, duration_sec) {
                return Some(lines);
            }
        }

        // DO NOT search by title only: prevents pulling lyrics for completely different songs!
        None
    }

    pub fn update_track(
        &mut self,
        title: &str,
        artist: &str,
        duration_ms: u64,
        lyrics_tx: crossbeam_channel::Sender<(String, Vec<LyricLine>)>,
    ) {
        let key = format!("{}|{}", title, artist);
        if key == self.current_key {
            return;
        }

        let now = Instant::now();
        if !self.lines.is_empty() {
            self.previous_text = if !self.is_gap && self.current_index >= 0 {
                self.lines.get(self.current_index as usize).map(|l| l.text.clone())
            } else {
                None
            };
            self.changed_at = now;
            self.hold_until = now + std::time::Duration::from_millis(2500);
        }

        self.current_key = key.clone();
        self.lines.clear();
        self.current_index = -1;
        self.is_gap = false;

        if let Some(cached) = self.cache.get(&key) {
            self.lines = cached.clone();
            self.widest_px = self
                .lines
                .iter()
                .map(|l| l.text.len() as f32 * 8.0)
                .fold(0.0f32, f32::max);
            return;
        }

        // Spawn background fetch
        let t = title.to_string();
        let a = artist.to_string();
        self.is_loading = true;

        std::thread::spawn(move || {
            if let Some(lines) = Self::fetch_lyrics(&t, &a, duration_ms / 1000) {
                let _ = lyrics_tx.send((key, lines));
            }
        });
    }

    pub fn receive_loaded(&mut self, key: String, lines: Vec<LyricLine>) {
        if self.current_key == key {
            self.widest_px = lines
                .iter()
                .map(|l| l.text.len() as f32 * 8.0)
                .fold(0.0f32, f32::max);
            self.lines = lines.clone();
            self.is_loading = false;
        }
        self.cache.insert(key, lines);
    }

    pub fn tick(&mut self, progress_ms: u64) {
        if self.lines.is_empty() {
            return;
        }

        const LEAD: u64 = 250;
        let pos = progress_ms + LEAD;

        let mut idx = -1i32;
        for (i, line) in self.lines.iter().enumerate() {
            if line.t <= pos {
                idx = i as i32;
            } else {
                break;
            }
        }

        let mut gap = false;
        if idx >= 0 {
            let curr_t = self.lines[idx as usize].t;
            let since = pos.saturating_sub(curr_t);
            if let Some(next_line) = self.lines.get((idx + 1) as usize) {
                gap = (next_line.t.saturating_sub(curr_t) > 14_000) && since > 9000;
            } else {
                gap = since > 8000;
            }
        }

        if idx != self.current_index || gap != self.is_gap {
            self.previous_text = if !self.is_gap && self.current_index >= 0 {
                self.lines.get(self.current_index as usize).map(|l| l.text.clone())
            } else {
                None
            };
            self.current_index = idx;
            self.is_gap = gap;
            self.changed_at = Instant::now();
        }
    }

    pub fn get_now_lyric(&self) -> ActiveLyric {
        let elapsed = self.changed_at.elapsed().as_millis() as f32;
        let raw_k = (elapsed / 420.0).clamp(0.0, 1.0);
        let k = 1.0 - (1.0 - raw_k).powi(4); // ease-out quartic

        let curr_text = if !self.is_gap && self.current_index >= 0 {
            self.lines.get(self.current_index as usize).map(|l| l.text.clone())
        } else {
            None
        };

        ActiveLyric {
            text: curr_text,
            previous: if raw_k < 1.0 { self.previous_text.clone() } else { None },
            k,
        }
    }

    pub fn get_around_lyrics(&self) -> (String, String, String) {
        let before = if self.current_index > 0 {
            self.lines.get((self.current_index - 1) as usize).map(|l| l.text.clone()).unwrap_or_default()
        } else {
            String::new()
        };

        let now = if !self.is_gap && self.current_index >= 0 {
            self.lines.get(self.current_index as usize).map(|l| l.text.clone()).unwrap_or_default()
        } else {
            String::new()
        };

        let next = if self.current_index >= 0 {
            self.lines.get((self.current_index + 1) as usize).map(|l| l.text.clone()).unwrap_or_default()
        } else {
            self.lines.first().map(|l| l.text.clone()).unwrap_or_default()
        };

        (before, now, next)
    }
}
