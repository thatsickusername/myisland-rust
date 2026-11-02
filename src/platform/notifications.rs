use crossbeam_channel::Sender;
use crate::state::NotificationPayload;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::NotificationKinds;

pub struct NotificationService;

impl NotificationService {
    pub fn start(tx: Sender<NotificationPayload>, hwnd_val: isize) {
        std::thread::spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_MULTITHREADED,
                );
            }

            if let Ok(listener) = UserNotificationListener::Current() {
                if let Ok(access_op) = listener.RequestAccessAsync() {
                    if let Ok(status) = access_op.get() {
                        if status == UserNotificationListenerAccessStatus::Allowed {
                            let tx_clone = tx.clone();
                            let l_clone = listener.clone();

                            let _ = listener.NotificationChanged(&TypedEventHandler::new(
                                move |_listener: &Option<UserNotificationListener>, _args| {
                                    Self::fetch_latest_and_send(&l_clone, &tx_clone, hwnd_val);
                                    Ok(())
                                },
                            ));
                        }
                    }
                }
            }
        });
    }

    fn fetch_latest_and_send(
        listener: &UserNotificationListener,
        tx: &Sender<NotificationPayload>,
        hwnd_val: isize,
    ) {
        if let Ok(notifs_op) = listener.GetNotificationsAsync(NotificationKinds::Toast) {
            if let Ok(notifs) = notifs_op.get() {
                let count = notifs.Size().unwrap_or(0);
                if count > 0 {
                    if let Ok(item) = notifs.GetAt(count - 1) {
                        let id = item.Id().unwrap_or(0) as u64;

                        // Extract text elements from toast binding
                        let mut title = "Notification".to_string();
                        let mut subtitle = String::new();

                        if let Ok(toast) = item.Notification() {
                            if let Ok(visual) = toast.Visual() {
                                if let Ok(bindings) = visual.Bindings() {
                                    let b_count = bindings.Size().unwrap_or(0);
                                    for i in 0..b_count {
                                        if let Ok(binding) = bindings.GetAt(i) {
                                            if let Ok(text_elements) = binding.GetTextElements() {
                                                let t_count = text_elements.Size().unwrap_or(0);
                                                if t_count > 0 {
                                                    if let Ok(el0) = text_elements.GetAt(0) {
                                                        title = el0.Text().map(|h| h.to_string()).unwrap_or(title);
                                                    }
                                                }
                                                if t_count > 1 {
                                                    if let Ok(el1) = text_elements.GetAt(1) {
                                                        subtitle = el1.Text().map(|h| h.to_string()).unwrap_or_default();
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        let clean_title = title.to_lowercase();
                        let is_snip = clean_title.contains("snip") || clean_title.contains("screenshot");
                        let is_call = clean_title.contains("call") || clean_title.contains("phone");

                        let brand_color = if clean_title.contains("whatsapp") {
                            [37, 211, 102, 255]
                        } else if clean_title.contains("discord") {
                            [88, 101, 242, 255]
                        } else if is_call {
                            [48, 209, 88, 255]
                        } else {
                            [100, 210, 255, 255]
                        };

                        let payload = NotificationPayload {
                            id,
                            app_name: title.clone(),
                            title,
                            subtitle,
                            time_text: "now".to_string(),
                            is_snip,
                            is_call,
                            is_device: false,
                            image_preview: None,
                            brand_color,
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
                }
            }
        }
    }
}
