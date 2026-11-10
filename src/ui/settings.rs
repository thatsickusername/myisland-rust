use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

use tiny_skia::{Color, FillRule, LineCap, Paint, PathBuilder, Stroke, Transform};
use windows::core::w;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromPoint,
    ScreenToClient, UpdateWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_BACK, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::platform::sports::{SportsConfig, TeamSubscription};
use crate::render::canvas::LayeredCanvas;

use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

// Window metrics
const WIN_WIDTH: i32 = 820;
const WIN_HEIGHT: i32 = 560;
const SIDEBAR_WIDTH: f32 = 210.0;
const CORNER_RADIUS: f32 = 16.0;

// Custom Messages
const WM_TEAMS_LOADED: u32 = WM_USER + 1;

// Sidebar Tabs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsTab {
    General,
    Appearance,
    Music,
    Focus,
    Notifications,
    Sports,
    Habits,
}

const TABS: [(SettingsTab, &str, (u8, u8, u8)); 7] = [
    (SettingsTab::General, "General", (94, 94, 98)),               // Gray
    (SettingsTab::Appearance, "Appearance", (88, 86, 214)),       // Indigo
    (SettingsTab::Music, "Music & Lyrics", (255, 45, 85)),         // Pink
    (SettingsTab::Focus, "Focus Timer", (255, 149, 0)),            // Orange
    (SettingsTab::Notifications, "Notifications", (255, 59, 48)), // Red
    (SettingsTab::Sports, "Sports", (52, 199, 89)),               // Green
    (SettingsTab::Habits, "Habits", (48, 176, 199)),               // Teal
];

// Unified App Settings
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct GeneralSettings {
    pub start_with_windows: bool,
    pub notch_size: usize, // 0: Small (85%), 1: Medium (100%), 2: Large (115%)
    #[serde(default)]
    pub minimal_notch: bool,
    pub mascot_enabled: bool,
    #[serde(default = "default_true")]
    pub haptic_feedback: bool,
    pub hide_in_fullscreen: bool,
}

