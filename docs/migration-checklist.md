# migration checklist — adapter 1 件あたり

downstream が旧 adapter を core plugin へ移すときの汎用チェックリスト。site-specific な
進捗は downstream 側の matrix で管理し、本ファイルは形だけを規範化する。

```
[ ] old output captured (canonical JSON, 非決定値は除外)
[ ] protocol wrapper completed (Bridge: stdio で同一 record)
[ ] discovery parity (source_url/title/brand_raw/brand_normalized/performers_raw/covers/pages/issue_no/external_links/dates/count 一致)
[ ] state_query limited to typed operations (任意 SQL なし)
[ ] post-ingest responsibilities separated (matching/suggestion/identity/materialization)
[ ] transaction rollback verified (失敗時に部分 record を残さない)
[ ] timeout leaves no descendants (process-tree 終了・orphan 0)
[ ] shadow runs completed (一定回数 discovery parity ゼロ差分)
[ ] rollback path tested (core→legacy に戻せる)
[ ] legacy implementation removed (cutover 完了後)
```

各項目の定義は `docs/protocol-v1.md` と `docs/adoption-guide.md`（§3 4 段階 / §4 2 層 parity）を参照。

## beta-readiness checklist

`0.1.0-beta` tag または beta SHA を downstream が pin する前に確認する項目。

```
[ ] protocol v1 audit reviewed and no semantic contract/code change required
[ ] Python SDK root plugin-author API documented and tested
[ ] public docs avoid site-specific names, private paths, real responses, cookies, proxy/challenge logic
[ ] examples use synthetic or generic source shapes only
[ ] conformance fixture inventory covers host_fetch, typed state, trusted self_fetch boundary, external links, page URLs, and fail-closed behavior
[ ] clean checkout install/smoke completed for Rust binary and Python SDK
[ ] release artifact checksums recorded in downstream lock
```

Conformance inventory evidence is tracked in `docs/conformance-fixture-inventory-2026-06-27.md`.

チェック項目が未完了の間は、downstream lock は alpha SHA または明示的な beta-candidate SHA に
留め、`0.1.0-beta` 完了とは扱わない。
