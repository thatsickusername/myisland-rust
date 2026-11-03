use crossbeam_channel::Sender;
use std::collections::HashSet;
use std::time::Duration;

use crate::state::NotificationPayload;
use windows::Foundation::Size;
use windows::Storage::Streams::{Buffer, DataReader, IRandomAccessStreamWithContentType, InputStreamOptions};
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::NotificationKinds;

fn decode_base64(input: &[u8]) -> Option<Vec<u8>> {
    let mut values = Vec::with_capacity(input.len());
    for byte in input.iter().copied().filter(|byte| !byte.is_ascii_whitespace()) {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => continue,
            _ => return None,
        };
        values.push(value);
    }
    let mut decoded = Vec::with_capacity(values.len() * 3 / 4);
    let mut chunk = [0u8; 4];
    for group in values.chunks(4) {
        if group.len() < 2 {
            break;
        }
        chunk.fill(0);
        chunk[..group.len()].copy_from_slice(group);
        decoded.push((chunk[0] << 2) | (chunk[1] >> 4));
        if group.len() >= 3 {
            decoded.push((chunk[1] << 4) | (chunk[2] >> 2));
        }
        if group.len() >= 4 {
            decoded.push((chunk[2] << 6) | chunk[3]);
        }
    }
    Some(decoded)
}

/// Windows notification listener integration.
///
/// The original Electron worker uses a short polling interval rather than
/// depending solely on NotificationChanged. This is intentional: the
/// NotificationChanged event can fail for unpackaged Win32 applications even
/// when GetNotificationsAsync works. Polling the notification collection also
/// gives us the same ID-based de-duplication and removal semantics as the
/// original worker.
pub struct NotificationService;

impl NotificationService {
    /// Request notification access from the Win32/UI thread.
    ///
    /// Windows documents RequestAccessAsync as a UI-thread operation. The old
    /// Rust implementation called it from a worker thread, so access could be
    /// silently denied before the listener ever started.
    pub fn request_access() -> windows::core::Result<UserNotificationListenerAccessStatus> {
        let listener = UserNotificationListener::Current()?;
        let current = listener.GetAccessStatus()?;

        if current == UserNotificationListenerAccessStatus::Unspecified {
            listener.RequestAccessAsync()?.get()
        } else {
            Ok(current)
        }
    }

