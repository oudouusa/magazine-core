# AGENTS.md — magazine-core

初見のエージェント（Codex / Claude 等）はこのファイルを最初に読む。本リポジトリで
開発を進めるためのコールドスタート手順と行動規範。

## 0. このリポジトリは何か

SQLite ベースの**出版物メタデータ収集・正規化フレームワーク**。単独で配布・
利用できる**公開プロダクト**であり、downstream consumer（private / third-party）
が plugin を載せて使う **upstream 正本**でもある。

- Rust host + 言語非依存 stdio plugin protocol + Python plugin SDK +
  canonical SQLite schema + conformance fixtures。
- `0.1.0-beta.1` として公開済み（GitHub prerelease）。`protocol_version = 1` /
  `record_schema_version = 1` は beta として凍結。
- private downstream consumer は本 repo を versioned artifact（tag / commit SHA /
  binary / Python SDK）として消費する。artifact / contract の流れは
  core -> downstream。**コード依存は downstream -> core のみ**で、core は
  downstream を import / 参照 / 前提にしない。
- **ここに置かないもの**: 実サイト adapter、回避実装（proxy / cookie / challenge）、
  実データ・実レスポンス、credential、private 運用設定。外部貢献の scope は
  `CONTRIBUTING.md`、trust model は `SECURITY.md`。

## 1. まずこの順で読む

0. **`docs/agent-coordination.md` — 複数エージェント同時開発のルール（必須）。**
1. `README.md` — 目的・ステータス・実装済み範囲。
2. `docs/development/two-repo-development-contract.md` — 公開 core / private
   downstream の契約と変更ルーティング。
3. `docs/protocol-v1.md` — protocol 正本（framing / JSON-RPC / SourceRecord /
   state machine / limits / golden）。
4. `docs/python-sdk.md` — SDK stable root API と advanced API tiers。
5. `docs/next-implementation-plan.md` — 現在の開発 front（downstream evidence 後の
   stabilization plan）。本ファイルや two-repo development contract と食い違う
   場合は AGENTS.md と contract を優先し、次の docs PR で同期する。
6. 必要に応じて `docs/adoption-guide.md` / `docs/migration-checklist.md` —
   downstream 消費側の視点。

> **着手前**: 別エージェントが作業中のブランチ / 未コミット変更があるか確認する。
> 1 エージェント = 1 working tree（own clone か `git worktree`）。共有ツリーで
> `git reset --hard` / branch 切替 / `git add -A` を**しない**。身に覚えのない
> 未コミット変更を見たら STOP。詳細は `docs/agent-coordination.md`。

## 2. 開発姿勢 — evidence-driven のみ

beta 公開後の本 repo は「機能を足す」repo ではなく、「downstream evidence が示した
generic gap を最小の契約変更で吸収する」repo である。

- **speculative な機能追加をしない。** 実 plugin / downstream 運用が generic な
  不足を示すまで、protocol / SDK / schema を拡張しない。
- maintainer が明示した product scope（単独配布性、管理閲覧 UI 等）はこの原則の
  reject 対象ではない。ただし protocol / SDK root API / canonical schema の
  contract には触れない前提で進め、contract が必要になったら evidence-driven
  ルートに戻す。
- 変更の入口は **generic gap note**（two-repo contract 参照）。synthetic 再現計画を
  書けない gap は core に入れない。
- protocol v1 内の **additive optional capability** を優先する。version bump を
  要する変更は、synthetic evidence と docs / golden / tests の同時更新を要する
  例外として扱う。
- 1.0 安定化は beta 運用実績（契約変更ゼロの期間、release artifact 消費実績、
  conformance の完全性）を根拠に判断する。先取りしない。

### gap 受け入れ基準（downstream からの promotion）

変更の多くは private downstream consumer の実運用で見つかった「汎用不足」として
入ってくる。受け入れるのは **generic で evidence のあるもの**だけ。

- **受け入れる**: protocol v1 の曖昧さ / 複数 plugin が要る Python SDK helper /
  汎用 `host_fetch` 安全機能 / 汎用 `state_query` op / conformance fixture /
  canonical DB・`inspect`・migration の汎用改善 / **synthetic example で再現できる**
  不具合。
- **受け入れない（downstream に残す）**: 特定実サイト向け logic、回避実装、
  実サイト名・実データ・実レスポンス、特定 source 専用の matching / retail /
  social、private 運用詳細。
