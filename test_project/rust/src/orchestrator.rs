// orchestrator.rs — Hub module importing from many modules.
// Triggers: HM (hub module), UM (unstable module), MM (middle man),
//           MF (misplaced function), IM (implicit module)

use crate::core_utils;
use crate::models::{UserModel, OrderModel, ProductModel, Model};
use crate::auth_service;
use crate::billing_service;
use crate::shipping_service;
use crate::notification_service;

/// Middle-man struct — each method delegates to exactly one function.
pub struct OrderOrchestrator;

impl OrderOrchestrator {
    pub fn authenticate_user(&self, data: &str) -> bool {
        auth_service::AuthManager.authenticate(data)
    }

    pub fn charge_order(&self, amount: f64, currency: &str) -> bool {
        billing_service::charge(amount, currency)
    }

    pub fn ship_order(&self, order: &str, address: &str) -> String {
        shipping_service::create_shipment(order, address)
    }

    pub fn notify_customer(&self, to: &str, subject: &str, body: &str) -> bool {
        notification_service::send_email(to, subject, body)
    }
}

pub fn process_full_order(user: &str, items: &[&str], amount: f64, address: &str) {
    core_utils::validate_input(user, "user_schema", true, "orchestrator");
    auth_service::AuthManager.authenticate(user);
    billing_service::charge(amount, "USD");
    shipping_service::create_shipment(items[0], address);
    notification_service::send_email(user, "Order Confirmed", "Your order is ready");
    core_utils::log_event("order_complete", user, "orchestrator", "info");
    core_utils::emit_metric("orders.complete", 1, "");

    // Constructor calls for FTY
    let _u = UserModel::new("test", "t@t.com");
    let _o = OrderModel::new(1, 100.0);
    let _p = ProductModel::new("SKU1", 9.99);
}

pub fn sync_services() {
    shipping_service::update_status("all", "synced");
    notification_service::broadcast("system", "sync complete");
    core_utils::log_event("sync", "complete", "orchestrator", "info");
}
