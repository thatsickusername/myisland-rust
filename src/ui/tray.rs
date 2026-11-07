use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, NIF_ICON, NIF_MESSAGE, NIF_TIP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow,
    TrackPopupMenu, HICON, MF_DISABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
};

pub const WM_TRAYICON: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 1;

pub const IDM_TOGGLE_EXPAND: usize = 1001;
pub const IDM_TUCK_AWAY: usize = 1002;
pub const IDM_TEST_WHATSAPP: usize = 1003;
pub const IDM_TEST_CALL: usize = 1004;
pub const IDM_TEST_SNIP: usize = 1005;
pub const IDM_TEST_DISCORD: usize = 1006;
pub const IDM_TEST_CONTROLLER: usize = 1007;
pub const IDM_TEST_HEADPHONES: usize = 1008;
pub const IDM_TEST_SPEAKER_DISCONNECT: usize = 1009;
pub const IDM_SPORTS_SETTINGS: usize = 1010;
pub const IDM_QUIT: usize = 1099;

pub struct TrayIcon {
    nid: NOTIFYICONDATAW,
}

impl TrayIcon {
    pub fn create(hwnd: HWND, hicon: HICON) -> windows::core::Result<Self> {
        let mut tip_buf = [0u16; 128];
        let tip_str = "Dynamic Island";
        for (i, c) in tip_str.encode_utf16().enumerate() {
            if i < 127 {
                tip_buf[i] = c;
            }
        }

        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: hicon,
            szTip: tip_buf,
            ..Default::default()
        };

        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }

        Ok(Self { nid })
    }

    pub fn show_context_menu(&self, hwnd: HWND) {
        unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);

            let menu = match CreatePopupMenu() {
                Ok(m) => m,
                Err(_) => return,
            };

            let _ = AppendMenuW(menu, MF_STRING | MF_DISABLED | MF_GRAYED, 0, w!("Dynamic Island"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, IDM_TOGGLE_EXPAND, w!("Toggle Expand"));
            let _ = AppendMenuW(menu, MF_STRING, IDM_TUCK_AWAY, w!("Tuck Away"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, IDM_SPORTS_SETTINGS, w!("Settings..."));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, IDM_TEST_WHATSAPP, w!("Test WhatsApp"));
            let _ = AppendMenuW(menu, MF_STRING, IDM_TEST_CALL, w!("Test Phone Call"));
            let _ = AppendMenuW(menu, MF_STRING, IDM_TEST_SNIP, w!("Test Snipping Tool"));
            let _ = AppendMenuW(menu, MF_STRING, IDM_TEST_DISCORD, w!("Test Discord DM"));
            let _ = AppendMenuW(menu, MF_STRING, IDM_TEST_CONTROLLER, w!("Test Controller Connected"));
            let _ = AppendMenuW(menu, MF_STRING, IDM_TEST_HEADPHONES, w!("Test Bluetooth Earbuds Connected"));
            let _ = AppendMenuW(menu, MF_STRING, IDM_TEST_SPEAKER_DISCONNECT, w!("Test Bluetooth Speaker Disconnected"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, IDM_QUIT, w!("Quit Dynamic Island"));

            let _ = SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                pt.x,
                pt.y,
                0,
                hwnd,
                None,
            );
            let _ = DestroyMenu(menu);
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
        }
    }
}
