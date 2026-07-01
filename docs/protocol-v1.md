# magazine-core plugin protocol — v1

正本の protocol 仕様。外部 plugin 作者はこの文書だけで実装できる。サイト固有の
adapter・回避実装・データは対象外（別 private 運用）。`protocol_version = 1`。

2026-06-27 の downstream evidence audit では、host-fetch、typed state、page metadata、
multi-stage discovery、extension parity の実装証跡を踏まえ、当時の v1 contract に
追加変更は不要と判断した。その後の downstream evidence で generic な discover limit
gap が出たため、`protocol_version = 1` の optional limits として `max_pages` /
`max_records` / `per_page` を明文化した。`record_schema_version = 1` は変更しない。
conformance fixture inventory は typed state、discover limits、non-empty `page_urls`
の golden coverage を含む。Python SDK の plugin-author API tier は
`docs/python-sdk.md` を参照する。

## 1. transport と framing

- transport: host と plugin の間の stdin/stdout（plugin は subprocess）。
- frame: `4-byte unsigned big-endian length` + `UTF-8 JSON payload`（length は payload の byte 数）。
- **canonical JSON は送信側の encoding 規則**（object key を再帰的にソート、whitespace なし）。
  golden vector を byte 一致させるためのもの。**受信側は任意の正当な JSON（key 順・whitespace 不問）を受理しなければならない**。言語間で number 表現（float 等）が byte 一致する保証はない。
- limits: `max frame = 8 MiB`、`max pending plugin→host requests = 16`、`max record batch = 100`。
- host implementation はこれに加えて run 単位の memory cap（record 件数・record byte・log byte）を持つ。
- 上限超の frame は**読み書きとも拒否**（fail-closed）。length prefix の途中や body の途中で EOF した場合は `truncated`（clean EOF とは区別し fail-closed）。
- **stdout は protocol 専用、log は stderr のみ**。plugin SDK は言語標準の stdout 書き込み（`print`、native、子プロセスの fd1）が frame を壊さないよう stdout を保護する。

## 2. semantics

- **JSON-RPC 2.0**（request / response / notification）。
- id は送信側で namespace 分離する（host=`h-<n>`、plugin=`p-<n>`）。
- **version は2系統を分離**: `protocol_version`（本 protocol）と `record_schema_version`（§6 の record 形）。
- timeout は絶対時刻でなく host が与える `remaining_ms`（残り時間）。

## 3. state machine

```
initialize → initialized → discover ─(処理中に fetch_request / state_query / record / log)→ complete | error | cancel
```

host は `discover` 応答を待つ間も plugin→host の request（`fetch_request` / `state_query`）を処理する**非同期 message loop**を持つ。
fail-closed host では、`discover` 応答後に plugin が clean exit してから DB ingest へ進む。
non-zero exit や終了しない plugin の spool は破棄する。

## 4. メッセージ

host→plugin（request）:
- `initialize` params `{ protocol_version, host_version }` → result `{ protocol_version, record_schema_version, manifest }`
  - `manifest`: `{ source_name, display_label, allowed_domains: [..], capabilities: [..] }`
- `discover` params `{ request_id, limits: { max_pages?, max_records?, per_page? }, remaining_ms }` → result `{ records: <int> }`
  - `records` は、この `request_id` で host が受理・spool した `SourceRecord` 件数と一致しなければならない。
  - `limits.max_pages` / `limits.per_page` は discovery scope の hint。plugin が page-based source に使う。
  - `limits.max_records` が指定された場合、plugin はそれを超えて `record` を送ってはならない。host も spool 側で同じ上限を強制する。
- `cancel` params `{ request_id }`（notification）

plugin→host:
- `fetch_request` params `{ id, request: { url, method, headers? } }` → result `{ id, status, final_url, body_base64 }`
- `state_query` params `{ id, op, args }` → result `{ id, result }`（op は §6）
- `record` params `{ request_id, record: <SourceRecord> }` または
  `{ request_id, records: [<SourceRecord>, ...] }`（notification, batch ≤100）。
  `record` と `records` の同時指定は malformed。`records` batch は単一 `discover`
  request の spool 上限と byte 上限を共有する。
- `log` params `{ level, message }`（notification, host は stderr へ）

malformed / oversized frame / unknown method は fail-closed（host は plugin を異常終了扱いにし ingest を rollback）。

## 5. fetch_request（host_fetch）

`fetch_request` は discover 中だけ有効。JSON-RPC id は plugin namespace の `p-*`、params 内の
`id` は plugin 側 correlation id。成功時は JSON-RPC response の top-level `id` に `p-*` を返し、
`result.id` に params 内の `id` を返す。

成功 result:

```json
{"id": "fetch-1", "status": 200, "final_url": "https://example.test/page", "body_base64": "..."}
```

HTTP 404/500 などの HTTP status は transport error ではなく通常 result として返す。policy error、
DNS/connect/total timeout、body 上限超過、redirect policy 違反は JSON-RPC error とし、host は
fail-closed して ingest に進まない。

host_fetch の安全方針:

- `url` は absolute `http` / `https` のみ。userinfo は禁止。
- method は `GET` / `HEAD` のみ。
- `manifest.allowed_domains` を強制する。host は exact host またはその subdomain だけを許可する。
- redirect は host が追跡し、各 hop で scheme / allowed_domains / DNS IP を再検査する。
- DNS 解決後の IP 検査で localhost / private / link-local / multicast / unspecified /
  carrier-grade NAT / IPv6 unique-local を拒否する。許可domainでもこの拒否は上書きできない。
- system proxy は明示的に無効化する。
- connect / total timeout を持ち、body read は total timeout に含める。
- raw response body 上限は 5 MiB。base64 expansion と JSON envelope を含めて protocol frame
  上限 8 MiB を超えないための値で、8 MiB raw body ではない。
- plugin からの `Authorization` / `Cookie` / `Proxy-Authorization` / `Set-Cookie` /
  `Host` / `Connection` / `Transfer-Encoding` / `Content-Length` header は拒否する。

## 6. state_query（typed のみ・任意 SQL 不可）

- `known_source_urls` args `{ source_name }` → `["url", ...]`
- `source_post_summary` args `{ source_name, source_url }` → `{ exists, title?, last_seen_at? } | null`
- `last_seen_at` args `{ source_name }` → `"ISO8601 | null"`
- `content_fingerprint` args `{ source_name, source_url }` → `"str | null"`

host が state provider を持たない、または backend error / unknown op / invalid args の場合は
JSON-RPC error とし、空配列や null を「既知 state なし」の代替値として返してはならない。

## 7. SourceRecord（record_schema_version = 1）

```json
{
  "source_name": "str (== manifest.source_name)",
  "source_url": "str (不透明な絶対 URI・idempotency key)",
  "title": "str",
  "brand_raw": "str",
  "performers_raw": ["str", ...],
  "cover_urls": ["str", ...],
  "page_urls": ["str", ...],
  "issue_no": "str | null",
  "external_links": [
    {"url": "str", "provider": "str | null", "label": "str | null",
     "kind": "str | null", "external_id": "str | null", "metadata": {}}
  ],
  "release_date": "YYYY-MM-DD | null",
  "post_date": "YYYY-MM-DD | null",
  "brand_normalized": "str | null",
  "normalizer_id": "str | null",
  "normalizer_version": "str | null",
  "extra": {}
}
```

不変条件:
- `record.source_name == manifest.source_name`。
- idempotency key = `(source_name, source_url)`。重複は upsert（件数が増えない）。
- **`source_url` は不透明な絶対 URI**であり fetch 可能な URL である必要はない。実ソースは http/https の post URL、synthetic/example は独自 scheme（例 `synthetic://`）でもよい。**host は `source_url` を idempotency key として扱い http/https を強制してはならない**（fetch は host_fetch が別途行う）。
- **`external_links` は typed array**（`extra` に入れない）。各要素は `url` 必須、`provider`/`label`/`kind`/`external_id` は null 可、`metadata` は自由 object。retail/外部リンクの core 解釈値を untyped dict に隠さないため。`mh-domain` の `ExternalLink` と shape を一致させる。
- `extra` は**保存のみ**。host は extra の中身で分岐しない。core が解釈する値は typed field に置く。
- upsert merge policy:
  - `last_seen_at` は観測ごとに更新する。
  - `updated_at` は host-computed `content_fingerprint` が変化した場合だけ更新する。
  - `release_date` は新 record が null の場合、既存 DB 値を保持する。新 record が non-null の場合は新値で置換する。
- 1 回の ingest は単一 transaction。失敗時は全体 rollback。adapter は durable DB write を行わない。

### 正規化（ハイブリッド）

- host: Unicode NFKC + dash/whitespace 正規化 + URL/date/基本 identifier の validation のみ。
- plugin/SDK: brand canonicalization / performer normalization 等のドメイン正規化。`brand_normalized` と `normalizer_id`/`normalizer_version` を埋める。
- 非対応 plugin は raw 値だけでも参加可（normalized は null 可）。

## 8. fetch 境界

- `host_fetch`（既定）: host が通信し上記安全方針を強制する。
- `self_fetch`（trusted plugin のみ・既定 off）: plugin 自身が通信する。

## 9. trust model

plugin は trusted な実行コード。subprocess はクラッシュと lifecycle を分離するが authority は分離しない（sandbox ではない）。host は plugin を argv 配列で直接 exec（shell を使わない）、環境変数を allowlist、cancel/timeout で plugin の**子孫プロセス全体**を終了する。

## 10. golden vectors

`crates/mh-protocol/golden/*.hex` に、全 field 値を固定した6メッセージ（`initialize` / `discover` / `record` / `record_batch` / `fetch_request` / `state_query`）の framed バイト列（hex）を pin する。`discover` は optional limits (`max_pages` / `max_records` / `per_page`) を含む。`record` と `record_batch` は non-empty `page_urls` と typed `external_links` を含む。`conformance/golden.py`（独立 oracle）が生成し、Rust 実装の `frame_bytes` が byte 一致することを test と CI で検証する。
