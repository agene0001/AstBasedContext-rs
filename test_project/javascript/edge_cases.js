// ── Environment variable usage (Check 88) ────────────────────────────────────
// Fires on process.env. or process.env[ inside function source.

function getServiceConfig() {
    return {
        apiKey: process.env.API_KEY,
        apiSecret: process.env.API_SECRET,
        serviceUrl: process.env.SERVICE_URL,
    };
}

function buildAuthHeader() {
    const token = process.env["AUTH_TOKEN"];
    const realm = process.env["AUTH_REALM"];
    return `Bearer ${token}@${realm}`;
}

// ── Hardcoded endpoints (Check 89) ───────────────────────────────────────────
// Fires when "https://" literal appears inside a function.

function fetchUserProfile(userId) {
    const url = `https://api.internal.company.com/v2/users/${userId}`;
    return fetch(url);
}

function refreshToken(refreshToken) {
    return fetch("https://auth.company.com/oauth/token", {
        method: "POST",
        body: JSON.stringify({ token: refreshToken }),
    });
}

// ── Empty catch (Check 67) ───────────────────────────────────────────────────
// Fires on `catch (_) {}` or `catch (_) { }` as a substring.

function parseConfig(json) {
    try {
        return JSON.parse(json);
    } catch (_) {}
    return null;
}

function loadUser(id) {
    try {
        return db.findById(id);
    } catch (_) { }
}

// ── Callback hell (Check 68) ─────────────────────────────────────────────────
// Fires when 4+ nested function( / => appear in one function's source.

function processOrder(orderId, callback) {
    fetchOrder(orderId, function(err, order) {
        if (err) { callback(err); return; }
        validateOrder(order, function(err, valid) {
            if (err) { callback(err); return; }
            chargePayment(order.total, function(err, receipt) {
                if (err) { callback(err); return; }
                sendConfirmation(order, receipt, function(err, result) {
                    if (err) { callback(err); return; }
                    updateInventory(order.items, function(err, done) {
                        callback(null, done);
                    });
                });
            });
        });
    });
}

function syncUserData(userId, opts, cb) {
    fetchUser(userId, function(err, user) {
        if (err) return cb(err);
        fetchPermissions(user.role, function(err, perms) {
            if (err) return cb(err);
            fetchPreferences(user.id, function(err, prefs) {
                if (err) return cb(err);
                mergeAndSave(user, perms, prefs, function(err, saved) {
                    cb(null, saved);
                });
            });
        });
    });
}

// ── True positive near-duplicates ────────────────────────────────────────────

function formatShippingAddress(street, city, state, zip) {
    return `${street}, ${city}, ${state} ${zip}`;
}

function formatBillingAddress(street, city, state, zip) {
    return `${street}, ${city}, ${state} ${zip}`;
}
