from __future__ import annotations

from datetime import UTC, datetime

from pydantic import Field, field_serializer

from .base import SchemaModel


class ParametrizeInfo(SchemaModel):
    schema_name = None

    params: list[str] = Field(default_factory=list)
    ids: list[str] = Field(default_factory=list)


class CollectionError(SchemaModel):
    schema_name = None

    path: str
    line: int | None = Field(default=None, ge=1)
    error_type: str
    message: str


class TestNode(SchemaModel):
    schema_name = None

    nodeid: str
    path: str
    line: int = Field(ge=1)
    function: str
    class_name: str | None = Field(default=None, alias="class")
    module: str
    markers: list[str] = Field(default_factory=list)
    fixtures: list[str] = Field(default_factory=list)
    parametrize: ParametrizeInfo | None = None


class TestsIndex(SchemaModel):
    schema_name = "tests.index.json"

    version: str
    generated_at: datetime = Field(default_factory=lambda: datetime.now(UTC))
    python_version: str
    pytest_version: str
    tests: list[TestNode] = Field(default_factory=list)
    collection_errors: list[CollectionError] = Field(default_factory=list)

    @field_serializer("generated_at", when_used="json")
    def serialize_generated_at(self, value: datetime) -> str:
        return value.astimezone(UTC).isoformat().replace("+00:00", "Z")
