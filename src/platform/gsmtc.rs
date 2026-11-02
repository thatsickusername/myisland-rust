use crossbeam_channel::Sender;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    MediaPropertiesChangedEventArgs,
    PlaybackInfoChangedEventArgs,
    TimelinePropertiesChangedEventArgs,
};
use windows::Storage::Streams::DataReader;

#[derive(Debug, Clone)]
pub struct MediaState {
    pub is_playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub last_sync_at: Instant,
    pub thumbnail_bytes: Option<Vec<u8>>,
    pub session_key: String,
}

impl Default for MediaState {
    fn default() -> Self {
        Self {
            is_playing: false,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            progress_ms: 0,
            duration_ms: 0,
            last_sync_at: Instant::now(),
            thumbnail_bytes: None,
            session_key: String::new(),
        }
    }
}

#[allow(dead_code)]
pub enum MediaControlCmd {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Seek(u64),
}

pub struct GsmtcService {
    cmd_tx: Sender<MediaControlCmd>,
}

impl GsmtcService {
    pub fn start(
        state_tx: Sender<MediaState>,
        hwnd_val: isize,
    ) -> windows::core::Result<Self> {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();

        std::thread::spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_MULTITHREADED,
                );
            }

            if let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .and_then(|op| op.get())
            {
                let subscribed = Arc::new(Mutex::new(HashSet::<String>::new()));

                // Initial subscription & fetch
                Self::subscribe_all_sessions(&manager, &state_tx, hwnd_val, &subscribed);
                Self::poll_and_send(&manager, &state_tx, hwnd_val, &subscribed);

                // Event subscription: CurrentSessionChanged
                let state_tx_clone = state_tx.clone();
                let mgr_clone = manager.clone();
                let sub_clone = subscribed.clone();
                let _ = manager.CurrentSessionChanged(&TypedEventHandler::new(
                    move |_mgr: &Option<GlobalSystemMediaTransportControlsSessionManager>, _args| {
                        Self::subscribe_all_sessions(&mgr_clone, &state_tx_clone, hwnd_val, &sub_clone);
                        Self::poll_and_send(&mgr_clone, &state_tx_clone, hwnd_val, &sub_clone);
                        Ok(())
                    },
                ));

                // Command listener loop with long safety-heartbeat (events provide <1ms push)
                loop {
                    match cmd_rx.recv_timeout(std::time::Duration::from_secs(3)) {
                        Ok(cmd) => {
                            if let Some(session) = Self::pick_session(&manager) {
                                match cmd {
                                    MediaControlCmd::Play => {
                                        let _ = session.TryPlayAsync().map(|op| op.get());
                                    }
                                    MediaControlCmd::Pause => {
                                        let _ = session.TryPauseAsync().map(|op| op.get());
                                    }
                                    MediaControlCmd::Toggle => {
                                        let _ = session.TryTogglePlayPauseAsync().map(|op| op.get());
                                    }
                                    MediaControlCmd::Next => {
                                        let _ = session.TrySkipNextAsync().map(|op| op.get());
                                    }
                                    MediaControlCmd::Previous => {
                                        let _ = session.TrySkipPreviousAsync().map(|op| op.get());
                                    }
                                    MediaControlCmd::Seek(ms) => {
                                        let pos_100ns = (ms * 10_000) as i64;
                                        let _ = session
                                            .TryChangePlaybackPositionAsync(pos_100ns)
                                            .map(|op| op.get());
                                    }
                                }
                                Self::poll_and_send(&manager, &state_tx, hwnd_val, &subscribed);
                            }
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                            // Periodic sync to refresh sessions and timeline drift
                            Self::subscribe_all_sessions(&manager, &state_tx, hwnd_val, &subscribed);
                            Self::poll_and_send(&manager, &state_tx, hwnd_val, &subscribed);
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                    }
                }
            }
        });

        Ok(Self { cmd_tx })
    }

    fn subscribe_session(
        session: &GlobalSystemMediaTransportControlsSession,
        manager: &GlobalSystemMediaTransportControlsSessionManager,
        state_tx: &Sender<MediaState>,
        hwnd_val: isize,
        subscribed: &Arc<Mutex<HashSet<String>>>,
    ) {
        let app_id = session
            .SourceAppUserModelId()
            .map(|h| h.to_string())
            .unwrap_or_default();

        if app_id.is_empty() {
            return;
        }

        let mut sub_lock = subscribed.lock().unwrap();
        if sub_lock.insert(app_id) {
            let tx1 = state_tx.clone();
            let mgr1 = manager.clone();
            let sub1 = subscribed.clone();
            let _ = session.MediaPropertiesChanged(&TypedEventHandler::new(
                move |_s: &Option<GlobalSystemMediaTransportControlsSession>, _args: &Option<MediaPropertiesChangedEventArgs>| {
                    Self::poll_and_send(&mgr1, &tx1, hwnd_val, &sub1);
                    Ok(())
                },
            ));

            let tx2 = state_tx.clone();
            let mgr2 = manager.clone();
            let sub2 = subscribed.clone();
            let _ = session.PlaybackInfoChanged(&TypedEventHandler::new(
                move |_s: &Option<GlobalSystemMediaTransportControlsSession>, _args: &Option<PlaybackInfoChangedEventArgs>| {
                    Self::poll_and_send(&mgr2, &tx2, hwnd_val, &sub2);
                    Ok(())
                },
            ));

            let tx3 = state_tx.clone();
            let mgr3 = manager.clone();
            let sub3 = subscribed.clone();
            let _ = session.TimelinePropertiesChanged(&TypedEventHandler::new(
                move |_s: &Option<GlobalSystemMediaTransportControlsSession>, _args: &Option<TimelinePropertiesChangedEventArgs>| {
                    Self::poll_and_send(&mgr3, &tx3, hwnd_val, &sub3);
                    Ok(())
                },
            ));
        }
    }

    fn subscribe_all_sessions(
        manager: &GlobalSystemMediaTransportControlsSessionManager,
        state_tx: &Sender<MediaState>,
        hwnd_val: isize,
        subscribed: &Arc<Mutex<HashSet<String>>>,
    ) {
        if let Ok(sessions) = manager.GetSessions() {
            let count = sessions.Size().unwrap_or(0);
            for i in 0..count {
                if let Ok(s) = sessions.GetAt(i) {
                    Self::subscribe_session(&s, manager, state_tx, hwnd_val, subscribed);
                }
            }
        }
        if let Ok(current) = manager.GetCurrentSession() {
            Self::subscribe_session(&current, manager, state_tx, hwnd_val, subscribed);
        }
    }

    fn pick_session(
        manager: &GlobalSystemMediaTransportControlsSessionManager,
    ) -> Option<windows::Media::Control::GlobalSystemMediaTransportControlsSession> {
        if let Ok(sessions) = manager.GetSessions() {
            let count = sessions.Size().unwrap_or(0);
            let mut playing_session = None;
            let mut spotify_session = None;

            for i in 0..count {
                if let Ok(s) = sessions.GetAt(i) {
                    let is_playing = s
                        .GetPlaybackInfo()
                        .and_then(|info| info.PlaybackStatus())
                        .map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
                        .unwrap_or(false);

                    let app_id = s
                        .SourceAppUserModelId()
                        .map(|h| h.to_string().to_lowercase())
                        .unwrap_or_default();

                    if is_playing {
                        if app_id.contains("spotify") {
                            return Some(s);
                        }
                        if playing_session.is_none() {
                            playing_session = Some(s.clone());
                        }
                    }

                    if app_id.contains("spotify") && spotify_session.is_none() {
                        spotify_session = Some(s.clone());
                    }
                }
            }

            if let Some(s) = playing_session {
                return Some(s);
            }
            if let Some(s) = spotify_session {
                return Some(s);
            }
        }

        manager.GetCurrentSession().ok()
    }

    fn poll_and_send(
        manager: &GlobalSystemMediaTransportControlsSessionManager,
        state_tx: &Sender<MediaState>,
        hwnd_val: isize,
        subscribed: &Arc<Mutex<HashSet<String>>>,
    ) {
        if let Some(session) = Self::pick_session(manager) {
            Self::subscribe_session(&session, manager, state_tx, hwnd_val, subscribed);
            let mut state = MediaState::default();

            if let Ok(info) = session.GetPlaybackInfo() {
                if let Ok(status) = info.PlaybackStatus() {
                    state.is_playing = status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
                }
            }

            if let Ok(props_op) = session.TryGetMediaPropertiesAsync() {
                if let Ok(props) = props_op.get() {
                    state.title = props.Title().map(|h| h.to_string()).unwrap_or_default();
                    state.artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
                    state.album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
                    state.session_key = format!("{}|{}", state.title, state.artist);

                    // Read thumbnail stream directly into bytes
                    if let Ok(thumb_ref) = props.Thumbnail() {
                        if let Ok(stream_op) = thumb_ref.OpenReadAsync() {
                            if let Ok(stream) = stream_op.get() {
                                if let Ok(size) = stream.Size() {
                                    if size > 0 && size < 10_000_000 {
                                        if let Ok(reader) = DataReader::CreateDataReader(&stream) {
                                            if let Ok(load_op) = reader.LoadAsync(size as u32) {
                                                if load_op.get().is_ok() {
                                                    let mut buf = vec![0u8; size as usize];
                                                    if reader.ReadBytes(&mut buf).is_ok() {
                                                        state.thumbnail_bytes = Some(buf);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Ok(timeline) = session.GetTimelineProperties() {
                let mut raw_pos = 0u64;
                if let Ok(pos) = timeline.Position() {
                    raw_pos = (pos.Duration / 10_000).max(0) as u64;
                }
                if let Ok(end) = timeline.EndTime() {
                    state.duration_ms = (end.Duration / 10_000).max(0) as u64;
                }

                if state.is_playing {
                    if let Ok(updated) = timeline.LastUpdatedTime() {
                        let updated_ticks = updated.UniversalTime;
                        if updated_ticks > 0 {
                            let filetime = unsafe {
                                let ft = windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime();
                                ((ft.dwHighDateTime as i64) << 32) | (ft.dwLowDateTime as u32 as i64)
                            };
                            if filetime > updated_ticks {
                                let elapsed_ms = ((filetime - updated_ticks) / 10_000) as u64;
                                raw_pos = raw_pos.saturating_add(elapsed_ms);
                            }
                        }
                    }
                }

                state.progress_ms = if state.duration_ms > 0 {
                    raw_pos.min(state.duration_ms)
                } else {
                    raw_pos
                };
            }

            state.last_sync_at = Instant::now();
            let _ = state_tx.send(state);
        } else {
            let _ = state_tx.send(MediaState::default());
        }

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

    pub fn send_command(&self, cmd: MediaControlCmd) {
        let _ = self.cmd_tx.send(cmd);
    }
}
