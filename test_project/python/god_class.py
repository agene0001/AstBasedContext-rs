# Check 10: God class (high LCOM)

class ApplicationManager:
    def __init__(self):
        self.users = []
        self.orders = []
        self.logs = []
        self.cache = {}
        self.config = {}
        self.metrics = {}
        self.notifications = []

    def add_user(self, name, email):
        self.users.append({"name": name, "email": email})

    def remove_user(self, email):
        self.users = [u for u in self.users if u["email"] != email]

    def find_user(self, email):
        for user in self.users:
            if user["email"] == email:
                return user
        return None

    def create_order(self, user_email, items):
        order = {"user": user_email, "items": items, "status": "pending"}
        self.orders.append(order)
        return order

    def cancel_order(self, order_id):
        for order in self.orders:
            if order.get("id") == order_id:
                order["status"] = "cancelled"
                return True
        return False

    def get_pending_orders(self):
        return [o for o in self.orders if o["status"] == "pending"]

    def log_info(self, message):
        self.logs.append({"level": "info", "message": message})

    def log_error(self, message):
        self.logs.append({"level": "error", "message": message})

    def get_error_logs(self):
        return [l for l in self.logs if l["level"] == "error"]

    def cache_set(self, key, value):
        self.cache[key] = value

    def cache_get(self, key):
        return self.cache.get(key)

    def cache_clear(self):
        self.cache.clear()

    def set_config(self, key, value):
        self.config[key] = value

    def get_config(self, key, default=None):
        return self.config.get(key, default)

    def record_metric(self, name, value):
        self.metrics[name] = value

    def get_metric(self, name):
        return self.metrics.get(name, 0)

    def send_notification(self, user_email, message):
        self.notifications.append({"to": user_email, "body": message})

    def get_unread_notifications(self, user_email):
        return [n for n in self.notifications if n["to"] == user_email]
