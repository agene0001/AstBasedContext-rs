# services/shipping/tracker.py — Shipping service calling core utilities

from core import utils


def create_shipment(order, address):
    utils.validate_input(order, "shipment_schema", True, "shipping")
    utils.log_event("shipment_created", order, "shipping", "info", None)
    utils.normalize_record(address, ["format_addr"], "en", {})
    utils.emit_metric("shipping.created", 1, {"carrier": "ups"}, None)
    return "SHIP-001"


def track_package(tracking_id):
    utils.validate_input(tracking_id, "tracking_schema", False, "shipping")
    utils.build_query("shipments", f"tracking='{tracking_id}'", "updated_at", 1)
    return {"status": "in_transit"}


def update_status(tracking_id, status):
    utils.validate_input(status, "status_schema", True, "shipping")
    utils.log_event("status_update", {"id": tracking_id, "status": status}, "shipping", "info", None)
    utils.emit_metric("shipping.status", 1, {"status": status}, None)
    return True


def estimate_delivery(origin, destination):
    utils.format_response({"days": 3}, 200, {}, {"origin": origin, "dest": destination})
    utils.compute_hash(f"{origin}-{destination}", "md5", "", 1)
    return 3


def cancel_shipment(tracking_id, reason):
    utils.log_event("shipment_cancel", {"id": tracking_id}, "shipping", "warn", None)
    utils.emit_metric("shipping.cancel", 1, {"reason": reason}, None)
    return True
