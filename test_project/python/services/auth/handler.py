# services/auth/handler.py — Auth service calling core utilities
# Contributes callers for: SG, HBR, DV, OBS, CD, FAC, FE, II, MF, IM

from core import utils
from core import models
from services.billing import processor


class AuthManager:
    """Triggers: FE (feature envy) — process_auth calls more BillingProcessor
    methods than its own class. Also II (inappropriate intimacy) with BillingProcessor."""

    def authenticate(self, user_data):
        utils.validate_input(user_data, "auth_schema", True, "auth")
        utils.log_event("auth_attempt", user_data, "auth", "info", None)
        return True

    def authorize(self, token, resource):
        utils.validate_input(token, "token_schema", False, "auth")
        utils.normalize_record({"token": token}, ["strip"], "en", {})
        return True

    def refresh_token(self, token):
        utils.validate_input(token, "refresh_schema", True, "auth")
        utils.log_event("token_refresh", token, "auth", "info", None)
        return "new_token"

    def revoke(self, token):
        utils.log_event("revoke", token, "auth", "warn", None)
        return True

    def process_auth(self, user, amount, currency):
        """Calls BillingProcessor methods more than own methods → FE."""
        processor.charge(amount, currency)
        processor.validate_payment(amount)
        processor.apply_discount(amount, 0.1)
        processor.calculate_tax(amount, "US")
        return True

    def check_billing_status(self, user):
        """More calls to billing → strengthens II."""
        processor.charge(0, "USD")
        processor.validate_payment(0)
        processor.apply_discount(0, 0)
        processor.calculate_tax(0, "US")
        processor.refund(0, "reason")
        return "ok"


def create_models_for_user(user_data):
    """Constructor calls from 2nd file → FTY (factory) for BaseModel hierarchy."""
    models.UserModel()
    models.OrderModel()
    models.ProductModel()
    return True
