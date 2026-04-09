# services/inventory/manager.py — Inventory service calling core utilities

from core import utils


def check_stock(product_id, warehouse):
    utils.validate_input(product_id, "stock_schema", True, "inventory")
    utils.log_event("stock_check", product_id, "inventory", "info", None)
    utils.build_query("inventory", f"sku='{product_id}'", "updated_at", 1)
    return 100


def reserve_stock(product_id, quantity):
    utils.validate_input(product_id, "reserve_schema", True, "inventory")
    utils.normalize_record({"sku": product_id, "qty": quantity}, ["validate"], "en", {})
    utils.log_event("reserve", {"sku": product_id}, "inventory", "info", None)
    utils.emit_metric("inventory.reserve", quantity, {"sku": product_id}, None)
    return True


def release_stock(product_id, quantity):
    utils.validate_input(product_id, "release_schema", False, "inventory")
    utils.log_event("release", {"sku": product_id}, "inventory", "info", None)
    utils.emit_metric("inventory.release", quantity, {"sku": product_id}, None)
    return True


def update_catalog(products, rules):
    utils.normalize_record(products, rules, "en", {})
    utils.format_response(products, 200, {}, {})
    return True


def get_product_info(sku):
    utils.build_query("products", f"sku='{sku}'", "name", 1)
    utils.get_config("inventory", "default_warehouse", "main", None)
    return {"sku": sku}
