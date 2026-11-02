use std::f32::consts::PI;

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
}

pub struct ParticleSystem {
    pub particles: Vec<ConfettiParticle>,
    rng_state: u64,
}

impl ParticleSystem {
    pub fn new() -> Self {
        Self {
            particles: Vec::with_capacity(64),
            rng_state: 0x9e3779b97f4a7c15,
        }
    }

    fn next_f32(&mut self) -> f32 {
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 7;
        self.rng_state ^= self.rng_state << 17;
        (self.rng_state as f32) / (u64::MAX as f32)
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

        let count = 52;
        for _ in 0..count {
            let r1 = self.next_f32();
            let r2 = self.next_f32();
            let r3 = self.next_f32();
            let r4 = self.next_f32();
            let r5 = self.next_f32();
            let r6 = self.next_f32();
            let r7 = self.next_f32();

            let angle = -PI / 2.0 + (r1 - 0.5) * 1.6;
            let speed = 2.5 + r2 * 6.0;
            let col_idx = (r7 * (colors.len() as f32)) as usize % colors.len();

            self.particles.push(ConfettiParticle {
                x: origin_x + (r3 - 0.5) * 40.0,
                y: origin_y + (r4 - 0.5) * 12.0,
                vx: angle.cos() * speed + (r5 - 0.5) * 1.4,
                vy: angle.sin() * speed - 1.6,
                color: colors[col_idx],
                w: 4.5 + r6 * 3.5,
                h: 3.5 + r7 * 3.5,
                rot: r1 * PI * 2.0,
                v_rot: (r2 - 0.5) * 0.22,
                gravity: 0.16 + r3 * 0.08,
                alpha: 1.0,
                drag: 0.985,
            });
        }
    }

    /// Update particles. Returns true if any active particles remain.
    pub fn update(&mut self) -> bool {
        if self.particles.is_empty() {
            return false;
        }

        let mut i = 0;
        while i < self.particles.len() {
            let p = &mut self.particles[i];
            p.x += p.vx;
            p.y += p.vy;
            p.vy += p.gravity;
            p.vx *= p.drag;
            p.rot += p.v_rot;
            p.alpha -= 0.007;

            if p.alpha <= 0.0 || p.y > 400.0 {
                self.particles.swap_remove(i);
            } else {
                i += 1;
            }
        }

        !self.particles.is_empty()
    }
}
