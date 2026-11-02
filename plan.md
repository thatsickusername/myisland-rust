# MyIsland-Rust: Production-Grade Native Windows Island Specification
> **Architectural Target:** 1 single OS process | **~25–35 MB RAM** | **0.0% idle CPU** | **< 4 MB standalone .exe**  
> **Target Platform:** Windows 10 (Build 19041+) & Windows 11 (x86_64 / aarch64)  
> **Source Reference Codebase:** `../MyIsland` (Full feature parity, zero compromise)

---

## 1. Executive Vision & Non-Negotiables

MyIsland is being ported from Electron to native Rust to eliminate all V8, Chromium, Node.js, and child-PowerShell overhead. The resulting binary must feel like a native Windows kernel/shell component: instant startup (< 50ms), zero memory bloat, zero background CPU cycles, and 100% event-driven push architecture.

### The Target Metrics (The "Nusa" Standards)
- **Processes:** Exactly **1 process** in Task Manager (no subprocesses, no helper daemons, no PowerShell).
- **RAM Footprint:** **~25–35 MB** in active compact mode; **< 20 MB** when tucked.
- **CPU Footprint:** **0.0%** when idle/still. Rendering halts completely when springs settle.
- **Binary Size:** **< 4 MB** standalone single-file `.exe` (via `cargo build --release` with LTO and symbol stripping).
- **Architecture:** 100% native Win32 message pump + WinRT event subscriptions via Microsoft's official `windows` crate.

---

## 2. Core Technology Stack

| Layer | Selected Crates | Rationale / Performance Guarantee |
| :--- | :--- | :--- |
| **Language & Toolchain** | Rust 2021 Edition, MSVC toolchain | Direct C-ABI interop, zero garbage collection, zero runtime. |
| **OS Windowing & Event Loop** | `windows` (v0.58+) | Direct Microsoft-maintained Win32 & WinRT bindings. `CreateWindowExW`, `MsgWaitForMultipleObjectsEx`, `GetMessageW`. |
| **2D GPU Rendering** | `femtovg` (with OpenGL/WGL or Direct3D) OR native `Direct2D` / `DirectWrite` via `windows::Win32::Graphics::Direct2D` | GPU-accelerated antialiased vector paths, quadratic beziers, gaussian drop shadows, and text layouts without Chromium. Draws directly to per-pixel alpha swapchain. |
| **WinRT Subscriptions** | `windows::Media::Control`, `windows::UI::Notifications::Management`, `windows::System::Power` | Native COM/WinRT event-driven async callbacks (no polling loops). |
| **Audio Loopback Visualizer**| `windows::Win32::Media::Audio` (WASAPI Loopback) | Native endpoint capture for real-time visualizer without screen recording or web audio overhead. |
| **Image & Icon Decoding** | `image` (PNG/JPEG) + native Win32 `ExtractIconExW` / `SHGetFileInfoW` | In-memory icon extraction directly from `.exe` or `.lnk` without base64 serialization. |
| **Font & Text Measurement** | `fontdue` / `cosmic-text` OR `DirectWrite` | Sub-millisecond text measurement for synced lyrics and compact width calculations. |

---

## 3. Window Architecture & Layered Alpha Transparency

### 3.1 Win32 Window Setup
Create a borderless, top-anchored, click-through capable window:
```rust
// Window Styles:
let ex_style = WS_EX_TOPMOST 
             | WS_EX_TOOLWINDOW            // Do not show in Taskbar or Alt-Tab
             | WS_EX_LAYERED               // Per-pixel alpha transparency
             | WS_EX_NOACTIVATE;           // Do not steal focus from active windows

let style = WS_POPUP;                       // Frameless, borderless
```

### 3.2 Dynamic Interactive Hit-Testing
- Use `WS_EX_TRANSPARENT` dynamically or intercept `WM_NCHITTEST`:
  - When the cursor is outside the visible island polygon: return `HTTRANSPARENT` (clicks pass through seamlessly to desktop/apps beneath).
  - When the cursor is inside the active island bounds: return `HTCLIENT` (enables clicks, drag, wheel scrolling).
