use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LRESULT, POINT, WPARAM, LPARAM};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromWindow,
    SetWindowRgn, HBRUSH, HRGN, MONITORINFO, MONITOR_DEFAULTTOPRIMARY, RGN_DIFF, RGN_OR,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_SPACE,
    VK_VOLUME_UP, VK_VOLUME_DOWN, VK_VOLUME_MUTE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetCursorPos, RegisterClassExW, ShowWindow, SW_SHOWNOACTIVATE,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

pub const WINDOW_WIDTH: i32 = 860;
pub const WINDOW_HEIGHT: i32 = 340;

pub const HOTKEY_TOGGLE_TUCK: i32 = 1;
pub const HOTKEY_TOGGLE_EXPAND: i32 = 2;
pub const HOTKEY_VOL_UP: i32 = 3;
pub const HOTKEY_VOL_DOWN: i32 = 4;
pub const HOTKEY_VOL_MUTE: i32 = 5;

pub struct Win32Window {
    pub hwnd: HWND,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Win32Window {
    pub fn create(
        wnd_proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    ) -> windows::core::Result<Self> {
        unsafe {
            let hinstance = GetModuleHandleW(None)?;
            let class_name = w!("MyIslandWindowClass");

            let wnd_class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: windows::Win32::UI::WindowsAndMessaging::WNDCLASS_STYLES(0),
                lpfnWndProc: Some(wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance.into(),
                hIcon: windows::Win32::UI::WindowsAndMessaging::HICON(std::ptr::null_mut()),
                hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                    None,
                    windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
                )?,
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                lpszMenuName: PCWSTR::null(),
                lpszClassName: class_name,
                hIconSm: windows::Win32::UI::WindowsAndMessaging::HICON(std::ptr::null_mut()),
            };

            RegisterClassExW(&wnd_class);

            let primary_mon = MonitorFromWindow(HWND(std::ptr::null_mut()), MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(primary_mon, &mut mi);

            let mon_rect = mi.rcMonitor;
            let mon_w = mon_rect.right - mon_rect.left;
            let win_x = mon_rect.left + (mon_w - WINDOW_WIDTH) / 2;
            let win_y = mon_rect.top;

            let ex_style = WS_EX_TOPMOST
                | WS_EX_TOOLWINDOW
                | WS_EX_LAYERED
                | WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_NOACTIVATE;

            let style = WS_POPUP;

            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                w!("Dynamic Island"),
                style,
                win_x,
                win_y,
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                None,
                None,
                hinstance,
                None,
            )?;

            // Register Global Hotkeys
            let _ = RegisterHotKey(hwnd, HOTKEY_TOGGLE_TUCK, MOD_CONTROL | MOD_NOREPEAT, VK_SPACE.0 as u32);
            let _ = RegisterHotKey(
                hwnd,
                HOTKEY_TOGGLE_EXPAND,
                MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
                VK_SPACE.0 as u32,
            );

            // Intercept hardware media keys to suppress the native Windows volume toast
            let _ = RegisterHotKey(hwnd, HOTKEY_VOL_UP, Default::default(), VK_VOLUME_UP.0 as u32);
            let _ = RegisterHotKey(hwnd, HOTKEY_VOL_DOWN, Default::default(), VK_VOLUME_DOWN.0 as u32);
            let _ = RegisterHotKey(hwnd, HOTKEY_VOL_MUTE, Default::default(), VK_VOLUME_MUTE.0 as u32);

            // Add clipboard format listener for event-driven clipboard updates
            let _ = windows::Win32::System::DataExchange::AddClipboardFormatListener(hwnd);

            // Register Raw Input for global mouse motion (event-driven waking with 0% CPU when idle)
            let rid = windows::Win32::UI::Input::RAWINPUTDEVICE {
                usUsagePage: 0x01, // HID_USAGE_PAGE_GENERIC
                usUsage: 0x02,     // HID_USAGE_GENERIC_MOUSE
                dwFlags: windows::Win32::UI::Input::RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            };
            let _ = windows::Win32::UI::Input::RegisterRawInputDevices(
                &[rid],
                std::mem::size_of::<windows::Win32::UI::Input::RAWINPUTDEVICE>() as u32,
            );

            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

            Ok(Self {
                hwnd,
                x: win_x,
                y: win_y,
                width: WINDOW_WIDTH,
                height: WINDOW_HEIGHT,
            })
        }
    }

