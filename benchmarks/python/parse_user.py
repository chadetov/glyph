from dataclasses import dataclass
from typing import Any


@dataclass
class User:
    name: str
    age: int


@dataclass
class ParseOk:
    value: User


@dataclass
class ParseErr:
    error: str


def parse_user(input: Any) -> ParseOk | ParseErr:
    if not isinstance(input, dict):
        return ParseErr("expected object")
    name = input.get("name")
    if not isinstance(name, str):
        return ParseErr("name: expected string")
    age = input.get("age")
    if not isinstance(age, (int, float)):
        return ParseErr("age: expected number")
    return ParseOk(User(name=name, age=age))
