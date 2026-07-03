from __future__ import annotations

import re
import sys
from dataclasses import dataclass, field
from html import unescape
from html.parser import HTMLParser
from pathlib import Path
from typing import Any, Callable, Mapping, Sequence
from urllib.parse import urlencode, urljoin, urlparse, urlunparse

try:
    import tomllib as _tomllib
except ModuleNotFoundError:  # pragma: no cover - depends on the user's Python version.
    _tomllib = None

from magazine_core_plugin_sdk import SourceRecord, run_plugin


DEFAULT_ENDPOINT = "/wp-json/wp/v2/posts"
DEFAULT_PER_PAGE = 20
DEFAULT_MAX_PAGES = 1
DEFAULT_MAX_RECORDS = 20
DEFAULT_HOST_FETCH_TIMEOUT_SECONDS = 30.0


class ProfileError(ValueError):
    pass


@dataclass(frozen=True)
class WordpressRestProfile:
    profile_version: int
    source_name: str
    display_label: str
    base_url: str
    allowed_domains: list[str]
    brand_raw: str
    endpoint: str = DEFAULT_ENDPOINT
    category_ids: list[int] = field(default_factory=list)
    default_per_page: int = DEFAULT_PER_PAGE
    default_max_pages: int = DEFAULT_MAX_PAGES
    default_max_records: int = DEFAULT_MAX_RECORDS