    #[allow(dead_code)]
    pub fn set_interactive(&self, _interactive: bool) {
        // Handled via WM_NCHITTEST HTTRANSPARENT / HTCLIENT for per-pixel irregular hit testing
    }

    /// Restrict the window's OS-level shape, which also defines its hit-test
    /// region, to the given rounded rectangles in window coordinates.
    ///
    /// The GPU/DirectComposition path does not use `WS_EX_LAYERED`, so Windows
    /// no longer performs per-pixel alpha hit-testing. A window region is the
    /// only mechanism that makes empty areas click-through across processes.
    /// `hole` (x, y, width, height) is subtracted from the region so a click at
    /// that point falls through while the surrounding visuals stay intact.
    /// Passing `None` for `rects` removes the region and restores the full
    /// window shape.
    pub fn set_interaction_region(
        &self,
        rects: Option<&[(f32, f32, f32, f32, f32)]>,
        hole: Option<(f32, f32, f32, f32)>,
    ) {
        unsafe {
            let Some(rects) = rects else {
                let _ = SetWindowRgn(self.hwnd, HRGN::default(), true);
                return;
            };

            let combined = CreateRectRgn(0, 0, 0, 0);
            if combined.is_invalid() {
                return;
            }

            let mut has_part = false;
            for &(x, y, w, h, r) in rects {
                if !w.is_finite() || !h.is_finite() || w <= 0.5 || h <= 0.5 {
                    continue;
                }
                let left = x.floor() as i32;
                let top = y.floor() as i32;
                let right = (x + w).ceil() as i32;
                let bottom = (y + h).ceil() as i32;
                if right <= left || bottom <= top {
                    continue;
                }
                let diameter = ((r * 2.0).round() as i32).max(1);
                let part = CreateRoundRectRgn(left, top, right, bottom, diameter, diameter);
                if part.is_invalid() {
                    continue;
                }
                let _ = CombineRgn(combined, combined, part, RGN_OR);
                let _ = DeleteObject(part);
                has_part = true;
            }

            if !has_part {
                let _ = DeleteObject(combined);
                let _ = SetWindowRgn(self.hwnd, HRGN::default(), true);
                return;
            }

            if let Some((x, y, w, h)) = hole {
                let left = x.floor() as i32;
                let top = y.floor() as i32;
                let right = (x + w).ceil() as i32;
                let bottom = (y + h).ceil() as i32;
                if right > left && bottom > top {
                    let hole_rgn = CreateRectRgn(left, top, right, bottom);
                    if !hole_rgn.is_invalid() {
                        let _ = CombineRgn(combined, combined, hole_rgn, RGN_DIFF);
                        let _ = DeleteObject(hole_rgn);
                    }
                }
            }

            // On success Windows owns `combined`; only free it if the call failed.
            if SetWindowRgn(self.hwnd, combined, true) == 0 {
                let _ = DeleteObject(combined);
            }
        }
    }

    pub fn get_cursor_relative(&self) -> (f32, f32, bool) {
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                let rel_x = (pt.x - self.x) as f32;
                let rel_y = (pt.y - self.y) as f32;
                let inside = pt.x >= self.x
                    && pt.x <= self.x + self.width
                    && pt.y >= self.y
                    && pt.y <= self.y + self.height;
                (rel_x, rel_y, inside)
            } else {
                (-9999.0, -9999.0, false)
            }
        }
    }
}
