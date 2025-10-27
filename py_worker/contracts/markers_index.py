from __future__ import annotations

from datetime import UTC, datetime

from pydantic import Field, field_serializer

from .base import SchemaModel


class MarkerInfo(SchemaModel):
    schema_name = None

    name: str
    description: str | None = None
    registered: bool = False
    usage_count: int = Field(default=0, ge=0)
    first_seen: str
    common_args: list[str] = Field(default_factory=list)


class MarkersIndex(SchemaModel):
    schema_name = "markers.index.json"

    version: str
    generated_at: datetime = Field(default_factory=lambda: datetime.now(UTC))
    markers: dict[str, MarkerInfo] = Field(default_factory=dict)
    test_markers: dict[str, list[str]] = Field(default_factory=dict)

    @field_serializer("generated_at", when_used="json")
    def serialize_generated_at(self, value: datetime) -> str:
        return value.astimezone(UTC).isoformat().replace("+00:00", "Z")
