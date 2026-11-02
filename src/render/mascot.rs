use std::f32::consts::PI;
use std::time::Instant;
use tiny_skia::{Color, Paint, PathBuilder, PixmapMut, Stroke, Transform};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MascotState {
    Default,
    IdleSleepy,
    IdleLookAround,
    IdleCurious,
    IdleHappy,
    FocusComplete,
    FocusFailed,
    FocusActive,
    MusicPlaying,
    MascotShy,
    MascotIrritated,
}

pub struct MascotEngine {
    pub current_state: MascotState,
    pub state_start: Instant,
    pub state_duration_ms: u64,
    pub last_activity_at: Instant,
    pub next_idle_check_at: Instant,

    // Saccades and eye positions
    pub smooth_look_x: f32,
    pub smooth_look_y: f32,
    pub smooth_bob_x: f32,
    pub smooth_bob_y: f32,
    pub smooth_scale_x: f32,
    pub smooth_scale_y: f32,
    pub blush_display_alpha: f32,

    // Blinking
    pub is_blinking: bool,
    pub blink_start_time: Instant,
    pub next_blink_time: Instant,

    // Hover dwell
    pub is_cursor_over_eyes: bool,
    pub has_triggered_hover_reaction: bool,
    pub hover_start_time: Option<Instant>,

    start_instant: Instant,
}

impl MascotEngine {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            current_state: MascotState::Default,
            state_start: now,
            state_duration_ms: 0,
            last_activity_at: now,
            next_idle_check_at: now + std::time::Duration::from_millis(5000),

            smooth_look_x: 0.0,
            smooth_look_y: 0.0,
            smooth_bob_x: 0.0,
            smooth_bob_y: 0.0,
            smooth_scale_x: 1.0,
            smooth_scale_y: 1.0,
            blush_display_alpha: 0.0,

            is_blinking: false,
            blink_start_time: now,
            next_blink_time: now + std::time::Duration::from_millis(2500),

            is_cursor_over_eyes: false,
            has_triggered_hover_reaction: false,
            hover_start_time: None,

