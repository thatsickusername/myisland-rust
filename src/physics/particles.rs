use std::f32::consts::PI;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

/// Matches the original Electron renderer:
/// ctx.shadowColor = p.color;
/// ctx.shadowBlur = 4;
const SHADOW_BLUR: f32 = 4.0;

#[derive(Debug, Clone)]
pub struct ConfettiParticle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub color: [u8; 4], // RGBA
    pub w: f32,
    pub h: f32,
    pub rot: f32,
    pub v_rot: f32,
    pub gravity: f32,
    pub alpha: f32,
    pub drag: f32,
    /// Pre-blurred colored glow sprite matching the Canvas shadowBlur pass.
    /// This is rotated with the particle at draw time.
    pub glow: Option<Pixmap>,
}

pub struct ParticleSystem {
    pub particles: Vec<ConfettiParticle>,
    rng_state: u64,
}

static RNG_SALT: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);

fn time_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let salt = RNG_SALT.fetch_add(0xA076_1D64_78BD_642F, Ordering::Relaxed);
    // splitmix64 finalizer to spread the entropy across all bits
    let mut z = nanos ^ salt;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl ParticleSystem {
    pub fn new() -> Self {
        let seed = time_seed();
        Self {
            particles: Vec::with_capacity(64),
            rng_state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    /// Uniform f32 in [0, 1). Matches `Math.random()` distribution.
    fn next_f32(&mut self) -> f32 {
        // xorshift64*
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((v >> 40) as f32) / ((1u32 << 24) as f32)
    }

    pub fn launch_confetti(&mut self, origin_x: f32, origin_y: f32) {
        let colors: [[u8; 4]; 7] = [
            [255, 51, 136, 255],  // #ff3388
            [255, 214, 10, 255],  // #ffd60a
            [100, 210, 255, 255], // #64d2ff
            [255, 159, 10, 255],  // #ff9f0a
            [34, 224, 90, 255],   // #22e05a
            [191, 90, 242, 255],  // #bf5af2
            [255, 255, 255, 255], // #ffffff
        ];

        // Same properties in the same random order as launchConfetti() in index.html.
        let count = 52;
        for _ in 0..count {
            let angle = -PI / 2.0 + (self.next_f32() - 0.5) * 1.6;
            let speed = 2.5 + self.next_f32() * 6.0;
            let x = origin_x + (self.next_f32() - 0.5) * 40.0;
            let y = origin_y + (self.next_f32() - 0.5) * 12.0;
            let vx = angle.cos() * speed + (self.next_f32() - 0.5) * 1.4;
            let vy = angle.sin() * speed - 1.6;
            let color = colors[(self.next_f32() * (colors.len() as f32)) as usize % colors.len()];
            let w = 4.5 + self.next_f32() * 3.5;
            let h = 3.5 + self.next_f32() * 3.5;
            let rot = self.next_f32() * PI * 2.0;
            let v_rot = (self.next_f32() - 0.5) * 0.22;
            let gravity = 0.16 + self.next_f32() * 0.08;

            let glow = make_glow_sprite(color, w, h);

            self.particles.push(ConfettiParticle {
                x,
                y,
                vx,
                vy,
                color,
                w,
                h,
                rot,
                v_rot,
                gravity,
                alpha: 1.0,
                drag: 0.985,
                glow,
            });
        }
    }

    /// Update particles. Returns true if any active particles remain.
    /// `surface_height` is the equivalent of `window.innerHeight` in the
    /// original renderer; particles are removed once they fall past it.
    pub fn update(&mut self, surface_height: f32) -> bool {
        if self.particles.is_empty() {
            return false;
        }

        // `retain` preserves draw order, matching the original splice() loop.
        self.particles.retain_mut(|p| {
            p.x += p.vx;
            p.y += p.vy;
            p.vy += p.gravity;
            p.vx *= p.drag;
            p.rot += p.v_rot;
            p.alpha -= 0.007;

            p.alpha > 0.0 && p.y <= surface_height
        });

        !self.particles.is_empty()
    }
}

/// Builds an axis-aligned, pre-blurred colored rectangle sprite.
/// Because the blur is isotropic, rotating this sprite at draw time is
/// identical to rotating the rectangle first and then blurring it, which is
/// exactly what Canvas `shadowBlur` does.
fn make_glow_sprite(color: [u8; 4], w: f32, h: f32) -> Option<Pixmap> {
    let sigma = (SHADOW_BLUR * 0.5).max(0.5);
    let pad = (sigma * 3.0).ceil() as i32;
    let pw = (w.ceil() as i32 + pad * 2).max(1) as u32;
    let ph = (h.ceil() as i32 + pad * 2).max(1) as u32;

    let mut pixmap = Pixmap::new(pw, ph)?;

    let rx = (pw as f32 - w) * 0.5;
    let ry = (ph as f32 - h) * 0.5;

    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color_rgba8(color[0], color[1], color[2], 255);

    let mut pb = PathBuilder::new();
    pb.push_rect(Rect::from_xywh(rx, ry, w, h)?);
    let path = pb.finish()?;
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);

    // Blur the alpha channel, then re-premultiply with the particle color.
    let num_pixels = (pw * ph) as usize;
    let mut alpha: Vec<u8> = pixmap.pixels().iter().map(|p| p.alpha()).collect();
    let mut temp = vec![0u8; num_pixels];
    let mut prefix = vec![0u32; (pw.max(ph) as usize) + 1];
    crate::render::shadow::gaussian_blur_alpha(
        &mut alpha,
        &mut temp,
        &mut prefix,
        pw as usize,
        ph as usize,
        sigma,
    );

    let data = pixmap.data_mut();
    for (i, &a) in alpha.iter().enumerate() {
        let idx = i * 4;
        let a_norm = a as f32 / 255.0;
        data[idx] = (color[0] as f32 * a_norm).round() as u8;
        data[idx + 1] = (color[1] as f32 * a_norm).round() as u8;
        data[idx + 2] = (color[2] as f32 * a_norm).round() as u8;
        data[idx + 3] = a;
    }

    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Point;

    #[test]
    fn rect_rotates_around_its_own_center() {
        let (px, py) = (430.0f32, 28.0f32);
        let t = Transform::from_rotate(37.0).post_translate(px, py);

        let mut center = Point { x: 0.0, y: 0.0 };
        t.map_point(&mut center);
        assert!((center.x - px).abs() < 0.001);
        assert!((center.y - py).abs() < 0.001);

        let mut corner = Point { x: 5.0, y: 0.0 };
        t.map_point(&mut corner);
        let dist = ((corner.x - px).powi(2) + (corner.y - py).powi(2)).sqrt();
        assert!((dist - 5.0).abs() < 0.001);
    }

    #[test]
    fn glow_sprite_center_maps_to_particle_position() {
        let glow = Pixmap::new(20, 20).unwrap();
        let (cx, cy) = (glow.width() as f32 * 0.5, glow.height() as f32 * 0.5);
        let (px, py) = (430.0f32, 28.0f32);
        let t = Transform::from_translate(-cx, -cy)
            .post_rotate(37.0)
            .post_translate(px, py);

        let mut center = Point { x: cx, y: cy };
        t.map_point(&mut center);
        assert!((center.x - px).abs() < 0.001);
        assert!((center.y - py).abs() < 0.001);
    }
}
