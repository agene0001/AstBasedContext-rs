# services/logging/logger.py — Logging service calling core utilities

from core import utils


def log_info(source, message, context_data):
    utils.validate_input(message, "log_schema", False, "logging")
    utils.log_event("info", {"msg": message, "src": source}, "logging", "info", None)
    utils.emit_metric("logging.info", 1, {"source": source}, None)
    return True


def log_error(source, error, stack_trace):
    utils.validate_input(error, "error_schema", True, "logging")
    utils.log_event("error", {"error": str(error)}, "logging", "error", None)
    utils.emit_metric("logging.error", 1, {"source": source}, None)
    utils.format_response({"error": str(error)}, 500, {}, {"stack": stack_trace})
    return True


def log_audit(actor, action, resource, result):
    utils.validate_input(actor, "audit_schema", True, "logging")
    utils.normalize_record({"actor": actor, "action": action}, ["redact_pii"], "en", {})
    utils.log_event("audit", {"actor": actor, "action": action}, "logging", "info", None)
    return True


def rotate_logs(max_size, retention_days):
    utils.get_config("logging", "rotation_policy", "daily", None)
    utils.log_event("log_rotation", {"max": max_size}, "logging", "info", None)
    return True


def search_logs(query, time_range, severity):
    utils.build_query("logs", query, "timestamp", 500)
    utils.format_response([], 200, {}, {"query": query})
    return []
