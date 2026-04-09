"""
patterns.py — triggers every redundancy-analyser check in AstBasedContext-rs.
Each section is labeled with the check abbreviation it is designed to fire.
"""

from abc import ABC, abstractmethod


# ─── EE: DetectedEventEmitter ────────────────────────────────────────────────

class EventBus:
    """Has on / off / emit → triggers the on/off/emit event-method set."""

    def on(self, event: str, handler) -> None:
        pass

    def off(self, event: str, handler) -> None:
        pass

    def emit(self, event: str, *args) -> None:
        pass


# ─── MEM: DetectedMemento ─────────────────────────────────────────────────────

class Editor:
    """Has save_state / restore_state → triggers the Memento pair."""

    def __init__(self) -> None:
        self._content = ""

    def save_state(self) -> dict:
        return {"content": self._content}

    def restore_state(self, state: dict) -> None:
        self._content = state["content"]


# ─── NO: DetectedNullObject ───────────────────────────────────────────────────

class Animal(ABC):
    @abstractmethod
    def speak(self) -> str: ...

    @abstractmethod
    def move(self) -> None: ...


class NullAnimal(Animal):
    """Name contains 'null', extends Animal, all methods ≤3 lines with no callees."""

    def speak(self) -> str:
        return ""

    def move(self) -> None:
        pass


# ─── VIS: DetectedVisitor ─────────────────────────────────────────────────────

class AstVisitor:
    """≥3 methods starting with visit_ → Visitor pattern."""

    def visit_function(self, node) -> None:
        pass

    def visit_class(self, node) -> None:
        pass

    def visit_import(self, node) -> None:
        pass

    def visit_assignment(self, node) -> None:
        pass


# ─── ITR: DetectedIterator ────────────────────────────────────────────────────

class NumberRange:
    """Has __iter__ and __next__ → Iterator pattern."""

    def __init__(self, start: int, stop: int) -> None:
        self._current = start
        self._stop = stop

    def __iter__(self):
        return self

    def __next__(self) -> int:
        if self._current >= self._stop:
            raise StopIteration
        val = self._current
        self._current += 1
        return val


# ─── PRT: DetectedPrototype ───────────────────────────────────────────────────

class Config:
    """Has a clone method → Prototype pattern."""

    def __init__(self, data: dict) -> None:
        self._data = data

    def clone(self) -> "Config":
        return Config(dict(self._data))


# ─── FLY: DetectedFlyweight ───────────────────────────────────────────────────

class Color:
    """
    Static _cache field (is_static=True in parser) + get_color factory method.
    The Python parser marks class-level assignments with is_static=True.
    """
    _cache: dict = {}

    def __init__(self, r: int, g: int, b: int) -> None:
        self.r = r
        self.g = g
        self.b = b

    @classmethod
    def get_color(cls, r: int, g: int, b: int) -> "Color":
        key = (r, g, b)
        if key not in cls._cache:
            cls._cache[key] = Color(r, g, b)
        return cls._cache[key]


# ─── FB: DetectedFluentBuilder ────────────────────────────────────────────────

class QueryBuilder:
    """≥3 methods whose return type annotation is the class name → Fluent Builder."""

    def __init__(self) -> None:
        self._table = ""
        self._conditions: list = []
        self._limit_val = 0

    def table(self, name: str) -> "QueryBuilder":
        self._table = name
        return self

    def where(self, condition: str) -> "QueryBuilder":
        self._conditions.append(condition)
        return self

    def limit(self, n: int) -> "QueryBuilder":
        self._limit_val = n
        return self

    def build(self) -> str:
        conds = " AND ".join(self._conditions)
        return f"SELECT * FROM {self._table} WHERE {conds} LIMIT {self._limit_val}"


# ─── SNG: DetectedSingleton ───────────────────────────────────────────────────