            start_instant: now,
        }
    }

    pub fn trigger_state(&mut self, state: MascotState, duration_ms: u64) {
        self.current_state = state;
        self.state_start = Instant::now();
        self.state_duration_ms = duration_ms;
    }

    pub fn notify_activity(&mut self) {
        let now = Instant::now();
        self.last_activity_at = now;
        self.next_idle_check_at = now + std::time::Duration::from_millis(6000);
        if matches!(
            self.current_state,
            MascotState::IdleSleepy
                | MascotState::IdleLookAround
                | MascotState::IdleCurious
                | MascotState::IdleHappy
        ) {
            self.current_state = MascotState::Default;
            self.state_duration_ms = 0;
        }
    }

    pub fn get_next_event_time(&self) -> Instant {
        let now = Instant::now();
        let mut next = self.next_blink_time;
        if self.next_idle_check_at > now {
            next = next.min(self.next_idle_check_at);
        }
        if self.state_duration_ms > 0 {
            let end = self.state_start + std::time::Duration::from_millis(self.state_duration_ms);
            if end > now {
                next = next.min(end);
            }
        }
        next
    }

    pub fn get_active_state(&mut self, is_music_playing: bool, is_focus_running: bool, is_rest: bool) -> MascotState {
        let now = Instant::now();
        if is_rest {
            if matches!(
                self.current_state,
                MascotState::IdleLookAround | MascotState::IdleCurious | MascotState::IdleHappy
            ) {
                self.current_state = MascotState::Default;
                self.state_duration_ms = 0;
            }
            if now >= self.next_idle_check_at {
                self.next_idle_check_at = now + std::time::Duration::from_millis(6000);
            }
            return MascotState::Default;
        }

        if self.state_duration_ms > 0 {
            let elapsed = now.duration_since(self.state_start).as_millis() as u64;
            if elapsed < self.state_duration_ms {
                return self.current_state;
            }
        }

        if is_focus_running {
            return MascotState::FocusActive;
        }
        if is_music_playing {
            return MascotState::MusicPlaying;
        }

        let idle_dur = now.duration_since(self.last_activity_at).as_millis() as u64;
        if idle_dur > 4500 {
            if now >= self.next_idle_check_at {
                let variations = [
                    MascotState::IdleSleepy,
                    MascotState::IdleLookAround,
                    MascotState::IdleCurious,
                    MascotState::IdleHappy,
                ];
                // Pseudo-random selection based on time
                let idx = (now.duration_since(self.start_instant).as_millis() / 100) as usize % variations.len();
                let chosen = variations[idx];
                let dur = match chosen {
                    MascotState::IdleLookAround => 3200,
                    MascotState::IdleSleepy => 5000,
                    _ => 2800,
                };
                self.trigger_state(chosen, dur);
                self.next_idle_check_at = now + std::time::Duration::from_millis(dur + 4000);
                return chosen;
            } else if matches!(
                self.current_state,
                MascotState::IdleSleepy
                    | MascotState::IdleLookAround
                    | MascotState::IdleCurious
                    | MascotState::IdleHappy
            ) {
                let elapsed = now.duration_since(self.state_start).as_millis() as u64;
                if elapsed < self.state_duration_ms {
                    return self.current_state;
                } else {
                    self.current_state = MascotState::Default;
                    self.state_duration_ms = 0;
                    return MascotState::Default;
                }
            }
        }

        MascotState::Default
    }

    pub fn update_and_draw(
        &mut self,
        pixmap: &mut PixmapMut,
        cx: f32,
        cy: f32,
        size: f32,
        pointer_x: f32,
        pointer_y: f32,
        is_music_playing: bool,
        is_focus_running: bool,
        is_rest: bool,
    ) -> bool {
        let now = Instant::now();
        let now_ms = now.duration_since(self.start_instant).as_millis() as f32;

        // Hover dwell check
        let is_over_eyes = (pointer_x - cx).abs() < 22.0 && (pointer_y - cy).abs() < 18.0;
        self.is_cursor_over_eyes = is_over_eyes;
        if is_over_eyes {
            if let Some(start) = self.hover_start_time {
                if !self.has_triggered_hover_reaction && now.duration_since(start).as_millis() >= 350 {
                    self.trigger_state(MascotState::MascotShy, 2200);
                    self.has_triggered_hover_reaction = true;
                }
            } else {
                self.hover_start_time = Some(now);
            }
        } else {
            self.hover_start_time = None;
            self.has_triggered_hover_reaction = false;
        }

        let state = self.get_active_state(is_music_playing, is_focus_running, is_rest);

        // Parameters matching MyIsland index.html
        let base_col = Color::from_rgba8(227, 255, 212, 255); // #e3ffd4
        let mut eye_color = base_col;
        let mut lerp_speed: f32 = 0.22;
        let mut bob_x: f32 = 0.0;
        let mut bob_y: f32 = 0.0;
        let mut scale_x: f32 = 1.0;
        let mut scale_y: f32 = 1.0;
        let mut eye_width: f32 = 7.0;
        let mut eye_height: f32 = 11.5;
        let mut can_blink = true;
        let mut shape_type = "ellipse";
        let mut eye_offset_y: f32 = 0.0;
        let mut draw_sweat_drop = false;
        let mut draw_blush = false;
        let mut blush_alpha: f32 = 0.0;

        let mut target_look_mul_x: f32 = 2.4;
        let mut target_look_mul_y: f32 = 1.0;

        match state {
            MascotState::Default => {}
            MascotState::IdleSleepy => {
                eye_width = 7.0;
                eye_height = 2.8;
                eye_offset_y = 2.0;
                lerp_speed = 0.04;
                bob_y = (now_ms / 520.0).sin() * 0.8;
            }
            MascotState::IdleLookAround => {
                lerp_speed = 0.15;
                target_look_mul_x = 0.05;
                target_look_mul_y = 0.05;
                scale_x = 0.94;
                scale_y = 1.06;

                let total_dur = self.state_duration_ms.max(1) as f32;
                let elapsed = now.duration_since(self.state_start).as_millis() as f32;
                let t = (elapsed / total_dur).clamp(0.0, 1.0);

                if t < 0.12 {
                    let ease = 0.5 - 0.5 * ((t / 0.12) * PI).cos();
                    bob_x = ease * 5.8;
                    bob_y = -ease * 0.8;
                } else if t < 0.38 {
                    bob_x = 5.8 + (now_ms / 180.0).sin() * 0.3;
                    bob_y = -0.8 + (now_ms / 220.0).cos() * 0.3;
                } else if t < 0.48 {
                    let k = (t - 0.38) / 0.10;
                    let ease = 0.5 - 0.5 * (k * PI).cos();
                    bob_x = 5.8 - ease * 11.6;
                    bob_y = -0.8 - ease * 0.8;
                } else if t < 0.74 {
                    bob_x = -5.8 + (now_ms / 180.0).sin() * 0.3;
                    bob_y = -1.6 + (now_ms / 220.0).cos() * 0.3;
                } else if t < 0.86 {
                    let k = (t - 0.74) / 0.12;
                    let ease = 0.5 - 0.5 * (k * PI).cos();
                    bob_x = -5.8 * (1.0 - ease);
                    bob_y = -1.6 * (1.0 - ease);
                } else {
                    bob_x = 0.0;
                    bob_y = 0.0;
                }
            }
            MascotState::IdleCurious => {
                bob_y = -2.0;
                scale_x = 0.88;
                scale_y = 1.20;
            }
            MascotState::IdleHappy => {
                shape_type = "happy_arc";
                bob_y = (now_ms / 200.0).sin() * 0.6;
            }
            MascotState::FocusComplete => {
                eye_color = Color::from_rgba8(255, 51, 136, 255); // #ff3388
                shape_type = "heart";
                let beat = (now_ms / 150.0).sin();
                let heart_scale = 1.0 + if beat > 0.35 { beat.powi(3) * 0.35 } else { 0.0 };
                scale_x = heart_scale;
                scale_y = heart_scale;
            }
            MascotState::FocusFailed => {
                shape_type = "cross";
                can_blink = false;
                bob_x = ((now_ms as i32 % 17) as f32 / 17.0 - 0.5) * 0.7;
                bob_y = ((now_ms as i32 % 19) as f32 / 19.0 - 0.5) * 0.7;
                lerp_speed = 0.35;
            }
            MascotState::FocusActive => {
                shape_type = "focus_brow";
                lerp_speed = 0.24;
                bob_x = (now_ms / 60.0).sin() * 0.35;
                bob_y = 0.4;
                draw_sweat_drop = true;
            }
            MascotState::MusicPlaying => {
                lerp_speed = 0.25;
                bob_y = -((now_ms / 135.0).sin().abs()) * 4.2;
                bob_x = (now_ms / 270.0).sin() * 4.0;
                scale_x = 1.0 + (now_ms / 135.0).cos() * 0.12;
                scale_y = 1.0 - (now_ms / 135.0).cos() * 0.12;
            }
            MascotState::MascotShy => {
                shape_type = "shy_eyes";
                draw_blush = true;
                target_look_mul_x = 0.25;
                target_look_mul_y = 0.35;
                lerp_speed = 0.18;

                let dur = self.state_duration_ms.max(1) as f32;
                let elapsed = now.duration_since(self.state_start).as_millis() as f32;
                let shy_p = (elapsed / dur).clamp(0.0, 1.0);

                if shy_p < 0.09 {
                    blush_alpha = 0.5 - 0.5 * ((shy_p / 0.09) * PI).cos();
                } else if shy_p < 0.75 {
                    blush_alpha = 1.0;
                } else {
                    let out_p = (shy_p - 0.75) / 0.25;
                    blush_alpha = 0.5 + 0.5 * (out_p * PI).cos();
                }

                bob_y = 1.4 + (now_ms / 220.0).sin() * 0.4;
                bob_x = (now_ms / 320.0).sin() * 0.6;
                scale_x = 0.96;
                scale_y = 0.88;
            }
            MascotState::MascotIrritated => {
                shape_type = "irritated";
                can_blink = false;
                target_look_mul_x = 0.1;
                lerp_speed = 0.35;

                let elapsed = now.duration_since(self.state_start).as_millis() as f32;
                let dur = self.state_duration_ms.max(1) as f32;
                let shake_p = (elapsed / dur).clamp(0.0, 1.0);
                let ramp_in = (elapsed / 200.0).min(1.0);
                let shake_decay = (1.0 - shake_p).powf(0.65) * ramp_in;

                bob_x = ((elapsed / 1000.0) * 42.0).sin() * 6.0 * shake_decay;
                bob_y = (((elapsed / 1000.0) * 21.0).sin().abs()) * 0.5;
                scale_x = 1.04;
                scale_y = 0.94;
            }
        }

        // Cursor tracking
        let raw_target_x = ((pointer_x - cx) / 320.0).clamp(-1.0, 1.0) * target_look_mul_x * size;
        let raw_target_y = ((pointer_y - cy) / 200.0).clamp(-1.0, 1.0) * target_look_mul_y * size;

        self.smooth_look_x += (raw_target_x - self.smooth_look_x) * lerp_speed;
        self.smooth_look_y += (raw_target_y - self.smooth_look_y) * lerp_speed;
        if (raw_target_x - self.smooth_look_x).abs() < 0.02 {
            self.smooth_look_x = raw_target_x;
        }
        if (raw_target_y - self.smooth_look_y).abs() < 0.02 {
            self.smooth_look_y = raw_target_y;
        }

        let bob_lerp = if state == MascotState::MascotIrritated { 0.48 } else { 0.22 };
        self.smooth_bob_x += (bob_x - self.smooth_bob_x) * bob_lerp;
        self.smooth_bob_y += (bob_y - self.smooth_bob_y) * 0.22;
        if (bob_x - self.smooth_bob_x).abs() < 0.02 {
            self.smooth_bob_x = bob_x;
        }
        if (bob_y - self.smooth_bob_y).abs() < 0.02 {
            self.smooth_bob_y = bob_y;
        }

        self.smooth_scale_x += (scale_x - self.smooth_scale_x) * 0.20;
        self.smooth_scale_y += (scale_y - self.smooth_scale_y) * 0.20;
        if (scale_x - self.smooth_scale_x).abs() < 0.01 {
            self.smooth_scale_x = scale_x;
        }
        if (scale_y - self.smooth_scale_y).abs() < 0.01 {
            self.smooth_scale_y = scale_y;
        }

        let target_blush = if draw_blush { blush_alpha } else { 0.0 };
        self.blush_display_alpha += (target_blush - self.blush_display_alpha) * 0.16;
        if (target_blush - self.blush_display_alpha).abs() < 0.01 {
            self.blush_display_alpha = target_blush;
        }

        // Blinking
        if can_blink {
            if !self.is_blinking && now >= self.next_blink_time {
                self.is_blinking = true;
                self.blink_start_time = now;
                let next_gap = 2400 + (now_ms as u64 % 3200);
                self.next_blink_time = now + std::time::Duration::from_millis(next_gap);
            }
        } else {
            self.is_blinking = false;
        }

        let mut is_lid_closed = false;
        if self.is_blinking {
            if now.duration_since(self.blink_start_time).as_millis() < 140 {
                is_lid_closed = true;
            } else {
                self.is_blinking = false;
            }
        }

        let is_animating = self.is_blinking
            || (now.duration_since(self.state_start).as_millis() as u64) < self.state_duration_ms
            || state == MascotState::MusicPlaying
            || (raw_target_x - self.smooth_look_x).abs() > 0.02
            || (raw_target_y - self.smooth_look_y).abs() > 0.02
            || (bob_x - self.smooth_bob_x).abs() > 0.02
            || (bob_y - self.smooth_bob_y).abs() > 0.02
            || (scale_x - self.smooth_scale_x).abs() > 0.01
            || (scale_y - self.smooth_scale_y).abs() > 0.01
            || self.blush_display_alpha > 0.01;

        // Vector drawing
        let root_x = cx + self.smooth_look_x + self.smooth_bob_x;
        let root_y = cy + self.smooth_look_y + self.smooth_bob_y;

        let eye_spacing = 9.0 * size;
        let base_scale = 0.72 * size;

        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(eye_color);

        let stroke_3 = Stroke {
            width: 3.0,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };

        for side in [-1.0f32, 1.0f32] {
            let eye_cx = root_x + side * eye_spacing;
            let eye_cy = root_y + eye_offset_y;

            let sx = base_scale * self.smooth_scale_x;
            let sy = base_scale * self.smooth_scale_y;
            let transform = Transform::from_scale(sx, sy).post_translate(eye_cx, eye_cy);

            if is_lid_closed {
                let mut pb = PathBuilder::new();
                pb.move_to(-5.2, 0.0);
                pb.quad_to(0.0, 4.0, 5.2, 0.0);
                if let Some(p) = pb.finish() {
                    pixmap.stroke_path(&p, &paint, &stroke_3, transform, None);
                }
            } else if shape_type == "heart" {
                let mut pb = PathBuilder::new();
                pb.move_to(0.0, 4.2);
                pb.cubic_to(-5.2, 0.4, -6.4, -5.2, -3.2, -6.2);
                pb.cubic_to(-1.0, -6.6, 0.0, -4.0, 0.0, -4.0);
                pb.cubic_to(0.0, -4.0, 1.0, -6.6, 3.2, -6.2);
                pb.cubic_to(6.4, -5.2, 5.2, 0.4, 0.0, 4.2);
                pb.close();
                if let Some(p) = pb.finish() {
                    pixmap.fill_path(&p, &paint, tiny_skia::FillRule::Winding, transform, None);
                }
            } else if shape_type == "cross" {
                let r = 4.2;
                let mut pb = PathBuilder::new();
                pb.move_to(-r, -r);
                pb.line_to(r, r);
                pb.move_to(r, -r);
                pb.line_to(-r, r);
                if let Some(p) = pb.finish() {
                    pixmap.stroke_path(&p, &paint, &stroke_3, transform, None);
                }
            } else if shape_type == "happy_arc" {
                let mut pb = PathBuilder::new();
                pb.move_to(-5.2, 1.5);
                pb.quad_to(0.0, -3.5, 5.2, 1.5);
                if let Some(p) = pb.finish() {
                    pixmap.stroke_path(&p, &paint, &stroke_3, transform, None);
                }
            } else if shape_type == "focus_brow" {
                let rot_t = Transform::from_rotate(-side * 11.5).post_scale(sx, sy).post_translate(eye_cx, eye_cy);
                // Ellipse
                let k = 0.55228475;
                let rx = eye_width / 2.0;
                let ry = (eye_height / 2.0 - 1.5).max(1.8);
                let cy_off = 1.2;

                let mut pb = PathBuilder::new();
                pb.move_to(0.0, cy_off - ry);
                pb.cubic_to(rx * k, cy_off - ry, rx, cy_off - ry * (1.0 - k), rx, cy_off);
                pb.cubic_to(rx, cy_off + ry * (1.0 - k), rx * k, cy_off + ry, 0.0, cy_off + ry);
                pb.cubic_to(-rx * k, cy_off + ry, -rx, cy_off + ry * (1.0 - k), -rx, cy_off);
                pb.cubic_to(-rx, cy_off - ry * (1.0 - k), -rx * k, cy_off - ry, 0.0, cy_off - ry);
                pb.close();

                // Upper brow slant line
                pb.move_to(-4.5, -4.0);
                pb.line_to(4.5, -2.5);

                if let Some(p) = pb.finish() {
                    pixmap.fill_path(&p, &paint, tiny_skia::FillRule::Winding, rot_t, None);
                    let brow_stroke = Stroke { width: 2.4, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
                    pixmap.stroke_path(&p, &paint, &brow_stroke, rot_t, None);
                }
            } else if shape_type == "shy_eyes" {
                let mut pb = PathBuilder::new();
                pb.move_to(-5.0, -1.0);
                pb.quad_to(0.0, 3.5, 5.0, -1.0);
                if let Some(p) = pb.finish() {
                    pixmap.stroke_path(&p, &paint, &stroke_3, transform, None);
                }
            } else if shape_type == "irritated" {
                let rot_t = Transform::from_rotate(-side * 16.0).post_scale(sx, sy).post_translate(eye_cx, eye_cy);
                let mut pb = PathBuilder::new();
                pb.move_to(-5.2, 0.0);
                pb.line_to(5.2, 0.0);
                if let Some(p) = pb.finish() {
                    pixmap.stroke_path(&p, &paint, &stroke_3, rot_t, None);
                }
            } else {
                // Standard solid ellipse with Apple Dynamic Island bioluminescent glow (#e3ffd4)
                let k = 0.55228475;
                let rx = eye_width / 2.0;
                let ry = (eye_height / 2.0).max(0.4);

                let mut pb = PathBuilder::new();
                pb.move_to(0.0, -ry);
                pb.cubic_to(rx * k, -ry, rx, -ry * (1.0 - k), rx, 0.0);
                pb.cubic_to(rx, ry * (1.0 - k), rx * k, ry, 0.0, ry);
                pb.cubic_to(-rx * k, ry, -rx, ry * (1.0 - k), -rx, 0.0);
                pb.cubic_to(-rx, -ry * (1.0 - k), -rx * k, -ry, 0.0, -ry);
                pb.close();

                if let Some(p) = pb.finish() {
                    // Soft bioluminescent mint glow (matching index.html shadowBlur: 12)
                    let mut glow_paint = Paint::default();
                    glow_paint.anti_alias = true;
                    glow_paint.set_color_rgba8(227, 255, 212, 38);
                    let glow_stroke_1 = Stroke { width: 3.5, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
                    pixmap.stroke_path(&p, &glow_paint, &glow_stroke_1, transform, None);

                    glow_paint.set_color_rgba8(227, 255, 212, 16);
                    let glow_stroke_2 = Stroke { width: 7.0, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
                    pixmap.stroke_path(&p, &glow_paint, &glow_stroke_2, transform, None);

                    // Solid eye pupil core
                    pixmap.fill_path(&p, &paint, tiny_skia::FillRule::Winding, transform, None);
                }
            }

            // Anime blush lines & soft radial bloom
            if self.blush_display_alpha > 0.02 {
                let mut blush_glow = Paint::default();
                blush_glow.anti_alias = true;
                let bg_a = (self.blush_display_alpha * 55.0).clamp(0.0, 255.0) as u8;
                blush_glow.set_color_rgba8(255, 105, 145, bg_a);
                let mut pb_bg = PathBuilder::new();
                pb_bg.push_circle(0.0, 7.5, 6.0);
                if let Some(p_bg) = pb_bg.finish() {
                    pixmap.fill_path(&p_bg, &blush_glow, tiny_skia::FillRule::Winding, transform, None);
                }

                let mut blush_paint = Paint::default();
                blush_paint.anti_alias = true;
                let a = (self.blush_display_alpha * 0.75).clamp(0.0, 1.0);
                blush_paint.set_color_rgba8(255, 120, 160, (a * 255.0) as u8);

                let blush_y = 8.5;
                let mut pb = PathBuilder::new();
                pb.move_to(-2.5, blush_y - 1.5);
                pb.line_to(-0.8, blush_y + 1.8);
                pb.move_to(1.0, blush_y - 1.5);
                pb.line_to(2.7, blush_y + 1.8);

                let blush_stroke = Stroke { width: 1.2, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
                if let Some(p) = pb.finish() {
                    pixmap.stroke_path(&p, &blush_paint, &blush_stroke, transform, None);
                }
            }
        }

        // Sweat drop
        if draw_sweat_drop {
            let cycle = now_ms as u64 % 3600;
            if cycle < 900 {
                let drop_phase = (cycle as f32) / 900.0;
                let drop_y = -2.0 + drop_phase * 10.0;
                let drop_alpha = (drop_phase * PI).sin();

                let mut drop_paint = Paint::default();
                drop_paint.anti_alias = true;
                drop_paint.set_color_rgba8(100, 210, 255, (drop_alpha * 255.0) as u8);

                let sx = root_x + eye_spacing + 9.0 * size;
                let mut pb = PathBuilder::new();
                pb.move_to(sx, drop_y - 2.5);
                pb.quad_to(sx + 2.0, drop_y, sx, drop_y + 2.5);
                pb.quad_to(sx - 2.0, drop_y, sx, drop_y - 2.5);
                pb.close();

                if let Some(p) = pb.finish() {
                    pixmap.fill_path(&p, &drop_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                }
            }
        }

        is_animating
    }
}
