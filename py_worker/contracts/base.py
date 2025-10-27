from __future__ import annotations

import json
from functools import cache
from pathlib import Path
from typing import Any, ClassVar

from jsonschema import Draft7Validator
from pydantic import BaseModel, ConfigDict


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _schemas_dir() -> Path:
    return _repo_root() / "schemas"


@cache
def _load_schema(schema_name: str) -> dict[str, Any]:
    schema_path = _schemas_dir() / schema_name
    with schema_path.open("r", encoding="utf-8") as handle:
        schema: dict[str, Any] = json.load(handle)
        return schema


@cache
def _schema_validator(schema_name: str) -> Draft7Validator:
    return Draft7Validator(_load_schema(schema_name))


class SchemaModel(BaseModel):
    """Base model that validates against a JSON schema before serialization."""

    schema_name: ClassVar[str | None] = None
    model_config = ConfigDict(populate_by_name=True, extra="forbid")

    def to_schema_dict(self) -> dict[str, Any]:
        data = self.model_dump(mode="json", by_alias=True)
        if self.schema_name is not None:
            _schema_validator(self.schema_name).validate(data)
        return data

    def to_schema_json(self) -> str:
        return json.dumps(self.to_schema_dict(), indent=2)