- When tucked, the hit-test zone is restricted strictly to a 4px strip at the top bezel.

### 3.3 Zero-Idle Render Gating (Dirty Rect & Halt)
- **Do not run a continuous game loop.**
- The application maintains a boolean `is_animating`.
- A frame is rendered **only** if:
  1. Any spring has `velocity.abs() > 0.001` or `(value - target).abs() > 0.001`.
  2. Confetti particles are active.
  3. Mascot eyes are transitioning.
  4. Audio visualizer is playing.
- Once all springs settle and particles expire, the render loop stops completely. The thread enters `GetMessageW` waiting for the next OS event or timer.

---

## 4. Physics Engine & Exact Curvature Mathematics

All spring physics and curves from `MyIsland/index.html` must be ported 1:1.

### 4.1 2nd-Order Euler Spring Struct
```rust
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32,  // Default: 190.0
    pub damping: f32,    // Default: 22.0
}

impl Spring {
    pub fn new(val: f32, stiffness: f32, damping: f32) -> Self {
        Self { value: val, target: val, velocity: 0.0, stiffness, damping }
    }

    pub fn update(&mut self, dt: f32) -> bool {
        let clamped_dt = dt.min(0.04);
        let force = -self.stiffness * (self.value - self.target);
        let damping_force = -self.damping * self.velocity;
        let acceleration = force + damping_force;
        
        self.velocity += acceleration * clamped_dt;
        self.value += self.velocity * clamped_dt;
        
        // Return true if still moving
        (self.velocity.abs() > 0.002) || ((self.value - self.target).abs() > 0.002)
    }
}
```

### 4.2 Exact Top-Bezel Ear Curve Algorithm
Ported directly from `MyIsland/index.html` (`islandPath`):
```rust
pub fn draw_island_path(path: &mut Path, x: f32, y: f32, width: f32, height: f32, radius: f32) {
    let r = radius.min(width / 2.0).min(height).max(0.0);
    // Symmetrical 10px flare: width / 2.0 ensures secondary 44px pill matches main island
    let ear = 10.0f32.min(height / 3.0).min(width / 2.0).max(0.0);

    path.move_to(x - ear, y);
    path.line_to(x + width + ear, y);
    path.quad_to(x + width, y, x + width, y + ear);
    path.line_to(x + width, y + height - r);
    path.arc_to(x + width, y + height, x + width - r, y + height, r);
    path.line_to(x + r, y + height);
    path.arc_to(x, y + height, x, y + height - r, r);
    path.line_to(x, y + ear);
    path.quad_to(x, y, x - ear, y);
    path.close();
}
```

### 4.3 Target Dimensions State Machine
- `Rest`: `w: 116.0, h: 32.0, r: 16.0` (Idle companion notch)
- `Compact`: `w: 300.0, h: 36.0, r: 18.0` (Media, focus, volume, notifications)
- `Expanded`: `w: 520.0, h: 170.0, r: 30.0` (Expanded app cards)
- `Tucked`: `w: 56.0, h: 4.0, r: 2.0` (Fullscreen tucked mode)
- `Secondary Bubble`: `w: 44.0, h: 36.0, r: 18.0` (Positioned `x + w + 28.0`)

---

## 5. Event-Driven Windows Subsystems (Zero-Polling)

### 5.1 Media Transport (GSMTC) via WinRT
Replace `island-session.ps1` polling with direct WinRT callbacks:
```rust
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;

// 1. Request session manager asynchronously once on startup
let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;

// 2. Subscribe to OS push events (zero background CPU!)
manager.CurrentSessionChanged(&TypedEventHandler::new(|mgr, _| {
    // Fired only when track or active player changes
    // Fetch track, artist, album, timeline, and thumbnail stream
    Ok(())
}))?;
```
- **Thumbnail Stream:** Read directly from `IRandomAccessStreamWithContentType` into memory bytes (zero disk write, zero base64 encoding).
- **Controls:** Direct WinRT methods `session.TryPlayAsync()`, `TryPauseAsync()`, `TrySkipNextAsync()`, `TrySkipPreviousAsync()`, `TryChangePlaybackPositionAsync()`.

