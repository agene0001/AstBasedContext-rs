// Check 1: Passthrough wrapper
function getUser(userId) {
    return fetchUserFromDB(userId);
}

function fetchUserFromDB(userId) {
    const db = connectToDatabase();
    const result = db.query("SELECT * FROM users WHERE id = ?", [userId]);
    if (!result) {
        throw new Error(`User ${userId} not found`);
    }
    return result;
}

// Check 2: Near-duplicates
function formatUserProfile(user) {
    const lines = [];
    lines.push(`Name: ${user.name}`);
    lines.push(`Email: ${user.email}`);
    lines.push(`Role: ${user.role}`);
    lines.push(`Active: ${user.active}`);
    lines.push(`Created: ${user.createdAt}`);
    return lines.join("\n");
}

function formatCustomerProfile(customer) {
    const lines = [];
    lines.push(`Name: ${customer.name}`);
    lines.push(`Email: ${customer.email}`);
    lines.push(`Role: ${customer.role}`);
    lines.push(`Active: ${customer.active}`);
    lines.push(`Created: ${customer.createdAt}`);
    return lines.join("\n");
}

// Check 95: String concat in loop
function buildHtmlTable(rows) {
    let html = "<table>";
    for (const row of rows) {
        html += "<tr>";
        for (const cell of row) {
            html += `<td>${cell}</td>`;
        }
        html += "</tr>";
    }
    html += "</table>";
    return html;
}

// Check 92: Array used as set
function collectUniqueTags(items) {
    const tags = [];
    for (const item of items) {
        for (const tag of item.tags) {
            if (!tags.includes(tag)) {
                tags.push(tag);
            }
        }
    }
    return tags;
}

// Check 97: Nested loop lookup
function findMatchingOrders(users, orders) {
    const matches = [];
    for (const user of users) {
        for (const order of orders) {
            if (user.id === order.userId) {
                matches.push({ user, order });
            }
        }
    }
    return matches;
}
