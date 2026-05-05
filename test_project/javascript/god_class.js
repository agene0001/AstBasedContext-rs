// Check 10: God class

class ServiceManager {
    constructor() {
        this.users = [];
        this.products = [];
        this.orders = [];
        this.logs = [];
        this.cache = {};
    }

    addUser(name, email) {
        this.users.push({ name, email });
    }

    removeUser(email) {
        this.users = this.users.filter(u => u.email !== email);
    }

    findUser(email) {
        return this.users.find(u => u.email === email);
    }

    addProduct(name, price) {
        this.products.push({ name, price });
    }

    removeProduct(name) {
        this.products = this.products.filter(p => p.name !== name);
    }

    findProduct(name) {
        return this.products.find(p => p.name === name);
    }

    createOrder(userId, productId) {
        this.orders.push({ userId, productId, status: "pending" });
    }

    cancelOrder(index) {
        if (this.orders[index]) {
            this.orders[index].status = "cancelled";
        }
    }

    pendingOrders() {
        return this.orders.filter(o => o.status === "pending");
    }

    logInfo(msg) {
        this.logs.push({ level: "info", msg });
    }

    logError(msg) {
        this.logs.push({ level: "error", msg });
    }

    getErrors() {
        return this.logs.filter(l => l.level === "error");
    }

    cacheSet(key, value) {
        this.cache[key] = value;
    }

    cacheGet(key) {
        return this.cache[key];
    }

    cacheClear() {
        this.cache = {};
    }
}

module.exports = { ServiceManager };
