#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub asymmetric: bool,
}

impl Spring {
    #[allow(dead_code)]
    pub fn new(val: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            value: val,
            target: val,
            velocity: 0.0,
            stiffness,
            damping,
            asymmetric: false,
        }
    }

    /// Default island spring with Apple Dynamic Island asymmetric physics:
    /// bouncy, crisp 3% overshoot on expansion, snappy snap-back on collapse.
    pub fn default_island(val: f32) -> Self {
        Self {
            value: val,
            target: val,
            velocity: 0.0,
            stiffness: 230.0,
            damping: 24.5,
            asymmetric: true,
        }
    }

    pub fn volume_spring(val: f32) -> Self {
        Self {
            value: val,
            target: val,
            velocity: 0.0,
            stiffness: 260.0,
            damping: 24.0,
            asymmetric: false,
        }
    }

    /// Update spring simulation by dt (seconds) using 240Hz sub-stepping.
    /// Returns true if still actively moving.
    pub fn update(&mut self, dt: f32) -> bool {
        let clamped_dt = dt.clamp(0.001, 0.04);
        let steps = ((clamped_dt / (1.0 / 240.0)).ceil() as usize).max(1).min(8);
        let h = clamped_dt / (steps as f32);

        let (k, d) = if self.asymmetric {
            if self.target > self.value {
                // Expanding / Growing: Apple elastic spring bounce (overshoot ~3%)
                (230.0, 24.5)
            } else {
                // Collapsing / Shrinking: Crisp, rapid Apple snap-back
                (270.0, 30.0)
            }
        } else {
            (self.stiffness, self.damping)
        };

        for _ in 0..steps {
            let force = -k * (self.value - self.target);
            let damp = -d * self.velocity;
            let accel = force + damp;
            self.velocity += accel * h;
            self.value += self.velocity * h;
        }

        if self.velocity.abs() < 0.05 && (self.value - self.target).abs() < 0.05 {
            self.value = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }
}

