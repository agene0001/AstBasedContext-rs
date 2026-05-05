# Check 5: Split candidates — monster function

def process_entire_pipeline(data, config):
    if not data:
        raise ValueError("No data provided")
    if not isinstance(data, list):
        data = [data]
    validated = []
    for item in data:
        if "id" not in item:
            continue
        if "value" not in item:
            continue
        if item["value"] < 0:
            continue
        if item["value"] > config.get("max_value", 1000):
            continue
        validated.append(item)

    transformed = []
    for item in validated:
        new_item = {}
        new_item["id"] = str(item["id"]).upper()
        new_item["value"] = item["value"] * config.get("multiplier", 1)
        new_item["category"] = categorize(item["value"])
        new_item["timestamp"] = get_current_time()
        new_item["source"] = config.get("source", "unknown")
        if config.get("include_metadata"):
            new_item["meta"] = extract_metadata(item)
        transformed.append(new_item)

    totals = {}
    for item in transformed:
        cat = item["category"]
        if cat not in totals:
            totals[cat] = {"count": 0, "sum": 0, "items": []}
        totals[cat]["count"] += 1
        totals[cat]["sum"] += item["value"]
        totals[cat]["items"].append(item["id"])

    report_lines = []
    report_lines.append(f"Pipeline Report - {get_current_time()}")
    report_lines.append(f"Total items processed: {len(transformed)}")
    report_lines.append(f"Categories found: {len(totals)}")
    for cat, stats in sorted(totals.items()):
        avg = stats["sum"] / max(stats["count"], 1)
        report_lines.append(f"  {cat}: count={stats['count']}, avg={avg:.2f}")

    if config.get("save_to_db"):
        db = connect_to_database(config["db_url"])
        for item in transformed:
            db.insert("processed_items", item)
        db.commit()

    if config.get("save_to_file"):
        with open(config["output_path"], "w") as f:
            for line in report_lines:
                f.write(line + "\n")

    if config.get("notify"):
        for recipient in config.get("recipients", []):
            send_email(recipient, "Pipeline Complete", "\n".join(report_lines))

    return {
        "processed": len(transformed),
        "categories": totals,
        "report": "\n".join(report_lines),
    }
