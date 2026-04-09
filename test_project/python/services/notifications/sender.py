# services/notifications/sender.py — Notification service calling core utilities

from core import utils


def send_email(to, subject, body):
    utils.validate_input(to, "email_schema", True, "notifications")
    utils.log_event("email_sent", {"to": to}, "notifications", "info", None)
    utils.format_response({"sent": True}, 200, {}, {"to": to})
    utils.emit_metric("notifications.email", 1, {"channel": "email"}, None)
    return True


def send_sms(phone, message):
    utils.validate_input(phone, "sms_schema", True, "notifications")
    utils.log_event("sms_sent", {"phone": phone}, "notifications", "info", None)
    utils.emit_metric("notifications.sms", 1, {"channel": "sms"}, None)
    return True


def send_push(device_id, title, payload):
    utils.validate_input(device_id, "push_schema", False, "notifications")
    utils.log_event("push_sent", {"device": device_id}, "notifications", "info", None)
    utils.format_response(payload, 200, {}, {"device": device_id})
    utils.emit_metric("notifications.push", 1, {"channel": "push"}, None)
    return True


def broadcast(channel, message, recipients):
    utils.validate_input(recipients, "broadcast_schema", True, "notifications")
    utils.normalize_record({"msg": message}, ["sanitize"], "en", {})
    utils.log_event("broadcast", {"channel": channel}, "notifications", "info", None)
    return True


def schedule_notification(template, delay, target):
    utils.get_config("notifications", "max_delay", 3600, None)
    utils.log_event("schedule", {"template": template}, "notifications", "info", None)
    return True
