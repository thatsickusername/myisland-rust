use std::path::PathBuf;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, DeleteObject, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, DEFAULT_CHARSET,
    DEFAULT_PITCH, DEFAULT_QUALITY, FF_DONTCARE, FW_NORMAL, HBRUSH, HGDIOBJ,
    OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::platform::sports::{SportsConfig, TeamSubscription};

// Control IDs
const ID_LIST: i32 = 101;
const ID_ABBR_TXT: i32 = 103;
const ID_URL_TXT: i32 = 105;
const ID_BTN_ADD: i32 = 106;
const ID_BTN_DEL: i32 = 107;
const ID_BTN_SAVE: i32 = 108;

struct DialogState {
    config: SportsConfig,
    hwnd_list: HWND,
    hwnd_abbr: HWND,
    hwnd_url: HWND,
    font: HGDIOBJ,
}

fn config_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("MyIsland"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Opens the native sports-subscription editor on its own UI thread so the
/// island's render loop is never blocked by the settings dialog.
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

        let state = Box::new(DialogState {
            config,
            hwnd_list: HWND::default(),
            hwnd_abbr: HWND::default(),
            hwnd_url: HWND::default(),
            font: HGDIOBJ::default(),
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
            420,
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

        let _ = hwnd;
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
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

            // Match the native Windows 11 dialog look with the system UI font.
            state.font = HGDIOBJ(
                CreateFontW(
                    16,
                    0,
                    0,
                    0,
                    FW_NORMAL.0 as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET.0 as u32,
                    OUT_DEFAULT_PRECIS.0 as u32,
                    CLIP_DEFAULT_PRECIS.0 as u32,
                    DEFAULT_QUALITY.0 as u32,
                    DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
                    w!("Segoe UI"),
                )
                .0,
            );

            let h_inst = create_struct.hInstance;

            state.hwnd_list = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("ListBox"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
                20,
                20,
                480,
                180,
                hwnd,
                HMENU(ID_LIST as usize as *mut _),
                h_inst,
                None,
            )
            .unwrap_or_default();

            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("Static"),
                w!("Team Abbreviation (e.g. ARS)"),
                WS_CHILD | WS_VISIBLE,
                20,
                210,
                200,
                20,
                hwnd,
                HMENU(102usize as *mut _),
                h_inst,
                None,
            );

            state.hwnd_abbr = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("Edit"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                20,
                230,
                120,
                26,
                hwnd,
                HMENU(ID_ABBR_TXT as usize as *mut _),
                h_inst,
                None,
            )
            .unwrap_or_default();

            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("Static"),
                w!("ESPN League Scoreboard URL"),
                WS_CHILD | WS_VISIBLE,
                160,
                210,
                300,
                20,
                hwnd,
                HMENU(104usize as *mut _),
                h_inst,
                None,
            );

            state.hwnd_url = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("Edit"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                160,
                230,
                340,
                26,
                hwnd,
                HMENU(ID_URL_TXT as usize as *mut _),
                h_inst,
                None,
            )
            .unwrap_or_default();

            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("Button"),
                w!("Add Team"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                20,
                270,
                100,
                30,
                hwnd,
                HMENU(ID_BTN_ADD as usize as *mut _),
                h_inst,
                None,
            );

            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("Button"),
                w!("Remove Selected"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                130,
                270,
                140,
                30,
                hwnd,
                HMENU(ID_BTN_DEL as usize as *mut _),
                h_inst,
                None,
            );

            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("Button"),
                w!("Save & Close"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                380,
                330,
                120,
                34,
                hwnd,
                HMENU(ID_BTN_SAVE as usize as *mut _),
                h_inst,
                None,
            );

            let _ = EnumChildWindows(
                hwnd,
                Some(set_font_callback),
                LPARAM(state.font.0 as isize),
            );
            refresh_listbox(state);

            LRESULT(0)
        }
        WM_COMMAND => {
            if state_ptr.is_null() {
                return LRESULT(0);
            }
            let state = &mut *state_ptr;
            let id = (wparam.0 & 0xFFFF) as i32;

            match id {
                ID_BTN_ADD => {
                    let abbr = get_window_text(state.hwnd_abbr);
                    let url = get_window_text(state.hwnd_url);
                    if !abbr.is_empty() && !url.is_empty() {
                        state.config.subscriptions.push(TeamSubscription {
                            team_abbr: abbr.trim().to_uppercase(),
                            league_endpoint: url.trim().to_string(),
                        });
                        refresh_listbox(state);
                        let _ = SetWindowTextW(state.hwnd_abbr, w!(""));
                        let _ = SetWindowTextW(state.hwnd_url, w!(""));
                    }
                }
                ID_BTN_DEL => {
                    let idx = SendMessageW(state.hwnd_list, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0
                        as isize;
                    if idx >= 0 && (idx as usize) < state.config.subscriptions.len() {
                        state.config.subscriptions.remove(idx as usize);
                        refresh_listbox(state);
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
        WM_DESTROY => {
            if !state_ptr.is_null() {
                let state = Box::from_raw(state_ptr);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if !state.font.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(state.font.0));
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

unsafe fn refresh_listbox(state: &DialogState) {
    SendMessageW(state.hwnd_list, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
    for sub in &state.config.subscriptions {
        let display = format!("{}  -  {}", sub.team_abbr, sub.league_endpoint);
        let mut wide: Vec<u16> = display.encode_utf16().collect();
        wide.push(0);
        SendMessageW(
            state.hwnd_list,
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