class AppConfig:
    """
    _instance static field (name matches sentinel list) + get_instance static
    accessor → 2/3 Singleton signals satisfied.
    """
    _instance: "AppConfig" = None  # type: ignore[assignment]

    @staticmethod
    def get_instance() -> "AppConfig":
        if AppConfig._instance is None:
            AppConfig._instance = AppConfig()
        return AppConfig._instance

    def setting(self, key: str) -> str:
        return ""


# ─── CMD: DetectedCommand ─────────────────────────────────────────────────────

class Command(ABC):
    """Single-method interface — triggers Command detection when ≥3 subclasses exist."""

    @abstractmethod
    def execute(self) -> None: ...


class SaveCommand(Command):
    def execute(self) -> None:
        pass


class DeleteCommand(Command):
    def execute(self) -> None:
        pass


class PrintCommand(Command):
    def execute(self) -> None:
        pass


# ─── LZ: LazyClass ────────────────────────────────────────────────────────────

class Wrapper:
    """No base classes, ≤2 methods, each ≤5 lines → LazyClass."""

    def value(self) -> int:
        return 42

    def label(self) -> str:
        return "wrapped"


# ─── RB: RefusedBequest ───────────────────────────────────────────────────────

class Vehicle:
    """Parent with its own distinct methods."""

    def start_engine(self) -> None:
        pass

    def stop_engine(self) -> None:
        pass

    def refuel(self) -> None:
        pass


class Bicycle(Vehicle):
    """
    Inherits Vehicle but has completely different method names (zero overlap),
    and never calls any parent method → RefusedBequest.
    """

    def pedal(self) -> None:
        pass

    def brake(self) -> None:
        pass

    def change_gear(self, gear: int) -> None:
        pass


# ─── AM: AnemicDomainModel ────────────────────────────────────────────────────

class UserRecord:
    """
    5 methods, all starting with get_ or set_ → ≥80 % getter/setter ratio,
    gs_count ≥ 4 → AnemicDomainModel.
    """

    def get_name(self) -> str:
        return self._name  # type: ignore[attr-defined]

    def set_name(self, value: str) -> None:
        self._name = value  # type: ignore[attr-defined]

    def get_email(self) -> str:
        return self._email  # type: ignore[attr-defined]

    def set_email(self, value: str) -> None:
        self._email = value  # type: ignore[attr-defined]

    def get_age(self) -> int:
        return self._age  # type: ignore[attr-defined]


# ─── HTE: SuggestEnumFromHierarchy ───────────────────────────────────────────

class Shape(ABC):
    """Base with ≥3 leaf subclasses that have no fields and ≤3 methods."""

    @abstractmethod
    def area(self) -> float: ...


class Circle(Shape):
    def area(self) -> float:
        return 0.0


class Square(Shape):
    def area(self) -> float:
        return 0.0


class Triangle(Shape):
    def area(self) -> float:
        return 0.0


# ─── STR: SuggestStrategy ─────────────────────────────────────────────────────

class SortStrategy(ABC):
    """Abstract interface with ≥3 concrete implementing subclasses."""

    @abstractmethod
    def sort(self, data: list) -> list: ...


class BubbleSort(SortStrategy):
    def sort(self, data: list) -> list:
        return sorted(data)


class MergeSort(SortStrategy):
    def sort(self, data: list) -> list:
        return sorted(data)


class QuickSort(SortStrategy):
    def sort(self, data: list) -> list:
        return sorted(data)


# ─── TM: SuggestTemplateMethod ────────────────────────────────────────────────

class DataProcessor:
    """
    Base class with methods load and process that ALL subclasses override
    → SuggestTemplateMethod (hook_methods.len() >= 2, subclass_count >= 2).
    """

    def load(self) -> None:
        pass

    def process(self) -> None:
        pass

    def run(self) -> None:
        self.load()
        self.process()


class CsvProcessor(DataProcessor):
    def load(self) -> None:
        pass

    def process(self) -> None:
        pass


class JsonProcessor(DataProcessor):
    def load(self) -> None:
        pass

    def process(self) -> None:
        pass


# ─── STE: SuggestTraitExtraction ─────────────────────────────────────────────

