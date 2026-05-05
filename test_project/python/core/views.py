# core/views.py — Parallel hierarchy to core/models.py
# Triggers: PI (parallel inheritance) — UserView/OrderView/ProductView mirror
#           UserModel/OrderModel/ProductModel (3-char prefix match)

from abc import ABC, abstractmethod


class BaseView(ABC):
    @abstractmethod
    def render(self) -> str: ...

    @abstractmethod
    def template(self) -> str: ...


class UserView(BaseView):
    def render(self) -> str:
        return "<UserView>"

    def template(self) -> str:
        return "user.html"


class OrderView(BaseView):
    def render(self) -> str:
        return "<OrderView>"

    def template(self) -> str:
        return "order.html"


class ProductView(BaseView):
    def render(self) -> str:
        return "<ProductView>"

    def template(self) -> str:
        return "product.html"
