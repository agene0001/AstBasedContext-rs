# `summarize` does real work; it is not a passthrough wrapper.
def _do_work(left, right):
    combined = left + right
    return combined

def summarize(values):
    total = 0
    for v in values:
        total = total + v
    average = total / len(values)
    return average