class FileLogger:
    """
    Shares serialize, validate, and reset with NetworkLogger.
    Both are unrelated by inheritance → SuggestTraitExtraction (≥3 shared methods).
    """

    def serialize(self, data: dict) -> str:
        return str(data)

    def validate(self, data: dict) -> bool:
        return bool(data)

    def reset(self) -> None:
        pass

    def write(self, msg: str) -> None:
        pass


class NetworkLogger:
    """Shares serialize, validate, and reset with FileLogger."""

    def serialize(self, data: dict) -> str:
        return str(data)

    def validate(self, data: dict) -> bool:
        return bool(data)

    def reset(self) -> None:
        pass

    def send(self, msg: str) -> None:
        pass


# ─── IN: InconsistentNaming ───────────────────────────────────────────────────
# These must be FILE-LEVEL functions (direct children of the file node).
# The check scans functions that have either '_' or uppercase chars,
# then splits into pure-snake vs camelCase.  Need ≥2 of each.

def process_data(items: list) -> list:
    return items


def load_config(path: str) -> dict:
    return {}


def parseResponse(raw: str) -> dict:  # camelCase
    return {}


def buildQuery(table: str) -> str:  # camelCase
    return ""


# ─── GC: GodClass ────────────────────────────────────────────────────────────

class GodClass:
    """≥20 methods → GodClass (threshold is 20 for classes)."""

    def method_01(self) -> None: pass
    def method_02(self) -> None: pass
    def method_03(self) -> None: pass
    def method_04(self) -> None: pass
    def method_05(self) -> None: pass
    def method_06(self) -> None: pass
    def method_07(self) -> None: pass
    def method_08(self) -> None: pass
    def method_09(self) -> None: pass
    def method_10(self) -> None: pass
    def method_11(self) -> None: pass
    def method_12(self) -> None: pass
    def method_13(self) -> None: pass
    def method_14(self) -> None: pass
    def method_15(self) -> None: pass
    def method_16(self) -> None: pass
    def method_17(self) -> None: pass
    def method_18(self) -> None: pass
    def method_19(self) -> None: pass
    def method_20(self) -> None: pass


# ─── LCL: LargeClass ─────────────────────────────────────────────────────────
# Source must be ≥ 500 lines.  We use docstrings to pad without fake logic.

