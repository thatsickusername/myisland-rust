use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipItem {
    pub text: String,
    pub timestamp: u64,
}

pub struct ClipboardManager {
    pub clips: Vec<ClipItem>,
    pub last_clip: String,
    pub note_text: String,
    storage_dir: PathBuf,
    is_note_dirty: bool,
    last_note_edit: Option<std::time::Instant>,
}

impl ClipboardManager {
    pub fn new() -> Self {
        let app_data = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        let storage_dir = PathBuf::from(app_data).join("MyIsland");
        let _ = fs::create_dir_all(&storage_dir);

        let note_path = storage_dir.join("note.json");
        let note_text = fs::read_to_string(note_path).unwrap_or_default();

        Self {
            clips: Vec::new(),
            last_clip: String::new(),
            note_text,
            storage_dir,
            is_note_dirty: false,
            last_note_edit: None,
        }
    }

    pub fn push_clip(&mut self, text: String) -> bool {
        let clean = text.trim();
        if clean.is_empty() || clean.len() > 20000 || clean == self.last_clip {
            return false;
        }

        self.last_clip = clean.to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Remove existing duplicates
        self.clips.retain(|c| c.text != clean);
        self.clips.insert(
            0,
            ClipItem {
                text: clean.to_string(),
                timestamp: now,
            },
        );

        if self.clips.len() > 10 {
            self.clips.truncate(10);
        }

        true
    }

    #[allow(dead_code)]
    pub fn remove_clip(&mut self, index: usize) {
        if index < self.clips.len() {
            self.clips.remove(index);
        }
    }

    #[allow(dead_code)]
    pub fn clear_clips(&mut self) {
        self.clips.clear();
    }

    #[allow(dead_code)]
    pub fn save_note(&mut self, text: String) {
        self.note_text = text;
        self.is_note_dirty = true;
        self.last_note_edit = Some(std::time::Instant::now());
    }

    pub fn flush_note_if_needed(&mut self) {
        if self.is_note_dirty {
            if let Some(t) = self.last_note_edit {
                if t.elapsed() >= std::time::Duration::from_millis(800) {
                    self.flush_note();
                }
            }
        }
    }

    pub fn flush_note(&mut self) {
        if self.is_note_dirty {
            self.is_note_dirty = false;
            let note_path = self.storage_dir.join("note.json");
            let text = self.note_text.clone();
            std::thread::spawn(move || {
                let _ = fs::write(note_path, text);
            });
        }
    }

    pub fn copy_to_system_clipboard(text: &str) {
        unsafe {
            use windows::Win32::System::DataExchange::*;
            use windows::Win32::System::Memory::*;
            if OpenClipboard(None).is_ok() {
                let _ = EmptyClipboard();
                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = wide.len() * 2;
                if let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
                    let ptr = GlobalLock(hmem);
                    if !ptr.is_null() {
                        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr as *mut u8, bytes);
                        let _ = GlobalUnlock(hmem);
                        let _ = SetClipboardData(13, windows::Win32::Foundation::HANDLE(hmem.0));
                    }
                }
                let _ = CloseClipboard();
            }
        }
    }

    /// Read the current Windows clipboard bitmap as PNG bytes.
    ///
    /// Snipping Tool places the captured image on the clipboard immediately
    /// before emitting its toast. Keeping this conversion native lets the
    /// expanded notification card show the same preview as the Electron app
    /// without writing a temporary screenshot file.
    pub fn read_clipboard_image_png() -> Option<Vec<u8>> {
        unsafe {
            use windows::Win32::Foundation::HGLOBAL;
            use windows::Win32::System::DataExchange::*;
            use windows::Win32::System::Memory::*;

            if OpenClipboard(None).is_err() {
                return None;
            }

            let handle = GetClipboardData(17) // CF_DIBV5
                .or_else(|_| GetClipboardData(8)) // CF_DIB
                .ok();
            let Some(handle) = handle else {
                let _ = CloseClipboard();
                return None;
            };

            let global = HGLOBAL(handle.0);
            let ptr = GlobalLock(global) as *const u8;
            let total = GlobalSize(global);
            let result = if ptr.is_null() || total < 40 {
                None
            } else {
                let header = std::slice::from_raw_parts(ptr, total);
                let header_size = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
                let width = i32::from_le_bytes(header[4..8].try_into().unwrap());
                let height_raw = i32::from_le_bytes(header[8..12].try_into().unwrap());
                let bit_count = u16::from_le_bytes(header[14..16].try_into().unwrap());
                let compression = u32::from_le_bytes(header[16..20].try_into().unwrap());
                let clr_used = u32::from_le_bytes(header[32..36].try_into().unwrap()) as usize;

                if width <= 0
                    || height_raw == 0
                    || width > 8192
                    || height_raw.unsigned_abs() > 8192
                    || (bit_count != 24 && bit_count != 32)
                    || (compression != 0 && compression != 3)
                {
                    None
                } else {
                    let width_u = width as usize;
                    let height_u = height_raw.unsigned_abs() as usize;
                    let top_down = height_raw < 0;
                    let bytes_per_pixel = (bit_count / 8) as usize;
                    let stride = ((width_u * bytes_per_pixel + 3) / 4) * 4;
                    let mask_bytes = if compression == 3 && header_size == 40 { 12 } else { 0 };
                    let palette_entries = if bit_count <= 8 {
                        if clr_used > 0 { clr_used } else { 1usize << bit_count }
                    } else {
                        0
                    };
                    let pixels_offset = header_size
                        .saturating_add(mask_bytes)
                        .saturating_add(palette_entries * 4);
                    let needed = pixels_offset.saturating_add(stride.saturating_mul(height_u));

                    if pixels_offset >= total || needed > total {
                        None
                    } else {
                        let mut rgba = image::RgbaImage::new(width as u32, height_u as u32);
                        for y in 0..height_u {
                            let source_y = if top_down { y } else { height_u - 1 - y };
                            let row = pixels_offset + source_y * stride;
                            for x in 0..width_u {
                                let px = row + x * bytes_per_pixel;
                                let b = header[px];
                                let g = header[px + 1];
                                let r = header[px + 2];
                                let a = if bytes_per_pixel == 4 && header[px + 3] != 0 {
                                    header[px + 3]
                                } else {
                                    255
                                };
                                rgba.put_pixel(x as u32, y as u32, image::Rgba([r, g, b, a]));
                            }
                        }

                        let mut output = std::io::Cursor::new(Vec::new());
                        if image::DynamicImage::ImageRgba8(rgba)
                            .write_to(&mut output, image::ImageFormat::Png)
                            .is_ok()
                        {
                            Some(output.into_inner())
                        } else {
                            None
                        }
                    }
                }
            };

            if !ptr.is_null() {
                let _ = GlobalUnlock(global);
            }
            let _ = CloseClipboard();
            result
        }
    }
}
