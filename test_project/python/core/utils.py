# core/utils.py — Shared utilities called from 7+ service directories
# Triggers: SG (shotgun surgery), HBR (high blast radius), OBS (observer),
#           DV (divergent change), UPA (unstable public API)

from core import models


class _InternalCache:
    """Internal type — name starts with '_', used to trigger LA (leaky abstraction)."""
    def __init__(self):
        self.data = {}

    def get(self, key):
        return self.data.get(key)

    def set(self, key, value):
        self.data[key] = value


def validate_input(data, schema, strict, context):
    """Public function called from 7+ service modules.
    Triggers: SG (5+ caller dirs), UPA (5+ callers + 4 params).
    """
    if not data:
        return False
    if strict:
        return _deep_validate(data, schema, context)
    return True


def normalize_record(record, rules, locale, fallback):
    """Another widely-called utility.
    Triggers: SG, UPA.
    """
    result = record.copy()
    for rule in rules:
        result = _apply_rule(result, rule, locale)
    if not result:
        result = fallback
    return result


def format_response(payload, status, headers, metadata):
    """Formatting utility called from many services.
    Triggers: SG, UPA.
    """
    return {
        "data": payload,
        "status": status,
        "headers": headers,
        "meta": metadata,
    }


def log_event(event_type, payload, source, severity, timestamp):
    """Logging called from everywhere.
    Triggers: SG, UPA.
    """
    return {"type": event_type, "payload": payload, "source": source}


def compute_hash(data, algorithm, salt, rounds):
    """Hash utility used across services."""
    return hash((data, algorithm, salt, rounds))


def build_query(table, conditions, order_by, limit):
    """Query builder called from many services."""
    return f"SELECT * FROM {table} WHERE {conditions} ORDER BY {order_by} LIMIT {limit}"


def emit_metric(name, value, tags, timestamp):
    """Metrics emission called from all services."""
    return {"metric": name, "value": value, "tags": tags, "ts": timestamp}


def get_config(section, key, default, env_override):
    """Config accessor used across modules."""
    return default


def create_public_cache_entry(key: str) -> _InternalCache:
    """Public function returning internal type _InternalCache.
    Triggers: LA (leaky abstraction).
    """
    cache = _InternalCache()
    cache.set(key, True)
    return cache


def process_with_internal_arg(cache: _InternalCache, key: str) -> str:
    """Public function taking internal type _InternalCache as argument.
    Triggers: LA (leaky abstraction).
    """
    value = cache.get(key)
    if value is None:
        return "missing"
    return str(value)


def _deep_validate(data, schema, context):
    return True


def _apply_rule(record, rule, locale):
    return record