### 5.2 Audio Endpoint Volume & Peak Metering
- Use Core Audio APIs: `IMMDeviceEnumerator` -> `GetDefaultAudioEndpoint` -> `IAudioEndpointVolume`.
- Register `IAudioEndpointVolumeCallback`: Windows calls back **only when volume or mute changes**.
- For visualizer: Use `IAudioMeterInformation::GetPeakValue` or `IAudioCaptureClient` WASAPI loopback buffer directly into a 6-band FFT.

### 5.3 System Notifications (Toasts) via UserNotificationListener
- Request access once: `UserNotificationListener::Current()?.RequestAccessAsync()`.
- Subscribe to `NotificationChanged` event handler.
- On notification arrival:
  - Extract App Name, Title, Body, and Hero Image stream.
  - **Snipping Tool Integration:** If app is `ScreenSketch` or `SnippingTool`, instantly open the clipboard image stream and present thumbnail in the notification card with one-click full-screen photo viewing.
  - Call `listener.RemoveNotification(id)` to suppress redundant Windows Action Center banners if desired.

### 5.4 Fullscreen Detection & Auto-Tuck
Implement the exact empirical signature discovered in our diagnostics:
- Poll foreground window or listen via `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`:
  ```rust
  pub fn is_window_fullscreen(hwnd: HWND) -> bool {
      // 1. Exclude Desktop / Shell / Self
      let class = get_window_class(hwnd);
      if class == "Progman" || class == "WorkerW" || class == "Shell_TrayWnd" || class.contains("XamlExplorerHostIslandWindow") {
          return false;
      }
      
      // 2. Direct3D Fullscreen check
      let mut quns = 0;
      unsafe { SHQueryUserNotificationState(&mut quns) };
      if quns == 3 { return true; } // QUNS_RUNNING_D3D_FULL_SCREEN

      // 3. Exact Geometry Check (Covers Monitor + Covers Taskbar)
      let w_rect = get_window_rect(hwnd);
      let m_rect = get_monitor_rect(hwnd);
      let covers_mon = w_rect.left <= m_rect.left && w_rect.top <= m_rect.top 
                    && w_rect.right >= m_rect.right && w_rect.bottom >= m_rect.bottom;
      let covers_tray = is_taskbar_covered(w_rect);

      // 4. Style Check (Dropped borders & caption)
      let style = get_window_long(hwnd, GWL_STYLE);
      let has_caption = (style & WS_CAPTION) != 0;
      let has_thick_frame = (style & WS_THICKFRAME) != 0;
      let is_popup = (style & WS_POPUP) != 0;

      covers_mon && covers_tray && (!has_caption && !has_thick_frame || is_popup)
  }
  ```
- **Auto-Tuck Execution:**
  - Transition to `tucked` (`56 × 4, r: 2`).
  - Notch hover peeks open to `compact`; cursor leave re-tucks.
  - Exiting fullscreen restores the exact prior state and calls `settle_size()`.

---

## 6. Complete Feature Parity Checklist

The newly generated `myisland-rust` application must implement all features present in `MyIsland`:

1. [ ] **Dual Activity Priority Stack:** Handles music and focus timer running simultaneously (primary island + secondary detached 44px bubble with 28px gap and smooth liquid bridge when merging).
2. [ ] **Synced Lyrics Engine:**
   - Multi-source fetchers with LRU caching.
   - Dynamic width adjustment: `Math.max(320, Math.min(500, lyrics_widest + 120))`.
   - Smooth vertical line transitions with bezier easing.
   - Preserves lyric width across focus timer completion celebration without clipping.
3. [ ] **Focus Timer & Celebration:**
   - Circular countdown progress ring in compact mode.
   - 3.8s celebration with confetti particle physics and mascot victory eyes.
   - Synthesizer chime sound (`Beep` or WASAPI mini-synth).
