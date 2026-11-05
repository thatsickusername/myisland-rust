pub mod compact;
pub mod expanded;
pub mod settings;
pub mod tray;

pub use compact::CompactRenderer;
pub use expanded::ExpandedRenderer;
pub use tray::{
    TrayIcon, WM_TRAYICON, IDM_QUIT, IDM_TEST_CALL, IDM_TEST_DISCORD, IDM_TEST_SNIP,
    IDM_TEST_WHATSAPP, IDM_TEST_CONTROLLER, IDM_TEST_HEADPHONES,
    IDM_TEST_SPEAKER_DISCONNECT, IDM_TOGGLE_EXPAND, IDM_TUCK_AWAY, IDM_SPORTS_SETTINGS,
};

pub fn format_focus_timer(ms: u64) -> String {
    let s = if ms == 0 { 0 } else { (ms + 999) / 1000 };
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, sec)
    } else {
        format!("{:02}:{:02}", m, sec)
    }
}
