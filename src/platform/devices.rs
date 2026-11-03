use crossbeam_channel::Sender;
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::state::NotificationPayload;

static SCAN_REQUESTED_AT: AtomicU64 = AtomicU64::new(0);

/// Mirrors the Electron worker's Win32 device-change hook plus periodic
/// fallback scan. PnP exposes Bluetooth audio endpoints and game controllers
/// as Win32_PnPEntity records, which is also what the original PowerShell
/// implementation reads.
pub struct DeviceService;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceInfo {
    name: String,
    device_type: String,
}

impl DeviceService {
    pub fn request_scan() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        SCAN_REQUESTED_AT.store(now, Ordering::Release);
    }

    pub fn start(tx: Sender<NotificationPayload>, hwnd_val: isize) {
        std::thread::spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_MULTITHREADED,
                );
            }

            // Existing devices are the baseline. Only transitions after
            // startup should create island notifications, exactly as in the
            // Electron worker.
        let mut known = Self::scan_devices().unwrap_or_default();
            let mut next_scan = Instant::now() + Duration::from_secs(15);

            loop {
                let requested_at = SCAN_REQUESTED_AT.load(Ordering::Acquire);
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let event_scan = requested_at != 0
                    && now_ms.saturating_sub(requested_at) >= 600
                    && SCAN_REQUESTED_AT
                        .compare_exchange(requested_at, 0, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok();
                if event_scan || Instant::now() >= next_scan {
                    if let Some(current) = Self::scan_devices() {
                        Self::emit_transitions(&known, &current, &tx, hwnd_val);
                        known = current;
                    }
                    next_scan = Instant::now() + Duration::from_secs(15);
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        });
    }

    fn scan_devices() -> Option<HashMap<String, DeviceInfo>> {
        let script = r#"
$ProgressPreference = 'SilentlyContinue'
$WarningPreference = 'SilentlyContinue'
$items = @(Get-CimInstance -ClassName Win32_PnPEntity -Filter "PNPClass = 'AudioEndpoint' or PNPClass = 'XboxComposite' or PNPClass = 'XnaComposite' or PNPClass = 'HIDClass'" -ErrorAction SilentlyContinue | Where-Object {
  $_.Status -eq 'OK' -and (
    ($_.PNPClass -eq 'AudioEndpoint' -and $_.Caption -notmatch 'Realtek|High Definition Audio|Default|Microphone Array|Steam Streaming|Camo|Virtual') -or
    ($_.Caption -match 'Xbox Wireless Controller|Xbox 360 Controller|Xbox One Controller|DualSense|DualShock|Gamepad' -and $_.Caption -notmatch 'system controller|keyboard|mouse')
  )
} | ForEach-Object {
  [PSCustomObject]@{ caption = [string]$_.Caption; pnpClass = [string]$_.PNPClass }
});
        ConvertTo-Json -InputObject @($items) -Compress -Depth 3
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
            .creation_flags(0x08000000)
            .output();

        let Ok(output) = output else {
            return None;
        };
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let json_start = stdout.find('[').or_else(|| stdout.find('{'))?;
        let json_end = stdout[json_start..]
            .rfind(']')
            .map(|index| json_start + index + 1)
            .or_else(|| stdout[json_start..].rfind('}').map(|index| json_start + index + 1))?;
        let json = &stdout[json_start..json_end];
        let Ok(value) = serde_json::from_str::<Value>(json.trim()) else {
            return None;
        };

        let rows: Vec<&Value> = match &value {
            Value::Array(rows) => rows.iter().collect(),
            Value::Object(_) => vec![&value],
            _ => Vec::new(),
        };
        let mut devices = HashMap::new();
        for row in rows {
            let caption = row.get("caption").and_then(Value::as_str).unwrap_or("");
            let pnp_class = row.get("pnpClass").and_then(Value::as_str).unwrap_or("");
            let name = Self::sanitize_name(caption);
            if name.is_empty() {
                continue;
            }
            let device_type = Self::classify(&name, caption, pnp_class);
            devices.entry(name.clone()).or_insert(DeviceInfo { name, device_type });
        }
        Some(devices)
    }

    /// Exact normalization from MyIsland/island-session.ps1.
    fn sanitize_name(name: &str) -> String {
        let mut clean = name.trim().to_string();
        for prefix in ["Headset", "Headphones", "Speakers", "Microphone"] {
            let start = format!("{prefix} (");
            if clean.starts_with(&start) {
                if let Some(end) = clean.find(')') {
                    clean = clean[start.len()..end].to_string();
                    break;
                }
            }
        }
        for suffix in [
            "Hands-Free",
            "AG Audio",
            "HF Audio",
            "Stereo",
            "Avrcp Transport",
            "A2DP SNK",
            "Generic Attribute Service",
        ] {
            if let Some(index) = clean.to_lowercase().find(&suffix.to_lowercase()) {
                clean.truncate(index);
                break;
            }
        }
        if [
            "Xbox 360 Controller for Windows",
            "Bluetooth XINPUT compatible device",
            "HID-compliant game controller",
        ]
        .iter()
        .any(|needle| clean.eq_ignore_ascii_case(needle))
        {
            clean = "Xbox Wireless Controller".to_string();
        }
        clean.trim().to_string()
    }

    /// Exact classification order from the original worker.
    fn classify(clean_name: &str, original: &str, pnp_class: &str) -> String {
        let clean = clean_name.to_lowercase();
        let original = original.to_lowercase();
        if ["controller", "gamepad", "dualsense", "dualshock", "xbox"]
            .iter()
            .any(|needle| clean.contains(needle) || original.contains(needle))
        {
            return "controller".to_string();
        }
        if ["speaker", "soundbar", "soundlink"]
            .iter()
            .any(|needle| clean.contains(needle) || original.contains(needle))
            || clean.contains("echo dot")
        {
            return "speaker".to_string();
        }
        if ["buds", "headset", "headphones", "earphones", "bullets", "airpods", "headphone"]
            .iter()
            .any(|needle| clean.contains(needle) || original.contains(needle))
        {
            return "headphone".to_string();
        }
        if pnp_class.eq_ignore_ascii_case("AudioEndpoint") {
            return "headphone".to_string();
        }
        "bluetooth".to_string()
    }

    fn emit_transitions(
        previous: &HashMap<String, DeviceInfo>,
        current: &HashMap<String, DeviceInfo>,
        tx: &Sender<NotificationPayload>,
        hwnd_val: isize,
    ) {
        for (key, info) in current {
            if !previous.contains_key(key) {
                Self::send(info, true, tx, hwnd_val);
            }
        }
        for (key, info) in previous {
            if !current.contains_key(key) {
                Self::send(info, false, tx, hwnd_val);
            }
        }
    }

    fn send(info: &DeviceInfo, connected: bool, tx: &Sender<NotificationPayload>, hwnd_val: isize) {
        let id = Self::notification_id(&info.device_type, connected, &info.name);
        let payload = NotificationPayload {
            id,
            app_name: info.name.clone(),
            app_user_model_id: String::new(),
            badge_app: info.device_type.clone(),
            title: info.name.clone(),
            subtitle: if connected { "Connected" } else { "Disconnected" }.to_string(),
            time_text: String::new(),
            is_snip: false,
            is_call: false,
            is_device: true,
            image_preview: None,
            icon_image: None,
            avatar_image: None,
            file_path: None,
            has_contact_avatar: false,
            brand_color: if connected {
                [48, 209, 88, 255]
            } else {
                [255, 69, 58, 255]
            },
        };
        let _ = tx.send(payload);
        if hwnd_val != 0 {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    windows::Win32::Foundation::HWND(hwnd_val as *mut _),
                    windows::Win32::UI::WindowsAndMessaging::WM_NULL,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        }
    }

    fn notification_id(device_type: &str, connected: bool, name: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        device_type.hash(&mut hasher);
        connected.hash(&mut hasher);
        name.to_lowercase().hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::DeviceService;

    #[test]
    fn normalizes_audio_endpoint_name() {
        assert_eq!(
            DeviceService::sanitize_name("Headphones (Quiet Buds) Stereo"),
            "Quiet Buds"
        );
        assert_eq!(
            DeviceService::sanitize_name("HID-compliant game controller"),
            "Xbox Wireless Controller"
        );
    }

    #[test]
    fn preserves_original_device_classification_order() {
        assert_eq!(
            DeviceService::classify("Xbox Wireless Controller", "Xbox Wireless Controller", "HIDClass"),
            "controller"
        );
        assert_eq!(
            DeviceService::classify("SoundLink Mini", "SoundLink Mini", "AudioEndpoint"),
            "speaker"
        );
        assert_eq!(
            DeviceService::classify("Unknown BT Audio", "Unknown BT Audio", "AudioEndpoint"),
            "headphone"
        );
    }
}
