import os
from pathlib import Path
from typing import List, Optional as Opt


def hello():
    """Say hello."""
    print("world")


def add(a, b):
    return a + b


x = 42
name = "test"


class Greeter:
    """A greeter class."""

    def __init__(self, name):
        self.name = name

    def greet(self):
        return f"Hello {self.name}"


class FormalGreeter(Greeter):
    def greet(self):
        return f"Good day, {self.name}"


def complex_func(a, b, c=10):
    if a > b:
        for i in range(c):
            if i % 2 == 0:
                print(i)
    while a > 0:
        a -= 1
    return a


double = lambda x: x * 2
