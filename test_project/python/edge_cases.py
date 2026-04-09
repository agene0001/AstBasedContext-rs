import os
import json

# ── Mutable global state (Check 66) ──────────────────────────────────────────
# Fires on module-level lowercase names with mutable collection values.

cache = []
registry = {}
pending_tasks = list()

# ── Environment variable usage (Check 88) ────────────────────────────────────
# Fires on os.getenv / os.environ inside a function.

def get_api_client():
    # TODO: read from a config object instead of env
    key = os.getenv("API_KEY")
    secret = os.environ.get("API_SECRET")
    host = os.environ["SERVICE_HOST"]
    return {"key": key, "secret": secret, "host": host}

def load_database_config():
    return {
        "host": os.getenv("DB_HOST", "localhost"),
        "port": int(os.getenv("DB_PORT", "5432")),
        "password": os.environ.get("DB_PASSWORD"),
    }

# ── Hardcoded endpoints (Check 89) ───────────────────────────────────────────
# Fires when a "https://" literal appears in function source.

def fetch_user_profile(user_id: str) -> dict:
    url = f"https://api.internal.company.com/v2/users/{user_id}"
    return {"url": url}

def refresh_auth_token(refresh_token: str) -> str:
    endpoint = "https://auth.company.com/oauth/token"
    return endpoint + "?token=" + refresh_token

# ── Empty catch / except (Check 67) ──────────────────────────────────────────
# Fires on `except:\n        pass` or `except Exception:\n        pass`.

def load_config(path: str) -> dict:
    try:
        with open(path) as f:
            return json.load(f)
    except:
        pass
    return {}

def parse_int_safe(value: str) -> int:
    try:
        return int(value)
    except Exception:
        pass
    return 0

# ── Tech debt comments (Check 102) ───────────────────────────────────────────

def compute_ranking(scores: list) -> list:
    # FIXME: does not handle ties correctly
    # TODO: replace bubble sort with timsort
    sorted_scores = sorted(scores, reverse=True)
    return sorted_scores

# ── True positive near-duplicates (ensure TP still fires) ────────────────────

def format_invoice_line(qty: int, unit_price: float, tax_rate: float) -> str:
    subtotal = qty * unit_price
    tax = subtotal * tax_rate
    total = subtotal + tax
    return f"qty={qty} subtotal={subtotal:.2f} tax={tax:.2f} total={total:.2f}"

def format_quote_line(qty: int, unit_price: float, tax_rate: float) -> str:
    subtotal = qty * unit_price
    tax = subtotal * tax_rate
    total = subtotal + tax
    return f"qty={qty} subtotal={subtotal:.2f} tax={tax:.2f} total={total:.2f}"
