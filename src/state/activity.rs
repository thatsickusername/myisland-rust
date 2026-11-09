use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityType {
    Music,
    Focus,
    Sports,
}

#[derive(Debug, Clone)]
pub struct SportsState {
    pub home_team: String,
    pub home_score: u32,
    pub home_color: [u8; 4],
    pub home_logo: Option<Vec<u8>>,
    pub away_team: String,
    pub away_score: u32,
    pub away_color: [u8; 4],
    pub away_logo: Option<Vec<u8>>,
    pub clock: String,
    pub is_live: bool,
    pub is_upcoming: bool,
    pub latest_event: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct NotificationPayload {
    pub id: u64,
    pub app_name: String,
    pub app_user_model_id: String,
    pub badge_app: String,
    pub title: String,
    pub subtitle: String,
    pub time_text: String,
    pub is_snip: bool,
    pub is_call: bool,
    pub is_device: bool,
    pub image_preview: Option<Vec<u8>>,
    /// The notification's actual app/logo image, when Windows exposes one.
    pub icon_image: Option<Vec<u8>>,
    /// A toast-provided contact/avatar image. This takes precedence over the
    /// app icon in the capsule, matching the Electron renderer.
    pub avatar_image: Option<Vec<u8>>,
    pub file_path: Option<String>,
    pub has_contact_avatar: bool,
    pub brand_color: [u8; 4],
}

pub struct ActivityManager {
    pub stack: Vec<ActivityType>,
    pub focus_reward_until: Option<Instant>,
    pub active_notification: Option<NotificationPayload>,
    pub notification_queue: Vec<NotificationPayload>,
    pub notification_until: Option<Instant>,
    pub is_notification_expanded: bool,
}

impl ActivityManager {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            focus_reward_until: None,
            active_notification: None,
            notification_queue: Vec::new(),
            notification_until: None,
            is_notification_expanded: false,
        }
    }

    pub fn update_music(&mut self, is_playing: bool) {
        if is_playing {
            if !self.stack.contains(&ActivityType::Music) {
                self.stack.push(ActivityType::Music);
            }
        } else {
            self.stack.retain(|a| *a != ActivityType::Music);
        }
    }

    pub fn update_focus(&mut self, is_running: bool) {
        if is_running {
            if !self.stack.contains(&ActivityType::Focus) {
                self.stack.push(ActivityType::Focus);
            }
        } else {
            self.stack.retain(|a| *a != ActivityType::Focus);
        }
    }

    pub fn update_sports(&mut self, is_live: bool) {
        if is_live {
            if !self.stack.contains(&ActivityType::Sports) {
                self.stack.push(ActivityType::Sports);
            }
        } else {
            self.stack.retain(|a| *a != ActivityType::Sports);
        }
    }

    pub fn trigger_focus_celebration(&mut self) {
        self.focus_reward_until = Some(Instant::now() + std::time::Duration::from_millis(3800));
    }

    pub fn get_activities(&self) -> (Option<ActivityType>, Option<ActivityType>) {
        if let Some(reward_time) = self.focus_reward_until {
            if Instant::now() < reward_time {
                let music_running = self.stack.contains(&ActivityType::Music);
                return (
                    Some(ActivityType::Focus),
                    if music_running {
                        Some(ActivityType::Music)
                    } else {
                        None
                    },
                );
            }
        }

        // Live sports only renders on the main island; if active, prioritize
        // it as primary so any secondary activity (Music/Focus) gets the side island.
        if self.stack.contains(&ActivityType::Sports) {
            let other = self.stack.iter().rev().copied().find(|&a| a != ActivityType::Sports);
            return (Some(ActivityType::Sports), other);
        }

        match self.stack.len() {
            0 => (None, None),
            1 => (Some(self.stack[0]), None),
            _ => (
                Some(self.stack[self.stack.len() - 1]),
                Some(self.stack[self.stack.len() - 2]),
            ),
        }
    }

    pub fn push_notification(&mut self, notif: NotificationPayload) {
        // The listener can report the same item more than once while the
        // notification is being removed. Do not show or queue duplicates.
        if self.active_notification.as_ref().map(|n| n.id) == Some(notif.id)
            || self.notification_queue.iter().any(|n| n.id == notif.id)
        {
            return;
        }

        if self.active_notification.is_none() {
            self.show_notification(notif);
        } else {
            self.notification_queue.push(notif);
        }
    }

    fn show_notification(&mut self, notif: NotificationPayload) {
        self.active_notification = Some(notif);
        self.is_notification_expanded = false;
        self.notification_until = Some(Instant::now() + std::time::Duration::from_millis(6000));
    }

    pub fn dismiss_notification(&mut self) {
        self.active_notification = None;
        self.is_notification_expanded = false;
        self.notification_until = None;

        if !self.notification_queue.is_empty() {
            let next = self.notification_queue.remove(0);
            self.show_notification(next);
        }
    }

    pub fn check_notification_expiry(&mut self) -> bool {
        if let Some(until) = self.notification_until {
            if Instant::now() >= until {
                self.dismiss_notification();
                return true;
            }
        }
        false
    }
}
