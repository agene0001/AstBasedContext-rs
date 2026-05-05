# Check 1: Passthrough wrappers

def fetch_user(user_id):
    return get_user_from_db(user_id)


def get_user_from_db(user_id):
    db = connect_to_database()
    result = db.query("SELECT * FROM users WHERE id = ?", user_id)
    if result is None:
        raise ValueError(f"User {user_id} not found")
    return result


def send_notification(user_id, message):
    return dispatch_notification(user_id, message)


def dispatch_notification(user_id, message):
    channel = get_channel_for_user(user_id)
    payload = build_payload(message, channel)
    response = channel.send(payload)
    if not response.ok:
        log_error(f"Failed to send to {user_id}")
    return response
