use crate::platform::MediaState;
use crate::render::{AlbumArtManager, LayeredCanvas, MascotEngine};
use crate::state::{clean_title_and_artist, ClipboardManager, LyricsState};
use tiny_skia::{Color, Paint, PathBuilder, Stroke, Transform};

pub const APP_NAMES: [&str; 4] = ["MUSIC", "CLIPBOARD", "NOTES", "FOCUS"];

pub struct ExpandedRenderer;

impl ExpandedRenderer {
    pub fn render(
        canvas: &mut LayeredCanvas,
        mascot: &mut MascotEngine,
        album_art: &AlbumArtManager,
        island_x: f32,
        w: f32,
        h: f32,
        current_app: usize,
        scroll_progress: f32,
        pointer_x: f32,
        pointer_y: f32,
        media: &MediaState,
        lyrics: &LyricsState,
        clips: &ClipboardManager,
        focus_active: bool,
        focus_paused: bool,
        focus_left_ms: u64,
        focus_total_ms: u64,
        focus_chosen_sec: u64,
        notes_focused: bool,
        notes_cursor_pos: usize,
        cursor_moved: bool,
        clip_copied_index: Option<usize>,
        clip_scroll_progress: f32,
        target_notes_scroll: &mut f32,
        notes_scroll_progress: f32,
    ) -> bool {
        // Faithful to MyIsland index.html line 3598: card content only renders when h > 80.0
        if h <= 80.0 {
            return false;
        }

        let content_alpha = ((h - 80.0) / 45.0).clamp(0.0, 1.0);
        let cx = island_x + w / 2.0;

        // 1. Small Companion eyes at top-left
        let animating = mascot.update_and_draw(
            &mut canvas.pixmap.as_mut(),
            island_x + 28.0,
            20.0,
            0.85,
            pointer_x,
            pointer_y,
            media.is_playing,
            focus_active,
            false,
        );

        // 2. Header: Time and Date (cached for 500ms to avoid querying Windows registry/timezones every 16ms frame)
        static TIME_CACHE: std::sync::Mutex<Option<(std::time::Instant, String, String, f32)>> = std::sync::Mutex::new(None);
        let (time_str, date_full, time_w) = {
            let mut cache = TIME_CACHE.lock().unwrap();
            let now_inst = std::time::Instant::now();
            if let Some((cached_at, ref t, ref d, w)) = *cache {
                if now_inst.duration_since(cached_at).as_millis() < 500 {
                    (t.clone(), d.clone(), w)
                } else {
                    let now = chrono::Local::now();
                    let t = now.format("%I:%M %p").to_string();
                    let d = format!("•  {}", now.format("%a, %b %e"));
                    let w = canvas.measure_text_width(&t, 13.0, true);
                    *cache = Some((now_inst, t.clone(), d.clone(), w));
                    (t, d, w)
                }
            } else {
                let now = chrono::Local::now();
                let t = now.format("%I:%M %p").to_string();
                let d = format!("•  {}", now.format("%a, %b %e"));
                let w = canvas.measure_text_width(&t, 13.0, true);
                *cache = Some((now_inst, t.clone(), d.clone(), w));
                (t, d, w)
            }
        };

        canvas.draw_text(&time_str, cx - time_w - 8.0, 14.0, 13.0, Color::WHITE, true);

        let date_col = Color::from_rgba8(255, 255, 255, (115.0 * content_alpha) as u8);
        canvas.draw_text(&date_full, cx + 8.0, 14.0, 11.5, date_col, false);

        // 3. 3-Dots Menu Button
        let dot_cx = island_x + w - 27.0;
        let dot_cy = 20.0;
        let mut dot_paint = Paint::default();
        dot_paint.anti_alias = true;
        dot_paint.set_color_rgba8(255, 255, 255, (165.0 * content_alpha) as u8);

        for i in [-1.0f32, 0.0, 1.0] {
            let mut pb = PathBuilder::new();
            pb.push_circle(dot_cx + i * 4.8, dot_cy, 1.4);
            if let Some(p) = pb.finish() {
                canvas.pixmap.fill_path(&p, &dot_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }
        }

        // 4. Header Divider Line
        let mut div_paint = Paint::default();
        div_paint.set_color_rgba8(255, 255, 255, (20.0 * content_alpha) as u8);
        let stroke_1 = Stroke { width: 1.0, ..Default::default() };
        let mut pb = PathBuilder::new();
        pb.move_to(island_x + 16.0, 36.0);
        pb.line_to(island_x + w - 16.0, 36.0);
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &div_paint, &stroke_1, Transform::identity(), None);
        }

        // 5. Active App Panel
        let body_x = island_x + 22.0;
        let body_y = 44.0;
        let body_w = w - 44.0;

        match current_app {
            0 => Self::render_music(canvas, album_art, body_x, body_y, body_w, pointer_x, pointer_y, media, lyrics),
            1 => Self::render_clipboard(canvas, body_x, body_y, body_w, h, content_alpha, clips, pointer_x, pointer_y, clip_copied_index, clip_scroll_progress),
            2 => Self::render_notes(canvas, body_x, body_y, body_w, clips, notes_focused, notes_cursor_pos, cursor_moved, target_notes_scroll, notes_scroll_progress),
            3 => Self::render_focus(
                canvas,
                body_x,
                body_y,
                body_w,
                focus_active,
                focus_paused,
                focus_left_ms,
                focus_total_ms,
                focus_chosen_sec,
            ),
            _ => {}
        }

