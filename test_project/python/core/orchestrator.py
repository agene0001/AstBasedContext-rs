# core/orchestrator.py — Hub module importing from 10+ modules
# Triggers: HM (10+ imports), UM (high efferent, low afferent instability)
#           MM (middle man — all methods delegate to one callee),
#           IM (implicit module — 5+ cross-file calls with utils.py)

from core import utils
from core import models
from core import views
from services.auth import handler
from services.billing import processor
from services.inventory import manager
from services.shipping import tracker
from services.notifications import sender
from services.analytics import reporter
from services.logging import logger


class OrderOrchestrator:
    """Middle-man class — each method delegates to exactly one function.
    Triggers: MM (80% of methods are passthroughs to other classes).
    """

    def authenticate_user(self, user_data):
        return handler.AuthManager().authenticate(user_data)

    def charge_order(self, amount, currency):
        return processor.charge(amount, currency)

    def check_inventory(self, sku, warehouse):
        return manager.check_stock(sku, warehouse)

    def ship_order(self, order, address):
        return tracker.create_shipment(order, address)

    def notify_customer(self, to, subject, body):
        return sender.send_email(to, subject, body)

    def log_action(self, source, message, ctx):
        return logger.log_info(source, message, ctx)


def process_full_order(user, items, payment, address):
    """Calls functions from 6+ different service modules.
    Triggers: MF (misplaced function — more connections to services than own file),
              IM (5+ cross-file function calls).
              FTY (factory — calls constructors of 3+ sibling classes from here + handler).
    """
    utils.validate_input(user, "user_schema", True, "orchestrator")
    handler.AuthManager().authenticate(user)
    # Constructor calls for FTY (factory) detection
    models.UserModel()
    models.OrderModel()
    models.ProductModel()
    processor.charge(payment["amount"], payment["currency"])
    manager.reserve_stock(items[0]["sku"], items[0]["qty"])
    tracker.create_shipment({"items": items}, address)
    sender.send_email(user["email"], "Order Confirmed", "Your order is ready")
    reporter.track_event("order_complete", {"user": user["id"]})
    logger.log_info("orchestrator", "order processed", {"user": user["id"]})
    utils.emit_metric("orders.complete", 1, {}, None)
    return True


def run_daily_analytics(date_range):
    """Calls analytics and logging from orchestrator."""
    reporter.generate_report({}, "daily")
    reporter.aggregate_metrics("all", date_range, "hourly")
    reporter.export_data({}, "s3", "parquet")
    logger.log_audit("system", "daily_report", "analytics", "success")
    logger.log_info("orchestrator", "daily analytics complete", {})
    utils.log_event("daily_analytics", {"range": date_range}, "orchestrator", "info", None)
    return True


def sync_services():
    """Cross-module synchronization."""
    manager.update_catalog([], [])
    tracker.update_status("all", "synced")
    sender.broadcast("system", "sync complete", [])
    logger.rotate_logs(1000000, 30)
    reporter.dashboard_summary("admin", "30d")
    return True
