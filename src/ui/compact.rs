use crate::render::{AlbumArtManager, LayeredCanvas, MascotEngine, Visualizer};
use crate::state::{ActivityType, LyricsState, NotificationPayload, SportsState};
use tiny_skia::{Color, FilterQuality, Paint, PathBuilder, Pattern, Pixmap, SpreadMode, Stroke, Transform};

pub struct CompactRenderer;

impl CompactRenderer {
    pub fn render(
        canvas: &mut LayeredCanvas,
        mascot: &mut MascotEngine,
        visualizer: &mut Visualizer,
        album_art: &AlbumArtManager,
        island_x: f32,
        w: f32,
        h: f32,
        pointer_x: f32,
        pointer_y: f32,
        primary_activity: Option<ActivityType>,
        is_music_playing: bool,
        is_focus_running: bool,
        focus_left_ms: u64,
        focus_total_ms: u64,
        lyrics: &LyricsState,
        active_notif: Option<&NotificationPayload>,
        is_volume_hud: bool,
        volume: f32,
        is_muted: bool,
        audio_levels: Option<&[f32]>,
        is_hearing_audio: bool,
        t: f32,
        clip_copied_banner_until: Option<std::time::Instant>,
        is_rest: bool,
        content_scale: f32,
        _volume_stretch: f32,
        sports: Option<&SportsState>,
    ) -> bool {
        let cy = h / 2.0;
        let left = island_x + 14.0;
        let right = island_x + w - 14.0;

        let mut is_animating = false;

        let now = std::time::Instant::now();
        let is_showing_copied = clip_copied_banner_until.map_or(false, |until| now < until);
        let copied_alpha = if is_showing_copied {
            let until = clip_copied_banner_until.unwrap();
            let remaining = (until - now).as_millis() as f32;
            let total_dur = 1800.0f32;
            if remaining < 250.0 {
                (remaining / 250.0).clamp(0.0, 1.0)
            } else if total_dur - remaining < 180.0 {
                ((total_dur - remaining) / 180.0).clamp(0.0, 1.0)
            } else {
                1.0
            }
        } else {
            0.0
        };

        if let Some(notif) = active_notif {
            // ==========================================
            // Notification Capsule View
            // ==========================================
            let avatar_size = 36.0;
            let avatar_x = left + 4.0;

            // Avatar background
            let mut paint = Paint::default();
            paint.anti_alias = true;
            if notif.is_device {
                paint.set_color_rgba8(28, 28, 30, 255);
            } else {
                paint.set_color_rgba8(
                    notif.brand_color[0],
                    notif.brand_color[1],
                    notif.brand_color[2],
                    notif.brand_color[3],
                );
            }

            let mut pb = PathBuilder::new();
            pb.push_circle(avatar_x + avatar_size / 2.0, cy, avatar_size / 2.0);
            if let Some(path) = pb.finish() {
                canvas.pixmap.fill_path(
                    &path,
                    &paint,
                    tiny_skia::FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }

            // Prefer the actual toast avatar/app icon. The original Electron
            // implementation used the toast image first, then the resolved
            // application icon, and only then its vector fallback.
            let icon_drawn = notif
                .avatar_image
                .as_ref()
                .and_then(|bytes| {
                    Self::draw_notification_image(
                        canvas,
                        bytes,
                        avatar_x + avatar_size / 2.0,
                        cy,
                        avatar_size,
                        true,
                    )
                    .then_some(())
                })
                .is_some()
                || notif
                    .icon_image
                    .as_ref()
                    .and_then(|bytes| {
                        Self::draw_notification_image(
                            canvas,
                            bytes,
                            avatar_x + avatar_size / 2.0,
                            cy,
                            avatar_size,
                            false,
                        )
                        .then_some(())
                    })
                    .is_some();

            if !icon_drawn {
                Self::draw_notification_icon(
                    canvas,
                    &notif.badge_app,
                    avatar_x + avatar_size / 2.0,
                    cy,
                    19.0,
                    notif.is_snip,
                    notif.is_device,
                    notif.brand_color,
                );
            }

            // Title & Subtitle
            // Refined baseline coordinates for proper vertical centering
            let text_left = avatar_x + avatar_size + 14.0;
            let right_margin = right - 4.0;
            let text_available_right = right_margin - 40.0;
            let text_max_width = (text_available_right - text_left - 10.0).max(140.0);
            let text_col_white = Color::from_rgba8(255, 255, 255, 255);
            let text_col_sub = Color::from_rgba8(235, 235, 245, 185);

            if !notif.subtitle.is_empty() {
                let title = canvas.truncate_to_width(&notif.title, text_max_width, 13.5, true);
                let subtitle = canvas.truncate_to_width(&notif.subtitle, text_max_width, 12.5, false);
                // Shifted up by 8px to account for the top-origin text rendering math
                canvas.draw_text(&title, text_left, cy - 14.0, 13.5, text_col_white, true);
                canvas.draw_text(&subtitle, text_left, cy + 2.0, 12.5, text_col_sub, false);
            } else {
                let title = canvas.truncate_to_width(&notif.title, text_max_width, 13.5, true);
                canvas.draw_text(&title, text_left, cy - 7.0, 13.5, text_col_white, true);
            }

            // Time & Dismiss icon
            let time_col = Color::from_rgba8(255, 255, 255, 95);
            let time_width = canvas.measure_text_width(&notif.time_text, 12.0, false);
            canvas.draw_text(
                &notif.time_text,
                right - 24.0 - time_width,
                cy - 14.0,
                12.0,
                time_col,
                false,
            );

            let mut dismiss_paint = Paint::default();
            dismiss_paint.anti_alias = true;
            dismiss_paint.set_color_rgba8(255, 255, 255, 140);
            let stroke_15 = Stroke {
                width: 1.5,
                line_cap: tiny_skia::LineCap::Round,
                ..Default::default()
            };

            let btn_x = right - 8.0;
            let mut dismiss_bg = Paint::default();
            dismiss_bg.anti_alias = true;
            dismiss_bg.set_color_rgba8(255, 255, 255, 31);
            let mut dismiss_circle = PathBuilder::new();
            dismiss_circle.push_circle(btn_x, cy, 9.5);
            if let Some(path) = dismiss_circle.finish() {
                canvas.pixmap.fill_path(
                    &path,
                    &dismiss_bg,
                    tiny_skia::FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
            let cr = 3.6;
            let mut pb = PathBuilder::new();
            pb.move_to(btn_x - cr, cy - cr);
            pb.line_to(btn_x + cr, cy + cr);
            pb.move_to(btn_x + cr, cy - cr);
            pb.line_to(btn_x - cr, cy + cr);
            if let Some(path) = pb.finish() {
                canvas.pixmap.stroke_path(
                    &path,
                    &dismiss_paint,
                    &stroke_15,
                    Transform::identity(),
                    None,
                );
            }
        } else if is_volume_hud {
            // ==========================================
            // Volume HUD Slider View
            // ==========================================
            let left_speaker_x = island_x + 22.0;
            let right_vol_x = island_x + w - 24.0;

            Self::draw_speaker_icon(canvas, left_speaker_x, cy, is_muted, volume, 1.0);

            let bar_h = 5.0;
            let bar_y = cy - bar_h / 2.0;
            let bar_w = (w - 92.0).max(10.0);
            let bar_x = island_x + (w - bar_w) / 2.0;

            // Track
            let mut bg_paint = Paint::default();
            bg_paint.anti_alias = true;
            bg_paint.set_color_rgba8(255, 255, 255, 56);
            let mut pb = PathBuilder::new();
            pb.move_to(bar_x + 2.5, bar_y);
            pb.line_to(bar_x + bar_w - 2.5, bar_y);
            pb.quad_to(bar_x + bar_w, bar_y, bar_x + bar_w, bar_y + 2.5);
            pb.quad_to(bar_x + bar_w, bar_y + bar_h, bar_x + bar_w - 2.5, bar_y + bar_h);
            pb.line_to(bar_x + 2.5, bar_y + bar_h);
            pb.quad_to(bar_x, bar_y + bar_h, bar_x, bar_y + 2.5);
            pb.quad_to(bar_x, bar_y, bar_x + 2.5, bar_y);
            pb.close();
            if let Some(p) = pb.finish() {
                canvas.pixmap.fill_path(&p, &bg_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            // Fill
            if !is_muted && volume > 0.0 {
                let fill_w = (bar_w * (volume / 100.0)).clamp(5.0, bar_w);
                let mut fill_paint = Paint::default();
                fill_paint.anti_alias = true;
                fill_paint.set_color_rgba8(255, 255, 255, 255);
                let mut pb = PathBuilder::new();
                pb.move_to(bar_x + 2.5, bar_y);
                pb.line_to(bar_x + fill_w - 2.5, bar_y);
                pb.quad_to(bar_x + fill_w, bar_y, bar_x + fill_w, bar_y + 2.5);
                pb.quad_to(bar_x + fill_w, bar_y + bar_h, bar_x + fill_w - 2.5, bar_y + bar_h);
                pb.line_to(bar_x + 2.5, bar_y + bar_h);
                pb.quad_to(bar_x, bar_y + bar_h, bar_x, bar_y + 2.5);
                pb.quad_to(bar_x, bar_y, bar_x + 2.5, bar_y);
                pb.close();
                if let Some(p) = pb.finish() {
                    canvas.pixmap.fill_path(&p, &fill_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                }
            }

            // Volume number
            let num_text = if is_muted { "0".to_string() } else { format!("{}", volume.round() as i32) };
            canvas.draw_text(&num_text, right_vol_x - 8.0, cy - 4.0, 12.0, Color::WHITE, true);
            
        } else if is_showing_copied && copied_alpha > 0.01 {
            // ==========================================
            // Copied Confirmation View
            // ==========================================
            let cx = island_x + w / 2.0;
            Self::draw_center_copied_badge(canvas, cx, cy, copied_alpha);
            is_animating = true;
            if copied_alpha < 0.99 {
                let animating = mascot.update_and_draw(
                    &mut canvas.pixmap.as_mut(),
                    cx,
                    cy,
                    (1.0 - copied_alpha).clamp(0.0, 1.0) * content_scale,
                    pointer_x,
                    pointer_y,
                    is_music_playing,
                    is_focus_running,
                    is_rest,
                );
                if animating {
                    is_animating = true;
                }
            }
        } else if primary_activity == Some(ActivityType::Music) {
            // ==========================================
            // Music Compact View (Album Art + Lyric/Eyes + Visualizer)
            // ==========================================
            // 1. Left: Album Cover Thumbnail (24x24 with radius 7.0)
            let cs = h - 12.0;
            album_art.draw(&mut canvas.pixmap.as_mut(), left - 4.0, cy - cs / 2.0, cs, 7.0);

            // 2. Center: Synced Lyric OR Companion Eyes
            let lyric = if lyrics.has_lyrics() {
                Some(lyrics.get_now_lyric())
            } else {
                None
            };

            let mut drew_lyrics = false;
            if let Some(ref l) = lyric {
                if l.text.is_some() || l.previous.is_some() {
                    drew_lyrics = true;
                    let x0 = left - 4.0 + cs + 12.0;
                    let x1 = right - 36.0 - 12.0;
                    let lw = (x1 - x0).max(20.0);
                    let band = 20.0f32;
                    let k = l.k;

                    // Transition mascot eyes if entering/leaving gap
                    if l.text.is_none() || l.previous.is_none() {
                        let ek = if l.text.is_some() {
                            (1.0 - k / 0.35).max(0.0)
                        } else {
                            ((k - 0.65) / 0.35).max(0.0)
                        };
                        if ek > 0.01 {
                            let animating = mascot.update_and_draw(
                                &mut canvas.pixmap.as_mut(),
                                island_x + w / 2.0,
                                cy,
                                0.95 * content_scale,
                                pointer_x,
                                pointer_y,
                                is_music_playing,
                                is_focus_running,
                                is_rest,
                            );
                            if animating {
                                is_animating = true;
                            }
                        }
                    }

                    // Truncate previous and current lyric lines if wider than lw
                    let truncated_prev = l.previous.as_ref().map(|prev| {
                        canvas.truncate_to_width(prev, lw, 13.0, true)
                    });

                    let truncated_curr = l.text.as_ref().map(|curr| {
                        canvas.truncate_to_width(curr, lw, 13.0, true)
                    });

                    // Draw previous lyric line scrolling up
                    if let Some(ref prev) = truncated_prev {
                        let alpha = (1.0 - (band * k).abs() / band).clamp(0.0, 1.0);
                        if alpha > 0.01 {
                            let tw = canvas.measure_text_width(prev, 13.0, true);
                            let tx = x0 + (lw - tw) / 2.0;
                            let ty = cy - band * k - 7.0;
                            let col = Color::from_rgba8(255, 255, 255, (alpha * 255.0) as u8);
                            canvas.draw_text(prev, tx, ty, 13.0, col, true);
                        }
                    }

                    // Draw current lyric line scrolling into center
                    if let Some(ref curr) = truncated_curr {
                        let alpha = (1.0 - (band * (1.0 - k)).abs() / band).clamp(0.0, 1.0);
                        if alpha > 0.01 {
                            let tw = canvas.measure_text_width(curr, 13.0, true);
                            let tx = x0 + (lw - tw) / 2.0;
                            let ty = cy + band * (1.0 - k) - 7.0;
                            let col = Color::from_rgba8(255, 255, 255, (alpha * 255.0) as u8);
                            canvas.draw_text(curr, tx, ty, 13.0, col, true);
                        }
                    }
                }
            }

            if !drew_lyrics {
                let cx = island_x + w / 2.0;
                let animating = mascot.update_and_draw(
                    &mut canvas.pixmap.as_mut(),
                    cx,
                    cy,
                    1.0 * content_scale,
                    pointer_x,
                    pointer_y,
                    is_music_playing,
                    is_focus_running,
                    is_rest,
                );
                if animating {
                    is_animating = true;
                }
            }

            // 3. Right: Real-time Sound Wave Visualizer in album art dominant color
            let [cr, cg, cb] = album_art.dominant_color;
            let viz_color = Color::from_rgba8(cr, cg, cb, 255);
            let viz_w = 32.0 * content_scale;
            let viz_h = 16.0 * content_scale;
            let _viz_moving = visualizer.draw(
                &mut canvas.pixmap.as_mut(),
                "compact",
                right - 34.0 + (32.0 - viz_w) * 0.5,
                cy,
                viz_w,
                viz_h,
                viz_color,
                6,
                is_music_playing,
                audio_levels,
                is_hearing_audio,
                t,
            );
            if is_music_playing {
                is_animating = true;
            }
        } else if primary_activity == Some(ActivityType::Focus) && !is_rest {
            // ==========================================
            // Focus Compact View (Timer Ring + Eyes + Time)
            // ==========================================
            let ring_x = left + 8.0;
            let rad = 9.0;
            let frac = if focus_total_ms > 0 {
                (focus_left_ms as f32 / focus_total_ms as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };

            // Background ring
            let mut bg_ring = Paint::default();
            bg_ring.anti_alias = true;
            bg_ring.set_color_rgba8(240, 160, 100, 64);
            let stroke_25 = Stroke { width: 2.5, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
            let mut pb = PathBuilder::new();
            pb.push_circle(ring_x, cy, rad);
            if let Some(p) = pb.finish() {
                canvas.pixmap.stroke_path(&p, &bg_ring, &stroke_25, Transform::identity(), None);
            }

            // Progress ring
            if frac > 0.0 {
                let mut fg_ring = Paint::default();
                fg_ring.anti_alias = true;
                fg_ring.set_color_rgba8(240, 160, 100, 255);
                let start_angle = -std::f32::consts::PI / 2.0;
                let end_angle = start_angle + frac * std::f32::consts::PI * 2.0;
                let steps = 36;
                let mut pb = PathBuilder::new();
                pb.move_to(ring_x + rad * start_angle.cos(), cy + rad * start_angle.sin());
                for s in 1..=steps {
                    let a = start_angle + (end_angle - start_angle) * (s as f32 / steps as f32);
                    pb.line_to(ring_x + rad * a.cos(), cy + rad * a.sin());
                }
                if let Some(p) = pb.finish() {
                    canvas.pixmap.stroke_path(&p, &fg_ring, &stroke_25, Transform::identity(), None);
                }
            }

            // Mascot eyes
            let cx = island_x + w / 2.0;
            let animating = mascot.update_and_draw(
                &mut canvas.pixmap.as_mut(),
                cx,
                cy,
                1.0,
                pointer_x,
                pointer_y,
                is_music_playing,
                is_focus_running,
                is_rest,
            );
            if animating {
                is_animating = true;
            }

            // Time string on right (formatted and right-aligned like MyIsland index.html line 3466)
            let time_str = crate::ui::format_focus_timer(focus_left_ms);
            let orange = Color::from_rgba8(240, 160, 100, 255);
            let tw = canvas.measure_text_width(&time_str, 13.0, true);
            canvas.draw_text(&time_str, right - tw, cy - 7.0, 13.0, orange, true);
        } else if primary_activity == Some(ActivityType::Sports) && !is_rest {
            // ==========================================
            // Sports Live Pill (Compact)
            // ==========================================
            if let Some(match_data) = sports {
                let text_color = Color::WHITE;
                let home_color = Color::from_rgba8(
                    match_data.home_color[0],
                    match_data.home_color[1],
                    match_data.home_color[2],
                    255,
                );
                let away_color = Color::from_rgba8(
                    match_data.away_color[0],
                    match_data.away_color[1],
                    match_data.away_color[2],
                    255,
                );

                let mut home_dot = Paint::default();
                home_dot.anti_alias = true;
                home_dot.set_color(home_color);
                let mut home_path = PathBuilder::new();
                home_path.push_circle(left + 1.0, cy, 3.5);
                if let Some(path) = home_path.finish() {
                    canvas.pixmap.fill_path(
                        &path,
                        &home_dot,
                        tiny_skia::FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }

                // Leading: Home team & Score
                let home_str = format!("{} {}", match_data.home_team, match_data.home_score);
                let home_w = canvas.measure_text_width(&home_str, 13.5, true);
                canvas.draw_text(&home_str, left + 9.0, cy - 7.0, 13.5, text_color, true);

                // Trailing: Away score & Team
                let away_str = format!("{} {}", match_data.away_score, match_data.away_team);
                let away_w = canvas.measure_text_width(&away_str, 13.5, true);
                let away_x = right - away_w - 9.0;

                // Center: live clock / kickoff, constrained to the space left
                // between the two team blocks so it can never overlap them.
                let center_str = if match_data.is_live {
                    format!("{} 🟢", match_data.clock)
                } else {
                    match_data.clock.trim().replace(" at ", " · ")
                };
                let center_left = left + 9.0 + home_w + 14.0;
                let center_right = away_x - 14.0;
                let center_max = (center_right - center_left).max(0.0);
                let center_str = canvas.truncate_to_width(&center_str, center_max, 12.0, false);
                let center_w = canvas.measure_text_width(&center_str, 12.0, false);
                let center_x = (center_left + center_right) / 2.0 - center_w / 2.0;
                canvas.draw_text(&center_str, center_x, cy - 6.0, 12.0, Color::from_rgba8(235, 235, 245, 185), false);

                let mut away_dot = Paint::default();
                away_dot.anti_alias = true;
                away_dot.set_color(away_color);
                let mut away_path = PathBuilder::new();
                away_path.push_circle(away_x - 5.0, cy, 3.5);
                if let Some(path) = away_path.finish() {
                    canvas.pixmap.fill_path(
                        &path,
                        &away_dot,
                        tiny_skia::FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
                canvas.draw_text(&away_str, away_x, cy - 7.0, 13.5, text_color, true);
            }
        } else {
            // ==========================================
            // Standard Compact Mode (Idle / Hovered) - MyIsland lines 3544-3572
            // Left: Speaker Icon (shows mute or wave levels)
            // Center: Mascot Eyes
            // Right: Microphone Icon
            // ==========================================
            let side_alpha = ((w - 140.0) / 70.0).clamp(0.0, 1.0);

            if side_alpha > 0.01 {
                Self::draw_speaker_icon(canvas, island_x + 22.0, cy, is_muted, volume, side_alpha * 0.88);
                Self::draw_mic_icon(canvas, island_x + w - 24.0, cy, side_alpha * 0.78);
            }

            let cx = island_x + w / 2.0;
            if is_showing_copied && copied_alpha > 0.01 {
                Self::draw_center_copied_badge(canvas, cx, cy, copied_alpha);
                is_animating = true;
                if copied_alpha < 0.99 {
                    let animating = mascot.update_and_draw(
                        &mut canvas.pixmap.as_mut(),
                        cx,
                        cy,
                        (1.0 - copied_alpha).clamp(0.0, 1.0),
                        pointer_x,
                        pointer_y,
                        is_music_playing,
                        is_focus_running,
                        is_rest,
                    );
                    if animating {
                        is_animating = true;
                    }
                }
            } else {
                let animating = mascot.update_and_draw(
                    &mut canvas.pixmap.as_mut(),
                    cx,
                    cy,
                    1.0,
                    pointer_x,
                    pointer_y,
                    is_music_playing,
                    is_focus_running,
                    is_rest,
                );
                if animating {
                    is_animating = true;
                }
            }
        }

        is_animating
    }

    fn draw_center_copied_badge(
        canvas: &mut LayeredCanvas,
        cx: f32,
        cy: f32,
        alpha: f32,
    ) {
        if alpha <= 0.01 {
            return;
        }

        let green = Color::from_rgba8(48, 209, 88, (alpha * 255.0) as u8); // Apple emerald #30d158
        let text = "Copied";
        let text_w = canvas.measure_text_width(text, 12.5, true);
        let icon_size = 11.0f32;
        let gap = 6.0f32;
        let total_w = icon_size + gap + text_w;
        let start_x = cx - total_w / 2.0;

        // Checkmark path: check_x + 1.5, cy - 0.5 -> check_x + 4.5, cy + 3.0 -> check_x + 10.5, cy - 3.5
        let check_x = start_x;
        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(green);
        let stroke = Stroke {
            width: 2.2,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };

        let mut pb = PathBuilder::new();
        pb.move_to(check_x + 1.5, cy - 0.5);
        pb.line_to(check_x + 4.5, cy + 3.0);
        pb.line_to(check_x + 10.5, cy - 3.5);
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &paint, &stroke, Transform::identity(), None);
        }

        // "Copied" text
        canvas.draw_text(text, start_x + icon_size + gap, cy - 7.0, 12.5, green, true);
    }

    fn draw_notification_icon(
        canvas: &mut LayeredCanvas,
        badge_app: &str,
        cx: f32,
        cy: f32,
        size: f32,
        is_snip: bool,
        is_device: bool,
        brand_color: [u8; 4],
    ) {
        let app = badge_app.to_lowercase();
        let icon_color = if is_device {
            Color::from_rgba8(brand_color[0], brand_color[1], brand_color[2], brand_color[3])
        } else if is_snip {
            Color::from_rgba8(100, 210, 255, 255)
        } else {
            Color::WHITE
        };

        // Use native Windows icons for specified hardware, fallback to vectors for software
        if app.contains("controller") || app.contains("gamepad") {
            canvas.draw_icon_centered('\u{E75E}', cx, cy, size * 1.15, icon_color);
            return;
        }
        if app.contains("headphone") || app.contains("speaker") || app.contains("buds") || app.contains("earbud") {
            let icon_char = if app.contains("speaker") {
                '\u{E7F5}'
            } else {
                '\u{E7F6}'
            };
            canvas.draw_icon_centered(icon_char, cx, cy, size * 1.05, icon_color);
            return;
        }

        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(icon_color);
        let stroke = Stroke {
            width: 1.8,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };

        let mut pb = PathBuilder::new();
        if app.contains("bluetooth") {
            let h = size * 0.95;
            pb.move_to(cx, cy - h / 2.0);
            pb.line_to(cx, cy + h / 2.0);
            pb.line_to(cx + size * 0.28, cy + h * 0.25);
            pb.line_to(cx - size * 0.23, cy - h * 0.25);
            pb.move_to(cx - size * 0.23, cy + h * 0.25);
            pb.line_to(cx + size * 0.28, cy - h * 0.25);
            pb.line_to(cx, cy - h / 2.0);
        } else if app.contains("whatsapp") || app.contains("phone") || app.contains("call") {
            // Compact handset glyph.
            pb.move_to(cx - size * 0.28, cy - size * 0.30);
            pb.cubic_to(
                cx - size * 0.40,
                cy - size * 0.15,
                cx - size * 0.08,
                cy + size * 0.28,
                cx + size * 0.22,
                cy + size * 0.31,
            );
            pb.line_to(cx + size * 0.38, cy + size * 0.15);
            pb.line_to(cx + size * 0.18, cy - size * 0.02);
            pb.line_to(cx + size * 0.02, cy + size * 0.10);
            pb.cubic_to(
                cx - size * 0.08,
                cy + size * 0.01,
                cx - size * 0.16,
                cy - size * 0.09,
                cx - size * 0.14,
                cy - size * 0.19,
            );
            pb.line_to(cx - size * 0.02, cy - size * 0.35);
        } else if app.contains("discord") {
            // Discord-like controller outline with two eyes.
            if let Some(rect) = tiny_skia::Rect::from_xywh(
                cx - size * 0.42,
                cy - size * 0.26,
                size * 0.84,
                size * 0.52,
            ) {
                pb.push_rect(rect);
            }
            pb.push_circle(cx - size * 0.15, cy, size * 0.045);
            pb.push_circle(cx + size * 0.15, cy, size * 0.045);
        } else if app.contains("snip") || app.contains("screen") {
            // Scissors / screenshot mark.
            pb.push_circle(cx - size * 0.26, cy - size * 0.23, size * 0.12);
            pb.push_circle(cx - size * 0.26, cy + size * 0.23, size * 0.12);
            pb.move_to(cx - size * 0.16, cy - size * 0.16);
            pb.line_to(cx + size * 0.32, cy + size * 0.30);
            pb.move_to(cx - size * 0.16, cy + size * 0.16);
            pb.line_to(cx + size * 0.32, cy - size * 0.30);
        } else if app.contains("slack") {
            // Four short rounded bars suggest the Slack mark.
            for offset in [-0.24f32, 0.08] {
                pb.move_to(cx - size * 0.28, cy + size * offset);
                pb.line_to(cx + size * 0.28, cy + size * offset);
            }
            for offset in [-0.24f32, 0.08] {
                pb.move_to(cx + size * offset, cy - size * 0.28);
                pb.line_to(cx + size * offset, cy + size * 0.28);
            }
        } else {
            // Generic notification bell.
            pb.move_to(cx - size * 0.30, cy + size * 0.20);
            pb.quad_to(cx - size * 0.22, cy + size * 0.08, cx - size * 0.22, cy - size * 0.10);
            pb.quad_to(cx - size * 0.22, cy - size * 0.34, cx, cy - size * 0.38);
            pb.quad_to(cx + size * 0.22, cy - size * 0.34, cx + size * 0.22, cy - size * 0.10);
            pb.quad_to(cx + size * 0.22, cy + size * 0.08, cx + size * 0.30, cy + size * 0.20);
            pb.move_to(cx - size * 0.36, cy + size * 0.20);
            pb.line_to(cx + size * 0.36, cy + size * 0.20);
            pb.push_circle(cx, cy + size * 0.31, size * 0.05);
        }

        if let Some(path) = pb.finish() {
            if app.contains("discord") {
                canvas.pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            } else {
                canvas.pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }
        }
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

    fn draw_speaker_icon(
        canvas: &mut LayeredCanvas,
        cx: f32,
        cy: f32,
        muted: bool,
        volume: f32,
        alpha: f32,
    ) {
        if alpha <= 0.01 {
            return;
        }

        let a = (alpha * 255.0) as u8;
        let col = if muted || volume == 0.0 {
            Color::from_rgba8(255, 69, 58, a) // #ff453a
        } else {
            Color::from_rgba8(255, 255, 255, (alpha * 224.0) as u8) // rgba(255, 255, 255, 0.88)
        };

        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(col);

        let stroke_14 = Stroke {
            width: 1.4,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };

        // Speaker body & cone (MyIsland lines 2171-2180)
        let mut pb = PathBuilder::new();
        pb.move_to(cx - 7.0, cy - 3.0);
        pb.line_to(cx - 3.5, cy - 3.0);
        pb.line_to(cx + 1.0, cy - 6.5);
        pb.line_to(cx + 1.0, cy + 6.5);
        pb.line_to(cx - 3.5, cy + 3.0);
        pb.line_to(cx - 7.0, cy + 3.0);
        pb.close();
        if let Some(p) = pb.finish() {
            canvas.pixmap.fill_path(&p, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }

        if muted || volume == 0.0 {
            // Small 'x' mute indicator (MyIsland lines 2184-2189)
            let mut pb = PathBuilder::new();
            pb.move_to(cx + 4.0, cy - 3.0);
            pb.line_to(cx + 8.0, cy + 3.0);
            pb.move_to(cx + 8.0, cy - 3.0);
            pb.line_to(cx + 4.0, cy + 3.0);
            if let Some(p) = pb.finish() {
                canvas.pixmap.stroke_path(&p, &paint, &stroke_14, Transform::identity(), None);
            }
        } else {
            // Sound waves (MyIsland lines 2191-2205)
            let draw_arc = |canvas: &mut LayeredCanvas, r: f32, max_angle: f32| {
                let mut pb = PathBuilder::new();
                let steps = 12;
                let start_a = -max_angle;
                let end_a = max_angle;
                pb.move_to(cx + 1.0 + r * start_a.cos(), cy + r * start_a.sin());
                for s in 1..=steps {
                    let a = start_a + (end_a - start_a) * (s as f32 / steps as f32);
                    pb.line_to(cx + 1.0 + r * a.cos(), cy + r * a.sin());
                }
                if let Some(p) = pb.finish() {
                    canvas.pixmap.stroke_path(&p, &paint, &stroke_14, Transform::identity(), None);
                }
            };

            // Wave 1
            draw_arc(canvas, 4.5, std::f32::consts::PI / 3.0);

            // Wave 2
            if volume >= 35.0 {
                draw_arc(canvas, 7.5, std::f32::consts::PI / 3.2);
            }

            // Wave 3
            if volume >= 70.0 {
                draw_arc(canvas, 10.5, std::f32::consts::PI / 3.5);
            }
        }
    }

    fn draw_mic_icon(
        canvas: &mut LayeredCanvas,
        cx: f32,
        cy: f32,
        alpha: f32,
    ) {
        if alpha <= 0.01 {
            return;
        }

        let col = Color::from_rgba8(255, 255, 255, (alpha * 199.0) as u8); // 0.78 alpha

        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(col);

        let stroke_14 = Stroke {
            width: 1.4,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };

        // 1. Mic capsule body: roundRect(cx - 2.5, cy - 6.5, 5, 8.5, 2.5)
        let mut pb = PathBuilder::new();
        let x = cx - 2.5;
        let y = cy - 6.5;
        let w = 5.0f32;
        let h = 8.5f32;
        let r = 2.5f32;
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
        if let Some(p) = pb.finish() {
            canvas.pixmap.fill_path(&p, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }

        // 2. Mic cradle / U-arc: arc(cx, cy - 2, 4.5, 0, PI)
        let mut pb = PathBuilder::new();
        let rad = 4.5f32;
        let steps = 16;
        let arc_cy = cy - 2.0;
        pb.move_to(cx + rad, arc_cy);
        for s in 1..=steps {
            let a = (s as f32 / steps as f32) * std::f32::consts::PI;
            pb.line_to(cx + rad * a.cos(), arc_cy + rad * a.sin());
        }
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &paint, &stroke_14, Transform::identity(), None);
        }

        // 3. Stem & Base stand
        let mut pb = PathBuilder::new();
        pb.move_to(cx, cy + 2.5);
        pb.line_to(cx, cy + 5.5);
        pb.move_to(cx - 3.0, cy + 5.5);
        pb.line_to(cx + 3.0, cy + 5.5);
        if let Some(p) = pb.finish() {
            canvas.pixmap.stroke_path(&p, &paint, &stroke_14, Transform::identity(), None);
        }
    }
}
