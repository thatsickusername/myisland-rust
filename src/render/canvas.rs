use std::collections::HashMap;
use std::fs;
use std::ptr;
use std::sync::Arc;
use fontdue::{Font, FontSettings, Metrics};
use tiny_skia::{Color, FillRule, Mask, Pixmap, PixmapPaint, Transform};
use windows::Win32::Foundation::{HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC, HBITMAP,
    BLENDFUNCTION, AC_SRC_ALPHA, AC_SRC_OVER,
};
use windows::Win32::UI::WindowsAndMessaging::{
    UpdateLayeredWindow, ULW_ALPHA,
};

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct GlyphKey {
    ch: char,
    size_bits: u32,
    bold: bool,
}

struct CachedGlyph {
    metrics: Metrics,
    bitmap: Vec<u8>,
}

pub struct LayeredCanvas {
    pub width: i32,
    pub height: i32,
    hdc_mem: HDC,
    hbitmap: HBITMAP,
    dib_ptr: *mut u8,
    pub pixmap: Pixmap,
    pub content_pixmap: Pixmap,
    pub mask: Mask,
    pub font_regular: Option<Arc<Font>>,
    pub font_bold: Option<Arc<Font>>,
    glyph_cache: HashMap<GlyphKey, CachedGlyph>,
}

