use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, DeleteObject, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, DEFAULT_CHARSET,
    DEFAULT_PITCH, DEFAULT_QUALITY, FF_DONTCARE, FW_NORMAL, HBRUSH, HGDIOBJ, OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::platform::sports::{SportsConfig, TeamSubscription};

// Control IDs
const ID_SUBSCRIBED_LIST: i32 = 101;
const ID_SEARCH_TXT: i32 = 102;
const ID_RESULTS_LIST: i32 = 103;
const ID_BTN_DEL: i32 = 104;
const ID_BTN_SAVE: i32 = 105;

// Custom Window Messages & Timers
const WM_TEAMS_LOADED: u32 = WM_USER + 1;
const TIMER_DEBOUNCE: usize = 1;

// Comprehensive endpoint mapping for major sports
const SOCCER_ENDPOINTS: &[&str] = &[
    "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.1/scoreboard", // EPL
    "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.fa/scoreboard", // FA Cup
    "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.league_cup/scoreboard", // EFL Cup
    "https://site.api.espn.com/apis/site/v2/sports/soccer/esp.1/scoreboard", // La Liga
    "https://site.api.espn.com/apis/site/v2/sports/soccer/esp.copa_del_rey/scoreboard", // Copa Del Rey
    "https://site.api.espn.com/apis/site/v2/sports/soccer/uefa.champions/scoreboard", // Champions League
    "https://site.api.espn.com/apis/site/v2/sports/soccer/uefa.europa/scoreboard", // Europa League
    "https://site.api.espn.com/apis/site/v2/sports/soccer/ger.1/scoreboard", // Bundesliga
    "https://site.api.espn.com/apis/site/v2/sports/soccer/ger.dfb_pokal/scoreboard", // DFB Pokal
    "https://site.api.espn.com/apis/site/v2/sports/soccer/ita.1/scoreboard", // Serie A
    "https://site.api.espn.com/apis/site/v2/sports/soccer/ita.coppa_italia/scoreboard", // Coppa Italia
    "https://site.api.espn.com/apis/site/v2/sports/soccer/fra.1/scoreboard", // Ligue 1
];

const NFL_ENDPOINTS: &[&str] = &["https://site.api.espn.com/apis/site/v2/sports/football/nfl/scoreboard"];
const NBA_ENDPOINTS: &[&str] = &["https://site.api.espn.com/apis/site/v2/sports/basketball/nba/scoreboard"];
const MLB_ENDPOINTS: &[&str] = &["https://site.api.espn.com/apis/site/v2/sports/baseball/mlb/scoreboard"];
const NHL_ENDPOINTS: &[&str] = &["https://site.api.espn.com/apis/site/v2/sports/hockey/nhl/scoreboard"];

#[derive(Clone)]
struct TeamInfo {
    display_name: String,
    abbr: String,
    sport_category: String,
}

struct DialogState {
    config: SportsConfig,
    hwnd_subscribed: HWND,
    hwnd_search: HWND,
    hwnd_results: HWND,
    font: HGDIOBJ,
    available_teams: Arc<Mutex<Vec<TeamInfo>>>,
    filtered_teams: Vec<TeamInfo>,
}

fn config_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("MyIsland"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn open_sports_settings() {
    std::thread::spawn(|| unsafe {
        let icex = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES | ICC_LISTVIEW_CLASSES,
        };
        let _ = InitCommonControlsEx(&icex);

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
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _),
            ..Default::default()
        };

        RegisterClassW(&wc);

        let dir = config_dir();
        let config_path = dir.join("sports_settings.json");
        let config = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|data| serde_json::from_str::<SportsConfig>(&data).ok())
            .unwrap_or_default();

        let available_teams = Arc::new(Mutex::new(Vec::new()));

        let state = Box::new(DialogState {
            config,
            hwnd_subscribed: HWND::default(),
            hwnd_search: HWND::default(),
            hwnd_results: HWND::default(),
            font: HGDIOBJ::default(),
            available_teams: available_teams.clone(),
            filtered_teams: Vec::new(),
        });
        let state_ptr = Box::into_raw(state);

        let hwnd = match CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            class_name,
            w!("Sports Subscriptions - MyIsland"),
            WS_OVERLAPPEDWINDOW & !WS_THICKFRAME & !WS_MAXIMIZEBOX | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            540,
            480,
            None,
            None,
            h_instance,
            Some(state_ptr as *const _),
        ) {
            Ok(hwnd) => hwnd,
            Err(_) => {
                drop(Box::from_raw(state_ptr));
                return;
            }
        };

        fetch_popular_teams(hwnd, available_teams);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

