// billing_service.rs — Billing service using core_utils.
// Triggers: CD (circular dep with auth via mutual calls)

use crate::core_utils;
use crate::auth_service;

pub fn charge(amount: f64, currency: &str) -> bool {
    core_utils::validate_input(&amount.to_string(), "charge_schema", true, "billing");
    core_utils::log_event("charge", &format!("{}{}", amount, currency), "billing", "info");
    core_utils::emit_metric("billing.charge", 1, &format!("currency:{}", currency));
    true
}

pub fn validate_payment(amount: f64) -> bool {
    core_utils::validate_input(&amount.to_string(), "payment_schema", true, "billing");
    true
}

pub fn apply_discount(amount: f64, rate: f64) -> f64 {
    core_utils::log_event("discount", &format!("{}", rate), "billing", "info");
    amount * (1.0 - rate)
}

pub fn calculate_tax(amount: f64, region: &str) -> f64 {
    core_utils::get_config("billing", "tax_rate", "0.08");
    let _ = region;
    amount * 0.08
}

pub fn refund(amount: f64, reason: &str) -> bool {
    core_utils::log_event("refund", reason, "billing", "warn");
    core_utils::emit_metric("billing.refund", 1, &format!("reason:{}", reason));
    true
}

pub fn check_auth_status(user: &str) -> bool {
    // Calls back into auth → circular dependency
    let auth = auth_service::AuthManager;
    auth.authenticate(user);
    auth.authorize("token", "billing");
    true
}
