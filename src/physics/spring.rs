/// Named physics profiles tuned for Apple-style Dynamic Island fluidity.
/// Stiffness (k) is expressed in px/s² per px of displacement and damping (c)
/// in 1/s. Lower damping ratios (ζ ≈ c / (2√k)) produce elastic overshoot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SpringPreset {
    /// Juicy rubber expansion: ζ ≈ 0.57, ~15% overshoot.
    FluidExpansion,
    /// Crisp snap-back recoil: ζ ≈ 0.69, single micro-oscillation.
    FluidRecoil,
    /// Volume HUD slider: quick but controlled.
    VolumeSlider,
    /// High-frequency organic jelly wobble for detached droplets.
    DropletWobble,
    /// Arbitrary stiffness/damping supplied by the caller.
    Custom,
}

impl SpringPreset {
    /// Returns `(stiffness, damping)` for the preset.
    pub fn params(self) -> (f32, f32) {
        match self {
            SpringPreset::FluidExpansion => (220.0, 17.0),
            SpringPreset::FluidRecoil => (260.0, 21.0),
            SpringPreset::VolumeSlider => (250.0, 22.0),
            SpringPreset::DropletWobble => (310.0, 15.0),
            SpringPreset::Custom => (230.0, 24.5),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub asymmetric: bool,
    pub preset: SpringPreset,
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
            preset: SpringPreset::Custom,
        }
    }

    /// Build a spring from a named physics profile.
    #[allow(dead_code)]
    pub fn from_preset(val: f32, preset: SpringPreset) -> Self {
        let (stiffness, damping) = preset.params();
        Self {
            value: val,
            target: val,
            velocity: 0.0,
            stiffness,
            damping,
            asymmetric: false,
            preset,
        }
    }

    /// Default island spring with Apple Dynamic Island asymmetric physics:
    /// elastic, rubbery expansion (~15% overshoot) and a crisp snap-back recoil.
    pub fn default_island(val: f32) -> Self {
        let (stiffness, damping) = SpringPreset::FluidExpansion.params();
        Self {
            value: val,
            target: val,
            velocity: 0.0,
            stiffness,
            damping,
            asymmetric: true,
            preset: SpringPreset::FluidExpansion,
        }
    }

    pub fn volume_spring(val: f32) -> Self {
        Self::from_preset(val, SpringPreset::VolumeSlider)
    }

    /// Instantly inject kinetic energy (px/s) without changing the target.
    /// Used on click/press triggers to avoid a sluggish zero-velocity takeoff.
    pub fn impulse(&mut self, delta_v: f32) {
        self.velocity += delta_v;
    }

    /// Change target and simultaneously inject kinetic energy (px/s).
    pub fn set_target_with_impulse(&mut self, new_target: f32, impulse: f32) {
        self.target = new_target;
        self.velocity += impulse;
    }

    /// Update spring simulation by dt (seconds) using 240Hz sub-stepping.
    /// Returns true if still actively moving.
    pub fn update(&mut self, dt: f32) -> bool {
        let clamped_dt = dt.clamp(0.001, 0.04);
        let steps = ((clamped_dt / (1.0 / 240.0)).ceil() as usize).max(1).min(8);
        let h = clamped_dt / (steps as f32);

        let (k, d) = if self.asymmetric {
            if self.target >= self.value {
                // Expanding / growing: juicy rubber bounce (~15% overshoot).
                (220.0, 17.2)
            } else {
                // Collapsing / shrinking: crisp, rapid snap-back.
                (265.0, 21.0)
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

        // Low-damping springs need a tighter settle window so we do not freeze
        // the simulation at a low-velocity oscillation peak. Droplet wobble
        // settles at an even tighter window to preserve the final ripple.
        let settle_eps = if self.preset == SpringPreset::DropletWobble {
            0.06
        } else if d < 20.0 {
            0.08
        } else {
            0.05
        };

        if self.velocity.abs() < settle_eps && (self.value - self.target).abs() < settle_eps {
            self.value = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }
}
