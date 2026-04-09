# orphan_utils.py — Isolated file with functions but no callers or importers.
# Triggers: OM (orphan module)


def orphan_helper_a(x, y):
    return x + y


def orphan_helper_b(data):
    return [d for d in data if d > 0]


def orphan_helper_c(name, value):
    return f"{name}={value}"
