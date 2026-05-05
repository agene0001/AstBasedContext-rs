// notification_service.rs — Notification service using core_utils.

use crate::core_utils;

pub fn send_email(to: &str, subject: &str, body: &str) -> bool {
    core_utils::validate_input(to, "email_schema", true, "notifications");
    core_utils::log_event("email_sent", to, "notifications", "info");
    core_utils::format_response(body, 200, "");
    core_utils::emit_metric("notifications.email", 1, "channel:email");
    let _ = subject;
    true
}

pub fn send_sms(phone: &str, message: &str) -> bool {
    core_utils::validate_input(phone, "sms_schema", true, "notifications");
    core_utils::log_event("sms_sent", phone, "notifications", "info");
    core_utils::emit_metric("notifications.sms", 1, "channel:sms");
    let _ = message;
    true
}

pub fn broadcast(channel: &str, message: &str) -> bool {
    core_utils::normalize_record(message, &["sanitize"], "en");
    core_utils::log_event("broadcast", channel, "notifications", "info");
    true
}
