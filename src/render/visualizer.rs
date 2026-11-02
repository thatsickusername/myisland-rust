use tiny_skia::{Color, Paint, PathBuilder, PixmapMut, Transform};

pub struct Visualizer {
    pub compact_bars: Vec<f32>,
    pub mini_bars: Vec<f32>,
    pub expanded_bars: Vec<f32>,
}

impl Visualizer {
    pub fn new() -> Self {
        Self {
            compact_bars: vec![0.15; 6],
            mini_bars: vec![0.15; 3],
            expanded_bars: vec![0.15; 18],
        }
    }

    pub fn draw(
        &mut self,
        pixmap: &mut PixmapMut,
        key: &str,
        x: f32,
        cy: f32,
        w: f32,
        h: f32,
        color: Color,
        n: usize,
        is_playing: bool,
        audio_levels: Option<&[f32]>,
        is_hearing_audio: bool,
        t: f32,
    ) -> bool {
        let bars = match key {
            "sec_mini" => &mut self.mini_bars,
            "expanded" => &mut self.expanded_bars,
            _ => &mut self.compact_bars,
        };

        if bars.len() != n {
            *bars = vec![0.08; n];
        }

        let gap = ((w / (n as f32)) * 0.4).max(2.0);
        let bw = (w - gap * ((n - 1) as f32)) / (n as f32);

        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(color);

        let mut any_moving = false;

        for i in 0..n {
            let want = if !is_playing {
                0.08
            } else if is_hearing_audio && audio_levels.map_or(false, |levs| !levs.is_empty()) {
                let levs = audio_levels.unwrap();
                let idx = (i * levs.len() / n).min(levs.len() - 1);
                let raw = levs[idx].max(0.06);
                (0.12 + 1.15 * raw.powf(0.65)).min(1.0)
            } else {
                // Responsive harmonic synthesis fallback (MyIsland lines 2342-2344)
                let fi = i as f32;
                let synth = 0.2 + 0.6 * ((t * (2.1 + fi * 0.43) + fi * 1.7).sin().abs())
                    * (0.65 + 0.35 * (t * 1.3 + fi * 0.9).sin());
                (0.12 + 1.15 * synth.powf(0.65)).min(1.0)
            };

            let diff = (want - bars[i]).abs();
            if diff > 0.015 {
                any_moving = true;
                let rate = if want > bars[i] { 0.52 } else { 0.16 };
                bars[i] += (want - bars[i]) * rate;
            } else {
                bars[i] = want;
            }

            let bh = bw.max(h * bars[i].min(1.0));
            let bx = x + (i as f32) * (bw + gap);
            let by = cy - bh / 2.0;

            let r = bw / 2.0;
            let mut pb = PathBuilder::new();
            pb.move_to(bx + r, by);
            pb.line_to(bx + bw - r, by);
            pb.quad_to(bx + bw, by, bx + bw, by + r);
            pb.line_to(bx + bw, by + bh - r);
            pb.quad_to(bx + bw, by + bh, bx + bw - r, by + bh);
            pb.line_to(bx + r, by + bh);
            pb.quad_to(bx, by + bh, bx, by + bh - r);
            pb.line_to(bx, by + r);
            pb.quad_to(bx, by, bx + r, by);
            pb.close();

            if let Some(path) = pb.finish() {
                pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }
        }

        any_moving
    }
}