4. [ ] **Companion Mascot (Island Eyes):**
   - Natural blinking intervals, smooth eye saccades tracking mouse coordinates.
   - Context states: happy, listening to music, celebratory party eyes, sleepy idle variations.
5. [ ] **Volume HUD Slider:**
   - Appears on hardware volume key press or wheel scroll.
   - Smooth spring fill animation with speaker cone / wave status icon.
6. [ ] **Expanded App Panels:**
   - Music Player with progress scrubber and media controls.
   - Focus Timer duration ruler.
   - Notes pad and Clipboard Manager with search and click-to-copy.
   - System monitoring (battery level, CPU/memory stats).
7. [ ] **Context Menu:**
   - Right-click menu for Quick Settings, Tuck Away, and Exit.
8. [ ] **System Tray Icon:**
   - Windows notification tray icon with options to show/hide, toggle tuck, and quit.

---

## 7. Recommended Project Layout (`myisland-rust`)

```
myisland-rust/
├── Cargo.toml                  # Configured with lto = true, opt-level = "z", strip = true
├── build.rs                    # Embeds Windows application manifest & high-DPI awareness
├── resources/
│   ├── app.ico                 # High-resolution application icon
│   └── app.manifest            # Win32 DPI awareness & Windows 10/11 compatibility
├── src/
│   ├── main.rs                 # Win32 message pump, window lifecycle, initialization
│   ├── config.rs               # Settings, color themes, dimension constants
│   ├── physics/
│   │   ├── mod.rs
│   │   ├── spring.rs           # 2nd-order Euler physics springs
│   │   └── particles.rs        # Confetti celebration particle engine
│   ├── platform/
│   │   ├── mod.rs
│   │   ├── win32.rs            # Win32 window creation, layered alpha, DWM, hooks
│   │   ├── gsmtc.rs            # WinRT GlobalSystemMediaTransportControls listener
│   │   ├── volume.rs           # Core Audio endpoint & volume callbacks
│   │   ├── notifications.rs    # UserNotificationListener & Snipping Tool capture
│   │   └── fullscreen.rs       # Native geometry & shell state fullscreen evaluator
│   ├── render/
│   │   ├── mod.rs
│   │   ├── backend.rs          # GPU context initialization (Direct2D / femtovg)
│   │   ├── curves.rs           # Symmetrical top-bezel ear flares & liquid bridge
│   │   ├── mascot.rs           # Companion eyes vector drawing & blink logic
│   │   └── visualizer.rs       # Compact & mini visualizer bars
│   ├── state/
│   │   ├── mod.rs
│   │   ├── activity.rs         # Activity stack (Music vs Focus vs Volume priority)
│   │   ├── lyrics.rs           # Synced lyrics fetcher, LRU cache, and timing
│   │   └── clipboard.rs        # Native clipboard history & notes storage
│   └── ui/
│       ├── mod.rs
│       ├── compact.rs          # Compact pill layout renderer
│       ├── expanded.rs         # Expanded card & panel layouts
│       └── tray.rs             # Native shell notification tray icon & context menu
```

---

## 8. Master Agent Execution Directive

When an AI developer takes on this implementation, they must execute according to the following principles:

1. **Do not create hybrid wrappers:** Do NOT use Tauri, WRY, or Webview2. The entire premise of this port is **zero browser engine**. All vector graphics are drawn natively on the GPU via Direct2D or `femtovg`.
2. **Zero Polling Loops:** Use native Windows COM/WinRT events. Never call `thread::sleep(100)` in an active loop to query media or volume. The OS must wake the process.
3. **Reference the Existing Math:** All dimensions, ear curves, spring constants, and timing offsets in `../MyIsland/index.html` have been user-tested and visually perfected. Port the mathematical formulas directly.
4. **Single Command Verification:** The project must build cleanly with `cargo build --release` and run as a standalone `.exe` with zero runtime dependencies.
