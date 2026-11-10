pub mod win32;
pub mod gsmtc;
pub mod volume;
pub mod notifications;
pub mod devices;
pub mod fullscreen;
pub mod audio_capture;
pub mod sports;
pub mod haptics;
pub use sports::{SportsEvent, SportsService};

pub use win32::Win32Window;
pub use gsmtc::{GsmtcService, MediaState, MediaControlCmd};
pub use volume::VolumeManager;
pub use notifications::NotificationService;
pub use devices::DeviceService;
pub use fullscreen::FullscreenDetector;
pub use audio_capture::AudioCaptureService;
