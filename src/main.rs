#![windows_subsystem = "windows"]

mod config;
mod physics;
mod platform;
mod render;
mod state;
mod ui;

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Instant;

use config::*;
use physics::{ParticleSystem, Spring};
use platform::{
    AudioCaptureService, FullscreenDetector, GsmtcService, MediaControlCmd, MediaState,
    NotificationService, VolumeManager, Win32Window,
};
use render::{
    build_island_path, build_liquid_bridge_path, AlbumArtManager, LayeredCanvas, MascotEngine,
    MascotState, ShadowRenderer, Visualizer,
};
use tiny_skia::{PathBuilder, Transform};
use state::{
    fetch_online_artwork, ActivityManager, ActivityType, ClipboardManager, LyricLine, LyricsState,
    NotificationPayload,
};
use ui::{
    CompactRenderer, ExpandedRenderer, TrayIcon, IDM_QUIT, IDM_TEST_CALL, IDM_TEST_DISCORD,
    IDM_TEST_SNIP, IDM_TEST_WHATSAPP, IDM_TOGGLE_EXPAND, IDM_TUCK_AWAY, WM_TRAYICON,
};

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PostQuitMessage, TranslateMessage, MSG, WM_CLIPBOARDUPDATE,
    WM_COMMAND, WM_DESTROY, WM_HOTKEY, WM_LBUTTONUP, WM_MOUSEWHEEL, WM_RBUTTONUP,
};

static SHOULD_QUIT: AtomicBool = AtomicBool::new(false);
static NOTES_FOCUSED: AtomicBool = AtomicBool::new(false);
static IS_EXPANDED: AtomicBool = AtomicBool::new(false);

static HIT_X: AtomicI32 = AtomicI32::new(0);
static HIT_Y: AtomicI32 = AtomicI32::new(0);
static HIT_W: AtomicI32 = AtomicI32::new(0);
static HIT_H: AtomicI32 = AtomicI32::new(0);

static SEC_HIT_X: AtomicI32 = AtomicI32::new(0);
static SEC_HIT_W: AtomicI32 = AtomicI32::new(0);
static SEC_HIT_H: AtomicI32 = AtomicI32::new(0);

static WIN_X: AtomicI32 = AtomicI32::new(0);
static WIN_Y: AtomicI32 = AtomicI32::new(0);

const WM_NCHITTEST: u32 = 0x0084;
const HTTRANSPARENT: isize = -1;
const HTCLIENT: isize = 1;
const WM_MOUSEACTIVATE: u32 = 0x0021;
const MA_ACTIVATE: isize = 1;

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => {
            if IS_EXPANDED.load(Ordering::Relaxed) {
                LRESULT(MA_ACTIVATE)
            } else {
                windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_NCHITTEST => {
            let screen_x = (lparam.0 & 0xffff) as i16 as i32;
            let screen_y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let px = screen_x - WIN_X.load(Ordering::Relaxed);
            let py = screen_y - WIN_Y.load(Ordering::Relaxed);

            let hx = HIT_X.load(Ordering::Relaxed);
            let hy = HIT_Y.load(Ordering::Relaxed);
            let hw = HIT_W.load(Ordering::Relaxed);
            let hh = HIT_H.load(Ordering::Relaxed);

            let in_main = px >= hx - 8 && px <= hx + hw + 8 && py >= hy && py <= hh + 8;

            let sw = SEC_HIT_W.load(Ordering::Relaxed);
            let in_sec = if sw > 0 {
                let sx = SEC_HIT_X.load(Ordering::Relaxed);
                let sh = SEC_HIT_H.load(Ordering::Relaxed);
                px >= sx - 4 && px <= sx + sw + 4 && py >= 0 && py <= sh + 4
            } else {
                false
            };

            if in_main || in_sec {
                LRESULT(HTCLIENT)
            } else {
                LRESULT(HTTRANSPARENT)
            }
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write("panic.txt", format!("Panic: {:?}", info));
    }));
    if let Err(e) = run_app() {
        let _ = std::fs::write("error.log", format!("run_app error: {:?}", e));
    }
}

