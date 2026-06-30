# AGENTS.md — magazine-core

初見のエージェント（Codex / Claude 等）はこのファイルを最初に読む。これは本リポジトリで
開発を進めるためのコールドスタート手順。

## このリポジトリは何か

SQLite ベースの**出版物メタデータ収集・正規化フレームワーク**の**正本**。
Rust host + 言語非依存 stdio plugin protocol + Python SDK。サイト固有の adapter・
回避実装・データは**ここに置かない**（private downstream repo が消費側）。

## まずこの順で読む

0. **`docs/agent-coordination.md` — 複数エージェント同時開発のルール（最初に確認・必須）。**
1. `README.md` — 目的とロードマップ。
2. `docs/protocol-v1.md` — protocol 正本（framing / JSON-RPC / SourceRecord / state machine / limits / golden）。
3. `docs/adoption-guide.md` — two-repo モデルと downstream の消費方法。
4. `docs/migration-checklist.md` — adapter 1 件あたりの汎用チェックリスト。
5. `docs/next-implementation-plan.md` — downstream evidence 後の stabilization plan。

> **着手前**: 別エージェントが作業中のブランチ/未コミット変更があるか確認する。
> 1 エージェント = 1 working tree（own clone か `git worktree`）。共有ツリーで
> `git reset --hard` / branch 切替 / `git add -A` を**しない**。詳細は `docs/agent-coordination.md`。

ADR-0001（Rust host + stdio protocol の意思決定）は private downstream repo にある。
本 repo は protocol v1 実装正本だが、ADR status の正本ではない。ADR status は downstream
側の文書で確認する。

## 外部参照実装（合体させない）

historical vertical-slice prototype artifacts は本 repo の外にある。参照する場合も external
reference として扱い、丸ごとコピーしない。独立実装で得た protocol 精度の知見は、
generic な contract / golden / conformance test としてだけ本 repo に取り込む。

本 repo へは generic な protocol/framing/golden/schema/ingestor/SDK core のみ移植する。

## 改善ループ実装プロトコル

実装は、変更リスクに応じて full loop または lightweight loop で進める。

full loop 必須:

- protocol contract / golden / framing
- DB schema / migration / ingest semantics
- host runtime / subprocess / process-tree / timeout / cancellation
- `state_query` / fetch safety / SSRF・size・timeout 境界
- SDK runtime / stdout guard / public API
- CI、release、互換境界、削除を伴う変更
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

検証証拠なしに成功を主張してはならない。失敗はスコープ拡大の理由ではなく、次ループへの
入力として扱う。

### full loop のサブエージェント役割

full loop では少なくとも観測・検証・レビューのサブエージェントを使う。設計と実装の
サブエージェントは、変更規模と衝突リスクに応じて使う。

- 観測: 現行挙動、関連ファイル、既存テスト、失敗モードを調べる。read-only。
- 設計: 不変条件、最小変更範囲、互換境界、削除する責務重複を定義する。read-only。
- 検証: 先に追加・更新すべき characterization / regression test を決める。read-only。
- 実装: 合意した最小差分だけを実装する。担当ファイル範囲を明示する。
- レビュー: blocking findings、verified commands、residual risks を必ず返す。read-only。

実装エージェントに書かせる場合は、担当ファイル範囲を分け、他エージェントや main agent の
変更を revert しないよう明示する。完了したサブエージェントは close する。
main agent はレビュー後、residual risks を解消するか PR 本文に明記する。

### lightweight loop

サブエージェントは必須ではない。ただし実装前後に次を短く記録する:

- 守る不変条件
- 実行する検証コマンド
- 採用 / revert の判定
- 残ったリスク

### magazine-core の不変条件

- protocol contract 変更は `docs/protocol-v1.md` と golden / tests を同じ PR で更新する。
- core から private downstream への逆依存を作らない。
- サイト固有名、cookie/proxy/challenge 回避、実データを core に置かない。
- canonical DB と legacy DB を混ぜない。
- `state_query` は typed operation のみ。任意 SQL を受け付けない。
- fetch safety は generic safety のみ core に置く。
- plugin stdout は framed protocol 専用とし、通常出力と混ぜない。

## 現状

