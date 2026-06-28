"""Typed dataclass models for protocol record payloads."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Mapping, Optional


JsonObject = dict[str, Any]


@dataclass
class ExternalLink:
    """Typed external link shape from ``record_schema_version = 1``."""

    url: str
    provider: Optional[str] = None
    label: Optional[str] = None
    kind: Optional[str] = None
    external_id: Optional[str] = None
    metadata: JsonObject = field(default_factory=dict)

    def to_dict(self) -> JsonObject:
        return {
            "url": self.url,
            "provider": self.provider,
            "label": self.label,
            "kind": self.kind,
            "external_id": self.external_id,
            "metadata": dict(self.metadata),
        }

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "ExternalLink":
        return cls(
            url=value["url"],
            provider=value.get("provider"),
            label=value.get("label"),
            kind=value.get("kind"),
            external_id=value.get("external_id"),
            metadata=dict(value.get("metadata") or {}),
        )


@dataclass
class SourceRecord:
    """Publication metadata record emitted by a source plugin."""

    source_name: str
    source_url: str
    title: str
    brand_raw: str
    performers_raw: list[str] = field(default_factory=list)
    cover_urls: list[str] = field(default_factory=list)
    page_urls: list[str] = field(default_factory=list)
    issue_no: Optional[str] = None
    external_links: list[ExternalLink] = field(default_factory=list)
    release_date: Optional[str] = None
    post_date: Optional[str] = None
    brand_normalized: Optional[str] = None
    normalizer_id: Optional[str] = None
    normalizer_version: Optional[str] = None
    extra: JsonObject = field(default_factory=dict)

    def to_dict(self) -> JsonObject:
        return {
            "source_name": self.source_name,
            "source_url": self.source_url,
            "title": self.title,
            "brand_raw": self.brand_raw,
            "performers_raw": list(self.performers_raw),
            "cover_urls": list(self.cover_urls),
            "page_urls": list(self.page_urls),
            "issue_no": self.issue_no,
            "external_links": [
                link.to_dict()
                if isinstance(link, ExternalLink)
                else ExternalLink.from_dict(link).to_dict()
                for link in self.external_links
            ],
            "release_date": self.release_date,
            "post_date": self.post_date,
            "brand_normalized": self.brand_normalized,
            "normalizer_id": self.normalizer_id,
            "normalizer_version": self.normalizer_version,
            "extra": dict(self.extra),
        }

    def to_json(self) -> str:
        from .framing import canonical_json

        return canonical_json(self.to_dict())

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "SourceRecord":
        return cls(
            source_name=value["source_name"],
            source_url=value["source_url"],
            title=value["title"],
            brand_raw=value["brand_raw"],
            performers_raw=list(value.get("performers_raw") or []),
            cover_urls=list(value.get("cover_urls") or []),
            page_urls=list(value.get("page_urls") or []),
            issue_no=value.get("issue_no"),
            external_links=[
                link if isinstance(link, ExternalLink) else ExternalLink.from_dict(link)
                for link in value.get("external_links") or []
            ],
            release_date=value.get("release_date"),
            post_date=value.get("post_date"),
            brand_normalized=value.get("brand_normalized"),
            normalizer_id=value.get("normalizer_id"),
            normalizer_version=value.get("normalizer_version"),
            extra=dict(value.get("extra") or {}),
        )
