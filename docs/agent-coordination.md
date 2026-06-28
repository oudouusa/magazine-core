# agent coordination — 複数エージェント同時開発のルール

複数の自律エージェント（cold Codex / Claude 等）が同じ repo 群を開発するときの調整方針。
**初見エージェントは AGENTS.md と本ファイルを読んでから着手する。**

## 背景（なぜ要るか）

2026-06-26、`magazine-core` で **PR2 を作業中の cold Codex** と **Claude** が
**同じ working tree** を共有して同時に作業し衝突した。Claude が共有ツリーで
`git reset --hard` / branch 切替 / ファイル編集を行い、Codex の未コミット PR2 作業
（`crates/mh-{domain,db,cli}` + Cargo.toml）を一時的に破棄/誤コミットの危険に晒した。
`cargo test --workspace` が他者の WIP を拾って初めて発覚した。教訓を規則化する。

## 鉄則

1. **1 エージェント = 1 working tree。** 2 つのエージェントが同じ git working directory を
   共有しない。各自 own clone か `git worktree` を使う。
2. 他者が触りうるツリーで次を**実行しない**（他者の未コミット作業を破棄/誤帰属しうる）:
   `git reset --hard`、HEAD を動かす `git switch` / `git checkout <branch>`、`git add -A`、
   `git commit -am`、`git stash`、force push。
3. **タスク所有 = ブランチ所有。** あるブランチ/PR を別エージェントが持っているなら触らない。
   新タスクは必ず**新しいブランチ**で。ブランチの存在を「所有の宣言」とみなす。
4. **core-first。** protocol contract（`crates/mh-protocol` の golden + `docs/protocol-v1.md`）の
   変更は consumer（`mh-domain` 等）より先に land する。consumer は **merged な contract** を参照し、
   contract を勝手に再定義しない。
5. 各自の worktree で **full gate**（`cargo fmt --check` / `clippy -D warnings` /
   `cargo test --workspace` / `conformance/check_golden.sh`）を通してから push。
6. **衝突検知時は STOP。** 身に覚えのない未コミット変更・他者のブランチ・他者の WIP を
   見たら、clobber せず人に報告する。auto-resolve しない。

## working tree 隔離の作法

```bash
# 他エージェントの working tree / HEAD / index に触れずに隔離して作業する
git worktree add -b <branch> /path/outside/repo origin/main
# … 編集 → commit → push → PR …
git worktree remove /path/outside/repo
```

`git worktree add` は新しいディレクトリと専用 index を作るだけで、稼働中の working tree の
HEAD/index には触れない（安全）。逆に、共有ディレクトリ内での `reset`/`switch` は危険。

## 役割分担（historical snapshot・2026-06-26）

以下は incident 当時の所有メモであり、現在の active ownership ではない。将来の conflict 調査で
何が起きたかを追うために残す。

- **PR2（`mh-domain` + `mh-db` + CLI）= 当時の cold Codex が所有**（ブランチ `feat/db-cli`）。
- **protocol contract（`mh-protocol`）の変更** = 別 PR・別 worktree。PR2 とは分ける。
- 1 つの repo の core 実装ラインは、原則 **同時に 1 ドライバ**にする。

## external_links の調整（resolved）

2026-06-26 時点では `mh-domain` と protocol contract の `external_links` shape が
分離していた。現在は protocol / golden / domain / SDK に typed shape が反映済み。

- shape:
  `{ "url": str, "provider": str|null, "label": str|null, "kind": str|null, "external_id": str|null, "metadata": {} }`。
- contract 変更は今後も **core-first**。`docs/protocol-v1.md`、Rust golden、Python oracle、
  domain/SDK tests を同じ PR で更新する。

## エスカレーション

衝突・破壊リスク・所有不明・core-first 違反のいずれかを見たら、STOP して人に報告。
独断で他者の作業を上書き/移動/削除しない。

## 関連

- `AGENTS.md` — コールドスタート入口（本ファイルへ誘導）。
- `docs/adoption-guide.md` — two-repo（core / downstream）モデルと core-first フロー。
- private downstream repo の `AGENTS.md` — downstream 側の multi-agent 安全ルール。
