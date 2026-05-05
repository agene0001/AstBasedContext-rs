// Check 5: Split candidate — high complexity function

pub fn process_pipeline(data: &[Record], config: &Config) -> Result<Report, Error> {
    // Section 1: Validate
    if data.is_empty() {
        return Err(Error::new("No data"));
    }
    let mut validated = Vec::new();
    for item in data {
        if item.id == 0 {
            continue;
        }
        if item.value.is_empty() {
            continue;
        }
        if item.score < 0 {
            continue;
        }
        if item.score > config.max_score {
            continue;
        }
        validated.push(item);
    }

    // Section 2: Transform
    let mut transformed = Vec::new();
    for item in &validated {
        let mut new_item = item.clone();
        new_item.value = new_item.value.trim().to_uppercase();
        new_item.score = new_item.score * config.multiplier;
        new_item.category = categorize(new_item.score);
        if config.include_meta {
            new_item.meta = Some(extract_meta(item));
        }
        transformed.push(new_item);
    }

    // Section 3: Aggregate
    let mut totals: HashMap<String, Stats> = HashMap::new();
    for item in &transformed {
        let entry = totals.entry(item.category.clone()).or_default();
        entry.count += 1;
        entry.sum += item.score;
        entry.ids.push(item.id);
    }

    // Section 4: Build report
    let mut lines = Vec::new();
    lines.push(format!("Report generated at {}", now()));
    lines.push(format!("Total: {}", transformed.len()));
    for (cat, stats) in &totals {
        let avg = stats.sum as f64 / stats.count.max(1) as f64;
        lines.push(format!("  {}: count={}, avg={:.2}", cat, stats.count, avg));
    }

    // Section 5: Persist
    if config.save_to_db {
        let db = Database::connect(&config.db_url)?;
        for item in &transformed {
            db.insert("items", item)?;
        }
        db.commit()?;
    }

    // Section 6: Notify
    if config.notify {
        for recipient in &config.recipients {
            send_email(recipient, "Done", &lines.join("\n"))?;
        }
    }

    Ok(Report {
        count: transformed.len(),
        categories: totals,
        text: lines.join("\n"),
    })
}
