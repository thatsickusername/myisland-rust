use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityType {
    Music,
    Focus,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct NotificationPayload {
    pub id: u64,
    pub app_name: String,
    pub title: String,
    pub subtitle: String,
    pub time_text: String,
    pub is_snip: bool,
    pub is_call: bool,
    pub is_device: bool,
    pub image_preview: Option<Vec<u8>>,
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
