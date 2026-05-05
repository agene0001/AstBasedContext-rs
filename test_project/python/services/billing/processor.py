# services/billing/processor.py — Billing service
# Triggers: CD (circular dep with auth via mutual calls), II (with AuthManager)

from core import utils
from services.auth import handler


def charge(amount, currency):
    utils.validate_input(amount, "charge_schema", True, "billing")
    utils.log_event("charge", {"amount": amount}, "billing", "info", None)
    utils.emit_metric("billing.charge", amount, {"currency": currency}, None)
    return True


def validate_payment(amount):
    utils.validate_input(amount, "payment_schema", False, "billing")
    return amount > 0


def apply_discount(amount, rate):
    utils.normalize_record({"amount": amount}, ["discount"], "en", {})
    return amount * (1 - rate)


def calculate_tax(amount, region):
    utils.format_response({"tax": amount * 0.1}, 200, {}, {"region": region})
    return amount * 0.1


def refund(amount, reason):
    utils.log_event("refund", {"amount": amount, "reason": reason}, "billing", "warn", None)
    utils.emit_metric("billing.refund", amount, {}, None)
    return True


def check_auth_status(user_id):
    """Calls back into auth → creates circular dependency."""
    handler.AuthManager().authenticate({"id": user_id})
    handler.AuthManager().authorize("token", "billing")
    handler.AuthManager().refresh_token("token")
    handler.AuthManager().revoke("old_token")
    handler.AuthManager().check_billing_status(user_id)
    return True
