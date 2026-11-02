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
}