fn fetch_popular_teams(hwnd: HWND, cache: Arc<Mutex<Vec<TeamInfo>>>) {
    // HWND is a raw pointer and therefore not `Send`; carry it as an integer
    // across the thread boundary and reconstruct it before posting back.
    let hwnd_value = hwnd.0 as isize;
    std::thread::spawn(move || {
        let leagues = vec![
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

            if let Ok(res) = ureq::get(&url).set("User-Agent", "MyIsland/0.1").call() {
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

unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;

    match msg {
        WM_CREATE => {
            let create_struct = &*(lparam.0 as *const CREATESTRUCTW);
            let state = &mut *(create_struct.lpCreateParams as *mut DialogState);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as *mut _ as isize);

            state.font = HGDIOBJ(
                CreateFontW(
                    16, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0,
                    DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32,
                    CLIP_DEFAULT_PRECIS.0 as u32, DEFAULT_QUALITY.0 as u32,
                    DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
                    w!("Segoe UI"),
                ).0,
            );

            let h_inst = create_struct.hInstance;

            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(), w!("Static"), w!("Search Team (e.g., Arsenal, Lakers)"),
                WS_CHILD | WS_VISIBLE,
                20, 20, 480, 20,
                hwnd, HMENU::default(), h_inst, None,
            );

            state.hwnd_search = CreateWindowExW(
                WS_EX_CLIENTEDGE, w!("Edit"), PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                20, 40, 480, 26,
                hwnd, HMENU(ID_SEARCH_TXT as usize as *mut _), h_inst, None,
            ).unwrap_or_default();

            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(), w!("Static"), w!("Subscribed Teams"),
                WS_CHILD | WS_VISIBLE,
                20, 90, 480, 20,
                hwnd, HMENU::default(), h_inst, None,
            );

            state.hwnd_subscribed = CreateWindowExW(
                WS_EX_CLIENTEDGE, w!("ListBox"), PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPSIBLINGS | WINDOW_STYLE(LBS_NOTIFY as u32),
                20, 110, 480, 260,
                hwnd, HMENU(ID_SUBSCRIBED_LIST as usize as *mut _), h_inst, None,
            ).unwrap_or_default();

            state.hwnd_results = CreateWindowExW(
                WS_EX_CLIENTEDGE, w!("ListBox"), PCWSTR::null(),
                WS_CHILD | WS_VSCROLL | WS_CLIPSIBLINGS | WINDOW_STYLE(LBS_NOTIFY as u32),
                20, 66, 480, 160,
                hwnd, HMENU(ID_RESULTS_LIST as usize as *mut _), h_inst, None,
            ).unwrap_or_default();
            let _ = ShowWindow(state.hwnd_results, SW_HIDE);

            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(), w!("Button"), w!("Remove Selected"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                20, 390, 140, 30,
                hwnd, HMENU(ID_BTN_DEL as usize as *mut _), h_inst, None,
            );

            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(), w!("Button"), w!("Save & Close"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                380, 390, 120, 34,
                hwnd, HMENU(ID_BTN_SAVE as usize as *mut _), h_inst, None,
            );

            let _ = EnumChildWindows(hwnd, Some(set_font_callback), LPARAM(state.font.0 as isize));
            refresh_subscribed_listbox(state);

            LRESULT(0)
        }
        WM_COMMAND => {
            if state_ptr.is_null() { return LRESULT(0); }
            let state = &mut *state_ptr;
            let control_id = (wparam.0 & 0xFFFF) as i32;
            let notify_code = (wparam.0 >> 16) as u16;

            match control_id {
                ID_SEARCH_TXT => {
                    if notify_code == EN_CHANGE as u16 {
                        let _ = KillTimer(hwnd, TIMER_DEBOUNCE);
                        let _ = SetTimer(hwnd, TIMER_DEBOUNCE, 300, None);
                    }
                }
                ID_RESULTS_LIST => {
                    if notify_code == LBN_SELCHANGE as u16 {
                        let idx = SendMessageW(state.hwnd_results, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as isize;
                        if idx >= 0 && (idx as usize) < state.filtered_teams.len() {
                            let selected = &state.filtered_teams[idx as usize];

                            let endpoints = match selected.sport_category.as_str() {
                                "soccer" => SOCCER_ENDPOINTS,
                                "nfl" => NFL_ENDPOINTS,
                                "nba" => NBA_ENDPOINTS,
                                "mlb" => MLB_ENDPOINTS,
                                "nhl" => NHL_ENDPOINTS,
                                _ => &[],
                            };

                            // Clear any existing subscriptions for this team to avoid overlap
                            state.config.subscriptions.retain(|s| s.team_abbr != selected.abbr);

                            // Bulk inject endpoints based on category
                            for ep in endpoints {
                                state.config.subscriptions.push(TeamSubscription {
                                    team_abbr: selected.abbr.clone(),
                                    league_endpoint: ep.to_string(),
                                });
                            }

                            refresh_subscribed_listbox(state);

                            let _ = SetWindowTextW(state.hwnd_search, w!(""));
                            let _ = ShowWindow(state.hwnd_results, SW_HIDE);
                        }
                    }
                }
                ID_BTN_DEL => {
                    let idx = SendMessageW(state.hwnd_subscribed, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as isize;
                    if idx >= 0 {
                        let mut unique_teams = Vec::new();
                        for sub in &state.config.subscriptions {
                            if !unique_teams.contains(&sub.team_abbr) {
                                unique_teams.push(sub.team_abbr.clone());
                            }
                        }

                        if (idx as usize) < unique_teams.len() {
                            let abbr_to_remove = &unique_teams[idx as usize];
                            // Purge ALL associated league endpoint subscriptions for this team
                            state.config.subscriptions.retain(|s| s.team_abbr != *abbr_to_remove);
                            refresh_subscribed_listbox(state);
                        }
                    }
                }
                ID_BTN_SAVE => {
                    let dir = config_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    if let Ok(json) = serde_json::to_string_pretty(&state.config) {
                        let _ = std::fs::write(dir.join("sports_settings.json"), json);
                    }
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_DEBOUNCE {
                let _ = KillTimer(hwnd, TIMER_DEBOUNCE);
                if !state_ptr.is_null() {
                    update_search_results(&mut *state_ptr);
                }
            }
            LRESULT(0)
        }
        WM_TEAMS_LOADED => {
            if !state_ptr.is_null() {
                let query = get_window_text((*state_ptr).hwnd_search);
                if !query.trim().is_empty() {
                    update_search_results(&mut *state_ptr);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if !state_ptr.is_null() {
                let state = Box::from_raw(state_ptr);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if !state.font.0.is_null() {
                    let _ = DeleteObject(state.font);
                }
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn set_font_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    SendMessageW(hwnd, WM_SETFONT, WPARAM(lparam.0 as usize), LPARAM(1));
    BOOL::from(true)
}

unsafe fn update_search_results(state: &mut DialogState) {
    let query = get_window_text(state.hwnd_search).to_lowercase();
    SendMessageW(state.hwnd_results, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
    state.filtered_teams.clear();

    if query.trim().is_empty() {
        let _ = ShowWindow(state.hwnd_results, SW_HIDE);
        return;
    }

    if let Ok(cache) = state.available_teams.lock() {
        for t in cache.iter() {
            if t.display_name.to_lowercase().contains(&query) || t.abbr.to_lowercase().contains(&query) {
                state.filtered_teams.push(t.clone());
            }
        }
    }

    // Deduplicate search results (e.g., a team fetched from both the
    // Champions League and its domestic league feed). `dedup_by` only removes
    // adjacent duplicates, so track seen abbreviations to handle feeds that
    // are separated by other teams.
    let mut seen = std::collections::HashSet::new();
    state.filtered_teams.retain(|t| seen.insert(t.abbr.clone()));

    for t in &state.filtered_teams {
        let display = format!("{} ({})", t.display_name, t.abbr);
        let mut wide: Vec<u16> = display.encode_utf16().collect();
        wide.push(0);
        SendMessageW(
            state.hwnd_results,
            LB_ADDSTRING,
            WPARAM(0),
            LPARAM(wide.as_ptr() as isize),
        );
    }

    if !state.filtered_teams.is_empty() {
        let _ = BringWindowToTop(state.hwnd_results);
        let _ = ShowWindow(state.hwnd_results, SW_SHOW);
    } else {
        let _ = ShowWindow(state.hwnd_results, SW_HIDE);
    }
}

unsafe fn refresh_subscribed_listbox(state: &DialogState) {
    SendMessageW(state.hwnd_subscribed, LB_RESETCONTENT, WPARAM(0), LPARAM(0));

    // Group multi-endpoint subscriptions so the UI doesn't spam 12 identical abbreviations
    let mut unique_teams = Vec::new();
    for sub in &state.config.subscriptions {
        if !unique_teams.contains(&sub.team_abbr) {
            unique_teams.push(sub.team_abbr.clone());
        }
    }

    for abbr in unique_teams {
        let count = state.config.subscriptions.iter().filter(|s| s.team_abbr == abbr).count();
        let display = format!("{} (Tracking {} competitions)", abbr, count);

        let mut wide: Vec<u16> = display.encode_utf16().collect();
        wide.push(0);
        SendMessageW(
            state.hwnd_subscribed,
            LB_ADDSTRING,
            WPARAM(0),
            LPARAM(wide.as_ptr() as isize),
        );
    }
}

unsafe fn get_window_text(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len > 0 {
        let mut buf = vec![0u16; (len + 1) as usize];
        GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    }
}