fn run_app() -> windows::core::Result<()> {
    unsafe {
        let _ = timeBeginPeriod(1);
    }

    // 1. Enforce single instance lock via Win32 Named Mutex
    let _mutex = unsafe {
        let m = CreateMutexW(None, true, w!("Local\\MyIslandSingleInstanceRust"));
        let err = windows::Win32::Foundation::GetLastError();
        let _ = std::fs::write("launch.log", format!("mutex created, err={:?}\n", err));
        if err == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        m.ok()
    };

    // 2. COM Apartment initialization
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }

    // 3. Create Win32 Borderless Alpha Window
    let window = Win32Window::create(window_proc)?;
    WIN_X.store(window.x, Ordering::Relaxed);
    WIN_Y.store(window.y, Ordering::Relaxed);

    // 4. Create Layered Canvas for 32-bit premultiplied alpha rendering
    let mut canvas = LayeredCanvas::new(WINDOW_WIDTH, WINDOW_HEIGHT)?;

    // 5. Physics Springs
    let mut spring_w = Spring::default_island(DIMS_REST.w);
    let mut spring_h = Spring::default_island(DIMS_REST.h);
    let mut spring_r = Spring::default_island(DIMS_REST.r);

    let mut sec_spring_w = Spring::default_island(0.0);
    let mut sec_spring_h = Spring::default_island(0.0);
    let mut sec_spring_r = Spring::default_island(0.0);
    let mut sec_spring_x = Spring::default_island((WINDOW_WIDTH as f32) / 2.0);

    let mut volume_spring = Spring::volume_spring(65.0);

    // 6. State Managers & Engines
    let mut current_state = IslandState::Rest;
    let mut previous_state = IslandState::Rest;
    let mut is_auto_tucked = false;
    let mut leave_timer_start: Option<Instant> = None;
    let mut expanded_leave_timer: Option<Instant> = None;

    let mut current_app: usize = 0;
    let mut scroll_wheel_progress: f32 = 0.0;
    let mut target_scroll_index: f32 = 0.0;
    let mut scroll_spring = Spring::new(0.0, 240.0, 24.0);
    let mut clip_spring = Spring::new(0.0, 220.0, 24.0);
    let mut notes_spring = Spring::new(0.0, 240.0, 25.0);

    let mut mascot = MascotEngine::new();
    let mut particles = ParticleSystem::new();
    let mut visualizer = Visualizer::new();
    let mut lyrics = LyricsState::new();
    let mut clipboard_mgr = ClipboardManager::new();
    let mut volume_mgr = VolumeManager::new();
    let mut fullscreen_detector = FullscreenDetector::new();
    let mut activity_mgr = ActivityManager::new();
    let mut shadow_renderer = ShadowRenderer::new();
    let mut sec_shadow_renderer = ShadowRenderer::new();
    let mut album_art_mgr = AlbumArtManager::new();
    let audio_capture = AudioCaptureService::start();
    let mut smoothed_flame_energy = 0.0f32;

    let mut clip_copied_banner_until: Option<Instant> = None;
    let mut clip_copied_index: Option<usize> = None;
    let mut target_clip_scroll: f32 = 0.0;
    let mut clip_scroll_progress: f32 = 0.0;
    let mut target_notes_scroll: f32 = 0.0;
    #[allow(unused_assignments)]
    let mut notes_scroll_progress: f32 = 0.0;
    let mut suppress_expand_until: Option<Instant> = None;
    let mut notes_focused = false;
    let mut notes_cursor_pos: usize = clipboard_mgr.note_text.len();
    let mut prev_notes_cursor_pos: usize = notes_cursor_pos;

    // Focus Timer state
    let mut focus_active = false;
    let mut focus_paused = false;
    let focus_chosen_sec: u64 = 15 * 60; // 15 mins default
    let mut focus_end_time: Option<Instant> = None;
    let mut focus_paused_remaining = std::time::Duration::from_secs(15 * 60);

    // Volume HUD state
    let mut is_volume_hud_active = false;
    let mut volume_hud_until = Instant::now();

    // 7. Event Communication Channels
    let (media_tx, media_rx) = crossbeam_channel::unbounded::<MediaState>();
    let (notif_tx, notif_rx) = crossbeam_channel::unbounded::<NotificationPayload>();
    let (lyrics_tx, lyrics_rx) = crossbeam_channel::unbounded::<(String, Vec<LyricLine>)>();
    let (art_tx, art_rx) = crossbeam_channel::unbounded::<(String, Vec<u8>)>();

    let gsmtc = GsmtcService::start(media_tx, window.hwnd.0 as isize)?;
    NotificationService::start(notif_tx, window.hwnd.0 as isize);

    let tray = TrayIcon::create(
        window.hwnd,
        windows::Win32::UI::WindowsAndMessaging::HICON(std::ptr::null_mut()),
    )?;

    let mut last_frame_time = Instant::now();
    let start_instant = Instant::now();

    let mut latest_media = MediaState::default();
    let mut current_progress_base: u64 = 0;
    let mut progress_anchor_time = Instant::now();

    let mut is_dirty = true;
    let mut prev_pointer_x: f32 = -9999.0;
    let mut prev_pointer_y: f32 = -9999.0;
    let mut last_cursor_move_time = Instant::now();
    let mut prev_state = current_state;

    // 8. Main Message Pump & Zero-Idle Render Gating Loop
    let mut msg = MSG::default();

    loop {
        // Non-blocking pump of all queued Windows OS messages
        unsafe {
            while windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                &mut msg,
                None,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::PM_REMOVE,
            )
            .as_bool()
            {
                is_dirty = true;
                if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                    SHOULD_QUIT.store(true, Ordering::SeqCst);
                    break;
                }

                let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);

                match msg.message {
                    WM_HOTKEY => {
                        let id = msg.wParam.0 as i32;
                        if id == platform::win32::HOTKEY_TOGGLE_TUCK {
                            current_state = if current_state == IslandState::Tucked {
                                IslandState::Compact
                            } else {
                                IslandState::Tucked
                            };
                        } else if id == platform::win32::HOTKEY_TOGGLE_EXPAND {
                            current_state = if current_state == IslandState::Expanded {
                                IslandState::Compact
                            } else {
                                IslandState::Expanded
                            };
                        }
                    }
                    WM_CLIPBOARDUPDATE => {
                        // Read clipboard text
                        if let Ok(()) = windows::Win32::System::DataExchange::OpenClipboard(window.hwnd) {
                            let handle = windows::Win32::System::DataExchange::GetClipboardData(13); // CF_UNICODETEXT
                            if let Ok(h) = handle {
                                let ptr = windows::Win32::System::Memory::GlobalLock(
                                    windows::Win32::Foundation::HGLOBAL(h.0),
                                ) as *const u16;
                                if !ptr.is_null() {
                                    let mut len = 0;
                                    while *ptr.add(len) != 0 {
                                        len += 1;
                                    }
                                    let slice = std::slice::from_raw_parts(ptr, len);
                                    let text = String::from_utf16_lossy(slice);
                                    if clipboard_mgr.push_clip(text) {
                                        target_clip_scroll = 0.0;
                                    }
                                    let _ = windows::Win32::System::Memory::GlobalUnlock(
                                        windows::Win32::Foundation::HGLOBAL(h.0),
                                    );
                                }
                            }
                            let _ = windows::Win32::System::DataExchange::CloseClipboard();
                        }
                    }
                    WM_TRAYICON => {
                        let event = (msg.lParam.0 & 0xffff) as u32;
                        if event == WM_RBUTTONUP {
                            tray.show_context_menu(window.hwnd);
                        } else if event == WM_LBUTTONUP {
                            current_state = if current_state == IslandState::Expanded {
                                IslandState::Compact
                            } else {
                                IslandState::Expanded
                            };
                        }
                    }
                    WM_COMMAND => {
                        let cmd_id = (msg.wParam.0 & 0xffff) as usize;
                        match cmd_id {
                            IDM_TOGGLE_EXPAND => {
                                current_state = if current_state == IslandState::Expanded {
                                    IslandState::Compact
                                } else {
                                    IslandState::Expanded
                                };
                            }
                            IDM_TUCK_AWAY => {
                                current_state = IslandState::Tucked;
                            }
                            IDM_TEST_WHATSAPP => {
                                activity_mgr.push_notification(NotificationPayload {
                                    id: 101,
                                    app_name: "WhatsApp".to_string(),
                                    title: "Sarah".to_string(),
                                    subtitle: "Hello messages in dynamic island!".to_string(),
                                    time_text: "now".to_string(),
                                    is_snip: false,
                                    is_call: false,
                                    is_device: false,
                                    image_preview: None,
                                    brand_color: [37, 211, 102, 255],
                                });
                            }
                            IDM_TEST_CALL => {
                                activity_mgr.push_notification(NotificationPayload {
                                    id: 102,
                                    app_name: "Calls".to_string(),
                                    title: "Me Jio".to_string(),
                                    subtitle: "Incoming Call • Phone Link".to_string(),
                                    time_text: "now".to_string(),
                                    is_snip: false,
                                    is_call: true,
                                    is_device: false,
                                    image_preview: None,
                                    brand_color: [48, 209, 88, 255],
                                });
                            }
                            IDM_TEST_SNIP => {
                                activity_mgr.push_notification(NotificationPayload {
                                    id: 103,
                                    app_name: "Snipping Tool".to_string(),
                                    title: "Snipping Tool".to_string(),
                                    subtitle: "Screenshot saved to clipboard".to_string(),
                                    time_text: "now".to_string(),
                                    is_snip: true,
                                    is_call: false,
                                    is_device: false,
                                    image_preview: None,
                                    brand_color: [100, 210, 255, 255],
                                });
                            }
                            IDM_TEST_DISCORD => {
                                activity_mgr.push_notification(NotificationPayload {
                                    id: 104,
                                    app_name: "Discord".to_string(),
                                    title: "biwi ji".to_string(),
                                    subtitle: "Its like youre screaming at meeee".to_string(),
                                    time_text: "now".to_string(),
                                    is_snip: false,
                                    is_call: false,
                                    is_device: false,
                                    image_preview: None,
                                    brand_color: [88, 101, 242, 255],
                                });
                            }
                            IDM_QUIT => {
                                SHOULD_QUIT.store(true, Ordering::SeqCst);
                            }
                            _ => {}
                        }
                    }
                    WM_MOUSEWHEEL => {
                        let delta = (msg.wParam.0 >> 16) as i16;
                        if current_state == IslandState::Compact {
                            let (vol, _) = volume_mgr.read();
                            let step = if delta > 0 { 4.0 } else { -4.0 };
                            let new_vol = (vol + step).clamp(0.0, 100.0);
                            volume_mgr.set_volume(new_vol);
                            volume_spring.target = new_vol;
                            is_volume_hud_active = true;
                            volume_hud_until = Instant::now() + std::time::Duration::from_millis(2500);
                        } else if current_state == IslandState::Expanded {
                            let (px, py, _) = window.get_cursor_relative();
                            let w_cur = spring_w.value;
                            let island_x_cur = ((WINDOW_WIDTH as f32) - w_cur) / 2.0;
                            let body_x = island_x_cur + 22.0;
                            let body_y = 44.0;
                            let body_w = w_cur - 44.0;
                            let body_h = 82.0;
                            let is_over_body = px >= body_x && px <= body_x + body_w && py >= body_y && py <= body_y + body_h;

                            if current_app == 1 && is_over_body {
                                // Selective condition: hovering over clipboard panel scrolls clipboard items smoothly, NOT switching apps!
                                let total_clips = clipboard_mgr.clips.len();
                                if total_clips > 2 {
                                    let max_scroll = (total_clips - 2) as f32;
                                    let dir = if delta < 0 { 1.0 } else { -1.0 };
                                    target_clip_scroll = (target_clip_scroll + dir).clamp(0.0, max_scroll);
                                }
                            } else if current_app == 2 && is_over_body {
                                // Hovering over notes panel: scroll notes vertically line-by-line!
                                let dir = if delta < 0 { 1.0 } else { -1.0 };
                                target_notes_scroll = (target_notes_scroll + dir).max(0.0);
                            } else {
                                let dir = if delta < 0 { 1.0 } else { -1.0 };
                                target_scroll_index += dir;
                                current_app = (((target_scroll_index.round() as i32 % 4) + 4) % 4) as usize;
                                if current_app != 2 && notes_focused {
                                    notes_focused = false;
                                    NOTES_FOCUSED.store(false, Ordering::SeqCst);
                                    let ex = windows::Win32::UI::WindowsAndMessaging::GetWindowLongW(window.hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE);
                                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowLongW(window.hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE, ex | (windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE.0 as i32));
                                }
                            }
                        }
                    }
                    WM_LBUTTONUP => {
                        let (px, py, _) = window.get_cursor_relative();
                        let w_cur = spring_w.value;
                        let island_x_cur = ((WINDOW_WIDTH as f32) - w_cur) / 2.0;

                        if current_state == IslandState::Compact
                            || current_state == IslandState::Rest
                            || current_state == IslandState::Tucked
                        {
                            let is_suppressed = suppress_expand_until.map_or(false, |until| Instant::now() < until);
                            if !is_suppressed {
                                if activity_mgr.active_notification.is_some() {
                                    activity_mgr.dismiss_notification();
                                } else {
                                    current_state = IslandState::Expanded;
                                    mascot.trigger_state(MascotState::IdleHappy, 1500);
                                }
                            }
                        } else if current_state == IslandState::Expanded {
                            if px >= island_x_cur + w_cur - 38.0
                                && px <= island_x_cur + w_cur - 16.0
                                && py >= 9.0
                                && py <= 31.0
                            {
                                tray.show_context_menu(window.hwnd);
                            } else if py >= spring_h.value - 32.0 && py <= spring_h.value {
                                // Bottom camera mode selector click with infinite slot wrapping
                                let cx = island_x_cur + w_cur / 2.0;
                                let center_slot = scroll_wheel_progress.round() as i32;
                                let spacing = 92.0;
                                let mut clicked_slot = None;
                                for offset in -3..=3 {
                                    let slot = center_slot + offset;
                                    let offset_dist = (slot as f32) - scroll_wheel_progress;
                                    let item_x = cx + offset_dist * spacing;
                                    if (px - item_x).abs() <= 38.0 {
                                        clicked_slot = Some(slot);
                                        break;
                                    }
                                }
                                if let Some(slot) = clicked_slot {
                                    target_scroll_index = slot as f32;
                                    current_app = (((slot % 4) + 4) % 4) as usize;
                                } else {
                                    let dir = if px > cx { 1.0 } else { -1.0 };
                                    target_scroll_index += dir;
                                    current_app = (((target_scroll_index.round() as i32 % 4) + 4) % 4) as usize;
                                }
                            } else if current_app == 0 {
                                let ctl_x = island_x_cur + w_cur - 22.0 - 50.0;
                                let ctrl_y = 44.0 + 28.0;
                                if (px - (ctl_x - 17.0)).abs() < 14.0 && (py - ctrl_y).abs() < 14.0 {
                                    gsmtc.send_command(MediaControlCmd::Toggle);
                                } else if (px - (ctl_x - 51.0)).abs() < 14.0 && (py - ctrl_y).abs() < 14.0 {
                                    gsmtc.send_command(MediaControlCmd::Previous);
                                } else if (px - (ctl_x + 17.0)).abs() < 14.0 && (py - ctrl_y).abs() < 14.0 {
                                    gsmtc.send_command(MediaControlCmd::Next);
                                } else if px < island_x_cur || px > island_x_cur + w_cur || py > spring_h.value {
                                    current_state = IslandState::Compact;
                                }
                            } else if current_app == 1 {
                                // Clipboard app: click row to copy & auto-collapse, or delete button
                                let body_x = island_x_cur + 22.0;
                                let body_y = 44.0;
                                let body_w = w_cur - 44.0;
                                let row_h = 34.0;
                                let pitch = 40.0;
                                let mut handled_clip = false;

                                for i in 0..clipboard_mgr.clips.len() {
                                    let row_y = body_y + (i as f32 - clip_scroll_progress) * pitch;
                                    if row_y + row_h < 36.0 || row_y > 124.0 {
                                        continue;
                                    }

                                    let del_x = body_x + body_w - 26.0;
                                    let del_y = row_y + 7.0;
                                    let del_size = 20.0;

                                    // Click delete button
                                    if px >= del_x && px <= del_x + del_size && py >= del_y && py <= del_y + del_size {
                                        clipboard_mgr.remove_clip(i);
                                        let total_clips = clipboard_mgr.clips.len();
                                        let max_scroll = if total_clips > 2 { (total_clips - 2) as f32 } else { 0.0 };
                                        target_clip_scroll = target_clip_scroll.min(max_scroll);
                                        handled_clip = true;
                                        break;
                                    }

                                    // Click row to copy and collapse to compact
                                    if py >= row_y && py <= row_y + row_h && px >= body_x && px <= body_x + body_w {
                                        if let Some(clip) = clipboard_mgr.clips.get(i) {
                                            ClipboardManager::copy_to_system_clipboard(&clip.text);
                                        }
                                        clip_copied_index = Some(i);
                                        clip_copied_banner_until = Some(Instant::now() + std::time::Duration::from_millis(1800));
                                        suppress_expand_until = Some(Instant::now() + std::time::Duration::from_millis(2000));
                                        current_state = IslandState::Compact;
                                        mascot.trigger_state(MascotState::IdleHappy, 1800);
                                        handled_clip = true;
                                        break;
                                    }
                                }

                                if !handled_clip && (px < island_x_cur || px > island_x_cur + w_cur || py > spring_h.value) {
                                    current_state = IslandState::Compact;
                                }
                            } else if current_app == 2 {
                                // Notes app: click inside to focus and take keyboard focus
                                let body_x = island_x_cur + 22.0;
                                let body_y = 44.0;
                                let body_w = w_cur - 44.0;
                                if px >= body_x && px <= body_x + body_w && py >= body_y && py <= body_y + 76.0 {
                                    notes_focused = true;
                                    NOTES_FOCUSED.store(true, Ordering::SeqCst);
                                    let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(window.hwnd);
                                    let _ = windows::Win32::UI::WindowsAndMessaging::BringWindowToTop(window.hwnd);
                                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetActiveWindow(window.hwnd);
                                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(window.hwnd);
                                    notes_cursor_pos = clipboard_mgr.note_text.len();
                                } else {
                                    if notes_focused {
                                        notes_focused = false;
                                        NOTES_FOCUSED.store(false, Ordering::SeqCst);
                                    }
                                    if px < island_x_cur || px > island_x_cur + w_cur || py > spring_h.value {
                                        current_state = IslandState::Compact;
                                    }
                                }
                            } else if current_app == 3 {
                                // Focus app: start/pause/stop
                                let body_x = island_x_cur + 22.0;
                                let body_y = 44.0;
                                let body_w = w_cur - 44.0;
                                if !focus_active {
                                    if px >= body_x && px <= body_x + body_w && py >= body_y + 40.0 && py <= body_y + 75.0 {
                                        focus_active = true;
                                        focus_paused = false;
                                        focus_end_time = Some(Instant::now() + std::time::Duration::from_secs(focus_chosen_sec));
                                        activity_mgr.update_focus(true);
                                        mascot.trigger_state(MascotState::FocusActive, 3000);
                                    } else if px < island_x_cur || px > island_x_cur + w_cur || py > spring_h.value {
                                        current_state = IslandState::Compact;
                                    }
                                } else {
                                    if px >= body_x + body_w - 120.0 && px <= body_x + body_w - 60.0 && py >= body_y + 15.0 && py <= body_y + 45.0 {
                                        if !focus_paused {
                                            focus_paused = true;
                                            if let Some(end) = focus_end_time {
                                                let now = Instant::now();
                                                focus_paused_remaining = if end > now { end - now } else { std::time::Duration::from_secs(0) };
                                            }
                                        } else {
                                            focus_paused = false;
                                            focus_end_time = Some(Instant::now() + focus_paused_remaining);
                                        }
                                    } else if px >= body_x + body_w - 55.0 && px <= body_x + body_w && py >= body_y + 15.0 && py <= body_y + 45.0 {
                                        focus_active = false;
                                        focus_paused = false;
                                        focus_end_time = None;
                                        activity_mgr.update_focus(false);
                                        mascot.trigger_state(MascotState::FocusFailed, 2500);
                                    } else if px < island_x_cur || px > island_x_cur + w_cur || py > spring_h.value {
                                        current_state = IslandState::Compact;
                                    }
                                }
                            } else if px < island_x_cur || px > island_x_cur + w_cur || py > spring_h.value {
                                current_state = IslandState::Compact;
                            }
                        }
                    }
                    windows::Win32::UI::WindowsAndMessaging::WM_CHAR => {
                        if notes_focused && current_state == IslandState::Expanded && current_app == 2 {
                            let ch = msg.wParam.0 as u32;
                            if ch == 8 {
                                // Backspace
                                if notes_cursor_pos > 0 && !clipboard_mgr.note_text.is_empty() {
                                    let char_indices: Vec<(usize, char)> = clipboard_mgr.note_text.char_indices().collect();
                                    if let Some(remove_idx) = char_indices.iter().rposition(|(idx, _)| *idx < notes_cursor_pos) {
                                        let (byte_pos, c) = char_indices[remove_idx];
                                        clipboard_mgr.note_text.drain(byte_pos..byte_pos + c.len_utf8());
                                        notes_cursor_pos = byte_pos;
                                        clipboard_mgr.save_note(clipboard_mgr.note_text.clone());
                                    }
                                }
                            } else if ch == 13 {
                                // Enter / Return
                                let pos = notes_cursor_pos.min(clipboard_mgr.note_text.len());
                                clipboard_mgr.note_text.insert(pos, '\n');
                                notes_cursor_pos = pos + 1;
                                clipboard_mgr.save_note(clipboard_mgr.note_text.clone());
                            } else if ch == 9 {
                                // Tab
                                let pos = notes_cursor_pos.min(clipboard_mgr.note_text.len());
                                clipboard_mgr.note_text.insert_str(pos, "  ");
                                notes_cursor_pos = pos + 2;
                                clipboard_mgr.save_note(clipboard_mgr.note_text.clone());
                            } else if ch == 27 {
                                // Escape
                                notes_focused = false;
                                NOTES_FOCUSED.store(false, Ordering::SeqCst);
                                let ex = windows::Win32::UI::WindowsAndMessaging::GetWindowLongW(window.hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE);
                                let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowLongW(window.hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE, ex | (windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE.0 as i32));
                            } else if ch >= 0x20 && ch != 0x7F {
                                if let Some(c) = char::from_u32(ch) {
                                    let pos = notes_cursor_pos.min(clipboard_mgr.note_text.len());
                                    clipboard_mgr.note_text.insert(pos, c);
                                    notes_cursor_pos = pos + c.len_utf8();
                                    clipboard_mgr.save_note(clipboard_mgr.note_text.clone());
                                }
                            }
                        }
                    }
                    windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN => {
                        if notes_focused && current_state == IslandState::Expanded && current_app == 2 {
                            let vk = msg.wParam.0 as usize;
                            let is_ctrl = windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) < 0; // VK_CONTROL

                            if is_ctrl && vk == 0x56 { // Ctrl + V
                                use windows::Win32::System::DataExchange::*;
                                use windows::Win32::System::Memory::*;
                                if OpenClipboard(window.hwnd).is_ok() {
                                        if let Ok(h) = GetClipboardData(13) { // CF_UNICODETEXT
                                            let ptr = GlobalLock(windows::Win32::Foundation::HGLOBAL(h.0)) as *const u16;
                                            if !ptr.is_null() {
                                                let mut len = 0;
                                                while *ptr.add(len) != 0 { len += 1; }
                                                let slice = std::slice::from_raw_parts(ptr, len);
                                                let text = String::from_utf16_lossy(slice);
                                                let pos = notes_cursor_pos.min(clipboard_mgr.note_text.len());
                                                clipboard_mgr.note_text.insert_str(pos, &text);
                                                notes_cursor_pos = pos + text.len();
                                                clipboard_mgr.save_note(clipboard_mgr.note_text.clone());
                                                let _ = GlobalUnlock(windows::Win32::Foundation::HGLOBAL(h.0));
                                            }
                                        }
                                        let _ = CloseClipboard();
                                    }
                            } else if is_ctrl && vk == 0x43 { // Ctrl + C
                                ClipboardManager::copy_to_system_clipboard(&clipboard_mgr.note_text);
                            } else if vk == 0x25 { // Left arrow
                                if notes_cursor_pos > 0 {
                                    let prev_boundary = clipboard_mgr.note_text[..notes_cursor_pos]
                                        .char_indices()
                                        .last()
                                        .map(|(i, _)| i)
                                        .unwrap_or(0);
                                    notes_cursor_pos = prev_boundary;
                                }
                            } else if vk == 0x27 { // Right arrow
                                if notes_cursor_pos < clipboard_mgr.note_text.len() {
                                    let next_boundary = clipboard_mgr.note_text[notes_cursor_pos..]
                                        .char_indices()
                                        .nth(1)
                                        .map(|(i, _)| notes_cursor_pos + i)
                                        .unwrap_or_else(|| clipboard_mgr.note_text.len());
                                    notes_cursor_pos = next_boundary;
                                }
                            } else if vk == 0x26 { // Up arrow
                                if notes_cursor_pos > 0 {
                                    let text = &clipboard_mgr.note_text;
                                    let before = &text[..notes_cursor_pos];
                                    if let Some(last_nl) = before.rfind('\n') {
                                        let col = notes_cursor_pos - (last_nl + 1);
                                        let prev_line_before = &text[..last_nl];
                                        let prev_start = prev_line_before.rfind('\n').map(|p| p + 1).unwrap_or(0);
                                        let prev_len = last_nl - prev_start;
                                        notes_cursor_pos = prev_start + col.min(prev_len);
                                    } else {
                                        notes_cursor_pos = 0;
                                    }
                                }
                            } else if vk == 0x28 { // Down arrow
                                let text = &clipboard_mgr.note_text;
                                if notes_cursor_pos < text.len() {
                                    let before = &text[..notes_cursor_pos];
                                    let line_start = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
                                    let col = notes_cursor_pos - line_start;
                                    let after = &text[notes_cursor_pos..];
                                    if let Some(next_nl) = after.find('\n') {
                                        let next_start = notes_cursor_pos + next_nl + 1;
                                        let next_line_after = &text[next_start..];
                                        let next_len = next_line_after.find('\n').unwrap_or(next_line_after.len());
                                        notes_cursor_pos = next_start + col.min(next_len);
                                    } else {
                                        notes_cursor_pos = text.len();
                                    }
                                }
                            } else if vk == 0x24 { // Home
                                notes_cursor_pos = 0;
                            } else if vk == 0x23 { // End
                                notes_cursor_pos = clipboard_mgr.note_text.len();
                            } else if vk == 0x2E { // Delete
                                if notes_cursor_pos < clipboard_mgr.note_text.len() {
                                    let char_indices: Vec<(usize, char)> = clipboard_mgr.note_text.char_indices().collect();
                                    if let Some(pos_idx) = char_indices.iter().position(|(idx, _)| *idx == notes_cursor_pos) {
                                        let (byte_pos, c) = char_indices[pos_idx];
                                        clipboard_mgr.note_text.drain(byte_pos..byte_pos + c.len_utf8());
                                        clipboard_mgr.save_note(clipboard_mgr.note_text.clone());
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        }

        if SHOULD_QUIT.load(Ordering::SeqCst) {
            break;
        }

        // Receive any async events from background workers
        while let Ok(media) = media_rx.try_recv() {
            is_dirty = true;
            let key_changed = media.session_key != latest_media.session_key;
            if key_changed {
                lyrics.update_track(&media.title, &media.artist, media.duration_ms, lyrics_tx.clone());

                // Fetch high-res official artwork from iTunes in background
                let t = media.title.clone();
                let a = media.artist.clone();
                let k = media.session_key.clone();
                let tx = art_tx.clone();
                std::thread::spawn(move || {
                    if let Some(bytes) = fetch_online_artwork(&t, &a) {
                        let _ = tx.send((k, bytes));
                    }
                });

                current_progress_base = media.progress_ms;
                progress_anchor_time = Instant::now();
            } else {
                let local_est = current_progress_base + progress_anchor_time.elapsed().as_millis() as u64;
                let diff = (media.progress_ms as i64 - local_est as i64).abs();
                if diff > 1200 {
                    current_progress_base = media.progress_ms;
                    progress_anchor_time = Instant::now();
                }
            }

            album_art_mgr.update(&media.session_key, media.thumbnail_bytes.as_deref());
            let was_playing = latest_media.is_playing;
            latest_media = media;
            activity_mgr.update_music(latest_media.is_playing);
            if latest_media.is_playing && (!was_playing || current_state == IslandState::Rest) {
                if current_state == IslandState::Rest {
                    current_state = IslandState::Compact;
                    leave_timer_start = None;
                }
            }
        }

        while let Ok(notif) = notif_rx.try_recv() {
            is_dirty = true;
            activity_mgr.push_notification(notif);
        }

        while let Ok((key, lines)) = lyrics_rx.try_recv() {
            is_dirty = true;
            lyrics.receive_loaded(key, lines);
        }

        while let Ok((key, art_bytes)) = art_rx.try_recv() {
            is_dirty = true;
            if key == latest_media.session_key {
                album_art_mgr.update_official_artwork(&key, &art_bytes);
            }
        }

        activity_mgr.check_notification_expiry();
        clipboard_mgr.flush_note_if_needed();

        if is_volume_hud_active && Instant::now() >= volume_hud_until {
            is_volume_hud_active = false;
            is_dirty = true;
        }

        let extrapolated_progress = if latest_media.is_playing {
            let p = current_progress_base + progress_anchor_time.elapsed().as_millis() as u64;
            if latest_media.duration_ms > 0 {
                p.min(latest_media.duration_ms)
            } else {
                p
            }
        } else {
            current_progress_base
        };

        if latest_media.is_playing {
            lyrics.tick(extrapolated_progress);
        }

        // Check Fullscreen Auto-Tuck
        let is_fs = fullscreen_detector.check_fullscreen(window.hwnd);
        if is_fs && !is_auto_tucked {
            is_auto_tucked = true;
            if current_state != IslandState::Tucked {
                previous_state = current_state;
                current_state = IslandState::Tucked;
            }
        } else if !is_fs && is_auto_tucked {
            is_auto_tucked = false;
            current_state = previous_state;
        }

        // Track Pointer & Proximity
        let (pointer_x, pointer_y, _inside_window) = window.get_cursor_relative();
        if (pointer_x - prev_pointer_x).abs() > 0.5 || (pointer_y - prev_pointer_y).abs() > 0.5 {
            prev_pointer_x = pointer_x;
            prev_pointer_y = pointer_y;
            last_cursor_move_time = Instant::now();
            mascot.notify_activity();
            is_dirty = true;
        }
        let now = Instant::now();

        // 1. Current island dimensions for hit testing
        let w_val = spring_w.value;
        let h_val = spring_h.value;
        let island_x = ((WINDOW_WIDTH as f32) - w_val) / 2.0;

        let is_over_main = if current_state == IslandState::Expanded {
            let exp_x = ((WINDOW_WIDTH as f32) - DIMS_EXPANDED.w) / 2.0;
            pointer_x >= exp_x - 14.0
                && pointer_x <= exp_x + DIMS_EXPANDED.w + 14.0
                && pointer_y >= 0.0
                && pointer_y <= (h_val.max(DIMS_EXPANDED.h) + 14.0)
        } else {
            pointer_x >= island_x - 8.0
                && pointer_x <= island_x + w_val + 8.0
                && pointer_y >= 0.0
                && pointer_y <= h_val + 8.0
        };

        // Update atomic bounds for native WM_NCHITTEST
        HIT_X.store(island_x as i32, Ordering::Relaxed);
        HIT_Y.store(0, Ordering::Relaxed);
        HIT_W.store(w_val as i32, Ordering::Relaxed);
        HIT_H.store(h_val as i32, Ordering::Relaxed);

        // 2. Hover state transitions (executed BEFORE computing targets)
        if current_state == IslandState::Rest {
            let has_active_content = latest_media.is_playing
                || focus_active
                || activity_mgr.active_notification.is_some()
                || is_volume_hud_active;
            if is_over_main || has_active_content {
                current_state = IslandState::Compact;
                leave_timer_start = None;
            }
        } else if current_state == IslandState::Compact {
            let has_active_content = latest_media.is_playing
                || focus_active
                || activity_mgr.active_notification.is_some()
                || is_volume_hud_active;
            if !is_over_main && !has_active_content {
                if let Some(start) = leave_timer_start {
                    if now.duration_since(start).as_millis() >= 220 {
                        current_state = IslandState::Rest;
                        leave_timer_start = None;
                    }
                } else {
                    leave_timer_start = Some(now);
                }
            } else {
                leave_timer_start = None;
            }
        } else if current_state == IslandState::Expanded {
            if !is_over_main {
                if let Some(start) = expanded_leave_timer {
                    if now.duration_since(start).as_millis() >= 220 {
                        current_state = IslandState::Compact;
                        expanded_leave_timer = None;
                    }
                } else {
                    expanded_leave_timer = Some(now);
                }
            } else {
                expanded_leave_timer = None;
            }
        } else if current_state == IslandState::Tucked {
            if is_over_main {
                current_state = IslandState::Compact;
            }
        }

        if current_state != prev_state {
            is_dirty = true;
            prev_state = current_state;
        }

        // 3. Target Dimensions State Machine (computed from newly resolved state)
        let target_dims = match current_state {
            IslandState::Rest => DIMS_REST,
            IslandState::Compact => {
                let (primary, _) = activity_mgr.get_activities();
                let mut w = DIMS_COMPACT.w;
                if is_volume_hud_active {
                    w = 300.0;
                } else if primary == Some(ActivityType::Music) && lyrics.has_lyrics() {
                    w = lyrics.get_compact_width();
                } else if primary == Some(ActivityType::Focus) {
                    w = 270.0;
                }
                IslandDims {
                    w,
                    h: DIMS_COMPACT.h,
                    r: DIMS_COMPACT.r,
                }
            }
            IslandState::Expanded => DIMS_EXPANDED,
            IslandState::Tucked => DIMS_TUCKED,
        };

        spring_w.target = target_dims.w;
        spring_h.target = target_dims.h;
        spring_r.target = target_dims.r;

        // Secondary bubble target
        let (primary_act, sec_act) = activity_mgr.get_activities();
        if current_state == IslandState::Compact && sec_act.is_some() {
            sec_spring_w.target = DIMS_SECONDARY.w;
            sec_spring_h.target = DIMS_SECONDARY.h;
            sec_spring_r.target = DIMS_SECONDARY.r;
        } else {
            sec_spring_w.target = 0.0;
            sec_spring_h.target = 0.0;
        }

        let dt = (now.duration_since(last_frame_time).as_secs_f32()).min(0.04);
        last_frame_time = now;

        // 4. Physics Step (with 240Hz sub-stepping & Apple asymmetric dynamics)
        let s1 = spring_w.update(dt);
        let s2 = spring_h.update(dt);
        let s3 = spring_r.update(dt);
        let s4 = sec_spring_w.update(dt);
        let s5 = sec_spring_h.update(dt);
        let s6 = sec_spring_r.update(dt);
        let s7 = sec_spring_x.update(dt);
        let s8 = volume_spring.update(dt);
        let has_secondary = sec_act.is_some() || sec_spring_w.value > 0.5;
        let springs_moving = s1 || s2 || s3 || s8
            || (has_secondary && (s4 || s5 || s6 || s7))
            || (spring_w.value - spring_w.target).abs() > 0.15
            || (spring_h.value - spring_h.target).abs() > 0.15
            || (spring_r.value - spring_r.target).abs() > 0.15
            || (has_secondary && (sec_spring_w.value - sec_spring_w.target).abs() > 0.15)
            || (has_secondary && (sec_spring_x.value - sec_spring_x.target).abs() > 0.15);

        let particles_moving = particles.update();

        // Synchronize Expanded window style & activation state
        let is_exp = current_state == IslandState::Expanded;
        if is_exp != IS_EXPANDED.load(Ordering::Relaxed) {
            IS_EXPANDED.store(is_exp, Ordering::SeqCst);
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::*;
                let ex = GetWindowLongW(window.hwnd, GWL_EXSTYLE);
                if is_exp {
                    let _ = SetWindowLongW(window.hwnd, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as i32));
                } else {
                    notes_focused = false;
                    NOTES_FOCUSED.store(false, Ordering::SeqCst);
                    let _ = SetWindowLongW(window.hwnd, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 as i32));
                }
                let _ = SetWindowPos(
                    window.hwnd,
                    None,
                    0, 0, 0, 0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
        }

        let mut ui_animating = false;

        // Carousel progression smoothing with fluid 240Hz physical spring
        scroll_spring.target = target_scroll_index;
        if scroll_spring.update(dt) || (target_scroll_index - scroll_spring.value).abs() > 0.002 {
            ui_animating = true;
        }
        scroll_wheel_progress = scroll_spring.value;

        // Clipboard smooth scroll progression
        clip_spring.target = target_clip_scroll;
        if clip_spring.update(dt) || (target_clip_scroll - clip_spring.value).abs() > 0.002 {
            ui_animating = true;
        }
        clip_scroll_progress = clip_spring.value;

        // Notes smooth scroll progression
        notes_spring.target = target_notes_scroll;
        if notes_spring.update(dt) || (target_notes_scroll - notes_spring.value).abs() > 0.002 {
            ui_animating = true;
        }
        notes_scroll_progress = notes_spring.value;

        // Focus timer tick
        let mut focus_left_ms = 0u64;
        let focus_total_ms = focus_chosen_sec * 1000;
        if focus_active {
            if focus_paused {
                focus_left_ms = focus_paused_remaining.as_millis() as u64;
            } else if let Some(end_time) = focus_end_time {
                if now >= end_time {
                    focus_active = false;
                    focus_end_time = None;
                    activity_mgr.update_focus(false);
                    activity_mgr.trigger_focus_celebration();
                    mascot.trigger_state(MascotState::FocusComplete, 6000);
                    particles.launch_confetti((WINDOW_WIDTH as f32) / 2.0, 28.0);
                } else {
                    focus_left_ms = end_time.duration_since(now).as_millis() as u64;
                }
            }
        }

        let cursor_active = now.duration_since(last_cursor_move_time).as_millis() < 250
            && (current_state == IslandState::Rest || current_state == IslandState::Compact);

        if springs_moving
            || particles_moving
            || ui_animating
            || cursor_active
            || leave_timer_start.is_some()
            || expanded_leave_timer.is_some()
            || (focus_active && !focus_paused)
            || is_volume_hud_active
            || activity_mgr.active_notification.is_some()
            || (latest_media.is_playing && current_state != IslandState::Tucked)
        {
            is_dirty = true;
        }

        // 9. Render Frame (Only when visual state is dirty)
        if is_dirty {
            canvas.clear();

            // 1. Atmosphere Drop Shadow (True Gaussian blur matching MyIsland index.html lines 2988-2996)
        let (shadow_blur, shadow_dy) = match current_state {
            IslandState::Expanded => (34.0, 6.0),
            IslandState::Tucked => (4.0, 1.0),
            _ => (18.0, 6.0),
        };

        shadow_renderer.render_shadow(
            &mut canvas.pixmap.as_mut(),
            &build_island_path,
            island_x,
            w_val,
            h_val,
            spring_r.value,
            shadow_blur,
            shadow_dy,
            false,
            springs_moving,
        );

        // Island Squircle Geometry Path
        let island_path = build_island_path(island_x, 0.0, w_val, h_val, spring_r.value);

        // Pitch Black Surface
        if let Some(ref path) = island_path {
            let mut body_paint = tiny_skia::Paint::default();
            body_paint.anti_alias = true;
            body_paint.set_color_rgba8(0, 0, 0, 255);
            canvas.pixmap.fill_path(
                path,
                &body_paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );

            // Subtle flame ambient floor glow (MyIsland index.html lines 3005-3028)
            let audio_levels_peek = audio_capture.get_levels();
            if current_state == IslandState::Expanded && current_app == 0 && latest_media.is_playing {
                let raw_energy = if audio_levels_peek.is_hearing {
                    audio_levels_peek.levels.iter().cloned().fold(0.0f32, f32::max)
                } else {
                    0.25
                };
                smoothed_flame_energy += (raw_energy - smoothed_flame_energy) * 0.04;

                let t_ms = start_instant.elapsed().as_millis() as f32;
                let flame_pulse = 0.5 + 0.5 * (t_ms / 1200.0).sin();
                let flame_height = (h_val * 0.62).min(38.0 + smoothed_flame_energy * 42.0 + flame_pulse * 12.0);
                let flame_alpha = (0.08 + smoothed_flame_energy * 0.16).min(0.26);

                let [cr, cg, cb] = album_art_mgr.dominant_color;
                let stops = vec![
                    tiny_skia::GradientStop::new(0.0, tiny_skia::Color::from_rgba8(cr, cg, cb, 0)),
                    tiny_skia::GradientStop::new(0.4, tiny_skia::Color::from_rgba8(cr, cg, cb, (flame_alpha * 0.2 * 255.0) as u8)),
                    tiny_skia::GradientStop::new(0.75, tiny_skia::Color::from_rgba8(cr, cg, cb, (flame_alpha * 0.6 * 255.0) as u8)),
                    tiny_skia::GradientStop::new(1.0, tiny_skia::Color::from_rgba8(cr, cg, cb, (flame_alpha * 255.0) as u8)),
                ];

                if let Some(shader) = tiny_skia::LinearGradient::new(
                    tiny_skia::Point::from_xy(0.0, h_val - flame_height),
                    tiny_skia::Point::from_xy(0.0, h_val),
                    stops,
                    tiny_skia::SpreadMode::Pad,
                    Transform::identity(),
                ) {
                    let mut flame_paint = tiny_skia::Paint::default();
                    flame_paint.anti_alias = true;
                    flame_paint.shader = shader;
                    canvas.pixmap.fill_path(path, &flame_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                }
            } else if !latest_media.is_playing {
                smoothed_flame_energy = 0.0;
            }
        }

        // Secondary Pill (Dual Activity)
        let sec_w = sec_spring_w.value;
        let sec_h = sec_spring_h.value;
        let target_sec_x = if current_state == IslandState::Compact && sec_act.is_some() {
            island_x + w_val + SECONDARY_GAP
        } else {
            island_x + w_val - sec_w.max(20.0)
        };
        sec_spring_x.target = target_sec_x;
        let sec_x = sec_spring_x.value;

        if current_state != IslandState::Expanded && current_state != IslandState::Tucked && sec_w > 1.5 && sec_h > 1.5 {
            SEC_HIT_X.store(sec_x as i32, Ordering::Relaxed);
            SEC_HIT_W.store(sec_w as i32, Ordering::Relaxed);
            SEC_HIT_H.store(sec_h as i32, Ordering::Relaxed);

            // Secondary Pill Drop Shadow (MyIsland line 3090: blur 18, offset Y 6)
            sec_shadow_renderer.render_shadow(
                &mut canvas.pixmap.as_mut(),
                &build_island_path,
                sec_x,
                sec_w,
                sec_h,
                sec_spring_r.value,
                18.0,
                6.0,
                true,
                springs_moving,
            );

            let gap = sec_x - (island_x + w_val);

            // Organic Liquid Bridge
            if let Some(bridge) = build_liquid_bridge_path(island_x, w_val, sec_x, h_val, sec_h, gap) {
                let mut bridge_paint = tiny_skia::Paint::default();
                bridge_paint.anti_alias = true;
                bridge_paint.set_color_rgba8(0, 0, 0, 255);
                canvas.pixmap.fill_path(&bridge, &bridge_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }

            // Secondary Body
            if let Some(sec_path) = build_island_path(sec_x, 0.0, sec_w, sec_h, sec_spring_r.value) {
                let mut sec_paint = tiny_skia::Paint::default();
                sec_paint.anti_alias = true;
                sec_paint.set_color_rgba8(0, 0, 0, 255);
                canvas.pixmap.fill_path(&sec_path, &sec_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);

                let mut stroke_paint = tiny_skia::Paint::default();
                stroke_paint.anti_alias = true;
                stroke_paint.set_color_rgba8(255, 255, 255, 28);
                let stroke_1 = tiny_skia::Stroke { width: 1.0, ..Default::default() };
                canvas.pixmap.stroke_path(&sec_path, &stroke_paint, &stroke_1, Transform::identity(), None);
            }
        } else {
            SEC_HIT_W.store(0, Ordering::Relaxed);
        }

        let t_sec = start_instant.elapsed().as_secs_f32();
        let (vol, muted) = volume_mgr.read();
        let audio_levels = audio_capture.get_levels();

        // Render Active Mode Content
        let mut render_media = latest_media.clone();
        render_media.progress_ms = extrapolated_progress;

        if let Some(until) = clip_copied_banner_until {
            if now >= until {
                clip_copied_banner_until = None;
                clip_copied_index = None;
            }
        }

        let cursor_moved = notes_cursor_pos != prev_notes_cursor_pos;
        prev_notes_cursor_pos = notes_cursor_pos;

        // Render Active Mode Content with true overflow:hidden squircle clipping
        canvas.begin_island_content();

        if current_state == IslandState::Expanded {
            let anim = ExpandedRenderer::render(
                &mut canvas,
                &mut mascot,
                &album_art_mgr,
                island_x,
                w_val,
                h_val,
                current_app,
                scroll_wheel_progress,
                pointer_x,
                pointer_y,
                &render_media,
                &lyrics,
                &clipboard_mgr,
                focus_active,
                focus_paused,
                focus_left_ms,
                focus_total_ms,
                focus_chosen_sec,
                notes_focused,
                notes_cursor_pos,
                cursor_moved,
                clip_copied_index,
                clip_scroll_progress,
                &mut target_notes_scroll,
                notes_scroll_progress,
            );
            if anim {
                ui_animating = true;
            }
        } else if current_state == IslandState::Compact || current_state == IslandState::Rest {
            let is_rest = current_state == IslandState::Rest;
            let anim = CompactRenderer::render(
                &mut canvas,
                &mut mascot,
                &mut visualizer,
                &album_art_mgr,
                island_x,
                w_val,
                h_val,
                pointer_x,
                pointer_y,
                primary_act,
                latest_media.is_playing,
                focus_active,
                focus_left_ms,
                focus_total_ms,
                &lyrics,
                activity_mgr.active_notification.as_ref(),
                is_volume_hud_active,
                vol,
                muted,
                Some(&audio_levels.levels),
                audio_levels.is_hearing,
                t_sec,
                clip_copied_banner_until,
                is_rest,
            );
            if anim {
                ui_animating = true;
            }
        }

        canvas.end_island_content(island_path.as_ref());

        // Apple Keyline Outline on top of clipped content
        if let Some(ref path) = island_path {
            let mut stroke_paint = tiny_skia::Paint::default();
            stroke_paint.anti_alias = true;
            stroke_paint.set_color_rgba8(255, 255, 255, 28);
            let stroke_1 = tiny_skia::Stroke { width: 1.0, ..Default::default() };
            canvas.pixmap.stroke_path(
                path,
                &stroke_paint,
                &stroke_1,
                Transform::identity(),
                None,
            );
        }

        // Draw Confetti Particles
        for p in &particles.particles {
            let mut paint = tiny_skia::Paint::default();
            paint.anti_alias = true;
            let a = (p.alpha.clamp(0.0, 1.0) * 255.0) as u8;
            paint.set_color_rgba8(p.color[0], p.color[1], p.color[2], a);

            let t = Transform::from_translate(p.x, p.y).post_rotate(p.rot.to_degrees());
            let mut pb = PathBuilder::new();
            pb.push_rect(tiny_skia::Rect::from_xywh(-p.w / 2.0, -p.h / 2.0, p.w, p.h).unwrap());
            if let Some(path) = pb.finish() {
                canvas.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, t, None);
            }
        }

        // Present Frame to Win32 layered alpha window
        canvas.present(window.hwnd, window.x, window.y);
        is_dirty = false;
    }

        // ZERO-IDLE RENDER GATING:
        // Run at buttery smooth 120 FPS while springs are moving,
        // 60 FPS during active UI animation, and sleep at 0.0% CPU in kernel wait when settled!
        let needs_next_frame = springs_moving
            || leave_timer_start.is_some()
            || expanded_leave_timer.is_some()
            || particles_moving
            || ui_animating
            || cursor_active
            || (focus_active && !focus_paused)
            || activity_mgr.active_notification.is_some()
            || is_volume_hud_active
            || (latest_media.is_playing && current_state != IslandState::Tucked);

        if needs_next_frame {
            is_dirty = true;
            let dwm_res = unsafe { windows::Win32::Graphics::Dwm::DwmFlush() };
            if dwm_res.is_err() {
                let sleep_ms = if springs_moving || particles_moving { 8 } else { 16 };
                std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
            }
        } else {
            let now = Instant::now();
            let next_event = mascot.get_next_event_time();
            let max_idle_sleep = match current_state {
                IslandState::Rest => 120,
                IslandState::Tucked => 250,
                _ => 30,
            };
            let delay_ms = if next_event > now {
                (next_event.duration_since(now).as_millis() as u32).clamp(16, max_idle_sleep)
            } else {
                is_dirty = true;
                16
            };

            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::*;
                let _ = MsgWaitForMultipleObjectsEx(
                    None,
                    delay_ms,
                    QS_ALLINPUT,
                    MWMO_ALERTABLE,
                );
            }
        }
    }

    clipboard_mgr.flush_note();

    unsafe {
        let _ = timeEndPeriod(1);
    }

    Ok(())
}