class LargeService:
    """
    Large service class.  Its source (from 'class LargeService:' to the last
    method) must reach 500 lines so LargeClass fires.

    Line budget:
      - class header + this docstring   ~20 lines
      - 30 methods × ~16 lines each     480 lines
      Total                            ~500 lines
    """

    def do_alpha(self) -> None:
        """
        Alpha operation.
        Performs the alpha step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_beta(self) -> None:
        """
        Beta operation.
        Performs the beta step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_gamma(self) -> None:
        """
        Gamma operation.
        Performs the gamma step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_delta(self) -> None:
        """
        Delta operation.
        Performs the delta step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_epsilon(self) -> None:
        """
        Epsilon operation.
        Performs the epsilon step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_zeta(self) -> None:
        """
        Zeta operation.
        Performs the zeta step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_eta(self) -> None:
        """
        Eta operation.
        Performs the eta step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_theta(self) -> None:
        """
        Theta operation.
        Performs the theta step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_iota(self) -> None:
        """
        Iota operation.
        Performs the iota step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_kappa(self) -> None:
        """
        Kappa operation.
        Performs the kappa step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_lambda(self) -> None:
        """
        Lambda operation.
        Performs the lambda step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_mu(self) -> None:
        """
        Mu operation.
        Performs the mu step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_nu(self) -> None:
        """
        Nu operation.
        Performs the nu step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_xi(self) -> None:
        """
        Xi operation.
        Performs the xi step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_omicron(self) -> None:
        """
        Omicron operation.
        Performs the omicron step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_pi(self) -> None:
        """
        Pi operation.
        Performs the pi step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_rho(self) -> None:
        """
        Rho operation.
        Performs the rho step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_sigma(self) -> None:
        """
        Sigma operation.
        Performs the sigma step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_tau(self) -> None:
        """
        Tau operation.
        Performs the tau step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_upsilon(self) -> None:
        """
        Upsilon operation.
        Performs the upsilon step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_phi(self) -> None:
        """
        Phi operation.
        Performs the phi step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_chi(self) -> None:
        """
        Chi operation.
        Performs the chi step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_psi(self) -> None:
        """
        Psi operation.
        Performs the psi step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_omega(self) -> None:
        """
        Omega operation.
        Performs the omega step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_alpha2(self) -> None:
        """
        Alpha2 operation.
        Performs the alpha2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_beta2(self) -> None:
        """
        Beta2 operation.
        Performs the beta2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_gamma2(self) -> None:
        """
        Gamma2 operation.
        Performs the gamma2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_delta2(self) -> None:
        """
        Delta2 operation.
        Performs the delta2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_epsilon2(self) -> None:
        """
        Epsilon2 operation.
        Performs the epsilon2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_zeta2(self) -> None:
        """
        Zeta2 operation.
        Performs the zeta2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_eta2(self) -> None:
        """
        Eta2 operation.
        Performs the eta2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_theta2(self) -> None:
        """
        Theta2 operation.
        Performs the theta2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_iota2(self) -> None:
        """
        Iota2 operation.
        Performs the iota2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_kappa2(self) -> None:
        """
        Kappa2 operation.
        Performs the kappa2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_lambda2(self) -> None:
        """
        Lambda2 operation.
        Performs the lambda2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_mu2(self) -> None:
        """
        Mu2 operation.
        Performs the mu2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_nu2(self) -> None:
        """
        Nu2 operation.
        Performs the nu2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_xi2(self) -> None:
        """
        Xi2 operation.
        Performs the xi2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_omicron2(self) -> None:
        """
        Omicron2 operation.
        Performs the omicron2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_pi2(self) -> None:
        """
        Pi2 operation.
        Performs the pi2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_rho2(self) -> None:
        """
        Rho2 operation.
        Performs the rho2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_sigma2(self) -> None:
        """
        Sigma2 operation.
        Performs the sigma2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_tau2(self) -> None:
        """
        Tau2 operation.
        Performs the tau2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_upsilon2(self) -> None:
        """
        Upsilon2 operation.
        Performs the upsilon2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_phi2(self) -> None:
        """
        Phi2 operation.
        Performs the phi2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_chi2(self) -> None:
        """
        Chi2 operation.
        Performs the chi2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_psi2(self) -> None:
        """
        Psi2 operation.
        Performs the psi2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_omega2(self) -> None:
        """
        Omega2 operation.
        Performs the omega2 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_alpha3(self) -> None:
        """
        Alpha3 operation.
        Performs the alpha3 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_beta3(self) -> None:
        """
        Beta3 operation.
        Performs the beta3 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_gamma3(self) -> None:
        """
        Gamma3 operation.
        Performs the gamma3 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_delta3(self) -> None:
        """
        Delta3 operation.
        Performs the delta3 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_epsilon3(self) -> None:
        """
        Epsilon3 operation.
        Performs the epsilon3 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_zeta3(self) -> None:
        """
        Zeta3 operation.
        Performs the zeta3 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        """
        pass

    def do_eta3(self) -> None:
        """
        Eta3 operation.
        Performs the eta3 step of the large-service pipeline.
        Allocates resources, validates inputs, and delegates.
        This docstring intentionally occupies several lines.
        Padding to push the class past the 500-line source threshold.
        This line and the next are extra padding for the line counter.
        We want the class source to span well past 500 lines total.
        """
        pass


# ─── SED: SuggestEnumDispatch ─────────────────────────────────────────────────
# Param named is_* or *_mode, source branches on it with "if is_X", cc ≥ 3.

def render_output(data: list, is_verbose: bool, output_mode: str) -> str:
    """Render data with branching on flag params → SuggestEnumDispatch."""
    result = ""
    if is_verbose:
        for item in data:
            result += f"  - {item!r}\n"
    elif output_mode == "json":
        import json
        result = json.dumps(data)
    else:
        result = str(data)
    return result


# ─── CMP: DetectedComposite ───────────────────────────────────────────────────

class TreeNode:
    """Field children typed as list[TreeNode] → Composite pattern."""
    children: "list[TreeNode]"

    def __init__(self, value: int) -> None:
        self.value = value
        self.children: list[TreeNode] = []

    def add_child(self, child: "TreeNode") -> None:
        self.children.append(child)


# ─── COR: DetectedChainOfResponsibility ──────────────────────────────────────

class Handler:
    """Field next_handler typed as Handler → Chain of Responsibility."""
    next_handler: "Handler"

    def __init__(self) -> None:
        self.next_handler: Handler = None  # type: ignore[assignment]

    def handle(self, request: int) -> None:
        if self.next_handler is not None:
            self.next_handler.handle(request)


# ─── DI: DetectedDependencyInjection ─────────────────────────────────────────
# Constructor with params typed as abstract base classes → dependency injection.

class Logger(ABC):
    @abstractmethod
    def log(self, msg: str) -> None: ...

class ConsoleLogger(Logger):
    def log(self, msg: str) -> None:
        print(msg)

class FileLogger(Logger):
    def log(self, msg: str) -> None:
        pass

class NullLogger(Logger):
    def log(self, msg: str) -> None:
        pass

class Service:
    """Constructor takes Logger (abstract base with 3+ implementors) → DI detected."""
    def __init__(self, logger: Logger, name: str) -> None:
        self.logger = logger
        self.name = name

    def run(self) -> None:
        self.logger.log(f"Running {self.name}")


# ─── STA: DetectedState ──────────────────────────────────────────────────────
# Interface/ABC with 2+ implementors that have a field typed as the interface.

class ConnectionState(ABC):
    @abstractmethod
    def open(self) -> None: ...
    @abstractmethod
    def close(self) -> None: ...

class OpenState(ConnectionState):
    current_state: "ConnectionState" = None  # type: ignore

    def open(self) -> None:
        pass

    def close(self) -> None:
        pass

class ClosedState(ConnectionState):
    current_state: "ConnectionState" = None  # type: ignore

    def open(self) -> None:
        pass

    def close(self) -> None:
        pass

class IdleState(ConnectionState):
    current_state: "ConnectionState" = None  # type: ignore

    def open(self) -> None:
        pass

    def close(self) -> None:
        pass


# ─── PRX: DetectedProxy ──────────────────────────────────────────────────────
# Class implements an interface AND has a field typed as that same interface.

class ImageLoader(ABC):
    @abstractmethod
    def load(self, url: str) -> bytes: ...

class RealImageLoader(ImageLoader):
    def load(self, url: str) -> bytes:
        return b""

class CachedImageLoader(ImageLoader):
    """Implements ImageLoader and wraps an ImageLoader field → Proxy."""
    loader: ImageLoader = None  # type: ignore

    def load(self, url: str) -> bytes:
        return self.loader.load(url)


# ─── ADP: DetectedAdapter ────────────────────────────────────────────────────
# Class implements an interface and wraps a field of a DIFFERENT type.

class PaymentGateway(ABC):
    @abstractmethod
    def charge(self, amount: float) -> bool: ...

class StripeGateway(PaymentGateway):
    def charge(self, amount: float) -> bool:
        return True

class LegacyBilling:
    def process_payment(self, cents: int) -> int:
        return 0

class LegacyBillingAdapter(PaymentGateway):
    """Implements PaymentGateway, wraps LegacyBilling (different type) → Adapter."""
    billing: LegacyBilling = None  # type: ignore

    def charge(self, amount: float) -> bool:
        self.billing.process_payment(int(amount * 100))
        return True

    def refund(self, amount: float) -> bool:
        self.billing.process_payment(-int(amount * 100))
        return True


# ── FE (Feature Envy) ──────────────────────────────────────────────────────
# Method in ClassA calls 3+ methods on ClassB, more than its own class.
class DataStore:
    def get_item(self, key):
        return key

    def set_item(self, key, val):
        pass

    def delete_item(self, key):
        pass

    def list_items(self):
        return []

class DataAnalyzer:
    def local_work(self):
        return 1

    def envious_method(self, store: DataStore):
        """Calls 4 DataStore methods but only 1 own method → FE."""
        self.local_work()
        store.get_item("a")
        store.set_item("b", 1)
        store.delete_item("c")
        store.list_items()


# ── II (Inappropriate Intimacy) ────────────────────────────────────────────
# Two classes where each calls the other 5+ times.
class ClassAlpha:
    def a1(self): return 1
    def a2(self): return 2
    def a3(self): return 3
    def a4(self): return 4
    def a5(self): return 5
    def a6(self): return 6

    def call_beta(self, b):
        b.b1(); b.b2(); b.b3(); b.b4(); b.b5()

class ClassBeta:
    def b1(self): return 1
    def b2(self): return 2
    def b3(self): return 3
    def b4(self): return 4
    def b5(self): return 5
    def b6(self): return 6

    def call_alpha(self, a):
        a.a1(); a.a2(); a.a3(); a.a4(); a.a5()


# ── HC (High Coupling) ────────────────────────────────────────────────────
# Class calling methods from 10+ other classes.
class ExtA:
    def do_a(self): pass
class ExtB:
    def do_b(self): pass
class ExtC:
    def do_c(self): pass
class ExtD:
    def do_d(self): pass
class ExtE:
    def do_e(self): pass
class ExtF:
    def do_f(self): pass
class ExtG:
    def do_g(self): pass
class ExtH:
    def do_h(self): pass
class ExtI:
    def do_i(self): pass
class ExtJ:
    def do_j(self): pass
class ExtK:
    def do_k(self): pass

class HighlyCoupled:
    """CBO >= 11 → triggers HC."""
    def coupled_work(self, a: ExtA, b: ExtB, c: ExtC, d: ExtD, e: ExtE,
                     f: ExtF, g: ExtG, h: ExtH, i: ExtI, j: ExtJ, k: ExtK):
        a.do_a(); b.do_b(); c.do_c(); d.do_d(); e.do_e()
        f.do_f(); g.do_g(); h.do_h(); i.do_i(); j.do_j()
        k.do_k()


# ── MM (Middle Man) ────────────────────────────────────────────────────────
# Class with 4 methods, each delegating to exactly 1 method on another class.
class RealWorker:
    def task_one(self): return 1
    def task_two(self): return 2
    def task_three(self): return 3
    def task_four(self): return 4

class MiddleManClass:
    """80%+ passthroughs to RealWorker → MM."""
    def do_one(self, w: RealWorker):
        return w.task_one()
    def do_two(self, w: RealWorker):
        return w.task_two()
    def do_three(self, w: RealWorker):
        return w.task_three()
    def do_four(self, w: RealWorker):
        return w.task_four()


# ── M (Merge Candidate) ───────────────────────────────────────────────────
# Two functions with 40-80% shared lines, 8+ lines each, 2+ unique lines each.
def merge_candidate_alpha(data, mode):
    validated = validate_data(data)
    normalized = normalize_data(validated)
    result = compute_result(normalized)
    logged = log_result(result)
    formatted = format_output(logged)
    stored = store_result(formatted)
    if mode == "alpha":
        extra_alpha_step_one(stored)
        extra_alpha_step_two(stored)
    return stored

def merge_candidate_beta(data, mode):
    validated = validate_data(data)
    normalized = normalize_data(validated)
    result = compute_result(normalized)
    logged = log_result(result)
    formatted = format_output(logged)
    stored = store_result(formatted)
    if mode == "beta":
        extra_beta_step_one(stored)
        extra_beta_step_two(stored)
    return stored
