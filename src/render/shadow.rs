use tiny_skia::{FillRule, Paint, Path, Pixmap, PixmapMut, Transform};

pub struct ShadowRenderer {
    cached_pixmap: Option<Pixmap>,
    last_w: f32,
    last_h: f32,
    last_r: f32,
    last_blur: f32,
    last_pad: f32,
    last_is_sec: bool,
    alpha_buf: Vec<u8>,
    temp_buf: Vec<u8>,
    prefix_buf: Vec<u32>,
}

impl ShadowRenderer {
    pub fn new() -> Self {
        Self {
            cached_pixmap: None,
            last_w: 0.0,
            last_h: 0.0,
            last_r: 0.0,
            last_blur: 0.0,
            last_pad: 0.0,
            last_is_sec: false,
            alpha_buf: Vec::new(),
            temp_buf: Vec::new(),
            prefix_buf: Vec::new(),
        }
    }

    /// Renders the soft Gaussian atmospheric drop shadow matching MyIsland:
    /// ctx.shadowColor = 'rgba(0, 0, 0, 0.45)';
    /// ctx.shadowBlur = currentState === 'expanded' ? 34 : (currentState === 'tucked' ? 4 : 18);
    /// ctx.shadowOffsetY = currentState === 'tucked' ? 1 : 6;
    pub fn render_shadow(
        &mut self,
        target_pixmap: &mut PixmapMut,
        island_path_fn: &dyn Fn(f32, f32, f32, f32, f32) -> Option<Path>,
        x: f32,
        w: f32,
        h: f32,
        r: f32,
        blur: f32,
        offset_y: f32,
        is_sec: bool,
        is_moving: bool,
    ) {
        if w < 2.0 || h < 2.0 {
            return;
        }

        let scale = 0.5f32;
        let inv_scale = 2.0f32;

        let sw = (w * scale).max(1.0);
        let sh = (h * scale).max(1.0);
        let sr = (r * scale).max(0.0);
        let sblur = (blur * scale).max(0.5);

        // Velocity-aware shadow caching: during rapid spring flight, reuse and bilinearly
        // scale cached blur bitmaps in 0.04ms instead of blocking on 6ms CPU Gaussian blur.
        let tolerance = if is_moving { 40.0 } else { 0.5 };
        let needs_rebuild = self.cached_pixmap.is_none()
            || (self.last_w - sw).abs() > tolerance
            || (self.last_h - sh).abs() > tolerance
            || (self.last_blur - sblur).abs() > 2.0
            || self.last_is_sec != is_sec;

        let pad = (sblur * 1.5).ceil() as i32 + 4;
        let pw = (sw as i32 + pad * 2).max(1) as u32;
        let ph = (sh as i32 + pad * 2).max(1) as u32;

        if needs_rebuild {
            if let Some(mut local_pixmap) = Pixmap::new(pw, ph) {
                if let Some(path) = island_path_fn(pad as f32, pad as f32, sw, sh, sr) {
                    let mut solid_paint = Paint::default();
                    solid_paint.anti_alias = true;
                    solid_paint.set_color_rgba8(0, 0, 0, 255);
                    local_pixmap.fill_path(
                        &path,
                        &solid_paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );

                    let num_pixels = (pw * ph) as usize;
                    self.alpha_buf.clear();
                    self.alpha_buf.reserve(num_pixels);
                    for p in local_pixmap.pixels() {
                        self.alpha_buf.push(p.alpha());
                    }

                    let sigma = (sblur / 2.0).max(0.5);
                    self.temp_buf.resize(num_pixels, 0);
                    let max_dim = (pw as usize).max(ph as usize);
                    self.prefix_buf.resize(max_dim + 1, 0);

                    gaussian_blur_alpha(
                        &mut self.alpha_buf,
                        &mut self.temp_buf,
                        &mut self.prefix_buf,
                        pw as usize,
                        ph as usize,
                        sigma,
                    );

                    let raw_bytes = local_pixmap.data_mut();
                    for (i, &a) in self.alpha_buf.iter().enumerate() {
                        let shadow_a = ((a as f32) * 0.45).round() as u8;
                        let idx = i * 4;
                        raw_bytes[idx] = 0;
                        raw_bytes[idx + 1] = 0;
                        raw_bytes[idx + 2] = 0;
                        raw_bytes[idx + 3] = shadow_a;
                    }

                    self.cached_pixmap = Some(local_pixmap);
                    self.last_w = sw;
                    self.last_h = sh;
                    self.last_r = sr;
                    self.last_blur = sblur;
                    self.last_pad = pad as f32;
                    self.last_is_sec = is_sec;
                }
            }
        }

        if let Some(ref cached) = self.cached_pixmap {
            let scale_x = if self.last_w > 0.0 { sw / self.last_w } else { 1.0 };
            let scale_y = if self.last_h > 0.0 { sh / self.last_h } else { 1.0 };
            let pad_f = self.last_pad;
            let dest_x = x - pad_f * inv_scale * scale_x;
            let dest_y = offset_y - pad_f * inv_scale * scale_y;
            let mut paint = tiny_skia::PixmapPaint::default();
            paint.quality = tiny_skia::FilterQuality::Bilinear;
            target_pixmap.draw_pixmap(
                0,
                0,
                cached.as_ref(),
                &paint,
                Transform::from_scale(inv_scale * scale_x, inv_scale * scale_y).post_translate(dest_x, dest_y),
                None,
            );
        }
    }
}