- current implementation line: protocol、domain / DB / CLI、plugin host runtime、
  typed `external_links`、Python SDK、DB-backed typed state、safe host fetch broker、
  Python SDK `host_fetch` helper、CI 統合、protocol v1 audit、Python SDK public API freeze、
  public docs cleanup、beta-readiness docs alignment まで完了。downstream の host-fetch と multi-stage discovery /
  extension parity evidence 後に、generic な discover limit gap として CLI-driven
  optional `max_pages` / `max_records` / `per_page` を追加済み。conformance fixtures は beta readiness 向けに拡張済み。Python SDK root
  plugin-author API も `0.1.0-beta` 向けに凍結済み。
- conformance fixture inventory は、typed state golden、discover limits golden、non-empty `page_urls` golden を含めて完了。
  release hardening は `scripts/release-hardening.sh` と手動 `Release hardening` workflow で
  再実行可能にする。speculative な core 機能を先に足さない。

## 次のタスク

ロードマップ順に小さな PR で進める。各 PR はブランチを切る（main へ直接 commit しない・infra を除く）。

- downstream adapter 検証で出た generic 契約不足を core-first で修正。
- Python SDK runtime hardening は、実 downstream wrapper で必要になった API のみに絞る。
- typed state / fetch broker の edge case は、実 adapter の parity evidence と一緒に追加する。
- release hardening（clean install / Linux・macOS / SBOM・license・secret scan / docs）は
  `scripts/release-hardening.sh` または手動 workflow を clean main で実行し、artifact checksum
  を downstream lock または release notes に記録してから beta tag/SHA へ進む。

## ビルドと検証（CI と一致・全て green が必須）

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
bash conformance/check_golden.sh    # Python oracle が pinned golden を差分ゼロで再生成
```

toolchain: Rust 1.94+（rustfmt/clippy）、Python 3.x、C コンパイラ（rusqlite bundled）。
本番 DB と別 FS でビルドすること（target は数百 MB）。

## 守るべき境界（hard rules）

- **core-first**: 本 repo が source of truth。private downstream repo は versioned
  artifact として消費する。**core から private への逆依存を作らない。**
- **サイト固有名・回避実装（proxy/cookie/challenge）を本 repo に置かない。** generic safety
  （SSRF/サイズ/timeout/redaction）は core の責務で in-scope。
- **golden vector は pinned**。変更は Rust（`crates/mh-protocol/src/golden.rs`）と Python
  （`conformance/golden.py`）の両方のメッセージ定義を編集し、fixtures を再生成して
  `check_golden.sh` で検証する。**`*.hex` を手編集しない。**
- **contract 変更は同じ PR で `docs/protocol-v1.md` を更新する。**
- **複数エージェント**: 1 エージェント = 1 working tree。他者のブランチ/未コミット作業を
  尊重し、共有ツリーで `reset --hard` / branch 切替 / `git add -A` をしない。タスク所有 =
  ブランチ所有。詳細は `docs/agent-coordination.md`。
- **ADR-0001 status は downstream-owned。** core docs は status を先取りしない。今後の
  protocol contract 変更は新しい evidence と docs/golden/tests の同時更新を必要とする。
- 小さな PR・push 前にテスト green。

## gap 受け入れ基準（downstream からの promotion）

変更の多くは private downstream consumer の実運用で見つかった「汎用不足」として入ってくる。
受け入れるのは **generic で evidence のあるもの**だけ。

- **受け入れる**: protocol v1 の曖昧さ、複数 plugin が要る Python SDK helper、汎用 `host_fetch`
  安全機能、汎用 `state_query` op、conformance fixture、canonical DB・`inspect`・migration の
  汎用改善、**synthetic example で再現できる**不具合。
- **受け入れない（downstream に残す）**: 特定実サイト向け logic、回避実装（proxy/cookie/
  challenge）、実サイト名・実データ・実レスポンス、特定 source 専用の matching/retail/social。
- 入れる前に確認: synthetic fixture で再現できるか? site 固有名・実データを含まないか?
  protocol/SDK 契約変更なら docs/golden/tests を同じ PR で更新したか?
- downstream 由来でも、実サイト事情を core に持ち込まない（上の hard rules を優先）。

## コミット規約

- ブランチ命名: `feat/...` / `docs/...` / `chore/...`。
- 本リポジトリのライセンスは MIT、edition 2021、rust-version 1.94。
