# magazine-core

SQLite ベースの出版物メタデータ収集・正規化フレームワーク
(Plugin-based publication metadata ingestion framework on SQLite)。

Rust host + 言語非依存の stdio plugin protocol + Python SDK。スクレイピング対象
サイト固有の adapter・回避実装・データは本リポジトリに含めない（別 private 運用）。

## ステータス

- **`1.1.0` release prep**。tag / GitHub Release / push は作成せず、`1.0.0`
  からの additive minor release commit を準備する。`protocol_version = 1` /
  `record_schema_version = 1` は stable `1.x` contract のまま不変。
- protocol v1 foundation 実装済み。host-fetch・multi-stage discovery・extension
  parity の検証後、generic な discover limit gap として CLI-driven
  `max_pages` / `max_records` / `per_page` を protocol v1 の optional limits に追加済み。
  conformance fixtures は typed state、discover limits、non-empty `page_urls` の coverage を含む。
- `1.0.0` 以降の additive 変更として、release exact-SHA preflight script、
  repo-only WordPress REST plugin template、local synthetic smoke 用の
  dev-only loopback `host_fetch` opt-in を追加済み。contract 4 面は不変。
- 由来: ADR-0001（Rust host + stdio plugin protocol）の vertical-slice prototype
  （2つの独立実装が双方向で protocol 相互運用）。仕様は `docs/` の pinned
  CONTRACT に従う。

## 実装済み

- protocol foundation — framing codec / JSON-RPC 型 / pinned golden / conformance
- DB + CLI — SourceRecord / `0001_initial` / init-db / inspect / transaction・rollback
- plugin host — `plugins.d` discovery / subprocess supervision / discover / spool / cancel・process-tree
- Python SDK — framing / models / runtime / stdout guard / synthetic E2E、stable root plugin-author API（凍結）
- DB-backed typed state — `known_source_urls` / summary / timestamps / fingerprint
- safe host fetch — allowed domains / redirect validation / DNS IP checks / timeouts / dev-only loopback smoke opt-in
- read commands — `mh view sources` / `posts`（keyset ページング・URL 非出力）/ `assets`（明示 id のみ）
- release hardening — binary/wheel checksums / wheel smoke / SBOM / license inventory / secret scan / exact-SHA cut-release preflight

## 今後

- contract 変更は実 plugin が generic な不足を示したときのみ、evidence-driven に行う。
- stable tag / Release の作成は `docs/release/1.1.0.md` と
  `docs/release/public-visibility-checklist.md` の evidence gate に従う。

## Standalone quickstart

The currently published linux-x86_64 stable Release artifacts can be verified
without using repo paths. After `1.1.0` is published, use `RELEASE_TAG=1.1.0`.

```bash
RELEASE_TAG=1.0.0 VERIFY_UI=1 bash scripts/verify-standalone-quickstart.sh
```

See `docs/standalone-quickstart.md` for the manual artifact-only flow. Release
binaries that include `mh ui` are also checked for local read-only browsing and
guarded `--manage` discover in the same quickstart verifier.

## 開発

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python -m venv .venv
.venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest --rootdir=. sdk/python/tests examples/wordpress-rest-plugin/tests
bash conformance/check_golden.sh             # 独立 oracle と pinned golden の照合
bash scripts/release-hardening.sh             # release tag/SHA 前の artifact/checksum hardening
cargo run -p mh-cli -- init-db ./scratch.db
cargo run -p mh-cli -- inspect ./scratch.db
cargo run -p mh-cli -- discover ./scratch.db ./plugins.d example --max-pages 1 --per-page 30 --max-records 30 --timeout-seconds 60
```

`init-db` は新規 core DB、空の未初期化 DB、または既存 core DB だけを対象にする。
既存 downstream/legacy DB へ core migration を直接適用する運用は unsupported。

## 設計原則

- plugin 契約は言語非依存（entry-point でなく version 付き stdio protocol）。
- generic safety（SSRF/サイズ/timeout/redaction）は core。回避（proxy/cookie/challenge）は含めない。
- `source_url` は不透明な絶対 URI（fetch 可能 URL である必要はない）。

## サポート範囲とコントリビューション

- 受け付ける: generic core（protocol v1 / Rust host / canonical SQLite schema /
  minimal ingestor / safe `host_fetch` broker / Python SDK / synthetic example /
  conformance / docs）に関する issue・PR・security report。
- **受け付けない**: 特定実サイト向けの adapter 追加依頼、回避実装
  （proxy/cookie/challenge）、実データ。サイト固有の取り込みは利用者自身の
  別 plugin package として実装する（plugin 契約はそのために存在する）。
- plugin は trusted executable code であり sandbox は提供しない。詳細は `SECURITY.md`。
- bug / feature は issue template、変更は PR template に従う。security 報告は
  公開 issue ではなく private security report（`SECURITY.md`）へ。
- 行動規範は `CODE_OF_CONDUCT.md`、貢献手順は `CONTRIBUTING.md`。本リポジトリの
  ライセンスは MIT（`LICENSE`）。

## 関連ドキュメント

- `CONTRIBUTING.md` — scope, dev commands, contract-change rules
- `docs/protocol-v1.md` — stdio protocol / SourceRecord contract
- `docs/read-commands.md` — `mh view` の read サブコマンド（sources / posts / assets）
- `docs/plugin-host.md` — PR3 plugin discovery and host runtime
- `docs/next-implementation-plan.md` — downstream evidence 後の stabilization plan
- `docs/python-sdk.md` — Python SDK stable root API and advanced API tiers
- `docs/migration-checklist.md` — downstream adapter migration and beta-readiness checklist
- `docs/release/public-visibility-checklist.md` — public release and `1.0.0` eligibility gate
- `docs/compatibility-policy.md` — post-`1.0.0` compatibility policy