class _TextExtractor(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.parts: list[str] = []

    def handle_data(self, data: str) -> None:
        self.parts.append(data)

    def text(self) -> str:
        return re.sub(r"\s+", " ", unescape("".join(self.parts))).strip()


class _ImageSrcExtractor(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.sources: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag.lower() != "img":
            return
        for name, value in attrs:
            if name.lower() == "src" and value:
                self.sources.append(unescape(value.strip()))
                return


FetchJson = Callable[[str, float], Any]


def load_profile(path: Path) -> WordpressRestProfile:
    toml = _toml_module()
    with path.open("rb") as handle:
        return profile_from_mapping(toml.load(handle))


def profile_from_mapping(raw: Mapping[str, Any]) -> WordpressRestProfile:
    profile = WordpressRestProfile(
        profile_version=_int_field(raw, "profile_version"),
        source_name=_str_field(raw, "source_name"),
        display_label=_str_field(raw, "display_label"),
        base_url=_str_field(raw, "base_url"),
        allowed_domains=_str_list_field(raw, "allowed_domains"),
        endpoint=_optional_str_field(raw, "endpoint", default=DEFAULT_ENDPOINT),
        category_ids=_int_list_field(raw, "category_ids", default=[]),
        default_per_page=_int_field(raw, "default_per_page", default=DEFAULT_PER_PAGE),
        default_max_pages=_int_field(raw, "default_max_pages", default=DEFAULT_MAX_PAGES),
        default_max_records=_int_field(raw, "default_max_records", default=DEFAULT_MAX_RECORDS),
        brand_raw=_str_field(raw, "brand_raw"),
    )
    validate_profile(profile)
    return profile


def validate_profile(profile: WordpressRestProfile) -> None:
    if profile.profile_version != 1:
        raise ProfileError("profile_version must be 1")
    if not profile.source_name.strip():
        raise ProfileError("source_name must be non-empty")
    if not profile.display_label.strip():
        raise ProfileError("display_label must be non-empty")
    if not profile.brand_raw.strip():
        raise ProfileError("brand_raw must be non-empty")
    if not profile.allowed_domains:
        raise ProfileError("allowed_domains must contain at least one domain")
    for domain in profile.allowed_domains:
        _normalize_allowed_domain(domain)

    parsed_base = urlparse(profile.base_url)
    if parsed_base.scheme not in {"http", "https"}:
        raise ProfileError("base_url must use http or https")
    if not parsed_base.hostname:
        raise ProfileError("base_url must include a host")
    if not _host_in_allowed_domains(parsed_base.hostname, profile.allowed_domains):
        raise ProfileError("base_url host must be within allowed_domains")

    parsed_endpoint = urlparse(profile.endpoint)
    if parsed_endpoint.scheme or parsed_endpoint.netloc:
        raise ProfileError("endpoint must be a path, not an absolute URL")
    if parsed_endpoint.query or parsed_endpoint.fragment:
        raise ProfileError("endpoint must not include query or fragment")
    if not profile.endpoint.startswith("/"):
        raise ProfileError("endpoint must start with /")
    if profile.default_per_page <= 0:
        raise ProfileError("default_per_page must be positive")
    if profile.default_max_pages < 0:
        raise ProfileError("default_max_pages must be zero or positive")
    if profile.default_max_records < 0:
        raise ProfileError("default_max_records must be zero or positive")
    if any(category_id <= 0 for category_id in profile.category_ids):
        raise ProfileError("category_ids must be positive integers")


def discover_records(
    profile: WordpressRestProfile,
    params: Mapping[str, Any],
    fetch_json: FetchJson,
) -> list[SourceRecord]:
    max_pages = _discover_limit(params, "max_pages", profile.default_max_pages)
    per_page = _discover_limit(params, "per_page", profile.default_per_page)
    max_records = _discover_limit(params, "max_records", profile.default_max_records)
    timeout_seconds = _host_timeout_seconds(params)

    if max_pages < 0:
        raise ValueError("max_pages must be zero or positive")
    if max_records < 0:
        raise ValueError("max_records must be zero or positive")
    if per_page <= 0:
        raise ValueError("per_page must be positive")
    if max_pages == 0 or max_records == 0:
        return []

    records: list[SourceRecord] = []
    for page in range(1, max_pages + 1):
        posts = fetch_json(_posts_url(profile, page=page, per_page=per_page), timeout_seconds)
        if not isinstance(posts, list):
            raise ValueError("WordPress posts response must be a JSON array")
        if not posts:
            break
        for post in posts:
            if not isinstance(post, Mapping):
                raise ValueError("WordPress post entry must be a JSON object")
            records.append(record_from_post(profile, post))
            if len(records) >= max_records:
                return records
    return records


def record_from_post(
    profile: WordpressRestProfile,
    post: Mapping[str, Any],
) -> SourceRecord:
    source_url = _str_from_post(post, "link")
    title = strip_html(_nested_str(post, "title", "rendered"))
    return SourceRecord(
        source_name=profile.source_name,
        source_url=source_url,
        title=title,
        brand_raw=profile.brand_raw,
        performers_raw=[],
        cover_urls=_cover_urls_from_content(_nested_str(post, "content", "rendered")),
        page_urls=[],
        post_date=_date_part(post.get("date")),
        extra={
            "wordpress_id": post.get("id"),
            "categories": _list_value(post.get("categories")),
            "tags": _list_value(post.get("tags")),
            "slug": post.get("slug"),
        },
    )


def strip_html(value: str) -> str:
    parser = _TextExtractor()
    parser.feed(value)
    parser.close()
    return parser.text()


def _cover_urls_from_content(content: str) -> list[str]:
    parser = _ImageSrcExtractor()
    parser.feed(content)
    parser.close()
    absolute_sources = [
        source
        for source in parser.sources
        if urlparse(source).scheme in {"http", "https"} and urlparse(source).netloc
    ]
    return absolute_sources if len(absolute_sources) == 1 else []


def _posts_url(profile: WordpressRestProfile, *, page: int, per_page: int) -> str:
    parsed_base = urlparse(profile.base_url)
    endpoint = urlparse(profile.endpoint)
    base_root = urlunparse((parsed_base.scheme, parsed_base.netloc, "/", "", "", ""))
    url = urljoin(base_root, endpoint.path.lstrip("/"))
    query: list[tuple[str, str]] = [
        ("per_page", str(per_page)),
        ("page", str(page)),
    ]
    if profile.category_ids:
        query.append(("categories", ",".join(str(value) for value in profile.category_ids)))
    return f"{url}?{urlencode(query)}"


def _discover_limit(
    params: Mapping[str, Any],
    key: str,
    default: int,
) -> int:
    limits = params.get("limits") or {}
    if not isinstance(limits, Mapping):
        raise ValueError("discover limits must be an object")
    if limits.get(key) is None:
        return default
    value = limits[key]
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"discover limit {key} must be an integer")
    return value


def _host_timeout_seconds(params: Mapping[str, Any]) -> float:
    remaining_ms = params.get("remaining_ms")
    if remaining_ms is None:
        return DEFAULT_HOST_FETCH_TIMEOUT_SECONDS
    if isinstance(remaining_ms, bool) or not isinstance(remaining_ms, (int, float)):
        raise ValueError("remaining_ms must be numeric")
    if remaining_ms <= 0:
        raise ValueError("remaining_ms must be positive")
    return remaining_ms / 1000.0


def _host_in_allowed_domains(host: str, allowed_domains: Sequence[str]) -> bool:
    normalized_host = host.rstrip(".").lower()
    return any(
        normalized_host == domain
        or normalized_host.endswith(f".{domain}")
        for domain in (_normalize_allowed_domain(value) for value in allowed_domains)
    )


def _normalize_allowed_domain(domain: str) -> str:
    normalized = domain.rstrip(".").lower()
    if not normalized or any(char in normalized for char in "/:@"):
        raise ProfileError("allowed_domains entries must be hostnames")
    return normalized


def _int_field(raw: Mapping[str, Any], key: str, *, default: int | None = None) -> int:
    value = raw.get(key, default)
    if isinstance(value, bool) or not isinstance(value, int):
        raise ProfileError(f"{key} must be an integer")
    return value


def _str_field(raw: Mapping[str, Any], key: str) -> str:
    value = raw.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ProfileError(f"{key} must be a non-empty string")
    return value


def _optional_str_field(raw: Mapping[str, Any], key: str, *, default: str) -> str:
    value = raw.get(key, default)
    if not isinstance(value, str) or not value.strip():
        raise ProfileError(f"{key} must be a non-empty string")
    return value


def _str_list_field(raw: Mapping[str, Any], key: str) -> list[str]:
    value = raw.get(key)
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        raise ProfileError(f"{key} must be a list of strings")
    return list(value)


def _int_list_field(
    raw: Mapping[str, Any],
    key: str,
    *,
    default: list[int],
) -> list[int]:
    value = raw.get(key, default)
    if not isinstance(value, list) or any(
        isinstance(item, bool) or not isinstance(item, int) for item in value
    ):
        raise ProfileError(f"{key} must be a list of integers")
    return list(value)


def _nested_str(post: Mapping[str, Any], outer: str, inner: str) -> str:
    value = post.get(outer) or {}
    if not isinstance(value, Mapping):
        raise ValueError(f"post.{outer} must be an object")
    nested = value.get(inner)
    if not isinstance(nested, str):
        raise ValueError(f"post.{outer}.{inner} must be a string")
    return nested


def _str_from_post(post: Mapping[str, Any], key: str) -> str:
    value = post.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"post.{key} must be a non-empty string")
    parsed = urlparse(value)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise ValueError(f"post.{key} must be an absolute http(s) URL")
    return value


def _date_part(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    match = re.match(r"^(\d{4}-\d{2}-\d{2})(?:T.*)?$", value)
    return match.group(1) if match else None


def _list_value(value: Any) -> list[Any]:
    return list(value) if isinstance(value, list) else []


def _toml_module():
    if _tomllib is not None:
        return _tomllib
    try:
        import tomli
    except ModuleNotFoundError as exc:
        raise RuntimeError(
            "reading TOML profiles on Python < 3.11 requires the optional tomli package"
        ) from exc
    return tomli


def _discover_callback(profile: WordpressRestProfile):
    def discover(context, params):
        records = discover_records(
            profile,
            params,
            lambda url, timeout: context.host_fetch(url, timeout=timeout).json(),
        )
        context.send_records(records)

    return discover


def main(argv: Sequence[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if len(args) != 1:
        print("usage: wordpress_rest_plugin.py <profile.toml>", file=sys.stderr)
        return 2
    profile = load_profile(Path(args[0]))
    run_plugin(
        source_name=profile.source_name,
        display_label=profile.display_label,
        discover=_discover_callback(profile),
        allowed_domains=profile.allowed_domains,
        capabilities=["discover", "host_fetch"],
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
