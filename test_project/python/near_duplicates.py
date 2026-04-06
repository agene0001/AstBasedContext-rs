# Check 2 & 3: Near-duplicates and structurally similar

def process_user_data(users):
    results = []
    for user in users:
        name = user["name"].strip().lower()
        email = user["email"].strip().lower()
        if not name or not email:
            continue
        if "@" not in email:
            continue
        normalized = {"name": name, "email": email, "active": True}
        results.append(normalized)
    return results


def process_customer_data(customers):
    results = []
    for customer in customers:
        name = customer["name"].strip().lower()
        email = customer["email"].strip().lower()
        if not name or not email:
            continue
        if "@" not in email:
            continue
        normalized = {"name": name, "email": email, "active": True}
        results.append(normalized)
    return results


def validate_shipping_address(address):
    errors = []
    if not address.get("street"):
        errors.append("Missing street")
    if not address.get("city"):
        errors.append("Missing city")
    if not address.get("zip"):
        errors.append("Missing zip code")
    if not address.get("country"):
        errors.append("Missing country")
    if len(address.get("zip", "")) < 3:
        errors.append("Zip code too short")
    return errors


def validate_billing_address(address):
    errors = []
    if not address.get("street"):
        errors.append("Missing street")
    if not address.get("city"):
        errors.append("Missing city")
    if not address.get("postal_code"):
        errors.append("Missing postal code")
    if not address.get("country"):
        errors.append("Missing country")
    if len(address.get("postal_code", "")) < 3:
        errors.append("Postal code too short")
    return errors
