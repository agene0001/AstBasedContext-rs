# core/models.py — Shared data models
# Triggers: PI (parallel inheritance) with core/views.py mirror hierarchy

from abc import ABC, abstractmethod


class BaseModel(ABC):
    @abstractmethod
    def validate(self) -> bool: ...

    @abstractmethod
    def serialize(self) -> dict: ...


class UserModel(BaseModel):
    def __init__(self, name: str, email: str):
        self.name = name
        self.email = email

    def validate(self) -> bool:
        return bool(self.name and self.email)

    def serialize(self) -> dict:
        return {"name": self.name, "email": self.email}


class OrderModel(BaseModel):
    def __init__(self, order_id: str, total: float):
        self.order_id = order_id
        self.total = total

    def validate(self) -> bool:
        return self.total > 0

    def serialize(self) -> dict:
        return {"id": self.order_id, "total": self.total}


class ProductModel(BaseModel):
    def __init__(self, sku: str, price: float):
        self.sku = sku
        self.price = price

    def validate(self) -> bool:
        return self.price >= 0

    def serialize(self) -> dict:
        return {"sku": self.sku, "price": self.price}
