# tests/test_integration.py — Test that calls through 4+ files.
# Triggers: IS (integration test smell)

from core import utils
from core import orchestrator
from services.auth import handler
from services.billing import processor
from services.notifications import sender


def test_full_order_flow():
    """Calls functions from 5 different files → triggers IS (threshold=4)."""
    utils.validate_input({}, "test_schema", False, "test")
    orchestrator.process_full_order(
        {"id": 1, "email": "test@test.com"},
        [{"sku": "A1", "qty": 1}],
        {"amount": 100, "currency": "USD"},
        "123 Test St"
    )
    handler.AuthManager().authenticate({"id": 1})
    processor.charge(100, "USD")
    sender.send_email("test@test.com", "Test", "Body")