- 入れる前に確認: synthetic fixture で再現できるか。site 固有名・実データを
  含まないか。外部 plugin author に有用か。public CI でテストできるか。
  protocol / SDK 契約変更なら docs / golden / tests を同じ PR で更新したか。

## 3. 守るべき境界（hard rules）

- **一方向依存**: core は private downstream を import / 参照 / 前提にしない。
- **public-safe**: サイト固有名・回避実装・実データ・private path / hostname を
  置かない。generic safety（SSRF / サイズ / timeout / redaction）は core の責務で
  in-scope。
- **golden vector は pinned**: 変更は Rust（`crates/mh-protocol/src/golden.rs`）と
  Python（`conformance/golden.py`）の両方のメッセージ定義を編集し、fixtures を
  再生成して `conformance/check_golden.sh` で検証する。**`*.hex` を手編集しない。**
- **contract 変更は同じ PR で** `docs/protocol-v1.md` + golden + tests を更新する。
- **Python SDK root API は凍結面**: root import surface の変更は契約変更として扱う。
  low-level framing / runtime helper は advanced submodule API（安定性保証なし）。
- `state_query` は typed operation のみ。任意 SQL を受け付けない。
- plugin stdout は framed protocol 専用とし、通常出力と混ぜない。
- canonical DB と legacy DB を混ぜない。`init-db` は新規 core DB、空の未初期化 DB、
  既存 core DB だけを対象にする。downstream/legacy DB への直接適用は unsupported。
- **ADR-0001 の status は downstream-owned**。core docs は acceptance status を
  先取りしない。
- plugin は trusted executable code であり sandbox は提供しない（`SECURITY.md`）。
- 歴史的な vertical-slice prototype artifacts は本 repo の外にある。参照しても
  丸ごとコピーしない。独立実装で得た知見は generic な contract / golden /
  conformance test としてだけ取り込む。

## 4. 改善ループ実装プロトコル

実装は、変更リスクに応じて full loop または lightweight loop で進める。

full loop 必須:

- protocol contract / golden / framing
- DB schema / migration / ingest semantics
- host runtime / subprocess / process-tree / timeout / cancellation
- `state_query` / fetch safety / SSRF・サイズ・timeout 境界
- SDK runtime / stdout guard / public API
- CI、release、packaging、checksum、互換境界、削除を伴う変更
- security boundary または fail-closed 挙動に触れる変更

lightweight loop 可:

- typo、純 docs、format のみ
- 生成物の再生成だけで、生成元と検証が明確な変更
- 明白な test expectation 修正で production code を触らない変更

判断に迷う場合は full loop を選ぶ。

サブエージェントを使えない環境では、full loop 対象の production code 変更に入らず、
必要な役割・依頼文・未検証リスク・推奨検証コマンドを返して停止する。

### ループ

Observe → Characterize → Change → Verify → Decide → Learn

full loop では、実装前に必ず明示する:

- 守る不変条件
- 再現または防止したい failure mode
- 削除する重複または曖昧さ
- 実行する検証コマンド
- 採用 / revert の判定基準

full loop では、実装後に必ず明示する:

- 採用 / revert
- 検証結果
- 残ったリスク
- 削除できた重複または曖昧さ
- 次ループで扱うこと

検証証拠なしに成功を主張してはならない。失敗はスコープ拡大の理由ではなく、
次ループへの入力として扱う。

### full loop のサブエージェント役割

full loop では少なくとも観測・検証・レビューのサブエージェントを使う。設計と実装の
サブエージェントは、変更規模と衝突リスクに応じて使う。

- 観測: 現行挙動、関連ファイル、既存テスト、失敗モードを調べる。read-only。
- 設計: 不変条件、最小変更範囲、互換境界、削除する責務重複を定義する。read-only。
- 検証: 先に追加・更新すべき characterization / regression test を決める。read-only。
- 実装: 合意した最小差分だけを実装する。担当ファイル範囲を明示する。
- レビュー: blocking findings、verified commands、residual risks を必ず返す。read-only。

実装エージェントに書かせる場合は、担当ファイル範囲を分け、他エージェントや
main agent の変更を revert しないよう明示する。完了したサブエージェントは close する。
main agent はレビュー後、residual risks を解消するか PR 本文に明記する。

### lightweight loop

サブエージェントは必須ではない。ただし実装前後に次を短く記録する:

- 守る不変条件
- 実行する検証コマンド
- 採用 / revert の判定
- 残ったリスク

## 5. ビルドと検証（CI と一致・全て green が必須）

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python -m venv .venv && .venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh    # Python oracle が pinned golden を差分ゼロで再生成
```

CLI smoke:

```bash
cargo run -p mh-cli -- init-db ./scratch.db
cargo run -p mh-cli -- inspect ./scratch.db
cargo run -p mh-cli -- discover ./scratch.db ./plugins.d example --max-pages 1 --per-page 30 --max-records 30
```

toolchain: Rust 1.94+（rustfmt / clippy）、Python 3.x、C コンパイラ（rusqlite bundled）。
本番 DB と別 FS でビルドすること（target は数百 MB）。

## 6. リリース手順

release / packaging / checksum に触れる変更は full loop 対象。

1. clean main で `bash scripts/release-hardening.sh` を実行する
   （または手動 `Release hardening` workflow。`docs/release-hardening.md` 参照）。
2. binary / wheel / SBOM の checksum を記録する。
3. tag を切る（prerelease は `X.Y.Z-beta.N`）。GitHub Release を作成し、
   artifact / checksum を添付する。
4. downstream は自身の lock file で新 tag / SHA を pin して再検証する。
   core 側から downstream を操作しない（一方向依存）。

## 7. 現状（2026-07 時点）

- `0.1.0-beta.1` public prerelease 済み。protocol foundation / domain / DB / CLI /
  plugin host runtime / typed `external_links` / Python SDK（root plugin-author API
  凍結）/ DB-backed typed state / safe host fetch broker / conformance fixture
  inventory / release hardening まで実装済み。
- downstream evidence（host-fetch、multi-stage discovery、extension parity）に
  よる protocol v1 audit 済み。契約の semantic 変更は不要と判断。generic gap と
  して CLI-driven optional discover limits（`max_pages` / `max_records` /
  `per_page`）を追加済み。conformance fixtures は typed state、discover limits、
  non-empty `page_urls` を cover する。
- plugin の速やかな shutdown を host が許容する fix 済み（browser-backed plugin
  が子孫プロセスを閉じる短い grace。終了しない plugin は引き続き bounded に
  fail-closed）。

## 8. 次のタスク（evidence 順）

ロードマップ順に小さな PR で進める。各 PR はブランチを切る（main へ直接 commit
しない・infra を除く）。

1. **batched record emission** — downstream evidence 済みの最初の generic gap 候補。
   one-frame-per-record emission が host queue / runtime limit に当たる圧力への
   対応。protocol v1 内の additive optional capability を優先し、SDK-level batching
   か host queue 挙動かは synthetic 再現で決める。
2. **bounded plugin runtime limits** — synthetic plugin で generic な runtime limit
   gap を再現できた場合のみ実装する。再現できなければ downstream 責務のまま。
3. **release artifact 自動化** — release-hardening 出力（binary / wheel /
   checksums / SBOM）を GitHub Release に自動添付し、downstream が checksum 検証で
   消費できるようにする。
4. downstream で実証された edge case の conformance fixture 追加（実データ持ち込み
   禁止・synthetic 化必須）。
5. SDK ergonomics は実 wrapper が必要とした API のみ追加する。
6. **単独配布可能性**（maintainer decision 2026-07）— public release artifact
   のみで install -> `init-db` -> synthetic example `discover` -> `inspect` が
   完結する self-contained 配布。quickstart docs とクリーン環境での cold-start
   検証を含む。
7. **管理閲覧 UI**（maintainer decision 2026-07）— 同梱の generic local web UI。
   `127.0.0.1` bind 既定、read-only 既定（mutating op は明示 opt-in flag）、
   runtime Node 依存なし、protocol contract 非侵食。ADR で形態と管理範囲を
   決めてから実装する。
8. **1.0 基準の明文化**と、充足時の `1.0.0` 判断（単独配布と UI の完了を含む）。

## 9. コミット / PR 規約

- ブランチ命名: `feat/...` / `fix/...` / `docs/...` / `chore/...`。
- 小さな PR・1 commit 1 論理変更・push 前に検証 green。
- PR template の contract-impact section を埋める。
- examples / fixtures は synthetic のみ。実サイト名・実レスポンス・cookie・
  proxy / challenge logic・private path / hostname を追加しない。
- ライセンス MIT、edition 2021、rust-version 1.94。
