use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::Shell::SHQueryUserNotificationState;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetClassNameW, GetForegroundWindow, GetWindowLongW,
    GetWindowRect, GWL_STYLE, WS_CAPTION, WS_POPUP, WS_THICKFRAME,
};
use windows::core::w;

use std::time::{Duration, Instant};

pub struct FullscreenDetector {
    pub is_fullscreen: bool,
    last_check: Instant,
    last_fg: HWND,
}

impl FullscreenDetector {
    pub fn new() -> Self {
        Self {
            is_fullscreen: false,
            last_check: Instant::now() - Duration::from_secs(1),
            last_fg: HWND(std::ptr::null_mut()),
        }
    }

    pub fn check_fullscreen(&mut self, our_hwnd: HWND) -> bool {
        let fg = unsafe { GetForegroundWindow() };
        let now = Instant::now();

        // If foreground window is the same and checked recently (< 250ms), return cached result
        if fg == self.last_fg && now.duration_since(self.last_check) < Duration::from_millis(250) {
            return self.is_fullscreen;
        }

        self.last_fg = fg;
        self.last_check = now;

        let is_fs = unsafe { is_window_fullscreen_internal(fg, our_hwnd) };
        self.is_fullscreen = is_fs;
        is_fs
    }
}

unsafe fn is_window_fullscreen_internal(fg: HWND, our_hwnd: HWND) -> bool {
    if fg.0.is_null() || fg == our_hwnd {
        return false;
    }

    // 1. Exclude Desktop / Shell / Taskbar
    let mut class_buf = [0u16; 256];
    let len = GetClassNameW(fg, &mut class_buf);
    let class_str = String::from_utf16_lossy(&class_buf[..len as usize]);

    if class_str == "Progman"
        || class_str == "WorkerW"
        || class_str == "Shell_TrayWnd"
        || class_str.contains("XamlExplorerHostIslandWindow")
    {
        return false;
    }

    // 2. Direct3D Fullscreen check
    if let Ok(quns) = SHQueryUserNotificationState() {
        if quns.0 == 3 {
            // QUNS_RUNNING_D3D_FULL_SCREEN
            return true;
        }
    }

    // 3. Geometry check (Window covers monitor)
    let mut w_rect = RECT::default();
    if !GetWindowRect(fg, &mut w_rect).is_ok() {
        return false;
    }

    let h_mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(h_mon, &mut mi).as_bool() {
        return false;
    }

    let m_rect = mi.rcMonitor;
    let covers_mon = w_rect.left <= m_rect.left
        && w_rect.top <= m_rect.top
        && w_rect.right >= m_rect.right
        && w_rect.bottom >= m_rect.bottom;

    // Check if taskbar is covered
    let covers_tray = if let Ok(tray_hwnd) = FindWindowW(w!("Shell_TrayWnd"), None) {
        let mut tray_rect = RECT::default();
        if !tray_hwnd.0.is_null() && GetWindowRect(tray_hwnd, &mut tray_rect).is_ok() {
            w_rect.left <= tray_rect.left
                && w_rect.right >= tray_rect.right
                && w_rect.bottom >= tray_rect.bottom
        } else {
            true
        }
    } else {
        true
    };

    // 4. Style check (dropped borders & caption)
    let style = GetWindowLongW(fg, GWL_STYLE) as u32;
    let has_caption = (style & WS_CAPTION.0) != 0;
    let has_thick_frame = (style & WS_THICKFRAME.0) != 0;
    let is_popup = (style & WS_POPUP.0) != 0;

    covers_mon && (covers_tray || is_popup) && ((!has_caption && !has_thick_frame) || is_popup)
}
