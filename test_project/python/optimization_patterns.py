# optimization_patterns.py — Test patterns for optimization checks (103-109)


# ── CIL (clone-in-loop) — Python equivalent: unnecessary copy/str() in loop ──
# Note: .clone() / .to_string() are Rust-specific; tested in Rust file.
# Python uses copy/deepcopy which are less common.


# ── RCI (redundant-collect-iterate) — list() then iterate ─────────────────
def redundant_materialize(items):
    filtered = list(x for x in items if x > 0)
    for item in filtered:
        print(item)
    return len(filtered)


# ── RML (repeated-map-lookup) — same key looked up 3+ times ──────────────
def repeated_lookup(config):
    host = config["database"]["host"]
    port = config["database"]["port"]
    name = config["database"]["name"]
    user = config["database"]["user"]
    return f"{user}@{host}:{port}/{name}"


# ── VNP (vec-no-presize) — list created then appended in loop ─────────────
def build_results_no_presize(data):
    results = []
    for item in data:
        results.append(item * 2)
    return results


# ── STF (sort-then-find) — sort then linear search ───────────────────────
def find_after_sort(items, target):
    items.sort()
    for item in items:
        if item == target:
            return item
    return None


# ── LCO (list-concat-in-loop) — list += [...] in loop ────────────────────
def build_with_concat(records):
    result = []
    for r in records:
        result += [r["name"]]
    return result


# ── URB (unbounded-recursion) — recursive call with no depth param ────────
def flatten_tree(node):
    """Recursively flattens a tree with no depth limit."""
    result = [node["value"]]
    for child in node.get("children", []):
        result.extend(flatten_tree(child))
    return result


# ── VEC (vectorize / suggest NumPy) ───────────────────────────────────────
import math

def compute_distances(points_x, points_y, origin_x, origin_y):
    """Element-wise math in a loop — should use NumPy."""
    distances = []
    for i in range(len(points_x)):
        dx = points_x[i] - origin_x
        dy = points_y[i] - origin_y
        dist = math.sqrt(dx * dx + dy * dy)
        distances.append(dist)
    return distances


def normalize_scores(scores):
    """Reduction + element-wise arithmetic — vectorizable."""
    total = 0
    for i in range(len(scores)):
        total += scores[i]
    result = []
    for i in range(len(scores)):
        result.append(scores[i] / total)
    return result


# ── POL (suggest Polars over Pandas) ──────────────────────────────────────
import pandas as pd

def load_and_process_data(path):
    """Uses Pandas patterns that Polars handles faster."""
    df = pd.read_csv(path)
    grouped = df.groupby("category")
    totals = grouped["amount"].sum()
    return totals

def slow_row_processing(df):
    """iterrows() is extremely slow — Polars avoids this entirely."""
    results = []
    for idx, row in df.iterrows():
        results.append(row["value"] * 2)
    return results

def transform_with_apply(df):
    """apply() runs Python per-row — Polars expressions are faster."""
    df["normalized"] = df["score"].apply(lambda x: x / 100)
    return df


# ── RRC (regex-recompile-in-loop) ─────────────────────────────────────
import re

def validate_emails(emails):
    """re.match() inside loop — compile once outside."""
    results = []
    for email in emails:
        if re.match(r"^[\w.]+@[\w]+\.[\w]+$", email):
            results.append(email)
    return results


# ── EFC (exception-for-control-flow) ──────────────────────────────────
def safe_get_value(data, key):
    """Catches KeyError instead of using .get()."""
    try:
        return data[key]
    except KeyError:
        return None


# ── N1Q (n-plus-one-query) ───────────────────────────────────────────
def fetch_user_details(user_ids, session):
    """Database query inside loop — N+1 pattern."""
    results = []
    for uid in user_ids:
        user = session.query(uid)
        results.append(user)
    return results


# ── SAC (sync-async-conflict) ────────────────────────────────────────
import asyncio

async def fetch_data_async(urls):
    """Blocking requests.get() inside async function."""
    await asyncio.sleep(0)  # async context indicator
    results = []
    for url in urls:
        resp = requests.get(url)
        results.append(resp.text)
    return results


# ── MCM (memoization-candidate) ──────────────────────────────────────
def process_with_repeated_calls(items):
    """Same expensive call repeated with identical args."""
    a = compute_hash("salt_value")
    b = compute_hash("salt_value")
    c = compute_hash("salt_value")
    return [a, b, c, items]


# ── RFI (repeated-format-in-loop) ────────────────────────────────────
def generate_report_lines(records):
    """format!() equivalent used repeatedly in loop."""
    lines = []
    for r in records:
        lines.append(f"Name: {r['name']}")
        lines.append(f"Age: {r['age']}")
        lines.append(f"Score: {r['score']}")
        lines.append(f"Rank: {r['rank']}")
    return lines


# ── SLA (sleep-in-loop) ──────────────────────────────────────────────
import time

def poll_until_ready(check_fn):
    """Polling with sleep in loop — busy wait."""
    while True:
        if check_fn():
            return True
        time.sleep(1)


# ── GEN (generator-over-list) ────────────────────────────────────────
def total_positives(items):
    """sum([...]) instead of sum(generator)."""
    return sum([x for x in items if x > 0])


# ── LLI (large-list-in) ─────────────────────────────────────────────
def check_status(status):
    """Membership test on list literal with 4+ elements."""
    if status in ["pending", "active", "paused", "cancelled", "expired"]:
        return True
    return False


# ── DLK (dict-keys-iter) ─────────────────────────────────────────────
def print_keys(config):
    """for k in dict.keys() — iterate directly."""
    for k in config.keys():
        print(k)


# ── UCM (unclosed-resource) ──────────────────────────────────────────
def read_file_unsafe(path):
    """open() without with — resource leak."""
    f = open(path, "r")
    data = f.read()
    return data


# ── ELV (enumerate-vs-range-len) ─────────────────────────────────────
def index_loop(items):
    """for i in range(len(...)) — use enumerate."""
    for i in range(len(items)):
        print(i, items[i])


# ── YLD (yield-from) ─────────────────────────────────────────────────
def flatten_generator(nested):
    """for x in iterable: yield x — use yield from."""
    for sublist in nested:
        for item in sublist:
            yield item


# ── APD (append-in-loop-extend) ──────────────────────────────────────
def copy_items(src, dst):
    """for x in src: dst.append(x) — use extend."""
    for x in src:
        dst.append(x)


# ── DWS (double-with-statement) ──────────────────────────────────────
def copy_file(src_path, dst_path):
    """Nested with blocks — can combine."""
    with open(src_path) as src:
        with open(dst_path, "w") as dst:
            dst.write(src.read())


# ── IIF (import-in-function) ─────────────────────────────────────────
def compute_json(data):
    """import inside function body."""
    import json
    return json.dumps(data)


# ── CST (constant-condition) ─────────────────────────────────────────
def guarded_block(x):
    """if False — dead branch."""
    if False:
        print("unreachable")
    return x


# ── RNE (redundant-negation) ─────────────────────────────────────────
def check_value(a, b):
    """not a == b — use !=."""
    if not a == b:
        return True
    return False


# ── DFC (default-dict-pattern) ──────────────────────────────────────
def group_by_category(items):
    """if key not in d: d[key] = [] — use defaultdict or setdefault."""
    groups = {}
    for item in items:
        key = item["category"]
        if key not in groups:
            groups[key] = []
        groups[key].append(item)
    return groups


# ── ESE (empty-string-check) ───────────────────────────────────────
def validate_name(name):
    """if name == '' — use if not name."""
    if name == '':
        return "anonymous"
    return name
