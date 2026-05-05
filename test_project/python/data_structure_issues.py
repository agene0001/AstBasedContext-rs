# Checks 92-99: Data structure misuse patterns

def manage_user_tags(users):
    tags = []
    for user in users:
        for tag in user.get("tags", []):
            if tag not in tags:
                tags.append(tag)
    return tags


def find_config_value(config_pairs, key):
    for k, v in config_pairs:
        if k == key:
            return v
    return None


def check_permissions(users, permissions):
    results = []
    for user in users:
        for perm in permissions:
            if perm in user.get("roles", []):
                results.append((user["name"], perm))
    return results


def build_report(items):
    report = ""
    for item in items:
        report += f"Item: {item['name']} - Value: {item['value']}\n"
    report += "---\n"
    report += f"Total items: {len(items)}\n"
    return report


def find_user_by_id_sorted(sorted_users, target_id):
    sorted_users.sort(key=lambda u: u["id"])
    import bisect
    idx = bisect.bisect_left([u["id"] for u in sorted_users], target_id)
    if idx < len(sorted_users) and sorted_users[idx]["id"] == target_id:
        return sorted_users[idx]
    return None


def find_matching_pairs(list_a, list_b):
    matches = []
    for a in list_a:
        for b in list_b:
            if a["id"] == b["id"]:
                matches.append((a, b))
    return matches


def index_items(items):
    indexed = {}
    for i, item in enumerate(items):
        indexed[i] = item
    return indexed
