# Two functions that are genuinely different in both structure and purpose.
# Must NOT be reported as near-duplicates.
def compute_invoice_total(line_items, tax_rate):
    running_sum = 0
    for entry in line_items:
        running_sum = running_sum + entry.amount
    return running_sum * (1 + tax_rate)

def classify_http_status(status_code):
    if status_code >= 500:
        return "server_error"
    elif status_code >= 400:
        return "client_error"
    elif status_code >= 300:
        return "redirect"
    return "ok"