    pub fn start(tx: Sender<NotificationPayload>, hwnd_val: isize) {
        std::thread::spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_MULTITHREADED,
                );
            }

            let listener = match UserNotificationListener::Current() {
                Ok(listener) => listener,
                Err(err) => {
                    Self::log(&format!("UserNotificationListener::Current failed: {err:?}"));
                    return;
                }
            };

            let access = match listener.GetAccessStatus() {
                Ok(status) => status,
                Err(err) => {
                    Self::log(&format!("GetAccessStatus failed: {err:?}"));
                    return;
                }
            };

            if access != UserNotificationListenerAccessStatus::Allowed {
                Self::log(&format!("Notification access is {access:?}"));
                return;
            }

            let mut seen_ids = HashSet::<u32>::new();
            // Match the Electron worker: existing toasts are history, not new
            // popups that should appear when MyIsland starts.
            Self::seed_existing(&listener, &mut seen_ids);

            // NotificationChanged is useful on packaged apps, but can return
            // ELEMENT_NOT_FOUND for unpackaged Win32 apps. Keep this worker
            // alive and use the same reliable collection scan as Electron.
            loop {
                Self::scan_and_send(&listener, &tx, hwnd_val, &mut seen_ids);
                std::thread::sleep(Duration::from_millis(150));
            }
        });
    }

    fn seed_existing(
        listener: &UserNotificationListener,
        seen_ids: &mut HashSet<u32>,
    ) {
        let Ok(op) = listener.GetNotificationsAsync(NotificationKinds::Toast) else {
            return;
        };
        let Ok(notifications) = op.get() else {
            return;
        };

        let count = notifications.Size().unwrap_or(0);
        for index in 0..count {
            if let Ok(notification) = notifications.GetAt(index) {
                if let Ok(id) = notification.Id() {
                    seen_ids.insert(id);
                }
            }
        }
    }

    fn scan_and_send(
        listener: &UserNotificationListener,
        tx: &Sender<NotificationPayload>,
        hwnd_val: isize,
        seen_ids: &mut HashSet<u32>,
    ) {
        let Ok(op) = listener.GetNotificationsAsync(NotificationKinds::Toast) else {
            return;
        };
        let Ok(notifications) = op.get() else {
            return;
        };

        let count = notifications.Size().unwrap_or(0);
        for index in 0..count {
            let Ok(notification) = notifications.GetAt(index) else {
                continue;
            };
            let Ok(id) = notification.Id() else {
                continue;
            };

            if !seen_ids.insert(id) {
                continue;
            }

            // Keep the listener alive even if one malformed toast cannot be
            // decoded. The original worker treated each notification
            // independently for the same reason.
            let payload = Self::decode_notification(&notification, id);
            let should_remove = payload.is_some();

            if let Some(payload) = payload {
                let _ = tx.send(payload);
                Self::wake_window(hwnd_val);

                // MyIsland is a replacement banner, not a duplicate toast.
                // Remove only after successfully forwarding it to the island.
                if should_remove {
                    let _ = listener.RemoveNotification(id);
                }
            }
        }

        // Bound memory if a user leaves a very large number of toasts in the
        // notification center over a long-running session.
        if seen_ids.len() > 4096 {
            seen_ids.clear();
        }
    }

    fn decode_notification(
        notification: &windows::UI::Notifications::UserNotification,
        id: u32,
    ) -> Option<NotificationPayload> {
        let mut app_name = String::from("Notification");
        let mut app_user_model_id = String::new();

        if let Ok(app_info) = notification.AppInfo() {
            if let Ok(aumid) = app_info.AppUserModelId() {
                app_user_model_id = aumid.to_string();
            }
            if let Ok(display_info) = app_info.DisplayInfo() {
                if let Ok(display_name) = display_info.DisplayName() {
                    let name = display_name.to_string();
                    if !name.trim().is_empty() {
                        app_name = name;
                    }
                }
            }
        }

        let lower_identity = format!("{} {}", app_name, app_user_model_id).to_lowercase();
        // Do not intercept notifications produced by this application itself.
        if lower_identity.contains("myisland") || lower_identity.contains("electron") {
            return None;
        }

        let mut raw_texts = Vec::new();
        let mut avatar_image = None;
        if let Ok(notification_content) = notification.Notification() {
            if let Ok(visual) = notification_content.Visual() {
                // ToastGeneric is the documented binding for standard app
                // toasts. Some apps expose more than one binding, so the
                // fallback scan keeps this compatible with older toast XML.
                if let Ok(bindings) = visual.Bindings() {
                    let binding_count = bindings.Size().unwrap_or(0);
                    for binding_index in 0..binding_count {
                        let Ok(binding) = bindings.GetAt(binding_index) else {
                            continue;
                        };
                        let Ok(text_elements) = binding.GetTextElements() else {
                            continue;
                        };
                        let text_count = text_elements.Size().unwrap_or(0);
                        for text_index in 0..text_count {
                            if let Ok(element) = text_elements.GetAt(text_index) {
                                if let Ok(text) = element.Text() {
                                    let text = text.to_string();
                                    if !text.trim().is_empty() {
                                        raw_texts.push(text.trim().to_string());
                                    }
                                }
                            }
                        }
                        if avatar_image.is_none() {
                            avatar_image = Self::binding_image(&binding);
                        }
                        if !raw_texts.is_empty() {
                            break;
                        }
                    }
                }
            }
        }

        let (clean_app, badge_app, title, subtitle, is_snip, is_call) =
            Self::classify_notification(&app_name, &app_user_model_id, &raw_texts);

        // AppInfo.DisplayInfo.GetLogo is the Windows-provided equivalent of
        // the Electron helper's real application icon lookup. Phone Link
        // forwards often include a separate per-app icon in its local cache;
        // use that for WhatsApp/Discord/etc. before falling back to Phone
        // Link's own logo.
        let icon_image = Self::resolve_app_icon(
            &clean_app,
            &app_user_model_id,
            &notification,
        );
        let file_path = if is_snip {
            Self::latest_screenshot_path()
        } else {
            None
        };

        let brand_color = if badge_app == "whatsapp" {
            [37, 211, 102, 255]
        } else if badge_app == "discord" {
            [88, 101, 242, 255]
        } else if badge_app == "snipping" {
            [100, 210, 255, 255]
        } else if badge_app == "phone" || is_call {
            [48, 209, 88, 255]
        } else if badge_app == "slack" {
            [74, 21, 75, 255]
        } else {
            [100, 210, 255, 255]
        };

        Some(NotificationPayload {
            id: id as u64,
            app_name: clean_app,
            app_user_model_id,
            badge_app,
            title,
            subtitle,
            time_text: "now".to_string(),
            is_snip,
            is_call,
            is_device: false,
            image_preview: None,
            icon_image,
            avatar_image,
            file_path,
            has_contact_avatar: false,
            brand_color,
        })
    }

    fn binding_image(
        binding: &windows::UI::Notifications::NotificationBinding,
    ) -> Option<Vec<u8>> {
        let hints = binding.Hints().ok()?;
        for pair in &hints {
            let key = pair.Key().ok()?.to_string().to_lowercase();
            if !key.contains("image")
                && !key.contains("logo")
                && !key.contains("avatar")
                && !key.contains("hero")
            {
                continue;
            }
            let value = pair.Value().ok()?.to_string();
            if let Ok(bytes) = std::fs::read(&value) {
                if bytes.len() > 100 && image::load_from_memory(&bytes).is_ok() {
                    return Some(bytes);
                }
            }
        }
        None
    }

    fn resolve_app_icon(
        clean_app: &str,
        app_user_model_id: &str,
        notification: &windows::UI::Notifications::UserNotification,
    ) -> Option<Vec<u8>> {
        if let Some(bytes) = Self::find_phone_icon(clean_app) {
            return Some(bytes);
        }

        if let Ok(app_info) = notification.AppInfo() {
            if let Ok(display_info) = app_info.DisplayInfo() {
                if let Ok(logo) = display_info.GetLogo(Size {
                    Width: 64.0,
                    Height: 64.0,
                }) {
                    if let Ok(stream) = logo.OpenReadAsync().and_then(|op| op.get()) {
                        if let Some(bytes) = Self::read_stream(&stream) {
                            if !bytes.is_empty() {
                                return Some(bytes);
                            }
                        }
                    }
                }
            }
        }

        Self::resolve_desktop_icon(clean_app, app_user_model_id)
    }

    fn resolve_desktop_icon(app_name: &str, aumid: &str) -> Option<Vec<u8>> {
        // Match icon-helper.ps1's desktop fallback for unpackaged Win32 apps:
        // locate a Start-menu/Desktop shortcut and extract its real shell icon.
        let script = r#"
Add-Type -AssemblyName System.Drawing
$shell = New-Object -ComObject WScript.Shell
$name = $env:ISLAND_ICON_APP.Trim().ToLower()
$dirs = @(
  [Environment]::GetFolderPath('Desktop'),
  [Environment]::GetFolderPath('CommonDesktopDirectory'),
  "$env:APPDATA\Microsoft\Windows\Start Menu\Programs",
  "$env:ProgramData\Microsoft\Windows\Start Menu\Programs"
)
$shortcut = $null
foreach ($dir in $dirs) {
  if (-not (Test-Path $dir)) { continue }
  $direct = Join-Path $dir "$name.lnk"
  if (Test-Path $direct) { $shortcut = $direct; break }
  foreach ($sub in (Get-ChildItem -LiteralPath $dir -Directory -ErrorAction SilentlyContinue)) {
    $candidate = Get-ChildItem -LiteralPath $sub.FullName -Filter "*$name*.lnk" -File -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($candidate) { $shortcut = $candidate.FullName; break }
  }
  if ($shortcut) { break }
}
$source = $shortcut
if ($shortcut) {
  try {
    $sc = $shell.CreateShortcut($shortcut)
    if ($sc.TargetPath -and (Test-Path $sc.TargetPath)) { $source = $sc.TargetPath }
  } catch {}
}
if (-not $source) { exit 0 }
try {
  $ico = [System.Drawing.Icon]::ExtractAssociatedIcon($source)
  if ($ico) {
    $ms = New-Object System.IO.MemoryStream
    $ico.ToBitmap().Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    [Console]::Out.Write([Convert]::ToBase64String($ms.ToArray()))
  }
} catch {}
"#;
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .env("ISLAND_ICON_APP", app_name)
            .env("ISLAND_ICON_AUMID", aumid)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let bytes = decode_base64(&output.stdout)?;
        if bytes.len() > 100 && image::load_from_memory(&bytes).is_ok() {
            Some(bytes)
        } else {
            None
        }
    }

    fn find_phone_icon(app_name: &str) -> Option<Vec<u8>> {
        let local_app_data = std::env::var_os("LOCALAPPDATA")?;
        let icon_dir = std::path::PathBuf::from(local_app_data)
            .join("Packages")
            .join("Microsoft.YourPhone_8wekyb3d8bbwe")
            .join("TempState")
            .join("app_icons");
        if !icon_dir.is_dir() {
            return None;
        }

        let clean = app_name.to_lowercase();
        let key = if clean.contains("whatsapp") {
            "whatsapp"
        } else if clean.contains("telegram") {
            "telegram"
        } else if clean.contains("instagram") {
            "instagram"
        } else if clean.contains("messenger") {
            "orca"
        } else if clean.contains("slack") {
            "slack"
        } else if clean.contains("discord") {
            "discord"
        } else {
            return None;
        };

        let entries = std::fs::read_dir(icon_dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy().to_lowercase();
            if !name.contains(key) || path.extension()?.to_string_lossy().to_lowercase() != "png" {
                continue;
            }
            let bytes = std::fs::read(path).ok()?;
            if bytes.len() > 100 && image::load_from_memory(&bytes).is_ok() {
                return Some(bytes);
            }
        }
        None
    }

    fn latest_screenshot_path() -> Option<String> {
        let pictures = std::env::var_os("USERPROFILE")
            .map(std::path::PathBuf::from)?
            .join("Pictures")
            .join("Screenshots");
        let mut latest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
        for entry in std::fs::read_dir(pictures).ok()?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("png") {
                continue;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            if latest
                .as_ref()
                .map(|(time, _)| modified > *time)
                .unwrap_or(true)
            {
                latest = Some((modified, path));
            }
        }
        latest.map(|(_, path)| path.to_string_lossy().into_owned())
    }

    fn read_stream(stream: &IRandomAccessStreamWithContentType) -> Option<Vec<u8>> {
        let size = stream.Size().ok()?.min(2 * 1024 * 1024) as u32;
        if size == 0 {
            return None;
        }
        let buffer = Buffer::Create(size).ok()?;
        let result = stream
            .ReadAsync(&buffer, size, InputStreamOptions::None)
            .ok()?
            .get()
            .ok()?;
        let reader = DataReader::FromBuffer(&result).ok()?;
        let length = reader.UnconsumedBufferLength().ok()? as usize;
        let mut bytes = vec![0u8; length];
        reader.ReadBytes(&mut bytes).ok()?;
        let _ = stream.Close();
        Some(bytes)
    }

    fn classify_notification(
        app_name: &str,
        app_user_model_id: &str,
        raw_texts: &[String],
    ) -> (String, String, String, String, bool, bool) {
        let identity = format!("{} {}", app_name, app_user_model_id).to_lowercase();
        let joined = raw_texts.join(" ");

        let is_phone_link = identity.contains("yourphone")
            || identity.contains("phonelink")
            || identity.contains("phone link")
            || identity.contains("link to windows");
        let is_call = identity.contains("call")
            || joined.to_lowercase().contains("incoming call")
            || joined.to_lowercase().contains("call from");

        if is_phone_link && is_call {
            let title = raw_texts
                .first()
                .cloned()
                .unwrap_or_else(|| "Incoming Call".to_string());
            return (
                "Calls".to_string(),
                "phone".to_string(),
                title,
                "Incoming Call • Phone Link".to_string(),
                false,
                true,
            );
        }

        if identity.contains("screensketch")
            || identity.contains("snipping")
            || identity.contains("snip")
            || app_name.to_lowercase().contains("screenshot")
        {
            return (
                "Snipping Tool".to_string(),
                "snipping".to_string(),
                "Snipping Tool".to_string(),
                "Screenshot saved to clipboard".to_string(),
                true,
                false,
            );
        }

        if identity.contains("discord") {
            let (title, body) = if raw_texts.len() >= 2 {
                (raw_texts[0].clone(), raw_texts[1].clone())
            } else {
                (
                    "Discord".to_string(),
                    raw_texts.first().cloned().unwrap_or_default(),
                )
            };
            return (
                "Discord".to_string(),
                "discord".to_string(),
                title,
                body,
                false,
                false,
            );
        }

        if identity.contains("slack") {
            let (title, body) = if raw_texts.len() >= 2 {
                (raw_texts[0].clone(), raw_texts[1].clone())
            } else {
                (
                    "Slack".to_string(),
                    raw_texts.first().cloned().unwrap_or_default(),
                )
            };
            return (
                "Slack".to_string(),
                "slack".to_string(),
                title,
                body,
                false,
                false,
            );
        }

        // Phone Link forwards often put the originating app name in the first
        // toast text, e.g. "WhatsApp", followed by contact and body text.
        if is_phone_link {
            let phone_apps = [
                ("whatsapp", "WhatsApp"),
                ("telegram", "Telegram"),
                ("instagram", "Instagram"),
                ("messages", "Messages"),
                ("sms", "SMS"),
                ("gmail", "Gmail"),
                ("snapchat", "Snapchat"),
                ("signal", "Signal"),
                ("twitter", "Twitter"),
                ("x", "X"),
                ("messenger", "Messenger"),
                ("outlook", "Outlook"),
                ("slack", "Slack"),
                ("discord", "Discord"),
                ("reddit", "Reddit"),
                ("youtube", "YouTube"),
            ];
            if let Some((badge, display)) = raw_texts.first().and_then(|first| {
                let lower = first.to_lowercase();
                phone_apps
                    .iter()
                    .find(|(key, _)| {
                        let key = key.trim();
                        lower == key || lower.starts_with(&format!("{} ", key)) || lower.starts_with(&format!("{}...", key))
                    })
                    .copied()
            }) {
                let (title, body) = if raw_texts.len() >= 3 {
                    (raw_texts[1].clone(), raw_texts[2].clone())
                } else if raw_texts.len() == 2 {
                    if let Some((name, body)) = raw_texts[1].split_once(':') {
                        (name.trim().to_string(), body.trim().to_string())
                    } else {
                        (raw_texts[1].clone(), "New message".to_string())
                    }
                } else {
                    (display.to_string(), "New notification".to_string())
                };
                return (
                    display.to_string(),
                    badge.to_string(),
                    title,
                    body,
                    false,
                    false,
                );
            }

            if raw_texts.len() >= 3 {
                return (
                    raw_texts[0].clone(),
                    raw_texts[0].to_lowercase(),
                    raw_texts[1].clone(),
                    raw_texts[2].clone(),
                    false,
                    false,
                );
            }
            if raw_texts.len() >= 2 {
                return (
                    app_name.to_string(),
                    app_name.to_lowercase(),
                    raw_texts[0].clone(),
                    raw_texts[1].clone(),
                    false,
                    false,
                );
            }
            if let Some(text) = raw_texts.first() {
                return (
                    app_name.to_string(),
                    app_name.to_lowercase(),
                    text.clone(),
                    String::new(),
                    false,
                    false,
                );
            }
        }

        let (title, body) = if raw_texts.len() >= 2 {
            (raw_texts[0].clone(), raw_texts[1].clone())
        } else if let Some(body) = raw_texts.first() {
            (app_name.to_string(), body.clone())
        } else {
            (app_name.to_string(), String::new())
        };
        let badge = app_name.to_lowercase();
        (
            app_name.to_string(),
            badge,
            title,
            body,
            false,
            is_call,
        )
    }

    fn wake_window(hwnd_val: isize) {
        if hwnd_val == 0 {
            return;
        }
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                windows::Win32::Foundation::HWND(hwnd_val as *mut _),
                windows::Win32::UI::WindowsAndMessaging::WM_NULL,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }

    fn log(message: &str) {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("notification.log")
        {
            let _ = writeln!(file, "{message}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NotificationService;

    #[test]
    fn classifies_snipping_tool() {
        let (app, badge, title, body, snip, call) =
            NotificationService::classify_notification("Snipping Tool", "Microsoft.ScreenSketch", &[]);
        assert_eq!(app, "Snipping Tool");
        assert_eq!(badge, "snipping");
        assert_eq!(title, "Snipping Tool");
        assert!(body.contains("clipboard"));
        assert!(snip);
        assert!(!call);
    }

    #[test]
    fn classifies_phone_link_whatsapp() {
        let texts = vec![
            "WhatsApp".to_string(),
            "Sarah".to_string(),
            "Hello messages in dynamic island!".to_string(),
        ];
        let (app, badge, title, body, snip, call) = NotificationService::classify_notification(
            "Phone Link",
            "Microsoft.YourPhone",
            &texts,
        );
        assert_eq!(app, "WhatsApp");
        assert_eq!(badge, "whatsapp");
        assert_eq!(title, "Sarah");
        assert_eq!(body, "Hello messages in dynamic island!");
        assert!(!snip);
        assert!(!call);
    }
}