fn default_true() -> bool {
    true
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            notch_size: 1, // Medium (100%)
            minimal_notch: false,
            mascot_enabled: true,
            haptic_feedback: true,
            hide_in_fullscreen: true,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct AppearanceSettings {
    pub theme_mode: usize, // 0: Dark, 1: System, 2: Light
    pub accent_color: usize, // 0: Green, 1: Blue, 2: Orange, 3: Purple
    pub shadow_blur: bool,
    pub translucent_acrylic: bool,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme_mode: 0,
            accent_color: 0,
            shadow_blur: true,
            translucent_acrylic: true,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MusicSettings {
    pub synchronized_lyrics: bool,
    pub audio_visualizer: bool,
    pub preferred_player: usize, // 0: Auto, 1: Spotify, 2: Apple Music, 3: Web
}

impl Default for MusicSettings {
    fn default() -> Self {
        Self {
            synchronized_lyrics: true,
            audio_visualizer: true,
            preferred_player: 0,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct FocusSettings {
    pub default_duration: usize, // 0: 15m, 1: 25m, 2: 45m, 3: 60m
    pub celebration_confetti: bool,
    pub break_reminders: bool,
}

impl Default for FocusSettings {
    fn default() -> Self {
        Self {
            default_duration: 1, // 25m
            celebration_confetti: true,
            break_reminders: true,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct NotificationSettings {
    pub intercept_toasts: bool,
    pub dismiss_system_toast: bool,
    pub screenshot_preview: bool,
    pub call_alerts: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            intercept_toasts: true,
            dismiss_system_toast: true,
            screenshot_preview: true,
            call_alerts: true,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct HabitsSettings {
    pub hydration_tracker: bool,
    pub stand_and_stretch: bool,
    pub eye_rest: bool,
}

impl Default for HabitsSettings {
    fn default() -> Self {
        Self {
            hydration_tracker: true,
            stand_and_stretch: false,
            eye_rest: true,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct AppSettings {
    #[serde(default)]
    pub general: GeneralSettings,
    #[serde(default)]
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub music: MusicSettings,
    #[serde(default)]
    pub focus: FocusSettings,
    #[serde(default)]
    pub notifications: NotificationSettings,
    #[serde(default)]
    pub habits: HabitsSettings,
    #[serde(default)]
    pub subscriptions: Vec<TeamSubscription>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            general: GeneralSettings::default(),
            appearance: AppearanceSettings::default(),
            music: MusicSettings::default(),
            focus: FocusSettings::default(),
            notifications: NotificationSettings::default(),
            habits: HabitsSettings::default(),
            subscriptions: vec![TeamSubscription {
                team_abbr: "ARS".to_string(),
                league_endpoint: "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.1/scoreboard".to_string(),
            }],
        }
    }
}

const SOCCER_ENDPOINTS: &[&str] = &[
    "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.1/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.fa/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.league_cup/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/esp.1/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/esp.copa_del_rey/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/uefa.champions/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/uefa.europa/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/ger.1/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/ger.dfb_pokal/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/ita.1/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/ita.coppa_italia/scoreboard",
    "https://site.api.espn.com/apis/site/v2/sports/soccer/fra.1/scoreboard",
];
const NFL_ENDPOINTS: &[&str] = &["https://site.api.espn.com/apis/site/v2/sports/football/nfl/scoreboard"];
const NBA_ENDPOINTS: &[&str] = &["https://site.api.espn.com/apis/site/v2/sports/basketball/nba/scoreboard"];
const MLB_ENDPOINTS: &[&str] = &["https://site.api.espn.com/apis/site/v2/sports/baseball/mlb/scoreboard"];
const NHL_ENDPOINTS: &[&str] = &["https://site.api.espn.com/apis/site/v2/sports/hockey/nhl/scoreboard"];

#[derive(Clone, Debug)]
struct TeamInfo {
    display_name: String,
    abbr: String,
    sport_category: String,
}

// Interactive element hit bounds
#[derive(Clone, Debug)]
struct ClickTarget {
    id: u32,
    rect: (f32, f32, f32, f32), // x, y, width, height
}

struct DialogState {
    canvas: LayeredCanvas,
    settings: AppSettings,
    active_tab: SettingsTab,
    cursor_pos: POINT,
    hovered_id: Option<u32>,

    // Sports tab live state
    search_query: String,
    search_focused: bool,
    available_teams: Arc<Mutex<Vec<TeamInfo>>>,
    filtered_teams: Vec<TeamInfo>,

    // Layout interactive targets collected during each paint
    targets: Vec<ClickTarget>,

    // Window position
    window_x: i32,
    window_y: i32,
}

// Element ID constants
const ID_CLOSE: u32 = 1;
const ID_TAB_BASE: u32 = 10; // 10..16
const ID_SPO_SEARCH: u32 = 600;
const ID_SPO_ADD_BASE: u32 = 610; // 610..629
const ID_SPO_DEL_BASE: u32 = 650; // 650..679

fn config_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("MyIsland"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn load_app_settings() -> AppSettings {
    let dir = config_dir();
    let unified_path = dir.join("settings.json");
    if let Ok(data) = std::fs::read_to_string(&unified_path) {
        if let Ok(settings) = serde_json::from_str::<AppSettings>(&data) {
            return settings;
        }
    }

    // Fallback: migrate from legacy sports_settings.json
    let sports_path = dir.join("sports_settings.json");
    let mut settings = AppSettings::default();
    if let Ok(data) = std::fs::read_to_string(&sports_path) {
        if let Ok(cfg) = serde_json::from_str::<SportsConfig>(&data) {
            settings.subscriptions = cfg.subscriptions;
        }
    }
    settings
}

fn set_startup_registry(enabled: bool) {
    let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let app_name = w!("MyIsland");

    unsafe {
        let mut hkey = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey,
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        )
        .is_ok()
        {
            if enabled {
                if let Ok(exe_path) = std::env::current_exe() {
                    let mut path_str = exe_path.to_string_lossy().to_string();
                    if !path_str.starts_with('"') {
                        path_str = format!("\"{}\"", path_str);
                    }
                    let wide: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
                    let byte_len = (wide.len() * 2) as u32;
                    let _ = RegSetValueExW(
                        hkey,
                        app_name,
                        0,
                        REG_SZ,
                        Some(std::slice::from_raw_parts(wide.as_ptr() as *const u8, byte_len as usize)),
                    );
                }
            } else {
                let _ = RegDeleteValueW(hkey, app_name);
            }
            let _ = RegCloseKey(hkey);
        }
    }
}

pub fn save_app_settings(settings: &AppSettings) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    set_startup_registry(settings.general.start_with_windows);
    crate::platform::haptics::set_haptics_enabled(settings.general.haptic_feedback);
    let s_clone = settings.clone();
    std::thread::spawn(move || {
        let dir = config_dir();
        if let Ok(json) = serde_json::to_string_pretty(&s_clone) {
            let _ = std::fs::write(dir.join("settings.json"), &json);
        }
        // Mirror sports subscriptions back to sports_settings.json for backwards compatibility
        let sports_cfg = SportsConfig {
            subscriptions: s_clone.subscriptions.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&sports_cfg) {
            let _ = std::fs::write(dir.join("sports_settings.json"), json);
        }
    });
}

pub fn open_sports_settings() {
    std::thread::spawn(|| unsafe {
        let h_instance = match GetModuleHandleW(None) {
            Ok(module) => module,
            Err(_) => return,
        };
        let class_name = w!("MyIslandSportsSettings");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(settings_wnd_proc),
            hInstance: h_instance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };

        let _ = RegisterClassW(&wc);

        // Calculate screen center
        let mut pt = POINT { x: 0, y: 0 };
        let _ = GetCursorPos(&mut pt);
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let (win_x, win_y) = if GetMonitorInfoW(monitor, &mut mi).as_bool() {
            let cx = mi.rcWork.left + (mi.rcWork.right - mi.rcWork.left - WIN_WIDTH) / 2;
            let cy = mi.rcWork.top + (mi.rcWork.bottom - mi.rcWork.top - WIN_HEIGHT) / 2;
            (cx, cy)
        } else {
            (100, 100)
        };

        let settings = load_app_settings();
        crate::platform::haptics::set_haptics_enabled(settings.general.haptic_feedback);
        let available_teams = Arc::new(Mutex::new(Vec::new()));

        // Create window with borderless popup style and alpha composition capability
        let ex_style = WS_EX_LAYERED | WS_EX_NOREDIRECTIONBITMAP;
        let style = WS_POPUP | WS_CLIPCHILDREN;

        let hwnd = match CreateWindowExW(
            ex_style,
            class_name,
            w!("MyIsland Settings"),
            style,
            win_x,
            win_y,
            WIN_WIDTH,
            WIN_HEIGHT,
            None,
            None,
            h_instance,
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(_) => return,
        };

        // Enable DWM immersive dark mode
        let dark_mode: BOOL = true.into();
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const BOOL as *const _,
            std::mem::size_of::<BOOL>() as u32,
        );

        let canvas = match LayeredCanvas::new(WIN_WIDTH, WIN_HEIGHT, hwnd) {
            Ok(c) => c,
            Err(_) => {
                let _ = DestroyWindow(hwnd);
                return;
            }
        };

        let state = Box::new(DialogState {
            canvas,
            settings,
            active_tab: SettingsTab::General,
            cursor_pos: POINT { x: 0, y: 0 },
            hovered_id: None,
            search_query: String::new(),
            search_focused: false,
            available_teams: available_teams.clone(),
            filtered_teams: Vec::new(),
            targets: Vec::with_capacity(64),
            window_x: win_x,
            window_y: win_y,
        });

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);

        fetch_popular_teams(hwnd, available_teams);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

fn fetch_popular_teams(hwnd: HWND, cache: Arc<Mutex<Vec<TeamInfo>>>) {
    let hwnd_value = hwnd.0 as isize;
    std::thread::spawn(move || {
        let leagues = [
            ("soccer", "soccer/eng.1"),
            ("soccer", "soccer/esp.1"),
            ("soccer", "soccer/uefa.champions"),
            ("soccer", "soccer/ger.1"),
            ("soccer", "soccer/ita.1"),
            ("nfl", "football/nfl"),
            ("nba", "basketball/nba"),
            ("mlb", "baseball/mlb"),
            ("nhl", "hockey/nhl"),
        ];

        for (category, league_path) in leagues {
            let url = format!("https://site.api.espn.com/apis/site/v2/sports/{}/teams", league_path);

            if let Ok(res) = ureq::get(&url).set("User-Agent", "MyIsland/0.1").timeout(std::time::Duration::from_secs(5)).call() {
                if let Ok(json) = res.into_json::<serde_json::Value>() {
                    if let Some(sports) = json.get("sports").and_then(|s| s.as_array()) {
                        if let Some(leagues) = sports.first().and_then(|s| s.get("leagues")).and_then(|l| l.as_array()) {
                            if let Some(teams) = leagues.first().and_then(|l| l.get("teams")).and_then(|t| t.as_array()) {
                                let mut new_teams = Vec::new();
                                for t in teams {
                                    if let Some(team) = t.get("team") {
                                        let abbr = team.get("abbreviation").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        let name = team.get("displayName").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        if !abbr.is_empty() && !name.is_empty() {
                                            new_teams.push(TeamInfo {
                                                display_name: name,
                                                abbr,
                                                sport_category: category.to_string(),
                                            });
                                        }
                                    }
                                }
                                if let Ok(mut lock) = cache.lock() {
                                    lock.extend(new_teams);
                                }
                            }
                        }
                    }
                }
            }
        }

        unsafe {
            let hwnd = HWND(hwnd_value as *mut core::ffi::c_void);
            let _ = PostMessageW(hwnd, WM_TEAMS_LOADED, WPARAM(0), LPARAM(0));
        }
    });
}

// ---------------------------------------------------------------------------
// Window Procedure & Event Dispatch
// ---------------------------------------------------------------------------
unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;

    match msg {
        WM_NCHITTEST => {
            let screen_x = (lparam.0 & 0xFFFF) as i16 as i32;
            let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;

            let mut pt = POINT { x: screen_x, y: screen_y };
            let _ = ScreenToClient(hwnd, &mut pt);

            // Traffic light close button at (20, 20), radius 6px
            let dx = pt.x - 20;
            let dy = pt.y - 20;
            if dx * dx + dy * dy <= 12 * 12 {
                return LRESULT(HTCLIENT as isize);
            }

            // Top drag region (title bar & sidebar drag handle)
            if pt.y >= 0 && pt.y < 46 {
                // If over an interactive tab in sidebar or search field in sports, allow client interaction
                if (pt.x as f32) < SIDEBAR_WIDTH && pt.y >= 46 {
                    return LRESULT(HTCLIENT as isize);
                }
                return LRESULT(HTCAPTION as isize);
            }

            LRESULT(HTCLIENT as isize)
        }

        WM_MOUSEMOVE => {
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let state = &mut *state_ptr;

            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            state.cursor_pos = POINT { x: x as i32, y: y as i32 };

            let mut new_hover = None;
            for target in &state.targets {
                let (tx, ty, tw, th) = target.rect;
                if x >= tx && x <= tx + tw && y >= ty && y <= ty + th {
                    new_hover = Some(target.id);
                    break;
                }
            }

            if new_hover != state.hovered_id {
                state.hovered_id = new_hover;
                let _ = InvalidateRect(hwnd, None, BOOL::from(false));
            }
            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let state = &mut *state_ptr;

            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

            // Close button check
            let dx = x - 20.0;
            let dy = y - 20.0;
            if dx * dx + dy * dy <= 14.0 * 14.0 {
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }

            // Hit test against interactive targets
            let mut clicked_id = None;
            for target in &state.targets {
                let (tx, ty, tw, th) = target.rect;
                if x >= tx && x <= tx + tw && y >= ty && y <= ty + th {
                    clicked_id = Some(target.id);
                    break;
                }
            }

            if let Some(id) = clicked_id {
                crate::platform::haptics::play_click_haptic();
                handle_element_click(hwnd, state, id);
            } else if state.search_focused {
                state.search_focused = false;
                let _ = InvalidateRect(hwnd, None, BOOL::from(false));
            }

            LRESULT(0)
        }

        WM_KEYDOWN => {
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let state = &mut *state_ptr;

            if wparam.0 == VK_ESCAPE.0 as usize {
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }

            if state.search_focused {
                if wparam.0 == VK_BACK.0 as usize {
                    state.search_query.pop();
                    update_search_filter(state);
                    let _ = InvalidateRect(hwnd, None, BOOL::from(false));
                    return LRESULT(0);
                } else if wparam.0 == VK_RETURN.0 as usize {
                    state.search_focused = false;
                    let _ = InvalidateRect(hwnd, None, BOOL::from(false));
                    return LRESULT(0);
                }
            }

            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        WM_CHAR => {
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let state = &mut *state_ptr;

            if state.search_focused {
                let ch = char::from_u32(wparam.0 as u32);
                if let Some(c) = ch {
                    if !c.is_control() {
                        state.search_query.push(c);
                        update_search_filter(state);
                        let _ = InvalidateRect(hwnd, None, BOOL::from(false));
                    }
                }
                return LRESULT(0);
            }

            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        WM_PAINT => {
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let state = &mut *state_ptr;

            let mut ps = PAINTSTRUCT::default();
            let _hdc = BeginPaint(hwnd, &mut ps);

            render_settings_window(state);

            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            state.window_x = rect.left;
            state.window_y = rect.top;

            state.canvas.present_with_dirty_rect(hwnd, rect.left, rect.top, None);
            let _ = EndPaint(hwnd, &ps);

            LRESULT(0)
        }

        WM_TEAMS_LOADED => {
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                update_search_filter(state);
                let _ = InvalidateRect(hwnd, None, BOOL::from(false));
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            if !state_ptr.is_null() {
                let _ = Box::from_raw(state_ptr);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ---------------------------------------------------------------------------
// IMGUI Element Click Handler
// ---------------------------------------------------------------------------
unsafe fn handle_element_click(hwnd: HWND, state: &mut DialogState, id: u32) {
    let mut changed = false;

    // Tabs 10..16
    if id >= ID_TAB_BASE && id < ID_TAB_BASE + 7 {
        let tab_idx = (id - ID_TAB_BASE) as usize;
        if tab_idx < TABS.len() {
            state.active_tab = TABS[tab_idx].0;
            changed = true;
        }
    } else {
        match state.active_tab {
            SettingsTab::General => match id {
                100 => {
                    state.settings.general.start_with_windows = !state.settings.general.start_with_windows;
                    changed = true;
                }
                101..=103 => {
                    state.settings.general.notch_size = (id - 101) as usize;
                    changed = true;
                }
                111 => {
                    state.settings.general.mascot_enabled = !state.settings.general.mascot_enabled;
                    changed = true;
                }
                112 => {
                    state.settings.general.haptic_feedback = !state.settings.general.haptic_feedback;
                    changed = true;
                }
                113 => {
                    state.settings.general.hide_in_fullscreen = !state.settings.general.hide_in_fullscreen;
                    changed = true;
                }
                _ => {}
            },
            SettingsTab::Appearance => match id {
                200..=202 => {
                    state.settings.appearance.theme_mode = (id - 200) as usize;
                    changed = true;
                }
                210..=213 => {
                    state.settings.appearance.accent_color = (id - 210) as usize;
                    changed = true;
                }
                220 => {
                    state.settings.appearance.shadow_blur = !state.settings.appearance.shadow_blur;
                    changed = true;
                }
                221 => {
                    state.settings.appearance.translucent_acrylic = !state.settings.appearance.translucent_acrylic;
                    changed = true;
                }
                _ => {}
            },
            SettingsTab::Music => match id {
                300 => {
                    state.settings.music.synchronized_lyrics = !state.settings.music.synchronized_lyrics;
                    changed = true;
                }
                301 => {
                    state.settings.music.audio_visualizer = !state.settings.music.audio_visualizer;
                    changed = true;
                }
                310..=313 => {
                    state.settings.music.preferred_player = (id - 310) as usize;
                    changed = true;
                }
                _ => {}
            },
            SettingsTab::Focus => match id {
                400..=403 => {
                    state.settings.focus.default_duration = (id - 400) as usize;
                    changed = true;
                }
                410 => {
                    state.settings.focus.celebration_confetti = !state.settings.focus.celebration_confetti;
                    changed = true;
                }
                411 => {
                    state.settings.focus.break_reminders = !state.settings.focus.break_reminders;
                    changed = true;
                }
                _ => {}
            },
            SettingsTab::Notifications => match id {
                500 => {
                    state.settings.notifications.intercept_toasts = !state.settings.notifications.intercept_toasts;
                    changed = true;
                }
                501 => {
                    state.settings.notifications.dismiss_system_toast = !state.settings.notifications.dismiss_system_toast;
                    changed = true;
                }
                502 => {
                    state.settings.notifications.screenshot_preview = !state.settings.notifications.screenshot_preview;
                    changed = true;
                }
                503 => {
                    state.settings.notifications.call_alerts = !state.settings.notifications.call_alerts;
                    changed = true;
                }
                _ => {}
            },
            SettingsTab::Sports => {
                if id == ID_SPO_SEARCH {
                    state.search_focused = true;
                    changed = true;
                } else if id >= ID_SPO_ADD_BASE && id < ID_SPO_ADD_BASE + 20 {
                    let idx = (id - ID_SPO_ADD_BASE) as usize;
                    if idx < state.filtered_teams.len() {
                        let selected = &state.filtered_teams[idx];
                        let endpoints = match selected.sport_category.as_str() {
                            "soccer" => SOCCER_ENDPOINTS,
                            "nfl" => NFL_ENDPOINTS,
                            "nba" => NBA_ENDPOINTS,
                            "mlb" => MLB_ENDPOINTS,
                            "nhl" => NHL_ENDPOINTS,
                            _ => &[],
                        };

                        state.settings.subscriptions.retain(|s| s.team_abbr != selected.abbr);
                        for ep in endpoints {
                            state.settings.subscriptions.push(TeamSubscription {
                                team_abbr: selected.abbr.clone(),
                                league_endpoint: ep.to_string(),
                            });
                        }
                        state.search_query.clear();
                        state.search_focused = false;
                        update_search_filter(state);
                        changed = true;
                    }
                } else if id >= ID_SPO_DEL_BASE && id < ID_SPO_DEL_BASE + 30 {
                    let idx = (id - ID_SPO_DEL_BASE) as usize;
                    let mut unique_teams = Vec::new();
                    for sub in &state.settings.subscriptions {
                        if !unique_teams.contains(&sub.team_abbr) {
                            unique_teams.push(sub.team_abbr.clone());
                        }
                    }
                    if idx < unique_teams.len() {
                        let abbr_to_remove = &unique_teams[idx];
                        state.settings.subscriptions.retain(|s| s.team_abbr != *abbr_to_remove);
                        changed = true;
                    }
                }
            }
            SettingsTab::Habits => match id {
                700 => {
                    state.settings.habits.hydration_tracker = !state.settings.habits.hydration_tracker;
                    changed = true;
                }
                701 => {
                    state.settings.habits.stand_and_stretch = !state.settings.habits.stand_and_stretch;
                    changed = true;
                }
                702 => {
                    state.settings.habits.eye_rest = !state.settings.habits.eye_rest;
                    changed = true;
                }
                _ => {}
            },
        }
    }

    if changed {
        save_app_settings(&state.settings);
        let _ = InvalidateRect(hwnd, None, BOOL::from(false));
    }
}

fn update_search_filter(state: &mut DialogState) {
    state.filtered_teams.clear();
    let q = state.search_query.trim().to_lowercase();
    if q.is_empty() {
        return;
    }

    if let Ok(cache) = state.available_teams.lock() {
        for t in cache.iter() {
            if t.display_name.to_lowercase().contains(&q) || t.abbr.to_lowercase().contains(&q) {
                state.filtered_teams.push(t.clone());
                if state.filtered_teams.len() >= 5 {
                    break;
                }
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    state.filtered_teams.retain(|t| seen.insert(t.abbr.clone()));
}

// ---------------------------------------------------------------------------
// Main Rendering Engine
// ---------------------------------------------------------------------------
fn render_settings_window(state: &mut DialogState) {
    state.targets.clear();
    state.canvas.clear();

    let width = WIN_WIDTH as f32;
    let height = WIN_HEIGHT as f32;

    // 1. Outer rounded container mask & clip
    let mut pb_outer = PathBuilder::new();
    add_round_rect(&mut pb_outer, 0.0, 0.0, width, height, CORNER_RADIUS);
    let outer_path = match pb_outer.finish() {
        Some(p) => p,
        None => return,
    };

    // Fill Sidebar (#191919) and Main Content (#121212)
    let mut sidebar_paint = Paint::default();
    sidebar_paint.set_color(Color::from_rgba8(25, 25, 25, 255));
    sidebar_paint.anti_alias = true;

    let mut content_paint = Paint::default();
    content_paint.set_color(Color::from_rgba8(18, 18, 18, 255));
    content_paint.anti_alias = true;

    // Draw main background
    state.canvas.pixmap.fill_path(
        &outer_path,
        &content_paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    // Clip sidebar onto left 210px
    let mut pb_sidebar = PathBuilder::new();
    add_round_rect_left(&mut pb_sidebar, 0.0, 0.0, SIDEBAR_WIDTH, height, CORNER_RADIUS);
    if let Some(sb_path) = pb_sidebar.finish() {
        state.canvas.pixmap.fill_path(
            &sb_path,
            &sidebar_paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    // Subtle 1px vertical divider between sidebar and content
    let mut div_paint = Paint::default();
    div_paint.set_color(Color::from_rgba8(36, 36, 38, 255));
    state.canvas.pixmap.fill_rect(
        tiny_skia::Rect::from_xywh(SIDEBAR_WIDTH - 0.5, 0.0, 1.0, height).unwrap(),
        &div_paint,
        Transform::identity(),
        None,
    );

    // Subtle outer border
    let mut border_paint = Paint::default();
    border_paint.set_color(Color::from_rgba8(48, 48, 52, 255));
    border_paint.anti_alias = true;
    let stroke = Stroke {
        width: 1.0,
        ..Default::default()
    };
    state.canvas.pixmap.stroke_path(
        &outer_path,
        &border_paint,
        &stroke,
        Transform::identity(),
        None,
    );

    // 2. Render Sidebar Elements
    render_sidebar(state);

    // 3. Render Active Tab Content
    render_active_tab(state);
}

// ---------------------------------------------------------------------------
// Sidebar Drawing
// ---------------------------------------------------------------------------
fn render_sidebar(state: &mut DialogState) {
    // A. Red traffic-light close button at (20, 20)
    let is_close_hover = state.hovered_id == Some(ID_CLOSE);
    let mut close_paint = Paint::default();
    close_paint.anti_alias = true;
    close_paint.set_color(if is_close_hover {
        Color::from_rgba8(255, 69, 58, 255)
    } else {
        Color::from_rgba8(255, 95, 86, 255)
    });

    let mut pb_close = PathBuilder::new();
    pb_close.push_circle(20.0, 20.0, 6.0);
    if let Some(path) = pb_close.finish() {
        state.canvas.pixmap.fill_path(&path, &close_paint, FillRule::Winding, Transform::identity(), None);

        // Darker ring border
        let mut border_paint = Paint::default();
        border_paint.set_color(Color::from_rgba8(214, 63, 54, 255));
        border_paint.anti_alias = true;
        let stroke = Stroke { width: 0.75, ..Default::default() };
        state.canvas.pixmap.stroke_path(&path, &border_paint, &stroke, Transform::identity(), None);

        // If hovered, draw subtle cross icon
        if is_close_hover {
            let mut cross_paint = Paint::default();
            cross_paint.set_color(Color::from_rgba8(100, 10, 10, 220));
            cross_paint.anti_alias = true;
            let cross_stroke = Stroke { width: 1.2, line_cap: LineCap::Round, ..Default::default() };
            let mut pb_x = PathBuilder::new();
            pb_x.move_to(17.5, 17.5);
            pb_x.line_to(22.5, 22.5);
            pb_x.move_to(22.5, 17.5);
            pb_x.line_to(17.5, 22.5);
            if let Some(xp) = pb_x.finish() {
                state.canvas.pixmap.stroke_path(&xp, &cross_paint, &cross_stroke, Transform::identity(), None);
            }
        }
    }
    state.targets.push(ClickTarget {
        id: ID_CLOSE,
        rect: (8.0, 8.0, 24.0, 24.0),
    });

    // B. Tabs List
    let start_y = 52.0;
    let item_h = 38.0;
    let item_w = SIDEBAR_WIDTH - 24.0;
    let item_x = 12.0;

    for (index, (tab, title, (r, g, b))) in TABS.iter().enumerate() {
        let item_y = start_y + index as f32 * 42.0;
        let tab_id = ID_TAB_BASE + index as u32;
        let is_selected = state.active_tab == *tab;
        let is_hovered = state.hovered_id == Some(tab_id);

        state.targets.push(ClickTarget {
            id: tab_id,
            rect: (item_x, item_y, item_w, item_h),
        });

        // Tab Pill Background
        if is_selected {
            let mut sel_paint = Paint::default();
            sel_paint.set_color(Color::from_rgba8(44, 44, 48, 255));
            sel_paint.anti_alias = true;
            let mut pb = PathBuilder::new();
            add_round_rect(&mut pb, item_x, item_y, item_w, item_h, 8.0);
            if let Some(p) = pb.finish() {
                state.canvas.pixmap.fill_path(&p, &sel_paint, FillRule::Winding, Transform::identity(), None);
            }
        } else if is_hovered {
            let mut hov_paint = Paint::default();
            hov_paint.set_color(Color::from_rgba8(34, 34, 38, 255));
            hov_paint.anti_alias = true;
            let mut pb = PathBuilder::new();
            add_round_rect(&mut pb, item_x, item_y, item_w, item_h, 8.0);
            if let Some(p) = pb.finish() {
                state.canvas.pixmap.fill_path(&p, &hov_paint, FillRule::Winding, Transform::identity(), None);
            }
        }

        // Icon Badge (rounded square with vibrant background)
        let badge_x = item_x + 8.0;
        let badge_y = item_y + (item_h - 22.0) * 0.5;
        let mut badge_paint = Paint::default();
        badge_paint.set_color(Color::from_rgba8(*r, *g, *b, 255));
        badge_paint.anti_alias = true;

        let mut pb_b = PathBuilder::new();
        add_round_rect(&mut pb_b, badge_x, badge_y, 22.0, 22.0, 5.5);
        if let Some(bp) = pb_b.finish() {
            state.canvas.pixmap.fill_path(&bp, &badge_paint, FillRule::Winding, Transform::identity(), None);
        }

        // Vector glyph inside badge
        draw_tab_glyph(&mut state.canvas, badge_x + 11.0, badge_y + 11.0, *tab);

        // Tab Title Text
        let text_x = badge_x + 30.0;
        let text_y = state.canvas.text_y_for_vertical_center(title, 13.5, is_selected, item_y + item_h * 0.5);
        let text_color = if is_selected {
            Color::WHITE
        } else {
            Color::from_rgba8(168, 168, 172, 255)
        };
        state.canvas.draw_text(title, text_x, text_y, 13.5, text_color, is_selected);
    }
}

// ---------------------------------------------------------------------------
// Vector Glyph Helpers for Tab Badges
// ---------------------------------------------------------------------------
fn draw_tab_glyph(canvas: &mut LayeredCanvas, cx: f32, cy: f32, tab: SettingsTab) {
    let mut paint = Paint::default();
    paint.set_color(Color::WHITE);
    paint.anti_alias = true;
    let stroke = Stroke {
        width: 1.4,
        line_cap: LineCap::Round,
        ..Default::default()
    };

    let mut pb = PathBuilder::new();
    match tab {
        SettingsTab::General => {
            // Gear ring
            pb.push_circle(cx, cy, 3.2);
            for i in 0..6 {
                let angle = (i as f32) * std::f32::consts::PI / 3.0;
                let (sin, cos) = angle.sin_cos();
                pb.move_to(cx + cos * 4.0, cy + sin * 4.0);
                pb.line_to(cx + cos * 5.8, cy + sin * 5.8);
            }
        }
        SettingsTab::Appearance => {
            // Half-filled contrast circle
            pb.push_circle(cx, cy, 5.0);
            pb.move_to(cx, cy - 5.0);
            pb.line_to(cx, cy + 5.0);
        }
        SettingsTab::Music => {
            // Eighth note
            pb.move_to(cx - 2.5, cy + 3.0);
            pb.push_circle(cx - 2.5, cy + 3.0, 1.8);
            pb.move_to(cx - 1.0, cy + 3.0);
            pb.line_to(cx - 1.0, cy - 4.0);
            pb.line_to(cx + 3.0, cy - 2.0);
            pb.line_to(cx + 3.0, cy + 1.5);
            pb.push_circle(cx + 1.8, cy + 2.0, 1.5);
        }
        SettingsTab::Focus => {
            // Stopwatch
            pb.push_circle(cx, cy + 0.8, 4.6);
            pb.move_to(cx, cy + 0.8);
            pb.line_to(cx + 1.8, cy - 0.8);
            pb.move_to(cx - 1.5, cy - 4.4);
            pb.line_to(cx + 1.5, cy - 4.4);
        }
        SettingsTab::Notifications => {
            // Bell
            pb.move_to(cx - 4.0, cy + 2.5);
            pb.quad_to(cx - 3.5, cy - 2.0, cx, cy - 3.5);
            pb.quad_to(cx + 3.5, cy - 2.0, cx + 4.0, cy + 2.5);
            pb.line_to(cx - 4.0, cy + 2.5);
            pb.push_circle(cx, cy + 4.0, 0.9);
        }
        SettingsTab::Sports => {
            // Soccer ball / Trophy arc
            pb.push_circle(cx, cy, 5.0);
            pb.move_to(cx - 2.0, cy - 1.0);
            pb.line_to(cx + 2.0, cy - 1.0);
            pb.line_to(cx, cy + 2.5);
            pb.line_to(cx - 2.0, cy - 1.0);
        }
        SettingsTab::Habits => {
            // Checkmark in circle or heart
            pb.move_to(cx - 3.8, cy - 0.2);
            pb.line_to(cx - 1.0, cy + 2.8);
            pb.line_to(cx + 3.8, cy - 2.4);
        }
    }

    if let Some(path) = pb.finish() {
        canvas.pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

// ---------------------------------------------------------------------------
// Active Tab Content Views
// ---------------------------------------------------------------------------
fn render_active_tab(state: &mut DialogState) {
    let content_x = 236.0;
    let content_w = 556.0;

    // Header Title
    let title = match state.active_tab {
        SettingsTab::General => "General",
        SettingsTab::Appearance => "Appearance",
        SettingsTab::Music => "Music & Lyrics",
        SettingsTab::Focus => "Focus Timer",
        SettingsTab::Notifications => "Notifications",
        SettingsTab::Sports => "Sports",
        SettingsTab::Habits => "Habits",
    };
    state.canvas.draw_text(title, content_x, 24.0, 24.0, Color::WHITE, true);

    let start_y = 66.0;

    match state.active_tab {
        SettingsTab::General => render_general_tab(state, content_x, start_y, content_w),
        SettingsTab::Appearance => render_appearance_tab(state, content_x, start_y, content_w),
        SettingsTab::Music => render_music_tab(state, content_x, start_y, content_w),
        SettingsTab::Focus => render_focus_tab(state, content_x, start_y, content_w),
        SettingsTab::Notifications => render_notifications_tab(state, content_x, start_y, content_w),
        SettingsTab::Sports => render_sports_tab(state, content_x, start_y, content_w),
        SettingsTab::Habits => render_habits_tab(state, content_x, start_y, content_w),
    }
}

// ---------------------------------------------------------------------------
// Tab 1: General View
// ---------------------------------------------------------------------------
fn render_general_tab(state: &mut DialogState, x: f32, mut y: f32, w: f32) {
    // 1. Startup Section
    draw_section_header(&mut state.canvas, x, y, "STARTUP");
    y += 20.0;

    let card_h = 52.0;
    draw_card_background(&mut state.canvas, x, y, w, card_h);
    draw_card_row_text(&mut state.canvas, x + 16.0, y + 26.0, "Start with Windows", None);

    let is_on = state.settings.general.start_with_windows;
    draw_toggle(state, 100, x + w - 56.0, y + 14.0, is_on);
    y += card_h + 20.0;

    // 2. Notch Section
    draw_section_header(&mut state.canvas, x, y, "NOTCH");
    y += 20.0;

    let notch_card_h = 136.0;
    draw_card_background(&mut state.canvas, x, y, w, notch_card_h);

    // Row 1: Size Segmented Control
    draw_card_row_text(
        &mut state.canvas,
        x + 16.0,
        y + 24.0,
        "Size",
        Some("MyIsland scales membrane and content elements proportionally"),
    );
    let size_options = ["Small", "Medium", "Large"];
    draw_segmented_control(
        state,
        101,
        x + 16.0,
        y + 44.0,
        w - 32.0,
        28.0,
        &size_options,
        state.settings.general.notch_size,
    );

    draw_divider(&mut state.canvas, x + 16.0, y + 84.0, w - 32.0);

    // Row 2: Mascot
    draw_card_row_text(
        &mut state.canvas,
        x + 16.0,
        y + 110.0,
        "Mascot",
        Some("A dynamic friend that blinks, gazes, and reacts in the notch"),
    );
    draw_toggle(state, 111, x + w - 56.0, y + 98.0, state.settings.general.mascot_enabled);

    y += notch_card_h + 20.0;

    // 3. Sounds Section
    draw_section_header(&mut state.canvas, x, y, "SOUNDS");
    y += 20.0;

    let sound_h = 56.0;
    draw_card_background(&mut state.canvas, x, y, w, sound_h);
    draw_card_row_text(
        &mut state.canvas,
        x + 16.0,
        y + 28.0,
        "Haptic feedback",
        Some("Tactile audio ticks when clicking interactive elements and scrolling"),
    );
    draw_toggle(state, 112, x + w - 56.0, y + 16.0, state.settings.general.haptic_feedback);

    y += sound_h + 20.0;

    // 4. Full screen Section
    draw_section_header(&mut state.canvas, x, y, "FULL SCREEN");
    y += 20.0;

    let fs_h = 56.0;
    draw_card_background(&mut state.canvas, x, y, w, fs_h);
    draw_card_row_text(
        &mut state.canvas,
        x + 16.0,
        y + 28.0,
        "Hide in full screen",
        Some("Automatically tucks into a thin strip during games and videos"),
    );
    draw_toggle(state, 113, x + w - 56.0, y + 16.0, state.settings.general.hide_in_fullscreen);
}

// ---------------------------------------------------------------------------
// Tab 2: Appearance View
// ---------------------------------------------------------------------------
fn render_appearance_tab(state: &mut DialogState, x: f32, mut y: f32, w: f32) {
    draw_section_header(&mut state.canvas, x, y, "THEME");
    y += 20.0;

    let theme_h = 120.0;
    draw_card_background(&mut state.canvas, x, y, w, theme_h);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 28.0, "Color Theme", Some("System follows dark/light Windows preferences"));
    let themes = ["Dark", "System", "Light"];
    draw_segmented_control(state, 200, x + w - 210.0, y + 14.0, 194.0, 28.0, &themes, state.settings.appearance.theme_mode);

    draw_divider(&mut state.canvas, x + 16.0, y + 58.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 88.0, "Accent Color", Some("Highlight color used for active badges"));
    let accents = ["Green", "Blue", "Orange", "Purple"];
    draw_segmented_control(state, 210, x + w - 240.0, y + 74.0, 224.0, 28.0, &accents, state.settings.appearance.accent_color);

    y += theme_h + 22.0;

    draw_section_header(&mut state.canvas, x, y, "VISUAL EFFECTS");
    y += 20.0;

    let fx_h = 112.0;
    draw_card_background(&mut state.canvas, x, y, w, fx_h);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 28.0, "Dynamic Soft Shadows", Some("Multi-pass Gaussian blur shadow under the island"));
    draw_toggle(state, 220, x + w - 56.0, y + 16.0, state.settings.appearance.shadow_blur);

    draw_divider(&mut state.canvas, x + 16.0, y + 56.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 84.0, "DirectComposition Smoothing", Some("Hardware accelerated swapchain with subpixel anti-aliasing"));
    draw_toggle(state, 221, x + w - 56.0, y + 72.0, state.settings.appearance.translucent_acrylic);
}

// ---------------------------------------------------------------------------
// Tab 3: Music View
// ---------------------------------------------------------------------------
fn render_music_tab(state: &mut DialogState, x: f32, mut y: f32, w: f32) {
    draw_section_header(&mut state.canvas, x, y, "PLAYBACK & METADATA");
    y += 20.0;

    let card_h = 170.0;
    draw_card_background(&mut state.canvas, x, y, w, card_h);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 28.0, "Synchronized Lyrics", Some("Fetches and crossfades LRC lyrics from LRCLIB in real time"));
    draw_toggle(state, 300, x + w - 56.0, y + 16.0, state.settings.music.synchronized_lyrics);

    draw_divider(&mut state.canvas, x + 16.0, y + 56.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 84.0, "Audio Visualizer", Some("WASAPI loopback frequency analysis with Goertzel filter banks"));
    draw_toggle(state, 301, x + w - 56.0, y + 72.0, state.settings.music.audio_visualizer);

    draw_divider(&mut state.canvas, x + 16.0, y + 112.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 140.0, "Preferred Media Source", Some("Priority for Windows media transport sessions"));
    let players = ["Auto", "Spotify", "Apple", "Browser"];
    draw_segmented_control(state, 310, x + w - 240.0, y + 126.0, 224.0, 28.0, &players, state.settings.music.preferred_player);
}

// ---------------------------------------------------------------------------
// Tab 4: Focus Timer View
// ---------------------------------------------------------------------------
fn render_focus_tab(state: &mut DialogState, x: f32, mut y: f32, w: f32) {
    draw_section_header(&mut state.canvas, x, y, "POMODORO & FOCUS");
    y += 20.0;

    let card_h = 170.0;
    draw_card_background(&mut state.canvas, x, y, w, card_h);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 28.0, "Default Session Length", Some("Initial duration on the spring ruler"));
    let durations = ["15m", "25m", "45m", "60m"];
    draw_segmented_control(state, 400, x + w - 220.0, y + 14.0, 204.0, 28.0, &durations, state.settings.focus.default_duration);

    draw_divider(&mut state.canvas, x + 16.0, y + 56.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 84.0, "Completion Confetti", Some("Celebration confetti physics when timer reaches zero"));
    draw_toggle(state, 410, x + w - 56.0, y + 72.0, state.settings.focus.celebration_confetti);

    draw_divider(&mut state.canvas, x + 16.0, y + 112.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 140.0, "Break Reminders", Some("Suggest short breaks between focus sessions"));
    draw_toggle(state, 411, x + w - 56.0, y + 128.0, state.settings.focus.break_reminders);
}

// ---------------------------------------------------------------------------
// Tab 5: Notifications View
// ---------------------------------------------------------------------------
fn render_notifications_tab(state: &mut DialogState, x: f32, mut y: f32, w: f32) {
    draw_section_header(&mut state.canvas, x, y, "SYSTEM TOASTS");
    y += 20.0;

    let card_h = 224.0;
    draw_card_background(&mut state.canvas, x, y, w, card_h);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 28.0, "Intercept Windows Notifications", Some("Display incoming desktop toasts as Dynamic Island capsules"));
    draw_toggle(state, 500, x + w - 56.0, y + 16.0, state.settings.notifications.intercept_toasts);

    draw_divider(&mut state.canvas, x + 16.0, y + 56.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 84.0, "Dismiss Native Toast", Some("Removes duplicate banner from Windows Action Center"));
    draw_toggle(state, 501, x + w - 56.0, y + 72.0, state.settings.notifications.dismiss_system_toast);

    draw_divider(&mut state.canvas, x + 16.0, y + 56.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 140.0, "Screenshot Previews", Some("Expand Snipping Tool captures automatically with quick actions"));
    draw_toggle(state, 502, x + w - 56.0, y + 128.0, state.settings.notifications.screenshot_preview);

    draw_divider(&mut state.canvas, x + 16.0, y + 168.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 196.0, "Phone Link Call Cards", Some("Incoming call alerts with Accept and Decline buttons"));
    draw_toggle(state, 503, x + w - 56.0, y + 184.0, state.settings.notifications.call_alerts);
}

// ---------------------------------------------------------------------------
// Tab 6: Sports View
// ---------------------------------------------------------------------------
fn render_sports_tab(state: &mut DialogState, x: f32, mut y: f32, w: f32) {
    // 1. Search Box
    draw_section_header(&mut state.canvas, x, y, "ADD TEAM SUBSCRIPTION");
    y += 20.0;

    let search_h = 42.0;
    let is_focused = state.search_focused;
    let mut search_paint = Paint::default();
    search_paint.set_color(Color::from_rgba8(30, 30, 34, 255));
    search_paint.anti_alias = true;

    let mut pb_s = PathBuilder::new();
    add_round_rect(&mut pb_s, x, y, w, search_h, 8.0);
    if let Some(p) = pb_s.finish() {
        state.canvas.pixmap.fill_path(&p, &search_paint, FillRule::Winding, Transform::identity(), None);

        let mut stroke_paint = Paint::default();
        stroke_paint.set_color(if is_focused {
            Color::from_rgba8(52, 199, 89, 255)
        } else {
            Color::from_rgba8(50, 50, 54, 255)
        });
        stroke_paint.anti_alias = true;
        let stroke = Stroke { width: 1.0, ..Default::default() };
        state.canvas.pixmap.stroke_path(&p, &stroke_paint, &stroke, Transform::identity(), None);
    }

    state.targets.push(ClickTarget {
        id: ID_SPO_SEARCH,
        rect: (x, y, w, search_h),
    });

    // Search query or placeholder text
    let display_text = if state.search_query.is_empty() {
        if is_focused { "" } else { "Search team (e.g., Arsenal, Lakers, Real Madrid)..." }
    } else {
        &state.search_query
    };

    let text_col = if state.search_query.is_empty() {
        Color::from_rgba8(120, 120, 126, 255)
    } else {
        Color::WHITE
    };
    let text_y = state.canvas.text_y_for_vertical_center(display_text, 13.5, false, y + search_h * 0.5);
    state.canvas.draw_text(display_text, x + 16.0, text_y, 13.5, text_col, false);

    // Blinking cursor if focused
    if is_focused {
        let cursor_x = x + 16.0 + state.canvas.measure_text_width(&state.search_query, 13.5, false) + 2.0;
        let mut cur_paint = Paint::default();
        cur_paint.set_color(Color::from_rgba8(52, 199, 89, 255));
        state.canvas.pixmap.fill_rect(
            tiny_skia::Rect::from_xywh(cursor_x, y + 12.0, 1.5, 18.0).unwrap(),
            &cur_paint,
            Transform::identity(),
            None,
        );
    }

    y += search_h + 10.0;

    // Search Results Dropdown Card (if matches exist)
    if !state.filtered_teams.is_empty() {
        let res_h = (state.filtered_teams.len() as f32 * 40.0).max(40.0);
        draw_card_background(&mut state.canvas, x, y, w, res_h);

        for (idx, team) in state.filtered_teams.iter().enumerate() {
            let row_y = y + idx as f32 * 40.0;
            let add_id = ID_SPO_ADD_BASE + idx as u32;

            if idx > 0 {
                draw_divider(&mut state.canvas, x + 12.0, row_y, w - 24.0);
            }

            let label = format!("{} ({})  ·  {}", team.display_name, team.abbr, team.sport_category.to_uppercase());
            let ty = state.canvas.text_y_for_vertical_center(&label, 13.0, false, row_y + 20.0);
            state.canvas.draw_text(&label, x + 16.0, ty, 13.0, Color::WHITE, false);

            // "+ Add" button
            let btn_w = 64.0;
            let btn_h = 24.0;
            let btn_x = x + w - btn_w - 12.0;
            let btn_y = row_y + 8.0;

            let is_btn_hover = state.hovered_id == Some(add_id);
            let mut btn_paint = Paint::default();
            btn_paint.set_color(if is_btn_hover {
                Color::from_rgba8(40, 160, 70, 255)
            } else {
                Color::from_rgba8(52, 199, 89, 255)
            });
            btn_paint.anti_alias = true;

            let mut pb_btn = PathBuilder::new();
            add_round_rect(&mut pb_btn, btn_x, btn_y, btn_w, btn_h, 6.0);
            if let Some(p) = pb_btn.finish() {
                state.canvas.pixmap.fill_path(&p, &btn_paint, FillRule::Winding, Transform::identity(), None);
            }

            let bty = state.canvas.text_y_for_vertical_center("+ Add", 12.0, true, btn_y + btn_h * 0.5);
            let btw = state.canvas.measure_text_width("+ Add", 12.0, true);
            state.canvas.draw_text("+ Add", btn_x + (btn_w - btw) * 0.5, bty, 12.0, Color::WHITE, true);

            state.targets.push(ClickTarget {
                id: add_id,
                rect: (btn_x, btn_y, btn_w, btn_h),
            });
        }

        y += res_h + 16.0;
    }

    // 2. Subscribed Teams Card
    draw_section_header(&mut state.canvas, x, y, "CURRENT SUBSCRIPTIONS");
    y += 20.0;

    let mut unique_teams = Vec::new();
    for sub in &state.settings.subscriptions {
        if !unique_teams.contains(&sub.team_abbr) {
            unique_teams.push(sub.team_abbr.clone());
        }
    }

    if unique_teams.is_empty() {
        let card_h = 56.0;
        draw_card_background(&mut state.canvas, x, y, w, card_h);
        let ty = state.canvas.text_y_for_vertical_center("No teams subscribed yet. Search above to add.", 13.0, false, y + 28.0);
        state.canvas.draw_text("No teams subscribed yet. Search above to add.", x + 16.0, ty, 13.0, Color::from_rgba8(140, 140, 146, 255), false);
    } else {
        let card_h = (unique_teams.len() as f32 * 46.0).max(46.0);
        draw_card_background(&mut state.canvas, x, y, w, card_h);

        for (idx, abbr) in unique_teams.iter().enumerate() {
            let row_y = y + idx as f32 * 46.0;
            let del_id = ID_SPO_DEL_BASE + idx as u32;

            if idx > 0 {
                draw_divider(&mut state.canvas, x + 16.0, row_y, w - 32.0);
            }

            let count = state.settings.subscriptions.iter().filter(|s| s.team_abbr == *abbr).count();
            let label = format!("{}  ·  Tracking {} leagues / competitions", abbr, count);
            draw_card_row_text(&mut state.canvas, x + 16.0, row_y + 23.0, &label, None);

            // Red Delete Button
            let btn_w = 76.0;
            let btn_h = 24.0;
            let btn_x = x + w - btn_w - 16.0;
            let btn_y = row_y + 11.0;

            let is_del_hover = state.hovered_id == Some(del_id);
            let mut btn_paint = Paint::default();
            btn_paint.set_color(if is_del_hover {
                Color::from_rgba8(255, 69, 58, 255)
            } else {
                Color::from_rgba8(220, 50, 40, 220)
            });
            btn_paint.anti_alias = true;

            let mut pb_b = PathBuilder::new();
            add_round_rect(&mut pb_b, btn_x, btn_y, btn_w, btn_h, 6.0);
            if let Some(p) = pb_b.finish() {
                state.canvas.pixmap.fill_path(&p, &btn_paint, FillRule::Winding, Transform::identity(), None);
            }

            let bty = state.canvas.text_y_for_vertical_center("Remove", 12.0, true, btn_y + btn_h * 0.5);
            let btw = state.canvas.measure_text_width("Remove", 12.0, true);
            state.canvas.draw_text("Remove", btn_x + (btn_w - btw) * 0.5, bty, 12.0, Color::WHITE, true);

            state.targets.push(ClickTarget {
                id: del_id,
                rect: (btn_x, btn_y, btn_w, btn_h),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Tab 7: Habits View
// ---------------------------------------------------------------------------
fn render_habits_tab(state: &mut DialogState, x: f32, mut y: f32, w: f32) {
    draw_section_header(&mut state.canvas, x, y, "DAILY WELLNESS & HABITS");
    y += 20.0;

    let card_h = 168.0;
    draw_card_background(&mut state.canvas, x, y, w, card_h);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 28.0, "Hydration Reminder", Some("Periodic water drops in the notch to encourage hydration"));
    draw_toggle(state, 700, x + w - 56.0, y + 16.0, state.settings.habits.hydration_tracker);

    draw_divider(&mut state.canvas, x + 16.0, y + 56.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 84.0, "Stand & Stretch", Some("Hourly posture and mobility alerts during desk work"));
    draw_toggle(state, 701, x + w - 56.0, y + 72.0, state.settings.habits.stand_and_stretch);

    draw_divider(&mut state.canvas, x + 16.0, y + 112.0, w - 32.0);

    draw_card_row_text(&mut state.canvas, x + 16.0, y + 140.0, "20-20-20 Eye Rest", Some("Gentle reminders every 20 minutes to look at distance objects"));
    draw_toggle(state, 702, x + w - 56.0, y + 128.0, state.settings.habits.eye_rest);
}

// ---------------------------------------------------------------------------
// Reusable UI Component Primitives
// ---------------------------------------------------------------------------
fn draw_section_header(canvas: &mut LayeredCanvas, x: f32, y: f32, text: &str) {
    canvas.draw_text(text, x, y, 11.5, Color::from_rgba8(142, 142, 147, 255), true);
}

fn draw_card_background(canvas: &mut LayeredCanvas, x: f32, y: f32, w: f32, h: f32) {
    let mut bg_paint = Paint::default();
    bg_paint.set_color(Color::from_rgba8(30, 30, 32, 255));
    bg_paint.anti_alias = true;

    let mut pb = PathBuilder::new();
    add_round_rect(&mut pb, x, y, w, h, 10.0);
    if let Some(p) = pb.finish() {
        canvas.pixmap.fill_path(&p, &bg_paint, FillRule::Winding, Transform::identity(), None);

        // Subtle 1px card border
        let mut border_paint = Paint::default();
        border_paint.set_color(Color::from_rgba8(42, 42, 46, 255));
        border_paint.anti_alias = true;
        let stroke = Stroke { width: 1.0, ..Default::default() };
        canvas.pixmap.stroke_path(&p, &border_paint, &stroke, Transform::identity(), None);
    }
}

fn draw_card_row_text(canvas: &mut LayeredCanvas, x: f32, center_y: f32, title: &str, subtitle: Option<&str>) {
    if let Some(sub) = subtitle {
        let ty = canvas.text_y_for_vertical_center(title, 13.5, false, center_y - 8.0);
        canvas.draw_text(title, x, ty, 13.5, Color::WHITE, false);

        let sy = canvas.text_y_for_vertical_center(sub, 11.0, false, center_y + 9.0);
        canvas.draw_text(sub, x, sy, 11.0, Color::from_rgba8(142, 142, 147, 255), false);
    } else {
        let ty = canvas.text_y_for_vertical_center(title, 13.5, false, center_y);
        canvas.draw_text(title, x, ty, 13.5, Color::WHITE, false);
    }
}

fn draw_divider(canvas: &mut LayeredCanvas, x: f32, y: f32, w: f32) {
    let mut div_paint = Paint::default();
    div_paint.set_color(Color::from_rgba8(44, 44, 48, 255));
    canvas.pixmap.fill_rect(
        tiny_skia::Rect::from_xywh(x, y - 0.5, w, 1.0).unwrap(),
        &div_paint,
        Transform::identity(),
        None,
    );
}

fn draw_toggle(state: &mut DialogState, id: u32, x: f32, y: f32, is_on: bool) {
    let width = 42.0;
    let height = 24.0;
    let radius = 12.0;

    let is_hovered = state.hovered_id == Some(id);

    // Track Fill
    let mut track_paint = Paint::default();
    track_paint.anti_alias = true;
    track_paint.set_color(if is_on {
        if is_hovered {
            Color::from_rgba8(60, 215, 98, 255)
        } else {
            Color::from_rgba8(52, 199, 89, 255) // macOS Green
        }
    } else {
        if is_hovered {
            Color::from_rgba8(66, 66, 70, 255)
        } else {
            Color::from_rgba8(57, 57, 61, 255) // Dark Gray Track
        }
    });

    let mut pb_track = PathBuilder::new();
    add_round_rect(&mut pb_track, x, y, width, height, radius);
    if let Some(p) = pb_track.finish() {
        state.canvas.pixmap.fill_path(&p, &track_paint, FillRule::Winding, Transform::identity(), None);
    }

    // White circle thumb
    let thumb_r = 10.0;
    let thumb_cx = if is_on {
        x + width - radius
    } else {
        x + radius
    };
    let thumb_cy = y + radius;

    let mut thumb_paint = Paint::default();
    thumb_paint.set_color(Color::WHITE);
    thumb_paint.anti_alias = true;

    let mut pb_thumb = PathBuilder::new();
    pb_thumb.push_circle(thumb_cx, thumb_cy, thumb_r);
    if let Some(p) = pb_thumb.finish() {
        state.canvas.pixmap.fill_path(&p, &thumb_paint, FillRule::Winding, Transform::identity(), None);
    }

    state.targets.push(ClickTarget {
        id,
        rect: (x, y, width, height),
    });
}

fn draw_segmented_control(
    state: &mut DialogState,
    base_id: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    options: &[&str],
    selected_idx: usize,
) {
    // Background capsule
    let mut bg_paint = Paint::default();
    bg_paint.set_color(Color::from_rgba8(38, 38, 42, 255));
    bg_paint.anti_alias = true;

    let mut pb_bg = PathBuilder::new();
    add_round_rect(&mut pb_bg, x, y, w, h, 7.0);
    if let Some(p) = pb_bg.finish() {
        state.canvas.pixmap.fill_path(&p, &bg_paint, FillRule::Winding, Transform::identity(), None);
    }

    let count = options.len();
    if count == 0 {
        return;
    }
    let segment_w = (w - 4.0) / count as f32;

    for (i, opt) in options.iter().enumerate() {
        let seg_x = x + 2.0 + i as f32 * segment_w;
        let seg_y = y + 2.0;
        let seg_h = h - 4.0;
        let seg_id = base_id + i as u32;

        state.targets.push(ClickTarget {
            id: seg_id,
            rect: (seg_x, seg_y, segment_w, seg_h),
        });

        let is_sel = i == selected_idx;
        let is_hov = state.hovered_id == Some(seg_id);

        if is_sel {
            let mut sel_paint = Paint::default();
            sel_paint.set_color(Color::from_rgba8(78, 78, 84, 255));
            sel_paint.anti_alias = true;

            let mut pb_s = PathBuilder::new();
            add_round_rect(&mut pb_s, seg_x, seg_y, segment_w, seg_h, 6.0);
            if let Some(p) = pb_s.finish() {
                state.canvas.pixmap.fill_path(&p, &sel_paint, FillRule::Winding, Transform::identity(), None);
            }
        } else if is_hov {
            let mut hov_paint = Paint::default();
            hov_paint.set_color(Color::from_rgba8(48, 48, 52, 255));
            hov_paint.anti_alias = true;

            let mut pb_h = PathBuilder::new();
            add_round_rect(&mut pb_h, seg_x, seg_y, segment_w, seg_h, 6.0);
            if let Some(p) = pb_h.finish() {
                state.canvas.pixmap.fill_path(&p, &hov_paint, FillRule::Winding, Transform::identity(), None);
            }
        }

        let tw = state.canvas.measure_text_width(opt, 12.0, is_sel);
        let tx = seg_x + (segment_w - tw) * 0.5;
        let ty = state.canvas.text_y_for_vertical_center(opt, 12.0, is_sel, seg_y + seg_h * 0.5);
        let col = if is_sel {
            Color::WHITE
        } else {
            Color::from_rgba8(160, 160, 166, 255)
        };
        state.canvas.draw_text(opt, tx, ty, 12.0, col, is_sel);
    }
}

// ---------------------------------------------------------------------------
// Geometry Helpers
// ---------------------------------------------------------------------------
fn add_round_rect(pb: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32) {
    let r = r.min(w * 0.5).min(h * 0.5);
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

fn add_round_rect_left(pb: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32) {
    let r = r.min(w * 0.5).min(h * 0.5);
    pb.move_to(x + r, y);
    pb.line_to(x + w, y);
    pb.line_to(x + w, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
}