impl LayeredCanvas {
    pub fn new(width: i32, height: i32) -> windows::core::Result<Self> {
        unsafe {
            let hdc_mem = CreateCompatibleDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: (width * height * 4) as u32,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
            };

            let mut bits: *mut std::ffi::c_void = ptr::null_mut();
            let hbitmap = CreateDIBSection(
                hdc_mem,
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            )?;

            SelectObject(hdc_mem, hbitmap);

            let pixmap = Pixmap::new(width as u32, height as u32)
                .expect("Failed to allocate tiny-skia pixmap");
            let content_pixmap = Pixmap::new(width as u32, height as u32)
                .expect("Failed to allocate content pixmap");
            let mask = Mask::new(width as u32, height as u32)
                .expect("Failed to allocate mask");

            // Load Segoe UI fonts from Windows Fonts directory dynamically
            let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
            let fonts_dir = std::path::PathBuf::from(&system_root).join("Fonts");
            let reg_path = fonts_dir.join("segoeui.ttf");
            let reg_bytes = fs::read(&reg_path);
            let font_regular = reg_bytes
                .as_ref()
                .ok()
                .and_then(|data| Font::from_bytes(&data[..], FontSettings::default()).ok())
                .map(Arc::new);

            let bold_path = fonts_dir.join("segoeuib.ttf");
            let bold_bytes = fs::read(&bold_path);
            let font_bold = bold_bytes
                .as_ref()
                .ok()
                .and_then(|data| Font::from_bytes(&data[..], FontSettings::default()).ok())
                .map(Arc::new)
                .or_else(|| font_regular.clone());

            // Pre-warm glyph cache for instant, zero-stutter first-frame rendering
            let mut glyph_cache = HashMap::with_capacity(512);
            let common_sizes = [9.5f32, 11.5, 12.0, 12.5, 13.0, 13.5, 14.0];
            let prewarm_chars = "0123456789:•-—.,!/?% APMabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
            for &size in &common_sizes {
                if let Some(ref f) = font_regular {
                    for ch in prewarm_chars.chars() {
                        let (metrics, bitmap) = f.rasterize(ch, size);
                        glyph_cache.insert(GlyphKey { ch, size_bits: size.to_bits(), bold: false }, CachedGlyph { metrics, bitmap });
                    }
                }
                if let Some(ref f) = font_bold {
                    for ch in prewarm_chars.chars() {
                        let (metrics, bitmap) = f.rasterize(ch, size);
                        glyph_cache.insert(GlyphKey { ch, size_bits: size.to_bits(), bold: true }, CachedGlyph { metrics, bitmap });
                    }
                }
            }

            Ok(Self {
                width,
                height,
                hdc_mem,
                hbitmap,
                dib_ptr: bits as *mut u8,
                pixmap,
                content_pixmap,
                mask,
                font_regular,
                font_bold,
                glyph_cache,
            })
        }
    }

    pub fn clear(&mut self) {
        self.pixmap.fill(Color::TRANSPARENT);
    }

    /// Begin rendering island content with overflow:hidden clipping.
    /// Directs drawing operations to an offscreen transparent pixmap.
    pub fn begin_island_content(&mut self) {
        self.content_pixmap.fill(Color::TRANSPARENT);
        std::mem::swap(&mut self.pixmap, &mut self.content_pixmap);
    }

    /// Complete island content rendering, apply the exact squircle mask (overflow: hidden),
    /// and composite onto the main surface.
    pub fn end_island_content(&mut self, path: Option<&tiny_skia::Path>) {
        std::mem::swap(&mut self.pixmap, &mut self.content_pixmap);
        if let Some(p) = path {
            self.mask.clear();
            self.mask.fill_path(p, FillRule::Winding, true, Transform::identity());
            self.content_pixmap.apply_mask(&self.mask);
        }
        let paint = PixmapPaint::default();
        self.pixmap.draw_pixmap(0, 0, self.content_pixmap.as_ref(), &paint, Transform::identity(), None);
    }

    /// Present the pixmap to the Win32 layered window.
    pub fn present(&mut self, hwnd: HWND, window_x: i32, window_y: i32) {
        let src_data = self.pixmap.data();
        let _dib_slice = unsafe {
            std::slice::from_raw_parts_mut(self.dib_ptr, (self.width * self.height * 4) as usize)
        };

        // Convert RGBA premultiplied to BGRA premultiplied using fast 32-bit SIMD-vectorizable bit manipulation
        let total_pixels = (self.width * self.height) as usize;
        let src_pixels = unsafe {
            std::slice::from_raw_parts(src_data.as_ptr() as *const u32, total_pixels)
        };
        let dst_pixels = unsafe {
            std::slice::from_raw_parts_mut(self.dib_ptr as *mut u32, total_pixels)
        };

        for (src, dst) in src_pixels.iter().zip(dst_pixels.iter_mut()) {
            let px = *src;
            // In little-endian memory:
            // Input RGBA: R = bits 0..7, G = bits 8..15, B = bits 16..23, A = bits 24..31
            // Target BGRA: B = bits 0..7, G = bits 8..15, R = bits 16..23, A = bits 24..31
            *dst = (px & 0xFF00FF00) | ((px & 0x00FF0000) >> 16) | ((px & 0x000000FF) << 16);
        }

        let pt_dst = POINT { x: window_x, y: window_y };
        let size = SIZE { cx: self.width, cy: self.height };
        let pt_src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                None,
                Some(&pt_dst),
                Some(&size),
                self.hdc_mem,
                Some(&pt_src),
                None,
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    pub fn draw_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        bold: bool,
    ) {
        self.draw_text_clipped(text, x, y, size, color, bold, 0.0, self.width as f32);
    }

    pub fn draw_text_clipped(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        bold: bool,
        min_x: f32,
        max_x: f32,
    ) {
        self.draw_text_clipped_box(text, x, y, size, color, bold, min_x, max_x, 0.0, self.height as f32);
    }

    pub fn draw_text_clipped_box(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        bold: bool,
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
    ) {
        let font = if bold {
            self.font_bold.as_ref().or(self.font_regular.as_ref())
        } else {
            self.font_regular.as_ref()
        };

        let font = match font {
            Some(f) => f.clone(),
            None => return,
        };

        let mut curr_x = x;
        let pm_w = self.width as usize;
        let pm_h = self.height as usize;

        for ch in text.chars() {
            let key = GlyphKey {
                ch,
                size_bits: size.to_bits(),
                bold,
            };

            let cached = self.glyph_cache.entry(key).or_insert_with(|| {
                let (metrics, bitmap) = font.rasterize(ch, size);
                CachedGlyph { metrics, bitmap }
            });

            let metrics = cached.metrics;
            let gx = curr_x + metrics.xmin as f32;
            let gy = y + (size - metrics.ymin as f32 - metrics.height as f32);

            let bitmap = &cached.bitmap;
            let pm_buf = self.pixmap.data_mut();

            for row in 0..metrics.height {
                let py_f = gy + row as f32;
                if py_f < min_y || py_f >= max_y {
                    continue;
                }
                let py = py_f as usize;
                if py >= pm_h {
                    continue;
                }
                for col in 0..metrics.width {
                    let px_f = gx + col as f32;
                    if px_f < min_x || px_f >= max_x {
                        continue;
                    }
                    let px = px_f as usize;
                    if px >= pm_w {
                        continue;
                    }
                    let alpha_val = bitmap[row * metrics.width + col];
                    if alpha_val == 0 {
                        continue;
                    }

                    let a_norm = (alpha_val as f32 / 255.0) * color.alpha();
                    let idx = (py * pm_w + px) * 4;

                    // Premultiplied alpha composite
                    let r_pre = (color.red() * 255.0 * a_norm) as u32;
                    let g_pre = (color.green() * 255.0 * a_norm) as u32;
                    let b_pre = (color.blue() * 255.0 * a_norm) as u32;
                    let a_pre = (a_norm * 255.0) as u32;

                    let dst_r = pm_buf[idx] as u32;
                    let dst_g = pm_buf[idx + 1] as u32;
                    let dst_b = pm_buf[idx + 2] as u32;
                    let dst_a = pm_buf[idx + 3] as u32;

                    let inv_a = 255 - a_pre;
                    pm_buf[idx] = (r_pre + (dst_r * inv_a) / 255).min(255) as u8;
                    pm_buf[idx + 1] = (g_pre + (dst_g * inv_a) / 255).min(255) as u8;
                    pm_buf[idx + 2] = (b_pre + (dst_b * inv_a) / 255).min(255) as u8;
                    pm_buf[idx + 3] = (a_pre + (dst_a * inv_a) / 255).min(255) as u8;
                }
            }

            curr_x += metrics.advance_width;
        }
    }

    pub fn measure_text_width(&self, text: &str, size: f32, bold: bool) -> f32 {
        let font = if bold {
            self.font_bold.as_ref().or(self.font_regular.as_ref())
        } else {
            self.font_regular.as_ref()
        };

        match font {
            Some(f) => {
                let mut width = 0.0;
                for ch in text.chars() {
                    let metrics = f.metrics(ch, size);
                    width += metrics.advance_width;
                }
                width
            }
            None => text.len() as f32 * (size * 0.55),
        }
    }

    pub fn truncate_to_width(
        &self,
        text: &str,
        max_width: f32,
        size: f32,
        bold: bool,
    ) -> String {
        if max_width <= 8.0 {
            return String::new();
        }
        if self.measure_text_width(text, size, bold) <= max_width {
            return text.to_string();
        }

        let ell_w = self.measure_text_width("…", size, bold);
        let avail_w = max_width - ell_w;
        if avail_w <= 0.0 {
            return if max_width >= ell_w { "…".to_string() } else { String::new() };
        }

        let font = if bold {
            self.font_bold.as_ref().or(self.font_regular.as_ref())
        } else {
            self.font_regular.as_ref()
        };

        if let Some(f) = font {
            let mut acc_w = 0.0;
            let mut end_idx = 0;
            for (idx, ch) in text.char_indices() {
                let m = f.metrics(ch, size);
                if acc_w + m.advance_width > avail_w {
                    break;
                }
                acc_w += m.advance_width;
                end_idx = idx + ch.len_utf8();
            }
            if end_idx > 0 {
                let mut res = text[..end_idx].to_string();
                res.push('…');
                return res;
            }
        }

        if max_width >= 12.0 {
            "…".to_string()
        } else {
            String::new()
        }
    }
}

impl Drop for LayeredCanvas {
    fn drop(&mut self) {
        unsafe {
            if !self.hbitmap.is_invalid() {
                let _ = DeleteObject(self.hbitmap);
            }
            if !self.hdc_mem.is_invalid() {
                let _ = DeleteDC(self.hdc_mem);
            }
        }
    }
}
