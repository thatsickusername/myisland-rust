use crate::platform::MediaState;
use crate::render::{AlbumArtManager, LayeredCanvas, MascotEngine};
use crate::state::{clean_title_and_artist, ClipboardManager, LyricsState, NotificationPayload, SportsState};
use tiny_skia::{Color, FilterQuality, Paint, PathBuilder, Pattern, Pixmap, SpreadMode, Stroke, Transform};

pub const APP_NAMES: [&str; 5] = ["MUSIC", "CLIPBOARD", "NOTES", "FOCUS", "MATCHES"];

pub struct ExpandedRenderer;

impl ExpandedRenderer {
    /// Native equivalent of the Electron expanded notification card. The
    /// compact banner is rendered by CompactRenderer; this path is used after
    /// a snipping-tool/call notification receives a hover dwell.
    pub fn render_notification(
        canvas: &mut LayeredCanvas,
        notif: &NotificationPayload,
        island_x: f32,
        w: f32,
        _h: f32,
        pointer_x: f32,
        pointer_y: f32,
    ) -> bool {
        let left = island_x + 22.0;
        let top = 18.0;
        let brand = Color::from_rgba8(
            notif.brand_color[0],
            notif.brand_color[1],
            notif.brand_color[2],
            notif.brand_color[3],
        );

        // Header avatar.
        let mut avatar_bg = Paint::default();
        avatar_bg.anti_alias = true;
        avatar_bg.set_color(brand);
        let mut avatar_path = PathBuilder::new();
        avatar_path.push_circle(left + 16.0, top + 16.0, 16.0);
        if let Some(path) = avatar_path.finish() {
            canvas.pixmap.fill_path(
                &path,
                &avatar_bg,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        if let Some(bytes) = notif.avatar_image.as_ref().or(notif.icon_image.as_ref()) {
            let _ = Self::draw_notification_image(
                canvas,
                bytes,
                left + 16.0,
                top + 16.0,
                32.0,
                notif.avatar_image.is_some(),
            );
        }
        canvas.draw_text(
            &notif.app_name,
            left + 40.0,
            top + 12.0,
            13.5,
            Color::WHITE,
            true,
        );
        canvas.draw_text(
            if notif.is_call { "Phone Link" } else { &notif.subtitle },
            left + 40.0,
            top + 28.0,
            11.0,
            Color::from_rgba8(142, 142, 147, 255),
            false,
        );

        // Close affordance.
        let close_x = island_x + w - 27.0;
        let close_y = top + 16.0;
        let mut close_bg = Paint::default();
        close_bg.anti_alias = true;
        let close_hover = pointer_x >= close_x - 14.0
            && pointer_x <= close_x + 14.0
            && pointer_y >= close_y - 14.0
            && pointer_y <= close_y + 14.0;
        close_bg.set_color_rgba8(255, 255, 255, if close_hover { 58 } else { 30 });
        let mut close_path = PathBuilder::new();
        close_path.push_circle(close_x, close_y, 11.0);
        if let Some(path) = close_path.finish() {
            canvas.pixmap.fill_path(
                &path,
                &close_bg,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        let mut close_lines = PathBuilder::new();
        close_lines.move_to(close_x - 3.5, close_y - 3.5);
        close_lines.line_to(close_x + 3.5, close_y + 3.5);
        close_lines.move_to(close_x + 3.5, close_y - 3.5);
        close_lines.line_to(close_x - 3.5, close_y + 3.5);
        let close_paint = {
            let mut p = Paint::default();
            p.anti_alias = true;
            p.set_color_rgba8(255, 255, 255, 210);
            p
        };
        if let Some(path) = close_lines.finish() {
            canvas.pixmap.stroke_path(
                &path,
                &close_paint,
                &Stroke { width: 1.6, line_cap: tiny_skia::LineCap::Round, ..Default::default() },
                Transform::identity(),
                None,
            );
        }

        let body_y = 54.0;
        if notif.is_snip {
            // Screenshot preview surface. The clipboard image is collected by
            // the main thread when available; otherwise keep the same
            // intentional loading/placeholder treatment as Electron.
            let preview_x = left;
            let preview_y = body_y;
            let preview_w = 160.0;
            let preview_h = 94.0;
            let mut preview_bg = Paint::default();
            preview_bg.anti_alias = true;
            preview_bg.set_color_rgba8(28, 28, 30, 255);
            let mut preview_path = PathBuilder::new();
            Self::add_round_rect(
                &mut preview_path,
                preview_x,
                preview_y,
                preview_w,
                preview_h,
                10.0,
            );
            let preview_shape = preview_path.finish();
            if let Some(path) = preview_shape.as_ref() {
                canvas.pixmap.fill_path(
                    &path,
                    &preview_bg,
                    tiny_skia::FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }

            if let (Some(bytes), Some(path)) = (notif.image_preview.as_ref(), preview_shape.as_ref()) {
                if let Ok(image) = image::load_from_memory(bytes) {
                    let rgba = image.to_rgba8();
                    if let Some(mut image_pixmap) = Pixmap::new(rgba.width(), rgba.height()) {
                        for (source, target) in rgba
                            .as_raw()
                            .chunks_exact(4)
                            .zip(image_pixmap.data_mut().chunks_exact_mut(4))
                        {
                            let alpha = source[3] as u16;
                            target[0] = ((source[0] as u16 * alpha) / 255) as u8;
                            target[1] = ((source[1] as u16 * alpha) / 255) as u8;
                            target[2] = ((source[2] as u16 * alpha) / 255) as u8;
                            target[3] = source[3];
                        }
                        let transform = Transform::from_scale(
                            preview_w / image_pixmap.width() as f32,
                            preview_h / image_pixmap.height() as f32,
                        )
                        .post_translate(preview_x, preview_y);
                        let mut image_paint = Paint::default();
                        image_paint.anti_alias = true;
                        image_paint.shader = Pattern::new(
                            image_pixmap.as_ref(),
                            SpreadMode::Pad,
                            FilterQuality::Bilinear,
                            1.0,
                            transform,
                        );
                        canvas.pixmap.fill_path(
                            path,
                            &image_paint,
                            tiny_skia::FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
            let mut preview_border = Paint::default();
            preview_border.anti_alias = true;
            preview_border.set_color_rgba8(100, 210, 255, 145);
            let mut border_path = PathBuilder::new();
            Self::add_round_rect(
                &mut border_path,
                preview_x + 0.5,
                preview_y + 0.5,
                preview_w - 1.0,
                preview_h - 1.0,
                9.5,
            );
            if let Some(path) = border_path.finish() {
                canvas.pixmap.stroke_path(
                    &path,
                    &preview_border,
                    &Stroke { width: 1.0, ..Default::default() },
                    Transform::identity(),
                    None,
                );
            }
            if notif.image_preview.is_none() {
                canvas.draw_text(
                    "Preview unavailable",
                    preview_x + 20.0,
                    preview_y + preview_h / 2.0,
                    11.0,
                    Color::from_rgba8(255, 255, 255, 165),
                    false,
                );
            }

            let action_x = left + 176.0;
            Self::draw_notification_button(
                canvas,
                "Open Screenshot",
                action_x,
                body_y + 10.0,
                w - 44.0 - 176.0,
                38.0,
                Color::from_rgba8(0, 122, 255, 255),
            );
            Self::draw_notification_button(
                canvas,
                "Copy Image",
                action_x,
                body_y + 56.0,
                w - 44.0 - 176.0,
                38.0,
                Color::from_rgba8(255, 255, 255, 31),
            );
        } else {
            // Incoming call card.
            canvas.draw_text(
                &notif.title,
                left,
                body_y + 8.0,
                20.0,
                Color::WHITE,
                true,
            );
            canvas.draw_text(
                &notif.subtitle,
                left,
                body_y + 30.0,
                12.5,
                Color::from_rgba8(235, 235, 245, 210),
                false,
            );
            let button_w = (w - 66.0) / 2.0;
            Self::draw_notification_button(
                canvas,
                "Accept on PC",
                left,
                body_y + 55.0,
                button_w,
                42.0,
                Color::from_rgba8(48, 209, 88, 255),
            );
            Self::draw_notification_button(
                canvas,
                "Decline",
                left + button_w + 18.0,
                body_y + 55.0,
                button_w,
                42.0,
                Color::from_rgba8(255, 69, 58, 255),
            );
        }

        false
    }

    fn draw_notification_button(
        canvas: &mut LayeredCanvas,
        label: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
    ) {
        let mut bg = Paint::default();
        bg.anti_alias = true;
        bg.set_color(color);
        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, x, y, w, h, 12.0);
        if let Some(path) = pb.finish() {
            canvas.pixmap.fill_path(
                &path,
                &bg,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        let tw = canvas.measure_text_width(label, 12.5, true);
        // draw_text receives a baseline, not a top edge. Center the glyph's
        // baseline in the button instead of placing it at y+12.
        canvas.draw_text(
            label,
            x + (w - tw) / 2.0,
            y + h / 2.0 + 4.5,
            12.5,
            Color::WHITE,
            true,
        );
    }

    fn draw_notification_image(
        canvas: &mut LayeredCanvas,
        bytes: &[u8],
        cx: f32,
        cy: f32,
        size: f32,
        cover: bool,
    ) -> bool {
        let Ok(image) = image::load_from_memory(bytes) else {
            return false;
        };
        let rgba = image.to_rgba8();
        let Some(mut pixmap) = Pixmap::new(rgba.width(), rgba.height()) else {
            return false;
        };
        for (source, target) in rgba
            .as_raw()
            .chunks_exact(4)
            .zip(pixmap.data_mut().chunks_exact_mut(4))
        {
            let alpha = source[3] as u16;
            target[0] = ((source[0] as u16 * alpha) / 255) as u8;
            target[1] = ((source[1] as u16 * alpha) / 255) as u8;
            target[2] = ((source[2] as u16 * alpha) / 255) as u8;
            target[3] = source[3];
        }
        let image_w = pixmap.width() as f32;
        let image_h = pixmap.height() as f32;
        let scale = if cover {
            (size / image_w).max(size / image_h)
        } else {
            (size / image_w).min(size / image_h)
        };
        let draw_w = image_w * scale;
        let draw_h = image_h * scale;
        let transform = Transform::from_scale(scale, scale).post_translate(
            cx - draw_w / 2.0,
            cy - draw_h / 2.0,
        );
        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.shader = Pattern::new(
            pixmap.as_ref(),
            SpreadMode::Pad,
            FilterQuality::Bilinear,
            1.0,
            transform,
        );
        let mut clip = PathBuilder::new();
        clip.push_circle(cx, cy, size / 2.0);
        if let Some(path) = clip.finish() {
            canvas.pixmap.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
            true
        } else {
            false
        }
    }

    pub fn render(
        canvas: &mut LayeredCanvas,
        mascot: &mut MascotEngine,
        album_art: &AlbumArtManager,
        island_x: f32,
        w: f32,
        h: f32,
        content_entrance: f32,
        _current_app: usize, // <-- Prefixed with underscore
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
        ruler_visual_mins: f32,
        notes_focused: bool,
        notes_cursor_pos: usize,
        cursor_moved: bool,
        clip_copied_index: Option<usize>,
        clip_scroll_progress: f32,
        target_notes_scroll: &mut f32,
        notes_scroll_progress: f32,
        sports: Option<&SportsState>,
    ) -> bool {
        // Faithful to MyIsland index.html line 3598: card content only renders when h > 80.0
        if h <= 80.0 {
            return false;
        }

        let content_alpha = ((h - 80.0) / 45.0).clamp(0.0, 1.0);
        let cx = island_x + w / 2.0;

        // Staggered Apple-style entrance: album art pops first, text rolls in,
        // then controls bounce into place.
        let e_album = Self::stagger(content_entrance, 0.0, 0.45);
        let e_text = Self::stagger(content_entrance, 0.18, 0.50);
        let e_controls = Self::stagger_back(content_entrance, 0.34, 0.58);
        let header_dy = -4.0 * (1.0 - e_text);
        let header_alpha = content_alpha * e_text;

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

        canvas.draw_text(
            &time_str,
            cx - time_w - 8.0,
            14.0 + header_dy,
            13.0,
            Color::from_rgba8(255, 255, 255, (255.0 * header_alpha) as u8),
            true,
        );

        let date_col = Color::from_rgba8(255, 255, 255, (115.0 * header_alpha) as u8);
        canvas.draw_text(&date_full, cx + 8.0, 14.0 + header_dy, 11.5, date_col, false);

        // 3. 3-Dots Menu Button
        let dot_cx = island_x + w - 27.0;
        let dot_cy = 20.0 + header_dy;
        let mut dot_paint = Paint::default();
        dot_paint.anti_alias = true;
        dot_paint.set_color_rgba8(255, 255, 255, (165.0 * header_alpha) as u8);

        for i in [-1.0f32, 0.0, 1.0] {
            let mut pb = PathBuilder::new();
            pb.push_circle(dot_cx + i * 4.8, dot_cy, 1.4);
            if let Some(p) = pb.finish() {
                canvas.pixmap.fill_path(&p, &dot_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }
        }

        // 4. Header Divider Line
        let mut div_paint = Paint::default();
        div_paint.set_color_rgba8(255, 255, 255, (20.0 * header_alpha) as u8);
        let stroke_1 = Stroke { width: 1.0, ..Default::default() };
        let mut pb = PathBuilder::new();
        pb.move_to(island_x + 16.0, 36.0 + header_dy);
        pb.line_to(island_x + w - 16.0, 36.0 + header_dy);
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &div_paint, &stroke_1, Transform::identity(), None);
        }

        // 5. Active App Panels (Sliding Carousel)
        let body_y = 44.0 + 9.0 * (1.0 - e_text);
        let body_w = w - 44.0;
        let center_slot = scroll_progress.round() as i32;

        for offset in -1..=1 {
            let slot = center_slot + offset;
            let offset_dist = (slot as f32) - scroll_progress;
            
            // Skip rendering adjacent panels once they are fully outside the clipping mask
            if offset_dist.abs() >= 1.0 {
                continue;
            }
            
            let app_idx = (((slot % 5) + 5) % 5) as usize;
            
            // Translate the panel horizontally by the island's full width
            let panel_dx = offset_dist * w; 
            let body_x = island_x + 22.0 + panel_dx;

            match app_idx {
                0 => Self::render_music(canvas, album_art, body_x, body_y, body_w, pointer_x, pointer_y, media, lyrics, e_album, e_text, e_controls),
                1 => Self::render_clipboard(canvas, body_x, body_y, body_w, h, content_alpha, clips, pointer_x, pointer_y, clip_copied_index, clip_scroll_progress),
                2 => Self::render_notes(canvas, body_x, body_y, body_w, clips, notes_focused, notes_cursor_pos, cursor_moved, target_notes_scroll, notes_scroll_progress),
                3 => Self::render_focus(
                    canvas,
                    body_x,
                    body_y,
                    body_w,
                    pointer_x,
                    pointer_y,
                    focus_active,
                    focus_paused,
                    focus_left_ms,
                    focus_total_ms,
                    focus_chosen_sec,
                    ruler_visual_mins,
                ),
                4 => Self::render_sports(canvas, body_x, body_y, body_w, sports, e_text),
                _ => {}
            }
        }

        // 6. iPhone Camera Mode Selector (Carousel) - only shows when island is tall enough
        if h >= 110.0 {
            let cam_alpha = ((h - 110.0) / 25.0).clamp(0.0, 1.0) * e_controls;
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
        e_album: f32,
        e_text: f32,
        e_controls: f32,
    ) {
        let cs = 64.0;

        // 1. Album Art Drop Shadow (pops in first with a soft scale-up)
        let art_scale = 0.90 + 0.10 * e_album;
        let art_inset = cs * (1.0 - art_scale) * 0.5;
        let art_x = x + art_inset;
        let art_y = y + art_inset;
        let art_size = cs * art_scale;

        let mut shadow_paint = Paint::default();
        shadow_paint.anti_alias = true;
        shadow_paint.set_color_rgba8(0, 0, 0, 70);
        let mut pb = PathBuilder::new();
        Self::add_round_rect(&mut pb, art_x - 1.0, art_y + 3.0, art_size + 2.0, art_size + 2.0, 11.0);
        if let Some(p) = pb.finish() {
            canvas.pixmap.fill_path(&p, &shadow_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }

        // 2. Album Art Box (64x64 with radius 10.0)
        album_art.draw(&mut canvas.pixmap.as_mut(), art_x, art_y, art_size, 10.0);

        // Text group rolls in just after the artwork.
        let text_dy = (1.0 - e_text) * 6.0;

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
        canvas.draw_text(&title_str, tx, y + 8.0 + text_dy, 14.0, Color::WHITE, true);

        let artist_str = canvas.truncate_to_width(&clean_a, title_w, 12.0, false);
        let artist_col = Color::from_rgba8(255, 255, 255, 153);
        canvas.draw_text(&artist_str, tx, y + 26.0 + text_dy, 12.0, artist_col, false);

        // 4. Synced Lyric line in expanded card (MyIsland lines 3770-3781)
        let (_, now_l, next_l) = lyrics.get_around_lyrics();
        let display_l = if !now_l.is_empty() { now_l } else { next_l };
        if !display_l.is_empty() {
            let [cr, cg, cb] = album_art.dominant_color;
            let lyric_col = Color::from_rgba8(cr, cg, cb, 255);
            let ly_cy = y + 49.0 + text_dy;
            Self::draw_music_note_icon(canvas, tx, ly_cy, lyric_col);

            let max_w = title_w - 14.0;
            let lyric_line = canvas.truncate_to_width(&display_l, max_w, 11.5, false);
            canvas.draw_text(&lyric_line, tx + 12.0, y + 44.0 + text_dy, 11.5, lyric_col, false);
        }

        // 5. Progress scrubber (MyIsland lines 3784-3813)
        let bar_y = y + cs + 14.0 + text_dy;
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

        // 6. Media Buttons (Prev, Play/Pause, Next) - bounce into view last
        let controls_dy = (1.0 - e_controls) * 10.0;
        let ctrl_y = y + 28.0 + controls_dy;
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

    /// Eased stagger progress in `[0, 1]` for a delayed entrance.
    fn stagger(t: f32, delay: f32, duration: f32) -> f32 {
        let local = ((t - delay) / duration).clamp(0.0, 1.0);
        1.0 - (1.0 - local).powi(3)
    }

    /// Overshooting stagger used for controls so they bounce into place.
    fn stagger_back(t: f32, delay: f32, duration: f32) -> f32 {
        let local = ((t - delay) / duration).clamp(0.0, 1.0);
        let c1 = 1.70158;
        let c3 = c1 + 1.0;
        1.0 + c3 * (local - 1.0).powi(3) + c1 * (local - 1.0).powi(2)
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
        pointer_x: f32,
        pointer_y: f32,
        active: bool,
        paused: bool,
        left_ms: u64,
        total_ms: u64,
        chosen_sec: u64,
        visual_mins: f32,
    ) {
        let orange = Color::from_rgba8(240, 160, 100, 255);

        if !active {
            // Setup View (Precision Dynamic Dial Ruler Style - MyIsland index.html line 2626)
            let total_minutes = 60;
            let tick_width = 10.0f32;
            let current_mins = ((chosen_sec as f32 / 60.0).round() as i32).clamp(1, total_minutes);
            let visual_mins = visual_mins.clamp(-1.0, total_minutes as f32 + 1.0);
            let selected_mins = (visual_mins.round() as i32).clamp(1, total_minutes);

            let ruler_x = x + 6.0;
            let ruler_y = y + 2.0;
            let ruler_w = bw - 12.0;
            let ruler_h = 50.0;
            let center_x = ruler_x + ruler_w / 2.0;

            // Track target translation: centers selected tick under cursor.
            // The fractional value lets the ruler elastically overscroll at its
            // 1–60 minute limits before springing back.
            let track_x = center_x - (visual_mins * tick_width);

            // Draw ticks (0 to 60 minutes)
            for m in 0..=total_minutes {
                let tick_cx = track_x + (m as f32) * tick_width;
                if tick_cx < ruler_x - 15.0 || tick_cx > ruler_x + ruler_w + 15.0 {
                    continue;
                }

                // Mask fade at edges: 18% to 82% visible window (matching MyIsland CSS line 120)
                let dist_from_center = (tick_cx - center_x).abs();
                let half_w = ruler_w / 2.0;
                let edge_dist = half_w - dist_from_center;
                let fade = (edge_dist / (half_w * 0.36)).clamp(0.0, 1.0);
                if fade <= 0.01 {
                    continue;
                }

                let is_major = m % 5 == 0;
                let is_selected = m == selected_mins;

                if is_selected {
                    // Selected tick: glowing bold amber bar (MyIsland line 138: 3.2px wide, 28px tall, margin-top 8px)
                    let bar_w = 3.2f32;
                    let bar_h = 28.0f32;
                    let bar_top = ruler_y + 8.0;

                    // Glow behind
                    let mut glow_paint = Paint::default();
                    glow_paint.anti_alias = true;
                    glow_paint.set_color_rgba8(240, 160, 100, (110.0 * fade) as u8);
                    let mut pb_glow = PathBuilder::new();
                    Self::add_round_rect(&mut pb_glow, tick_cx - 3.0, bar_top - 2.0, 6.0, bar_h + 4.0, 2.5);
                    if let Some(p) = pb_glow.finish() {
                        canvas.pixmap.fill_path(&p, &glow_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                    }

                    let mut bar_paint = Paint::default();
                    bar_paint.anti_alias = true;
                    bar_paint.set_color_rgba8(240, 160, 100, (255.0 * fade) as u8);
                    let mut pb = PathBuilder::new();
                    Self::add_round_rect(&mut pb, tick_cx - bar_w / 2.0, bar_top, bar_w, bar_h, 1.2);
                    if let Some(p) = pb.finish() {
                        canvas.pixmap.fill_path(&p, &bar_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                    }
                } else if is_major {
                    // Major tick: 3px wide, 25px tall, margin-top 11px, alpha 0.72 (MyIsland line 135)
                    let bar_w = 3.0f32;
                    let bar_h = 25.0f32;
                    let bar_top = ruler_y + 11.0;

                    let mut bar_paint = Paint::default();
                    bar_paint.anti_alias = true;
                    bar_paint.set_color_rgba8(240, 160, 100, (184.0 * fade) as u8);
                    let mut pb = PathBuilder::new();
                    Self::add_round_rect(&mut pb, tick_cx - bar_w / 2.0, bar_top, bar_w, bar_h, 1.2);
                    if let Some(p) = pb.finish() {
                        canvas.pixmap.fill_path(&p, &bar_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                    }

                    // Major tick label (e.g. 0, 5, 10... 60)
                    let lbl = format!("{}", m);
                    let tw = canvas.measure_text_width(&lbl, 11.0, true);
                    let lbl_color = Color::from_rgba8(240, 160, 100, (216.0 * fade) as u8);
                    canvas.draw_text_clipped(
                        &lbl,
                        tick_cx - tw / 2.0,
                        ruler_y + 0.0,
                        11.0,
                        lbl_color,
                        true,
                        ruler_x,
                        ruler_x + ruler_w,
                    );
                } else {
                    // Minor tick: 2.5px wide, 17px tall, margin-top 19px, alpha 0.28 (MyIsland line 131)
                    let bar_w = 2.5f32;
                    let bar_h = 17.0f32;
                    let bar_top = ruler_y + 19.0;

                    let mut bar_paint = Paint::default();
                    bar_paint.anti_alias = true;
                    bar_paint.set_color_rgba8(240, 160, 100, (71.0 * fade) as u8);
                    let mut pb = PathBuilder::new();
                    Self::add_round_rect(&mut pb, tick_cx - bar_w / 2.0, bar_top, bar_w, bar_h, 1.2);
                    if let Some(p) = pb.finish() {
                        canvas.pixmap.fill_path(&p, &bar_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                    }
                }
            }

            // Upward pointing triangular cursor in the center bottom (MyIsland line 145)
            // width: 12px (6px left, 6px right), height: 7px, bottom: 1px from bottom of ruler
            let tri_apex_y = ruler_y + ruler_h - 9.0;
            let tri_base_y = ruler_y + ruler_h - 2.0;

            // Cursor glow
            let mut cursor_glow = Paint::default();
            cursor_glow.anti_alias = true;
            cursor_glow.set_color_rgba8(240, 160, 100, 100);
            let mut pb_cg = PathBuilder::new();
            pb_cg.move_to(center_x, tri_apex_y - 2.0);
            pb_cg.line_to(center_x + 8.0, tri_base_y + 1.0);
            pb_cg.line_to(center_x - 8.0, tri_base_y + 1.0);
            pb_cg.close();
            if let Some(p) = pb_cg.finish() {
                canvas.pixmap.fill_path(&p, &cursor_glow, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            let mut cursor_paint = Paint::default();
            cursor_paint.anti_alias = true;
            cursor_paint.set_color_rgba8(240, 160, 100, 255);
            let mut pb_c = PathBuilder::new();
            pb_c.move_to(center_x, tri_apex_y);
            pb_c.line_to(center_x + 6.0, tri_base_y);
            pb_c.line_to(center_x - 6.0, tri_base_y);
            pb_c.close();
            if let Some(p) = pb_c.finish() {
                canvas.pixmap.fill_path(&p, &cursor_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            // Setup Footer (MyIsland lines 150-164)
            let footer_y = ruler_y + ruler_h + 3.0;

            // Start Timer Button (Pill shape, height: 35px, border-radius: 18px)
            let btn_x = x + 10.0;
            let btn_y = footer_y;
            let btn_w = 126.0;
            let btn_h = 35.0;
            let btn_r = 17.5;

            let is_btn_hover = pointer_x >= btn_x && pointer_x <= btn_x + btn_w && pointer_y >= btn_y && pointer_y <= btn_y + btn_h;
            let btn_bg_alpha = if is_btn_hover { 71 } else { 46 }; // 0.28 vs 0.18

            let mut btn_paint = Paint::default();
            btn_paint.anti_alias = true;
            btn_paint.set_color_rgba8(240, 160, 100, btn_bg_alpha);
            let mut pb_btn = PathBuilder::new();
            Self::add_round_rect(&mut pb_btn, btn_x, btn_y, btn_w, btn_h, btn_r);
            if let Some(p) = pb_btn.finish() {
                canvas.pixmap.fill_path(&p, &btn_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            let btn_text = "Start Timer";
            let btn_text_w = canvas.measure_text_width(btn_text, 13.5, true);
            canvas.draw_text(
                btn_text,
                btn_x + (btn_w - btn_text_w) / 2.0,
                btn_y + 10.0,
                13.5,
                orange,
                true,
            );

            // Right side: Large Time Preview (32px tabular light font)
            let preview_str = format!("{:02}:00", current_mins);
            let preview_w = canvas.measure_text_width(&preview_str, 32.0, false);
            canvas.draw_text(
                &preview_str,
                x + bw - 12.0 - preview_w,
                btn_y - 2.0,
                32.0,
                orange,
                false,
            );
        } else {
            // Running View (Premium Stopwatch/Timer Style - MyIsland lines 2590-2624)
            let done_at = {
                let end_local = chrono::Local::now() + chrono::Duration::milliseconds(left_ms as i64);
                end_local.format("%I:%M %p").to_string()
            };

            let info_x = x + 10.0;
            let info_w = bw - 158.0;

            // Row 1: Header (Clock SVG icon + Status Text)
            // 13x13 clock icon matching MyIsland SVG line 2600
            let icon_cx = info_x + 6.0;
            let icon_cy = y + 10.0;
            let icon_r = 5.0;
            let mut icon_paint = Paint::default();
            icon_paint.anti_alias = true;
            icon_paint.set_color_rgba8(240, 160, 100, 255);

            let stroke_icon = Stroke { width: 1.8, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
            let mut pb_clock = PathBuilder::new();
            pb_clock.push_circle(icon_cx, icon_cy, icon_r);
            pb_clock.move_to(icon_cx, icon_cy);
            pb_clock.line_to(icon_cx, icon_cy - 3.2);
            pb_clock.move_to(icon_cx, icon_cy);
            pb_clock.line_to(icon_cx + 2.6, icon_cy + 1.2);
            if let Some(p) = pb_clock.finish() {
                canvas.pixmap.stroke_path(&p, &icon_paint, &stroke_icon, Transform::identity(), None);
            }

            let status_prefix = if paused { "TIMER PAUSED" } else { "FOCUS SESSION" };
            let header_str = format!("{} · ENDS AT {}", status_prefix, done_at);
            let gray_header = Color::from_rgba8(142, 142, 147, 255); // #8e8e93
            canvas.draw_text(&header_str, info_x + 18.0, y + 4.0, 11.0, gray_header, true);

            // Row 2: Large Countdown Timer (38px font)
            let time_str = crate::ui::format_focus_timer(left_ms);
            canvas.draw_text(&time_str, info_x, y + 20.0, 38.0, Color::WHITE, true);

            // Row 3: Progress Bar (4px height, background 0.12, gradient fill + soft glow)
            let bar_y = y + 70.0;
            let bar_h = 4.0;
            let bar_w = info_w;

            let mut bg_paint = Paint::default();
            bg_paint.anti_alias = true;
            bg_paint.set_color_rgba8(255, 255, 255, 31); // 0.12
            let mut pb_bg = PathBuilder::new();
            Self::add_round_rect(&mut pb_bg, info_x, bar_y, bar_w, bar_h, 2.0);
            if let Some(p) = pb_bg.finish() {
                canvas.pixmap.fill_path(&p, &bg_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            let frac = if total_ms > 0 {
                (left_ms as f32 / total_ms as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let fill_w = (bar_w * frac).max(if frac > 0.0 { 4.0 } else { 0.0 });
            if fill_w > 0.0 {
                // Subtle glow underneath progress
                let mut glow_bar = Paint::default();
                glow_bar.anti_alias = true;
                glow_bar.set_color_rgba8(240, 160, 100, 75);
                let mut pb_gb = PathBuilder::new();
                Self::add_round_rect(&mut pb_gb, info_x, bar_y - 1.0, fill_w, bar_h + 2.0, 3.0);
                if let Some(p) = pb_gb.finish() {
                    canvas.pixmap.fill_path(&p, &glow_bar, tiny_skia::FillRule::Winding, Transform::identity(), None);
                }

                let mut fg_paint = Paint::default();
                fg_paint.anti_alias = true;
                fg_paint.set_color_rgba8(240, 160, 100, 255);
                let mut pb_fg = PathBuilder::new();
                Self::add_round_rect(&mut pb_fg, info_x, bar_y, fill_w, bar_h, 2.0);
                if let Some(p) = pb_fg.finish() {
                    canvas.pixmap.fill_path(&p, &fg_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                }
            }

            // Right Column: Vertical Action Buttons (Width 120px, Gap 8px, Height 35px each)
            let btn_x = x + bw - 130.0;
            let btn_w = 120.0;
            let btn_h = 35.0;
            let btn_r = 14.0;

            // 1. Primary Pause / Resume Button
            let pause_y = y + 7.0;
            let is_pause_hover = pointer_x >= btn_x && pointer_x <= btn_x + btn_w && pointer_y >= pause_y && pointer_y <= pause_y + btn_h;
            let pause_alpha = if is_pause_hover { 82 } else { 56 }; // 0.32 vs 0.22

            let mut pause_paint = Paint::default();
            pause_paint.anti_alias = true;
            pause_paint.set_color_rgba8(240, 160, 100, pause_alpha);
            let mut pb_pause = PathBuilder::new();
            Self::add_round_rect(&mut pb_pause, btn_x, pause_y, btn_w, btn_h, btn_r);
            if let Some(p) = pb_pause.finish() {
                canvas.pixmap.fill_path(&p, &pause_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            let pause_text = if paused { "Resume" } else { "Pause" };
            let p_w = canvas.measure_text_width(pause_text, 13.0, true);
            canvas.draw_text(
                pause_text,
                btn_x + (btn_w - p_w) / 2.0,
                pause_y + 10.0,
                13.0,
                orange,
                true,
            );

            // 2. Secondary Stop Button (#2c2c2e, hover #3a3a3c)
            let stop_y = y + 50.0;
            let is_stop_hover = pointer_x >= btn_x && pointer_x <= btn_x + btn_w && pointer_y >= stop_y && pointer_y <= stop_y + btn_h;
            let stop_col = if is_stop_hover {
                Color::from_rgba8(58, 58, 60, 255)
            } else {
                Color::from_rgba8(44, 44, 46, 255)
            };

            let mut stop_paint = Paint::default();
            stop_paint.anti_alias = true;
            stop_paint.set_color(stop_col);
            let mut pb_stop = PathBuilder::new();
            Self::add_round_rect(&mut pb_stop, btn_x, stop_y, btn_w, btn_h, btn_r);
            if let Some(p) = pb_stop.finish() {
                canvas.pixmap.fill_path(&p, &stop_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            let stop_text = "Stop";
            let s_w = canvas.measure_text_width(stop_text, 13.0, true);
            canvas.draw_text(
                stop_text,
                btn_x + (btn_w - s_w) / 2.0,
                stop_y + 10.0,
                13.0,
                Color::WHITE,
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

    fn render_sports(
        canvas: &mut LayeredCanvas,
        x: f32,
        y: f32,
        bw: f32,
        sports: Option<&SportsState>,
        e_text: f32,
    ) {
        let text_dy = (1.0 - e_text) * 8.0;
        let alpha_u8 = (255.0 * e_text) as u8;
        let muted = Color::from_rgba8(138, 138, 143, (215.0 * e_text) as u8);

        if let Some(match_data) = sports {
            // Header: quiet section label on the left, a slightly brighter
            // status group on the right. The status prefix is dimmed so the
            // kickoff/clock stays the focal point of the row.
            let header_y = y + 11.0 + text_dy;
            let section_title = if match_data.is_upcoming {
                "UPCOMING MATCH"
            } else if match_data.is_live {
                "LIVE SPORTS"
            } else {
                "FINAL"
            };
            let section_size = 10.5;
            canvas.draw_text(section_title, x + 16.0, header_y, section_size, muted, true);
            let section_w = canvas.measure_text_width(section_title, section_size, true);

            let prefix = if match_data.is_live {
                "LIVE"
            } else if match_data.is_upcoming {
                "UP NEXT"
            } else {
                ""
            };
            let detail = match_data.clock.trim().replace(" at ", " · ");
            let prefix_size = 9.5;
            let detail_size = 11.5;
            let group_gap = 6.0;
            let prefix_w = if prefix.is_empty() {
                0.0
            } else {
                canvas.measure_text_width(prefix, prefix_size, true)
            };
            let available = (bw - 32.0 - section_w - 24.0).max(100.0);
            let detail_max = (available - prefix_w - group_gap).max(70.0);
            let detail = canvas.truncate_to_width(&detail, detail_max, detail_size, true);
            let detail_w = canvas.measure_text_width(&detail, detail_size, true);
            let detail_x = x + bw - 16.0 - detail_w;
            let prefix_x = detail_x - group_gap - prefix_w;

            // Only show the prefix when it still leaves a clean gap after the
            // section label; long status text drops it instead of crowding.
            if !prefix.is_empty() && prefix_x >= x + 16.0 + section_w + 14.0 {
                canvas.draw_text(
                    prefix,
                    prefix_x,
                    header_y,
                    prefix_size,
                    Color::from_rgba8(142, 142, 147, alpha_u8),
                    true,
                );
            }
            canvas.draw_text(
                &detail,
                detail_x,
                header_y,
                detail_size,
                Color::from_rgba8(255, 255, 255, alpha_u8),
                true,
            );

            // Arena: logos, team codes, and scores all share one centerline.
            let arena_cy = y + 57.0 + text_dy;
            let logo_size = 32.0;
            let logo_y = arena_cy - logo_size / 2.0;
            Self::draw_sports_logo(
                canvas,
                match_data.home_logo.as_deref(),
                x + 4.0,
                logo_y,
                logo_size,
                match_data.home_color,
                e_text,
            );
            Self::draw_sports_logo(
                canvas,
                match_data.away_logo.as_deref(),
                x + bw - logo_size - 4.0,
                logo_y,
                logo_size,
                match_data.away_color,
                e_text,
            );

            let team_size = 21.0;
            let team_color = Color::from_rgba8(238, 238, 242, alpha_u8);
            let logo_gap = 10.0;
            let home_text_x = x + 4.0 + logo_size + logo_gap;
            let away_text_right = x + bw - (4.0 + logo_size + logo_gap);
            let home_text_y = canvas.text_y_for_vertical_center(
                &match_data.home_team,
                team_size,
                true,
                arena_cy,
            );
            let away_text_y = canvas.text_y_for_vertical_center(
                &match_data.away_team,
                team_size,
                true,
                arena_cy,
            );
            canvas.draw_text(
                &match_data.home_team,
                home_text_x,
                home_text_y,
                team_size,
                team_color,
                true,
            );
            let away_w = canvas.measure_text_width(&match_data.away_team, team_size, true);
            canvas.draw_text(
                &match_data.away_team,
                away_text_right - away_w,
                away_text_y,
                team_size,
                team_color,
                true,
            );

            // Scores are the visual anchor: larger and brighter than the team
            // codes, with a quiet vector separator between them.
            let center_x = x + bw / 2.0;
            let home_score = match_data.home_score.to_string();
            let away_score = match_data.away_score.to_string();
            let score_size = 29.0;
            let home_score_w = canvas.measure_text_width(&home_score, score_size, true);
            let score_gap = 22.0;
            let number_color = Color::from_rgba8(255, 255, 255, alpha_u8);
            let home_score_y =
                canvas.text_y_for_vertical_center(&home_score, score_size, true, arena_cy);
            let away_score_y =
                canvas.text_y_for_vertical_center(&away_score, score_size, true, arena_cy);
            canvas.draw_text(
                &home_score,
                center_x - score_gap - home_score_w,
                home_score_y,
                score_size,
                number_color,
                true,
            );
            canvas.draw_text(
                &away_score,
                center_x + score_gap,
                away_score_y,
                score_size,
                number_color,
                true,
            );

            // Use a native vector separator instead of a box-drawing glyph;
            // the latter renders as missing-glyph squares in the bundled font.
            let mut separator = Paint::default();
            separator.anti_alias = true;
            separator.set_color_rgba8(142, 142, 147, (190.0 * e_text) as u8);
            let mut separator_path = PathBuilder::new();
            separator_path.move_to(center_x - 6.0, arena_cy);
            separator_path.line_to(center_x + 6.0, arena_cy);
            if let Some(path) = separator_path.finish() {
                canvas.pixmap.stroke_path(
                    &path,
                    &separator,
                    &Stroke {
                        width: 2.0,
                        line_cap: tiny_skia::LineCap::Round,
                        ..Default::default()
                    },
                    Transform::identity(),
                    None,
                );
            }

            // Keep the optional event note in its own band between the arena
            // and the tab selector instead of letting it reach the tabs.
            if let Some(event) = match_data
                .latest_event
                .as_deref()
                .map(str::trim)
                .filter(|event| !event.is_empty())
            {
                let event = canvas.truncate_to_width(event, bw - 32.0, 11.5, false);
                canvas.draw_text(
                    &event,
                    x + 16.0,
                    y + 86.0 + text_dy,
                    11.5,
                    Color::from_rgba8(200, 200, 206, (180.0 * e_text) as u8),
                    false,
                );
            }
        } else {
            let empty_str = "No live matches pinned";
            let ew = canvas.measure_text_width(empty_str, 12.5, false);
            canvas.draw_text(
                empty_str,
                x + (bw - ew) / 2.0,
                y + 48.0 + text_dy,
                12.5,
                muted,
                false,
            );
        }
    }

    fn draw_sports_logo(
        canvas: &mut LayeredCanvas,
        bytes: Option<&[u8]>,
        x: f32,
        y: f32,
        size: f32,
        team_color: [u8; 4],
        alpha: f32,
    ) {
        let mut background = Paint::default();
        background.anti_alias = true;
        background.set_color_rgba8(
            team_color[0],
            team_color[1],
            team_color[2],
            (205.0 * alpha) as u8,
        );
        let mut background_path = PathBuilder::new();
        Self::add_round_rect(&mut background_path, x, y, size, size, 10.0);
        if let Some(path) = background_path.finish() {
            canvas.pixmap.fill_path(
                &path,
                &background,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        let Some(bytes) = bytes else {
            return;
        };
        let Ok(image) = image::load_from_memory(bytes) else {
            return;
        };
        let rgba = image.to_rgba8();
        let Some(mut pixmap) = Pixmap::new(rgba.width(), rgba.height()) else {
            return;
        };
        for (source, target) in rgba
            .as_raw()
            .chunks_exact(4)
            .zip(pixmap.data_mut().chunks_exact_mut(4))
        {
            let image_alpha = source[3] as u16;
            target[0] = ((source[0] as u16 * image_alpha) / 255) as u8;
            target[1] = ((source[1] as u16 * image_alpha) / 255) as u8;
            target[2] = ((source[2] as u16 * image_alpha) / 255) as u8;
            target[3] = ((source[3] as f32) * alpha) as u8;
        }

        let inner_size = size - 8.0;
        let scale = (inner_size / pixmap.width() as f32).min(inner_size / pixmap.height() as f32);
        let draw_w = pixmap.width() as f32 * scale;
        let draw_h = pixmap.height() as f32 * scale;
        let transform = Transform::from_scale(scale, scale).post_translate(
            x + (size - draw_w) / 2.0,
            y + (size - draw_h) / 2.0,
        );
        let mut logo_paint = Paint::default();
        logo_paint.anti_alias = true;
        logo_paint.shader = Pattern::new(
            pixmap.as_ref(),
            SpreadMode::Pad,
            FilterQuality::Bilinear,
            1.0,
            transform,
        );
        let mut clip = PathBuilder::new();
        Self::add_round_rect(&mut clip, x + 4.0, y + 4.0, inner_size, inner_size, 7.0);
        if let Some(path) = clip.finish() {
            canvas.pixmap.fill_path(
                &path,
                &logo_paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}
