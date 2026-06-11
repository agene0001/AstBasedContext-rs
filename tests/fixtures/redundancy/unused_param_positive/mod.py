# `context` and `options` are never used in the body.
def handler(event, context, options):
    result = event["id"]
    return result