        // 6. iPhone Camera Mode Selector (Carousel) - only shows when island is tall enough
        if h >= 110.0 {
            let cam_alpha = ((h - 110.0) / 25.0).clamp(0.0, 1.0);
            Self::render_camera_modes(canvas, island_x, h - 26.0, w, scroll_progress, cam_alpha);
        }

        animating || notes_focused
    }

    fn render_music(
        canvas: &mut LayeredCanvas,
        album_art: &AlbumArtManager,
        x: f32,
        y: f32,
        bw: f32,
        pointer_x: f32,
        pointer_y: f32,
        media: &MediaState,
        lyrics: &LyricsState,
    ) {
        let cs = 64.0;

        // 1. Album Art Drop Shadow (MyIsland lines 3716-3718: rgba(0, 0, 0, 0.55), blur 12, offset Y 4)
        let mut shadow_paint = Paint::default();
        shadow_paint.anti_alias = true;
        shadow_paint.set_color_rgba8(0, 0, 0, 70);
        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, x - 1.0, y + 3.0, cs + 2.0, cs + 2.0, 11.0);
        if let Some(p) = pb.finish() {
            canvas.pixmap.fill_path(&p, &shadow_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }

        // 2. Album Art Box (64x64 with radius 10.0)
        album_art.draw(&mut canvas.pixmap.as_mut(), x, y, cs, 10.0);

        // 3. Title and Artist with responsive truncation
        let tx = x + cs + 14.0;
        let title_w = bw - cs - 140.0;

        static MEDIA_CACHE: std::sync::Mutex<Option<(String, String, String, String)>> = std::sync::Mutex::new(None);
        let (clean_t, clean_a) = {
            let mut cache = MEDIA_CACHE.lock().unwrap();
            if let Some((ref prev_t, ref prev_a, ref ct, ref ca)) = *cache {
                if *prev_t == media.title && *prev_a == media.artist {
                    (ct.clone(), ca.clone())
                } else {
                    let (t, a) = if media.title.is_empty() {
                        ("Nothing playing".to_string(), "Start audio in Spotify, Chrome or Media Player".to_string())
                    } else {
                        let (t, a) = clean_title_and_artist(&media.title, &media.artist);
                        let artist_final = if a.is_empty() { "Unknown Artist".to_string() } else { a };
                        (t, artist_final)
                    };
                    *cache = Some((media.title.clone(), media.artist.clone(), t.clone(), a.clone()));
                    (t, a)
                }
            } else {
                let (t, a) = if media.title.is_empty() {
                    ("Nothing playing".to_string(), "Start audio in Spotify, Chrome or Media Player".to_string())
                } else {
                    let (t, a) = clean_title_and_artist(&media.title, &media.artist);
                    let artist_final = if a.is_empty() { "Unknown Artist".to_string() } else { a };
                    (t, artist_final)
                };
                *cache = Some((media.title.clone(), media.artist.clone(), t.clone(), a.clone()));
                (t, a)
            }
        };

        let title_str = canvas.truncate_to_width(&clean_t, title_w, 14.0, true);
        canvas.draw_text(&title_str, tx, y + 8.0, 14.0, Color::WHITE, true);

        let artist_str = canvas.truncate_to_width(&clean_a, title_w, 12.0, false);
        let artist_col = Color::from_rgba8(255, 255, 255, 153);
        canvas.draw_text(&artist_str, tx, y + 26.0, 12.0, artist_col, false);

        // 4. Synced Lyric line in expanded card (MyIsland lines 3770-3781)
        let (_, now_l, next_l) = lyrics.get_around_lyrics();
        let display_l = if !now_l.is_empty() { now_l } else { next_l };
        if !display_l.is_empty() {
            let [cr, cg, cb] = album_art.dominant_color;
            let lyric_col = Color::from_rgba8(cr, cg, cb, 255);
            let ly_cy = y + 49.0;
            Self::draw_music_note_icon(canvas, tx, ly_cy, lyric_col);

            let max_w = title_w - 14.0;
            let lyric_line = canvas.truncate_to_width(&display_l, max_w, 11.5, false);
            canvas.draw_text(&lyric_line, tx + 12.0, y + 44.0, 11.5, lyric_col, false);
        }

        // 5. Progress scrubber (MyIsland lines 3784-3813)
        let bar_y = y + cs + 14.0;
        let bx0 = x + 34.0;
        let bw_total = bw - 74.0;

        let cur_sec = media.progress_ms / 1000;
        let cur_str = format!("{}:{:02}", cur_sec / 60, cur_sec % 60);
        canvas.draw_text(&cur_str, x, bar_y - 4.0, 9.5, Color::from_rgba8(255, 255, 255, 115), false);

        let dur_sec = media.duration_ms / 1000;
        let rem_sec = dur_sec.saturating_sub(cur_sec);
        let rem_str = if dur_sec > 0 {
            format!("-{}:{:02}", rem_sec / 60, rem_sec % 60)
        } else {
            "0:00".to_string()
        };
        canvas.draw_text(&rem_str, x + bw - 30.0, bar_y - 4.0, 9.5, Color::from_rgba8(255, 255, 255, 115), false);

        // Scrubber track
        let mut bg_bar = Paint::default();
        bg_bar.set_color_rgba8(255, 255, 255, 41);
        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, bx0, bar_y - 1.25, bw_total, 2.5, 1.25);
        if let Some(p) = pb.finish() {
            canvas.pixmap.fill_path(&p, &bg_bar, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }

        // Scrubber fill
        if media.duration_ms > 0 {
            let frac = (media.progress_ms as f32 / media.duration_ms as f32).clamp(0.0, 1.0);
            let fill_w = (bw_total * frac).max(2.5);
            let mut fg_bar = Paint::default();
            fg_bar.set_color_rgba8(255, 255, 255, 255);
            let mut pb = PathBuilder::new();
            Self::add_round_rect(&mut pb, bx0, bar_y - 1.25, fill_w, 2.5, 1.25);
            if let Some(p) = pb.finish() {
                canvas.pixmap.fill_path(&p, &fg_bar, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }
        }

        // 6. Media Buttons (Prev, Play/Pause, Next) - Exact MyIsland icons & hover colors
        let ctrl_y = y + 28.0;
        let right_pos = x + bw;
        let ctl_x = right_pos - 50.0;
        let btn_size = 26.0f32;

        let prev_hit = (ctl_x - 64.0, ctrl_y - 13.0, btn_size, btn_size);
        let play_hit = (ctl_x - 30.0, ctrl_y - 13.0, btn_size, btn_size);
        let next_hit = (ctl_x + 4.0, ctrl_y - 13.0, btn_size, btn_size);

        let inside = |px: f32, py: f32, hit: (f32, f32, f32, f32)| -> bool {
            px >= hit.0 && px <= hit.0 + hit.2 && py >= hit.1 && py <= hit.1 + hit.3
        };

        let prev_col = if inside(pointer_x, pointer_y, prev_hit) {
            Color::from_rgba8(255, 255, 255, 153)
        } else {
            Color::WHITE
        };
        let play_col = if inside(pointer_x, pointer_y, play_hit) {
            Color::from_rgba8(255, 255, 255, 153)
        } else {
            Color::WHITE
        };
        let next_col = if inside(pointer_x, pointer_y, next_hit) {
            Color::from_rgba8(255, 255, 255, 153)
        } else {
            Color::WHITE
        };

        Self::draw_media_glyph(canvas, "prev", prev_hit.0 + 13.0, ctrl_y, 13.0, prev_col);
        Self::draw_media_glyph(
            canvas,
            if media.is_playing { "pause" } else { "play" },
            play_hit.0 + 13.0,
            ctrl_y,
            18.0,
            play_col,
        );
        Self::draw_media_glyph(canvas, "next", next_hit.0 + 13.0, ctrl_y, 13.0, next_col);
    }

    /// Exact 1:1 port of drawMediaIcon from MyIsland/index.html lines 2360-2383
    fn draw_media_glyph(
        canvas: &mut LayeredCanvas,
        kind: &str,
        x: f32,
        y: f32,
        s: f32,
        color: Color,
    ) {
        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(color);

        let mut pb = PathBuilder::new();
        match kind {
            "play" => {
                Self::add_tri(&mut pb, x - s * 0.3, y, s, 1.0);
            }
            "pause" => {
                Self::add_round_rect(&mut pb, x - s * 0.4, y - s / 2.0, s * 0.28, s, 2.0);
                Self::add_round_rect(&mut pb, x + s * 0.12, y - s / 2.0, s * 0.28, s, 2.0);
            }
            "next" => {
                let d = 1.0f32;
                Self::add_tri(&mut pb, x - d * s * 0.6, y, s, d);
                Self::add_tri(&mut pb, x - d * s * 0.05, y, s, d);
                Self::add_round_rect(&mut pb, x + d * s * 0.7, y - s / 2.0, s * 0.16, s, 1.0);
            }
            "prev" => {
                let d = -1.0f32;
                Self::add_tri(&mut pb, x - d * s * 0.6, y, s, d);
                Self::add_tri(&mut pb, x - d * s * 0.05, y, s, d);
                Self::add_round_rect(&mut pb, x + d * s * 0.7 - s * 0.16, y - s / 2.0, s * 0.16, s, 1.0);
            }
            _ => {}
        }

        if let Some(p) = pb.finish() {
            canvas.pixmap.fill_path(&p, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }
    }

    fn add_tri(pb: &mut PathBuilder, x0: f32, y: f32, s: f32, dir: f32) {
        pb.move_to(x0, y - s / 2.0);
        pb.line_to(x0 + dir * s * 0.75, y);
        pb.line_to(x0, y + s / 2.0);
        pb.close();
    }

    fn add_round_rect(pb: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let r = r.min(w / 2.0).min(h / 2.0);
        pb.move_to(x + r, y);
        pb.line_to(x + w - r, y);
        pb.quad_to(x + w, y, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.quad_to(x + w, y + h, x + w - r, y + h);
        pb.line_to(x + r, y + h);
        pb.quad_to(x, y + h, x, y + h - r);
        pb.line_to(x, y + r);
        pb.quad_to(x, y, x + r, y);
        pb.close();
    }

    fn render_clipboard(
        canvas: &mut LayeredCanvas,
        x: f32,
        y: f32,
        bw: f32,
        h: f32,
        content_alpha: f32,
        clips: &ClipboardManager,
        pointer_x: f32,
        pointer_y: f32,
        copied_index: Option<usize>,
        clip_scroll_progress: f32,
    ) {
        let bottom_limit = (h - 28.0).min(134.0);
        if bottom_limit <= y + 8.0 || content_alpha <= 0.01 {
            return;
        }

        if clips.clips.is_empty() {
            if bottom_limit < y + 42.0 {
                return;
            }
            let badge_size = 32.0;
            let badge_x = x + (bw - badge_size) / 2.0;
            let badge_y = y + 4.0;

            let mut bg_paint = Paint::default();
            bg_paint.anti_alias = true;
            bg_paint.set_color_rgba8(100, 210, 255, (26.0 * content_alpha) as u8);
            let mut pb = PathBuilder::new();
            Self::add_round_rect(&mut pb, badge_x, badge_y, badge_size, badge_size, 9.0);
            if let Some(p) = pb.finish() {
                canvas.pixmap.fill_path(&p, &bg_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            Self::draw_clipboard_glyph(canvas, badge_x + badge_size / 2.0, badge_y + badge_size / 2.0, Color::from_rgba8(100, 210, 255, (220.0 * content_alpha) as u8));

            let title = "No Clips Yet";
            let tw = canvas.measure_text_width(title, 12.5, true);
            canvas.draw_text(title, x + (bw - tw) / 2.0, badge_y + badge_size + 6.0, 12.5, Color::from_rgba8(255, 255, 255, (255.0 * content_alpha) as u8), true);

            let sub = "Copy text anywhere (Ctrl + C) to pin it here";
            let sw = canvas.measure_text_width(sub, 11.0, false);
            canvas.draw_text(sub, x + (bw - sw) / 2.0, badge_y + badge_size + 22.0, 11.0, Color::from_rgba8(255, 255, 255, (115.0 * content_alpha) as u8), false);
            return;
        }

        let row_h = 34.0;
        let pitch = 40.0; // 34px card + 6px gap

        // Render cards continuously along smooth scroll position
        for (i, clip) in clips.clips.iter().enumerate() {
            let row_y = y + (i as f32 - clip_scroll_progress) * pitch;

            // Cull offscreen cards: must be strictly above bottom_limit
            if row_y + row_h < 36.0 || row_y >= bottom_limit {
                continue;
            }

            // Alpha modulation for smooth entry/exit and expansion animation
            let top_alpha = if row_y < y {
                ((row_y + row_h - 36.0) / row_h).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let bot_alpha = if row_y > bottom_limit - 40.0 {
                ((bottom_limit - row_y) / 40.0).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let card_alpha = top_alpha * bot_alpha * content_alpha;
            if card_alpha <= 0.02 {
                continue;
            }

            let is_row_hovered = pointer_x >= x && pointer_x <= x + bw && pointer_y >= row_y && pointer_y <= row_y + row_h;
            let is_copied = copied_index == Some(i);

            // Row background
            let mut row_bg = Paint::default();
            row_bg.anti_alias = true;
            let bg_a = if is_row_hovered { (26.0 * card_alpha) as u8 } else { (13.0 * card_alpha) as u8 };
            row_bg.set_color_rgba8(255, 255, 255, bg_a);
            let mut pb = PathBuilder::new();
            Self::add_round_rect(&mut pb, x, row_y, bw, row_h, 11.0);
            if let Some(p) = pb.finish() {
                canvas.pixmap.fill_path(&p, &row_bg, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            // Row border
            let mut border_paint = Paint::default();
            border_paint.anti_alias = true;
            let border_col = if is_row_hovered {
                Color::from_rgba8(100, 210, 255, (82.0 * card_alpha) as u8)
            } else {
                Color::from_rgba8(255, 255, 255, (20.0 * card_alpha) as u8)
            };
            border_paint.set_color(border_col);
            let stroke_1 = Stroke { width: 1.0, ..Default::default() };
            let mut pb = PathBuilder::new();
            Self::add_round_rect(&mut pb, x + 0.5, row_y + 0.5, bw - 1.0, row_h - 1.0, 10.5);
            if let Some(p) = pb.finish() {
                canvas.pixmap.stroke_path(&p, &border_paint, &stroke_1, Transform::identity(), None);
            }

            // Clip text (strictly clipped to y: 36.0..bottom_limit)
            let text_max_w = bw - 100.0;
            let clean_text = clip.text.trim().replace('\n', " ").replace('\r', "");
            let truncated = canvas.truncate_to_width(&clean_text, text_max_w, 12.0, true);
            let text_col = Color::from_rgba8(255, 255, 255, (235.0 * card_alpha) as u8);
            canvas.draw_text_clipped_box(&truncated, x + 12.0, row_y + 10.0, 12.0, text_col, true, x + 10.0, x + bw - 80.0, 36.0, bottom_limit);

            // Action buttons on right: Copy badge + Delete button
            // 1. Copy Badge
            let copy_btn_w = 46.0;
            let copy_btn_h = 20.0;
            let copy_btn_x = x + bw - 78.0;
            let copy_btn_y = row_y + 7.0;

            if copy_btn_y + copy_btn_h <= bottom_limit {
                let mut copy_bg = Paint::default();
                copy_bg.anti_alias = true;
                let mut copy_border = Paint::default();
                copy_border.anti_alias = true;

                let (text_col_btn, copy_label) = if is_copied {
                    copy_bg.set_color_rgba8(48, 209, 88, (46.0 * card_alpha) as u8);
                    copy_border.set_color_rgba8(48, 209, 88, (107.0 * card_alpha) as u8);
                    (Color::from_rgba8(48, 209, 88, (255.0 * card_alpha) as u8), "Copied")
                } else if is_row_hovered {
                    copy_bg.set_color_rgba8(100, 210, 255, (51.0 * card_alpha) as u8);
                    copy_border.set_color_rgba8(100, 210, 255, (102.0 * card_alpha) as u8);
                    (Color::from_rgba8(100, 210, 255, (255.0 * card_alpha) as u8), "Copy")
                } else {
                    copy_bg.set_color_rgba8(100, 210, 255, (31.0 * card_alpha) as u8);
                    copy_border.set_color_rgba8(100, 210, 255, (66.0 * card_alpha) as u8);
                    (Color::from_rgba8(100, 210, 255, (255.0 * card_alpha) as u8), "Copy")
                };

                let mut pb = PathBuilder::new();
                Self::add_round_rect(&mut pb, copy_btn_x, copy_btn_y, copy_btn_w, copy_btn_h, 8.0);
                if let Some(p) = pb.finish() {
                    canvas.pixmap.fill_path(&p, &copy_bg, tiny_skia::FillRule::Winding, Transform::identity(), None);
                    canvas.pixmap.stroke_path(&p, &copy_border, &stroke_1, Transform::identity(), None);
                }

                if is_copied {
                    let mut pb = PathBuilder::new();
                    pb.move_to(copy_btn_x + 6.0, copy_btn_y + 10.0);
                    pb.line_to(copy_btn_x + 8.5, copy_btn_y + 13.0);
                    pb.line_to(copy_btn_x + 13.0, copy_btn_y + 7.0);
                    let check_stroke = Stroke { width: 1.5, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
                    let mut p_check = Paint::default();
                    p_check.anti_alias = true;
                    p_check.set_color(text_col_btn);
                    if let Some(p) = pb.finish() {
                        canvas.pixmap.stroke_path(&p, &p_check, &check_stroke, Transform::identity(), None);
                    }
                } else {
                    Self::draw_mini_copy_icon(canvas, copy_btn_x + 6.0, copy_btn_y + 5.0, text_col_btn);
                }
                canvas.draw_text_clipped_box(copy_label, copy_btn_x + 16.0, copy_btn_y + 4.5, 10.0, text_col_btn, true, copy_btn_x, copy_btn_x + copy_btn_w, 36.0, bottom_limit);
            }

            // 2. Delete button (Circle with 'x')
            let del_x = x + bw - 26.0;
            let del_y = row_y + 7.0;
            let del_size = 20.0;
            if del_y + del_size <= bottom_limit {
                let is_del_hovered = pointer_x >= del_x && pointer_x <= del_x + del_size && pointer_y >= del_y && pointer_y <= del_y + del_size;

                let mut del_bg = Paint::default();
                del_bg.anti_alias = true;
                let mut del_stroke_p = Paint::default();
                del_stroke_p.anti_alias = true;

                let cross_col = if is_del_hovered {
                    del_bg.set_color_rgba8(255, 69, 58, (64.0 * card_alpha) as u8);
                    del_stroke_p.set_color_rgba8(255, 69, 58, (115.0 * card_alpha) as u8);
                    Color::from_rgba8(255, 69, 58, (255.0 * card_alpha) as u8)
                } else if is_row_hovered {
                    del_bg.set_color_rgba8(255, 255, 255, (20.0 * card_alpha) as u8);
                    del_stroke_p.set_color_rgba8(255, 255, 255, (35.0 * card_alpha) as u8);
                    Color::from_rgba8(255, 255, 255, (178.0 * card_alpha) as u8)
                } else {
                    del_bg.set_color_rgba8(255, 255, 255, (13.0 * card_alpha) as u8);
                    del_stroke_p.set_color_rgba8(255, 255, 255, (20.0 * card_alpha) as u8);
                    Color::from_rgba8(255, 255, 255, (115.0 * card_alpha) as u8)
                };

                let mut pb = PathBuilder::new();
                pb.push_circle(del_x + del_size / 2.0, del_y + del_size / 2.0, del_size / 2.0);
                if let Some(p) = pb.finish() {
                    canvas.pixmap.fill_path(&p, &del_bg, tiny_skia::FillRule::Winding, Transform::identity(), None);
                    canvas.pixmap.stroke_path(&p, &del_stroke_p, &stroke_1, Transform::identity(), None);
                }

                let cx_del = del_x + del_size / 2.0;
                let cy_del = del_y + del_size / 2.0;
                let r = 3.2;
                let mut cross_paint = Paint::default();
                cross_paint.anti_alias = true;
                cross_paint.set_color(cross_col);
                let cross_stroke = Stroke { width: 1.4, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
                let mut pb = PathBuilder::new();
                pb.move_to(cx_del - r, cy_del - r);
                pb.line_to(cx_del + r, cy_del + r);
                pb.move_to(cx_del + r, cy_del - r);
                pb.line_to(cx_del - r, cy_del + r);
                if let Some(p) = pb.finish() {
                    canvas.pixmap.stroke_path(&p, &cross_paint, &cross_stroke, Transform::identity(), None);
                }
            }
        }

        // Clear history button is commented out for now as requested to give cards more area
        /*
        let clear_w = 88.0;
        let clear_h = 17.0;
        let clear_x = x + (bw - clear_w) / 2.0;
        let clear_y = y + 2.0 * pitch + 2.0;
        let is_clear_hovered = pointer_x >= clear_x && pointer_x <= clear_x + clear_w && pointer_y >= clear_y && pointer_y <= clear_y + clear_h;

        let mut clear_bg = Paint::default();
        clear_bg.anti_alias = true;
        let mut clear_border = Paint::default();
        clear_border.anti_alias = true;
        let clear_text_col = if is_clear_hovered {
            clear_bg.set_color_rgba8(255, 69, 58, 46);
            clear_border.set_color_rgba8(255, 69, 58, 97);
            Color::from_rgba8(255, 69, 58, 255)
        } else {
            clear_bg.set_color_rgba8(255, 69, 58, 20);
            clear_border.set_color_rgba8(255, 69, 58, 56);
            Color::from_rgba8(255, 69, 58, 204)
        };

        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, clear_x, clear_y, clear_w, clear_h, 8.5);
        if let Some(p) = pb.finish() {
            canvas.pixmap.fill_path(&p, &clear_bg, tiny_skia::FillRule::Winding, Transform::identity(), None);
            canvas.pixmap.stroke_path(&p, &clear_border, &Stroke { width: 1.0, ..Default::default() }, Transform::identity(), None);
        }

        canvas.draw_text("Clear history", clear_x + 12.0, clear_y + 3.0, 9.5, clear_text_col, true);
        */
    }

    fn render_notes(
        canvas: &mut LayeredCanvas,
        x: f32,
        y: f32,
        bw: f32,
        clips: &ClipboardManager,
        notes_focused: bool,
        notes_cursor_pos: usize,
        cursor_moved: bool,
        target_notes_scroll: &mut f32,
        notes_scroll_progress: f32,
    ) {
        let h = 76.0;

        // Note Card Container
        let mut note_bg = Paint::default();
        note_bg.anti_alias = true;
        note_bg.set_color_rgba8(255, 255, 255, 13);
        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, x, y, bw, h, 12.0);
        if let Some(p) = pb.finish() {
            canvas.pixmap.fill_path(&p, &note_bg, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }

        // Note Card Border (gold glow when focused)
        let mut border_paint = Paint::default();
        border_paint.anti_alias = true;
        if notes_focused {
            border_paint.set_color_rgba8(255, 214, 10, 90);
        } else {
            border_paint.set_color_rgba8(255, 255, 255, 20);
        }
        let stroke_1 = Stroke { width: 1.0, ..Default::default() };
        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, x + 0.5, y + 0.5, bw - 1.0, h - 1.0, 11.5);
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &border_paint, &stroke_1, Transform::identity(), None);
        }

        let padding_x = 12.0;
        let padding_y = 10.0;
        let content_w = bw - padding_x * 2.0;
        let font_size = 12.5;
        let line_height = 17.5;
        let min_clip_y = y + 4.0;
        let max_clip_y = y + h - 4.0;

        if clips.note_text.is_empty() {
            *target_notes_scroll = 0.0;
            if !notes_focused {
                canvas.draw_text(
                    "Type a quick note or task list...",
                    x + padding_x,
                    y + padding_y,
                    font_size,
                    Color::from_rgba8(99, 99, 102, 230),
                    false,
                );
            } else {
                // Empty but focused: draw blinking cursor at start
                let blink = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() % 1000) < 550;
                if blink {
                    let mut caret_p = Paint::default();
                    caret_p.anti_alias = true;
                    caret_p.set_color_rgba8(255, 214, 10, 255);
                    let mut pb = PathBuilder::new();
                    Self::add_round_rect(&mut pb, x + padding_x, y + padding_y, 2.0, 14.0, 1.0);
                    if let Some(p) = pb.finish() {
                        canvas.pixmap.fill_path(&p, &caret_p, tiny_skia::FillRule::Winding, Transform::identity(), None);
                    }
                }
            }
            return;
        }

        let text = &clips.note_text;
        let mut lines: Vec<(String, usize)> = Vec::new(); // (line_str, start_offset)
        let mut offset = 0;

        for (para_i, para) in text.split('\n').enumerate() {
            if para_i > 0 {
                offset += 1; // for '\n'
            }
            if para.is_empty() {
                lines.push((String::new(), offset));
                continue;
            }

            let mut seg = String::new();
            let mut seg_start = offset;
            let words = para.split(' ');

            for word in words {
                let candidate = if seg.is_empty() {
                    word.to_string()
                } else {
                    format!("{} {}", seg, word)
                };

                if canvas.measure_text_width(&candidate, font_size, false) <= content_w {
                    seg = candidate;
                } else {
                    if !seg.is_empty() {
                        let len = seg.len();
                        lines.push((seg, seg_start));
                        seg_start += len + 1; // +1 for space
                        seg = word.to_string();
                    } else {
                        lines.push((candidate, seg_start));
                        seg_start += word.len() + 1;
                        seg = String::new();
                    }
                }
            }
            if !seg.is_empty() {
                lines.push((seg, seg_start));
            }
            offset += para.len();
        }

        if text.ends_with('\n') {
            lines.push((String::new(), text.len()));
        }

        // Calculate cursor line and horizontal position
        let cursor_index = notes_cursor_pos.min(text.len());
        let mut cursor_line_idx = 0;
        let mut cursor_x = x + padding_x;

        for (idx, (line_str, start_off)) in lines.iter().enumerate() {
            let line_end = start_off + line_str.len();
            if cursor_index >= *start_off && (cursor_index <= line_end || idx == lines.len() - 1) {
                cursor_line_idx = idx;
                let offset_in_line = cursor_index.saturating_sub(*start_off).min(line_str.len());
                let sub_str = &line_str[..offset_in_line];
                cursor_x = x + padding_x + canvas.measure_text_width(sub_str, font_size, false);
                break;
            }
        }

        // Auto-scroll when typing to keep cursor line visible
        let total_lines = lines.len();
        let max_scroll = (total_lines.saturating_sub(3)) as f32;

        if cursor_moved {
            if (cursor_line_idx as f32) < *target_notes_scroll {
                *target_notes_scroll = cursor_line_idx as f32;
            } else if (cursor_line_idx as f32) >= *target_notes_scroll + 3.0 {
                *target_notes_scroll = (cursor_line_idx as f32 - 2.0).max(0.0);
            }
        }
        *target_notes_scroll = (*target_notes_scroll).clamp(0.0, max_scroll);

        // Render all visible lines using notes_scroll_progress
        for (line_idx, (line_str, _)) in lines.iter().enumerate() {
            let ly = y + padding_y + ((line_idx as f32) - notes_scroll_progress) * line_height;
            if ly + line_height < min_clip_y || ly > max_clip_y {
                continue;
            }
            canvas.draw_text_clipped_box(
                line_str,
                x + padding_x,
                ly,
                font_size,
                Color::WHITE,
                false,
                x + padding_x,
                x + bw - padding_x,
                min_clip_y,
                max_clip_y,
            );
        }

        // Draw Caret if focused
        if notes_focused {
            let cy = y + padding_y + ((cursor_line_idx as f32) - notes_scroll_progress) * line_height;
            let blink = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() % 1000) < 550;
            if blink && cy >= min_clip_y && cy + 14.0 <= max_clip_y {
                let mut caret_p = Paint::default();
                caret_p.anti_alias = true;
                caret_p.set_color_rgba8(255, 214, 10, 255);
                let mut pb = PathBuilder::new();
                Self::add_round_rect(&mut pb, cursor_x, cy, 2.0, 14.0, 1.0);
                if let Some(p) = pb.finish() {
                    canvas.pixmap.fill_path(&p, &caret_p, tiny_skia::FillRule::Winding, Transform::identity(), None);
                }
            }
        }
    }

    fn render_focus(
        canvas: &mut LayeredCanvas,
        x: f32,
        y: f32,
        bw: f32,
        active: bool,
        paused: bool,
        left_ms: u64,
        total_ms: u64,
        _chosen_sec: u64,
    ) {
        if !active {
            canvas.draw_text(
                "Focus Session",
                x + bw / 2.0 - 44.0,
                y + 10.0,
                14.0,
                Color::WHITE,
                true,
            );
            canvas.draw_text(
                "15 minutes countdown with soft ambient chime",
                x + bw / 2.0 - 110.0,
                y + 28.0,
                11.0,
                Color::from_rgba8(255, 255, 255, 125),
                false,
            );

            // Start Focus Button
            let btn_w = 120.0;
            let btn_x = x + (bw - btn_w) / 2.0;
            let btn_y = y + 44.0;
            let mut start_paint = Paint::default();
            start_paint.set_color_rgba8(240, 160, 100, 255);
            let mut pb = PathBuilder::new();
            Self::add_round_rect(&mut pb, btn_x, btn_y, btn_w, 28.0, 14.0);
            if let Some(p) = pb.finish() {
                canvas.pixmap.fill_path(&p, &start_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }
            canvas.draw_text("Start Focus", btn_x + 28.0, btn_y + 7.0, 12.5, Color::BLACK, true);
        } else {
            let s_tot = left_ms / 1000;
            let time_str = format!("{}:{:02}", s_tot / 60, s_tot % 60);
            let state_str = if paused { "Timer Paused" } else { "Focus Session • In Progress" };

            canvas.draw_text(state_str, x + 8.0, y + 10.0, 13.0, Color::WHITE, true);
            let orange = Color::from_rgba8(240, 160, 100, 255);
            canvas.draw_text(&time_str, x + 8.0, y + 30.0, 24.0, orange, true);

            // Progress bar
            let bar_y = y + 62.0;
            let mut bg_bar = Paint::default();
            bg_bar.set_color_rgba8(255, 255, 255, 30);
            let mut pb = PathBuilder::new();
            Self::add_round_rect(&mut pb, x + 8.0, bar_y, bw - 16.0, 4.0, 2.0);
            if let Some(p) = pb.finish() {
                canvas.pixmap.fill_path(&p, &bg_bar, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            if total_ms > 0 {
                let frac = (left_ms as f32 / total_ms as f32).clamp(0.0, 1.0);
                let mut fg_bar = Paint::default();
                fg_bar.set_color_rgba8(240, 160, 100, 255);
                let mut pb = PathBuilder::new();
                Self::add_round_rect(&mut pb, x + 8.0, bar_y, (bw - 16.0) * frac, 4.0, 2.0);
                if let Some(p) = pb.finish() {
                    canvas.pixmap.fill_path(&p, &fg_bar, tiny_skia::FillRule::Winding, Transform::identity(), None);
                }
            }

            // Controls (Pause/Resume & Cancel)
            let pause_btn_text = if paused { "Resume" } else { "Pause" };
            canvas.draw_text(
                pause_btn_text,
                x + bw - 110.0,
                y + 22.0,
                12.0,
                Color::from_rgba8(255, 255, 255, 200),
                true,
            );
            canvas.draw_text(
                "Cancel",
                x + bw - 45.0,
                y + 22.0,
                12.0,
                Color::from_rgba8(255, 69, 58, 255),
                true,
            );
        }
    }

    fn render_camera_modes(
        canvas: &mut LayeredCanvas,
        island_x: f32,
        y: f32,
        w: f32,
        scroll_pos: f32,
        cam_alpha: f32,
    ) {
        if cam_alpha <= 0.01 {
            return;
        }
        let cx = island_x + w / 2.0;
        let spacing = 92.0;
        let clip_min_x = island_x + 16.0;
        let clip_max_x = island_x + w - 16.0;

        // Golden Indicator Pip (positioned above the active tab text)
        let mut pip = Paint::default();
        pip.anti_alias = true;
        pip.set_color_rgba8(255, 214, 10, (255.0 * cam_alpha) as u8);
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, y - 2.0, 2.0);
        if let Some(p) = pb.finish() {
            canvas.pixmap.fill_path(&p, &pip, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }

        let center_slot = scroll_pos.round() as i32;
        let n = APP_NAMES.len() as i32;

        for offset in -3..=3 {
            let slot = center_slot + offset;
            let app_idx = (((slot % n) + n) % n) as usize;
            let name = APP_NAMES[app_idx];
            let offset_dist = (slot as f32) - scroll_pos;
            let item_x = cx + offset_dist * spacing;

            if item_x < island_x - 50.0 || item_x > island_x + w + 50.0 {
                continue;
            }

            let dist = offset_dist.abs();
            let alpha = (1.0 - dist * 0.52).clamp(0.15, 1.0) * cam_alpha;
            let is_selected = dist < 0.35;

            let color = if is_selected {
                Color::from_rgba8(255, 214, 10, (alpha * 255.0) as u8)
            } else {
                Color::from_rgba8(255, 255, 255, (alpha * 255.0) as u8)
            };

            let tw = canvas.measure_text_width(name, 11.0, is_selected);
            let text_x = item_x - tw / 2.0;

            canvas.draw_text_clipped(name, text_x, y + 5.0, 11.0, color, is_selected, clip_min_x, clip_max_x);
        }
    }

    fn draw_mini_copy_icon(canvas: &mut LayeredCanvas, x: f32, y: f32, color: Color) {
        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(color);
        let stroke = Stroke { width: 1.1, line_cap: tiny_skia::LineCap::Round, ..Default::default() };

        // Back rect
        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, x, y, 6.5, 6.5, 1.2);
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &paint, &stroke, Transform::identity(), None);
        }

        // Front rect
        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, x + 2.5, y + 2.5, 6.5, 6.5, 1.2);
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &paint, &stroke, Transform::identity(), None);
        }
    }

    fn draw_clipboard_glyph(canvas: &mut LayeredCanvas, cx: f32, cy: f32, color: Color) {
        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(color);
        let stroke = Stroke { width: 1.4, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Default::default() };

        // Clipboard board
        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, cx - 6.5, cy - 7.0, 13.0, 15.0, 2.0);
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &paint, &stroke, Transform::identity(), None);
        }

        // Clip on top
        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, cx - 3.5, cy - 9.0, 7.0, 3.5, 1.0);
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &paint, &stroke, Transform::identity(), None);
        }
    }

    fn draw_music_note_icon(canvas: &mut LayeredCanvas, x: f32, cy: f32, color: Color) {
        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(color);

        let stroke = Stroke {
            width: 1.2,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };

        // Note head: slanted oval
        let mut pb = PathBuilder::new();
        let cx = x + 2.4;
        let ny = cy + 2.0;
        pb.move_to(cx - 2.0, ny + 0.8);
        pb.quad_to(cx - 2.2, ny - 1.2, cx, ny - 1.4);
        pb.quad_to(cx + 2.2, ny - 1.6, cx + 2.0, ny + 0.8);
        pb.quad_to(cx + 1.8, ny + 1.8, cx, ny + 1.8);
        pb.quad_to(cx - 1.8, ny + 1.8, cx - 2.0, ny + 0.8);
        pb.close();
        if let Some(p) = pb.finish() {
            canvas.pixmap.fill_path(&p, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }

        // Stem and flag
        let mut pb = PathBuilder::new();
        let stem_x = cx + 1.6;
        pb.move_to(stem_x, ny + 0.5);
        pb.line_to(stem_x, cy - 4.5);
        pb.quad_to(stem_x + 1.4, cy - 4.2, stem_x + 3.0, cy - 1.2);
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &paint, &stroke, Transform::identity(), None);
        }
    }
}