fn boxes_for_gauss(sigma: f32) -> [usize; 3] {
    let n = 3;
    let w_ideal = ((12.0 * sigma * sigma / (n as f32)) + 1.0).sqrt();
    let mut wl = w_ideal.floor() as usize;
    if wl % 2 == 0 {
        wl = wl.saturating_sub(1);
    }
    let wu = wl + 2;
    let m_ideal = (12.0 * sigma * sigma
        - (n as f32) * (wl as f32) * (wl as f32)
        - 4.0 * (n as f32) * (wl as f32)
        - 3.0 * (n as f32))
        / (-4.0 * (wl as f32) - 4.0);
    let m = if m_ideal.is_finite() && m_ideal >= 0.0 {
        (m_ideal.round() as usize).min(n)
    } else {
        0
    };
    [
        if 0 < m { wl } else { wu },
        if 1 < m { wl } else { wu },
        if 2 < m { wl } else { wu },
    ]
}

fn gaussian_blur_alpha(
    data: &mut [u8],
    temp: &mut [u8],
    prefix: &mut [u32],
    w: usize,
    h: usize,
    sigma: f32,
) {
    if w == 0 || h == 0 || sigma < 0.1 {
        return;
    }

    let boxes = boxes_for_gauss(sigma);

    for &box_size in &boxes {
        let r = (box_size.saturating_sub(1) / 2).max(1);
        box_blur_h(data, temp, prefix, w, h, r);
        box_blur_v(temp, data, prefix, w, h, r);
    }
}

fn box_blur_h(src: &[u8], dst: &mut [u8], prefix: &mut [u32], w: usize, h: usize, r: usize) {
    for y in 0..h {
        let row_offset = y * w;
        let row_src = &src[row_offset..row_offset + w];

        prefix[0] = 0;
        for x in 0..w {
            prefix[x + 1] = prefix[x] + row_src[x] as u32;
        }

        let first = row_src[0] as u32;
        let last = row_src[w - 1] as u32;

        let div = (r * 2 + 1) as u32;

        for x in 0..w {
            let left_clamp = if x < r { (r - x) as u32 * first } else { 0 };
            let right_clamp = if x + r >= w { (x + r + 1 - w) as u32 * last } else { 0 };

            let x0 = x.saturating_sub(r);
            let x1 = (x + r + 1).min(w);
            let sum = prefix[x1] - prefix[x0] + left_clamp + right_clamp;

            dst[row_offset + x] = (sum / div) as u8;
        }
    }
}

fn box_blur_v(src: &[u8], dst: &mut [u8], prefix: &mut [u32], w: usize, h: usize, r: usize) {
    for x in 0..w {
        prefix[0] = 0;
        for y in 0..h {
            prefix[y + 1] = prefix[y] + src[y * w + x] as u32;
        }

        let first = src[x] as u32;
        let last = src[(h - 1) * w + x] as u32;

        let div = (r * 2 + 1) as u32;

        for y in 0..h {
            let top_clamp = if y < r { (r - y) as u32 * first } else { 0 };
            let btm_clamp = if y + r >= h { (y + r + 1 - h) as u32 * last } else { 0 };

            let y0 = y.saturating_sub(r);
            let y1 = (y + r + 1).min(h);
            let sum = prefix[y1] - prefix[y0] + top_clamp + btm_clamp;

            dst[y * w + x] = (sum / div) as u8;
        }
    }
}
