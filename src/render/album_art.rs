use tiny_skia::{FilterQuality, Paint, PathBuilder, Pattern, Pixmap, PixmapMut, SpreadMode, Transform};

pub struct AlbumArtManager {
    pub current_key: String,
    pub dominant_color: [u8; 3],
    pub pixmap: Option<Pixmap>,
    pub current_thumb_len: usize,
    pub has_official_art: bool,
}

impl AlbumArtManager {
    pub fn new() -> Self {
        Self {
            current_key: String::new(),
            dominant_color: [30, 215, 96], // #1ed760 fallback
            pixmap: None,
            current_thumb_len: 0,
            has_official_art: false,
        }
    }

    pub fn update(&mut self, session_key: &str, thumbnail_bytes: Option<&[u8]>) {
        if session_key != self.current_key {
            self.current_key = session_key.to_string();
            self.pixmap = None;
            self.current_thumb_len = 0;
            self.has_official_art = false;
            self.dominant_color = [30, 215, 96]; // #1ed760
        }

        // If we already loaded official high-res art for this song, don't overwrite with GSMTC stream
        if self.has_official_art {
            return;
        }

        if let Some(bytes) = thumbnail_bytes {
            if bytes.len() == self.current_thumb_len && self.pixmap.is_some() {
                return;
            }
            self.current_thumb_len = bytes.len();
            self.load_image_bytes(bytes);
        }
    }

    pub fn update_official_artwork(&mut self, session_key: &str, art_bytes: &[u8]) {
        if session_key == self.current_key || self.current_key.is_empty() {
            self.current_key = session_key.to_string();
            self.has_official_art = true;
            self.current_thumb_len = art_bytes.len();
            self.load_image_bytes(art_bytes);
        }
    }

    fn load_image_bytes(&mut self, bytes: &[u8]) {
        if let Ok(dyn_img) = image::load_from_memory(bytes) {
            let rgba = dyn_img.to_rgba8();
            let (w, h) = rgba.dimensions();

            // 1. Extract dominant vibrant color
            self.dominant_color = extract_album_color(&rgba);

            // 2. Convert to premultiplied tiny_skia Pixmap
            if let Some(mut pm) = Pixmap::new(w, h) {
                let src = rgba.as_raw();
                let dst = pm.data_mut();
                for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                    let a = s[3] as u16;
                    d[0] = ((s[0] as u16 * a) / 255) as u8;
                    d[1] = ((s[1] as u16 * a) / 255) as u8;
                    d[2] = ((s[2] as u16 * a) / 255) as u8;
                    d[3] = s[3];
                }
                self.pixmap = Some(pm);
            }
        }
    }

    /// Draws the album art inside a rounded rect of (x, y, size, size) with the given corner radius.
    /// If no album art is available, renders the fallback note icon inside a #1c1c1e rounded rect.
    pub fn draw(
        &self,
        pixmap_mut: &mut PixmapMut,
        x: f32,
        y: f32,
        size: f32,
        radius: f32,
    ) {
        let r = radius.min(size / 2.0);

        let mut pb = PathBuilder::new();
        pb.move_to(x + r, y);
        pb.line_to(x + size - r, y);
        pb.quad_to(x + size, y, x + size, y + r);
        pb.line_to(x + size, y + size - r);
        pb.quad_to(x + size, y + size, x + size - r, y + size);
        pb.line_to(x + r, y + size);
        pb.quad_to(x, y + size, x, y + size - r);
        pb.line_to(x, y + r);
        pb.quad_to(x, y, x + r, y);
        pb.close();

        let path = match pb.finish() {
            Some(p) => p,
            None => return,
        };

        if let Some(ref img_pm) = self.pixmap {
            let sx = size / img_pm.width() as f32;
            let sy = size / img_pm.height() as f32;
            let transform = Transform::from_scale(sx, sy).post_translate(x, y);

            let pattern = Pattern::new(
                img_pm.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Bilinear,
                1.0,
                transform,
            );

            let mut paint = Paint::default();
            paint.anti_alias = true;
            paint.shader = pattern;

            pixmap_mut.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        } else {
            // Fallback dark box (#1c1c1e)
            let mut bg_paint = Paint::default();
            bg_paint.anti_alias = true;
            bg_paint.set_color_rgba8(28, 28, 30, 255);
            pixmap_mut.fill_path(
                &path,
                &bg_paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );

            // Fallback Musical Note glyph (#8e8e93)
            let mut note_paint = Paint::default();
            note_paint.anti_alias = true;
            note_paint.set_color_rgba8(142, 142, 147, 255);
            let stroke = tiny_skia::Stroke {
                width: 1.4,
                line_cap: tiny_skia::LineCap::Round,
                ..Default::default()
            };

            let cy = y + size / 2.0;
            let scale = size / 24.0;
            let mut npb = PathBuilder::new();
            npb.move_to(x + 7.0 * scale, cy + 5.0 * scale);
            npb.line_to(x + 7.0 * scale, cy - 5.0 * scale);
            npb.line_to(x + 17.0 * scale, cy - 7.0 * scale);
            npb.line_to(x + 17.0 * scale, cy + 3.0 * scale);
            if let Some(np) = npb.finish() {
                pixmap_mut.stroke_path(&np, &note_paint, &stroke, Transform::identity(), None);
            }
        }
    }
}

/// Exact port of extractAlbumColor from MyIsland/index.html lines 1336-1354
fn extract_album_color(img: &image::RgbaImage) -> [u8; 3] {
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 {
        return [30, 215, 96]; // #1ed760
    }

    let mut r_acc = 0.0f32;
    let mut g_acc = 0.0f32;
    let mut b_acc = 0.0f32;
    let mut n_acc = 0.0f32;

    let sample_count = 12;
    for sy in 0..sample_count {
        for sx in 0..sample_count {
            let px = ((sx as f32 + 0.5) / sample_count as f32 * width as f32) as u32;
            let py = ((sy as f32 + 0.5) / sample_count as f32 * height as f32) as u32;
            let pixel = img.get_pixel(px.min(width - 1), py.min(height - 1));
            let r = pixel[0] as f32;
            let g = pixel[1] as f32;
            let b = pixel[2] as f32;

            let sat = r.max(g).max(b) - r.min(g).min(b);
            let w = 1.0 + sat / 16.0;

            r_acc += r * w;
            g_acc += g * w;
            b_acc += b * w;
            n_acc += w;
        }
    }

    if n_acc <= 0.0 {
        return [30, 215, 96];
    }

    let r = r_acc / n_acc;
    let g = g_acc / n_acc;
    let b = b_acc / n_acc;

    let max_val = r.max(g).max(b);
    let boost = 210.0 / 60.0f32.max(max_val);

    [
        (r * boost).clamp(0.0, 255.0).round() as u8,
        (g * boost).clamp(0.0, 255.0).round() as u8,
        (b * boost).clamp(0.0, 255.0).round() as u8,
    ]
}
