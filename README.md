# magazine-core

SQLite ベースの出版物メタデータ収集・正規化フレームワーク
(Plugin-based publication metadata ingestion framework on SQLite)。

Rust host + 言語非依存の stdio plugin protocol + Python SDK。スクレイピング対象
サイト固有の adapter・回避実装・データは本リポジトリに含めない（別 private 運用）。

## ステータス

- **protocol v1 foundation 実装済み**。downstream の host-fetch と multi-stage
  discovery / extension parity evidence では、v1 contract / code の追加変更は
  不要と判断済み。conformance fixtures は beta readiness 向けに typed state と
  non-empty `page_urls` の coverage を追加済み。
- ADR-0001（Rust host + stdio plugin protocol）の status は downstream 側が所有する。
  本 repo は v1 実装正本として、今後の変更を evidence-driven に扱う。
- 外部公開（visibility 切替）は release hardening + 実 adapter 検証後に行う。
- 由来: ADR-0001 の external vertical-slice prototype（2つの独立実装が双方向で
  protocol 相互運用）。
  仕様は `docs/` に置く pinned CONTRACT に従う。

## ロードマップ

| PR | 内容 |
|---|---|
| PR1 | protocol foundation — framing codec / JSON-RPC 型 / pinned golden / conformance |
| PR2 | DB + CLI — SourceRecord / 0001_initial / init-db / inspect / transaction・rollback |
| PR3 | plugin host — plugins.d discovery / subprocess supervision / discover / spool / cancel・process-tree |
| PR #6 | Python SDK foundation — framing / models / runtime / stdout guard / synthetic E2E |
| PR #7 | DB-backed typed state — `known_source_urls` / summary / timestamps / fingerprint |
| PR #8 | safe host fetch — allowed domains / redirect validation / DNS IP checks / timeouts |
| PR #14 | protocol v1 audit — downstream evidence review; v1 unchanged |
| PR #15 | Python SDK API freeze — stable root plugin-author API |
| PR #16 | public docs cleanup — generic public docs, no protocol semantic change |
| PR #17 | beta-readiness docs alignment |
| current | conformance fixture inventory complete; release hardening prep |
| current | release hardening script/workflow — binary/wheel checksums, wheel smoke, SBOM, license inventory, secret scan |
| later | beta artifact — run hardening on clean main, then tag or record beta-candidate SHA |

## 開発

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python -m venv .venv
.venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh             # 独立 oracle と pinned golden の照合
bash scripts/release-hardening.sh             # beta tag/SHA 前の artifact/checksum hardening
cargo run -p mh-cli -- init-db ./scratch.db
cargo run -p mh-cli -- inspect ./scratch.db
cargo run -p mh-cli -- discover ./scratch.db ./plugins.d example
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
- `docs/plugin-host.md` — PR3 plugin discovery and host runtime
- `docs/next-implementation-plan.md` — downstream evidence 後の stabilization plan
- `docs/python-sdk.md` — Python SDK stable root API and advanced API tiers
- `docs/migration-checklist.md` — downstream adapter migration and beta-readiness checklist
