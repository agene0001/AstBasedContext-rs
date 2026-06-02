# `add` is a thin wrapper that just forwards its args to `_do_work`.
def _do_work(left, right):
    combined = left + right
    return combined

def add(left, right):
    return _do_work(left, right)
