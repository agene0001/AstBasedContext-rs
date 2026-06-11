# Guards the raw-source decision: name/title are used ONLY inside an f-string
# interpolation, which is real usage. `_unused` is intentionally ignored.
def greet(name, title):
    return f"Hello {title} {name}, welcome"


def ignore_me(_unused, value):
    return value
