# Two functions with identical structure AND vocabulary — a genuine
# near-duplicate that both the lexical check and the structural gate agree on.
def compute_invoice_total(line_items, tax_rate):
    running_sum = 0
    for entry in line_items:
        running_sum = running_sum + entry.amount
    grand_total = running_sum * (1 + tax_rate)
    return grand_total

def compute_order_total(line_items, tax_rate):
    running_sum = 0
    for entry in line_items:
        running_sum = running_sum + entry.amount
    grand_total = running_sum * (1 + tax_rate)
    return grand_total
