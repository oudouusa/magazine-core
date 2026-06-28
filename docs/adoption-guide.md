# adoption guide — downstream が core を消費する

`magazine-core` は **公開契約と実装成果物の正本**。downstream（private）は core を
**versioned binary / Python SDK / protocol** として消費し、source 単位で旧経路を置換する。

```
magazine-core       (正本: contract + binary + SDK)
        ↓ versioned artifact / protocol（一方向）
downstream (private) (plugins / extensions / deployment / dashboard)
```

**core から private への逆依存を作らない。** core は private のサイト名・回避実装・データを
一切知らない。private 側の site-specific 進捗（adapter matrix 等）は core に置かない。

## 1. core を pin する（submodule/subtree でなく artifact 固定）

downstream は core ソースを複製せず、**binary / wheel / commit SHA または tag を固定**する。
正本が二つになるのを防ぐ。downstream に置く lock の一般形:

```toml
core_version = "0.1.0-alpha.N | 0.1.0-beta.N"
core_commit = "<full commit sha>"
protocol_version = 1
record_schema_version = 1

[artifacts]
linux_x86_64_sha256 = "..."
python_sdk_sha256 = "..."
```

`0.1.0-beta` へ進める条件は、downstream の shadow/parity evidence が対応する commit で
green であること、`docs/migration-checklist.md` の beta-readiness section が満たされること、
binary / Python SDK artifact の checksum を lock へ記録できることである。

## 2. DB 境界（core canonical DB と legacy DB は分離）

migration 中は **core canonical DB と既存 downstream/legacy DB を分ける**。
core migrations を既存 legacy DB へ直接適用する運用は unsupported。

```
plugin
  ↓
magazine-core canonical DB
  ↓ private extension / promotion adapter
existing downstream legacy DB
```

- `init-db` は新規ファイル、空の未初期化 SQLite DB、または既存 core DB だけを対象にする。
- 未知の既存 DB（legacy schema を含む）、壊れた core DB、未対応の新しい schema version は fail-closed。
- `inspect` は初期化済み core DB だけを read-only open し、存在しない path や空 DB に空 schema を作らない。
- legacy DB への promotion/import は private downstream の責務。core は legacy column 名や
  materialized schema を知らない。

## 3. 1 アダプタの移植 4 段階

| 段階 | 内容 |
|---|---|
| **Capture** | 旧 adapter 出力を canonical JSON で保存。DB ID・取得時刻・実行順など非決定値は比較から除外 |
| **Bridge** | 旧 adapter 本体は移動せず、private SDK wrapper から呼び、stdio protocol で同じ record を送らせる。スクレイピングロジックは書き直さず、protocol/host/ingestor 接続だけ検証 |
| **Separate** | adapter 内の責務を分離。plugin=discovery only / private extension=matching・suggestion・identity・materialization |
| **Cutover** | source 単位の engine モード `legacy`（旧のみ）/ `shadow`（旧が正本・core canonical DB の結果を比較）/ `core`（core が正本・旧は rollback 用）。shadow で差分ゼロを一定回数確認後、1 source ずつ core へ |

## 4. 比較は 2 層（責務分離を「不一致」と誤判定しない）

責務を post-ingest に分離するため、旧 `SourceRecord` 全体の完全一致を要求しない。

- **Discovery parity（必須一致）**: `source_url` / `title` / `brand_raw` / `brand_normalized` / `performers_raw` / `cover_urls` / `page_urls` / `issue_no` / `external_links` / `release_date` / `post_date` / record count。
- **Extension parity（別比較）**: `matched_source_post_id` / `score` / `suggestions` / identity decisions / materialized rows。

## 5. cross-repo の変更は core-first

両 repo に波及する変更は必ず core 先行:

```
1. core で契約・実装を変更
2. alpha/beta tag または commit SHA を作る
3. downstream の lock を更新
4. downstream で integration / parity 実行
5. 問題があれば core へ修正 PR
6. 新 alpha/beta artifact を downstream で再検証
7. 安定後に旧 downstream 実装を削除
```

**禁止**: downstream 側だけで core の複製コードを hotfix すること。
`core change = public-first` / `downstream change = plugin/extension/deployment のみ`。
release hardening や artifact checksum の問題も core 側で直し、downstream は修正版の
versioned artifact を再 pin する。

## 6. adapter は capability 単位でまとめて移す

11 件一括でなく、次の単位で段階移行する:

```
host-fetch + typed skip + page metadata /
trusted self-fetch + post-ingest matching /
multi-stage discovery + retail/cross-source extensions
```

最初は host-fetch で安全境界を core に寄せ、typed `state_query` による skip と
`page_urls`/`external_links` の discovery parity を固める。trusted self-fetch や
post-ingest matching は、discovery-only の record が安定してから段階的に移す。
retail/cross-source extension や multi-stage discovery は、core DB から private extension へ
promotion/import する境界を固定してから扱う。

## 7. 関連（core 側の generic 成果物）

- `docs/protocol-v1.md` — protocol 正本
- `docs/migration-checklist.md` — adapter 1 件ごとの汎用チェックリスト
- `docs/next-implementation-plan.md` — downstream evidence 後の stabilization plan
- `examples/python-synthetic-plugin/` — synthetic Python plugin
- `conformance/` — golden oracle and `check_golden.sh`
- `sdk/python/` — Python SDK runtime / models / stdout guard

site-specific な移行計画・adapter matrix・cutover runbook は **downstream（private）** に置く。
