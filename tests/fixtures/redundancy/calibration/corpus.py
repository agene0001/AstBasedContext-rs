"""Labeled calibration corpus.

Naming convention encodes ground truth:
  dup<G>_<n>  -> genuine similar cluster <G> (within-group pairs SHOULD be flagged)
  trap_*      -> shares vocabulary with a cluster but DIFFERENT structure (must NOT flag)
  uniq_*      -> unrelated singletons
"""
import functools


# ── Cluster A: loop-accumulate (genuine near-duplicates) ──────────────────
def dupA_1(records, factor):
    total = 0
    for r in records:
        total += r.value
    return total * factor


def dupA_2(records, factor):
    total = 0
    for r in records:
        total += r.value
    return total * factor


def dupA_3(records, rate):
    total = 0
    for r in records:
        total += r.value
    return total * rate


# ── Cluster B: dict-building loop (genuine near-duplicates) ────────────────
def dupB_1(items):
    out = {}
    for it in items:
        out[it.key] = it.value
    return out


def dupB_2(items):
    out = {}
    for it in items:
        out[it.key] = it.value
    return out


# ── Cluster C: guard/validate then return (genuine) ───────────────────────
def dupC_1(payload):
    if payload is None:
        raise ValueError("missing")
    if payload.size <= 0:
        raise ValueError("empty")
    return payload


def dupC_2(payload):
    if payload is None:
        raise ValueError("missing")
    if payload.size <= 0:
        raise ValueError("empty")
    return payload


# ── Traps: same vocabulary as a cluster, different AST shape ───────────────
def trap_accumulate_comp(records, factor):
    values = [r.value for r in records]   # shares dupA vocab, comprehension shape
    total = sum(values)
    return total * factor


def trap_accumulate_reduce(records, factor):
    total = functools.reduce(lambda a, r: a + r.value, records, 0)
    return total * factor


def trap_dict_comp(items):
    pairs = [(it.key, it.value) for it in items]   # shares dupB vocab, comp shape
    out = dict(pairs)
    return out


# ── Unique singletons ─────────────────────────────────────────────────────
def uniq_tokenize(text):
    tokens = text.split()
    cleaned = [t.strip().lower() for t in tokens]
    return cleaned


def uniq_fetch(url, headers):
    conn = open_connection(url)
    conn.send(headers)
    return conn.recv()


# ── Cluster D: same intent, different control flow (moderately similar) ────
# A reviewer would want these consolidated, but their AST shapes only partly
# agree — so an over-high structural threshold will miss them (recall cost).
def dupD_1(queue, limit):
    seen = 0
    for job in queue:
        seen += 1
        if seen >= limit:
            break
    return seen


def dupD_2(queue, limit):
    seen = 0
    idx = 0
    while idx < len(queue):
        seen += 1
        idx += 1
        if seen >= limit:
            break
    return seen
