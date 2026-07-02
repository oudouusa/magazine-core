# plugin host runtime

`mh-host` は protocol v1 plugin を subprocess として起動し、`initialize` → `discover` の
message loop を実行する。site-specific adapter や proxy/cookie/challenge 回避は含めない。

## plugins.d manifest

`plugins.d/*.json` をファイル名順に読む。manifest は host が実行する command line だけを
持つ。plugin 自身の `source_name` は `initialize` result の manifest が正本。

```json
{
  "id": "synthetic",
  "argv": ["/absolute/path/to/plugin", "--flag"],
  "env": {"KEY": "value"},
  "working_dir": "."
}
```

- `argv` は必須で、shell を使わず argv 配列として直接 exec する。
- `id` は CLI 選択用。省略時は `source_name`、それも無ければ JSON ファイル名を使う。
- 同じ `id` が複数の manifest に現れた場合は discovery error にする。CLI は曖昧な plugin 選択をしない。
- `working_dir` が相対 path の場合、manifest の親 directory から解決する。
- child env は allowlist。既定で `PATH` と `LANG` を引き継ぎ、manifest の `env` を追加する。

## runtime

1. host が plugin subprocess を stdin/stdout pipe 付きで起動する。
2. host→plugin `initialize` request を送り、`protocol_version = 1` と
   `record_schema_version = 1` を検証する。
3. host→plugin `discover` request を送り、応答待ち中に plugin→host の message を処理する。
4. `record` notification は `SourceRecord` として検証し、memory spool に追加する。
5. CLI `mh discover <db-path> <plugins-dir> <plugin-id> [--max-pages N] [--max-records N] [--per-page N] [--timeout-seconds N]` は canonical DB を先に
   open/init し、read-only state provider と指定された discover limits を渡して discovery を走らせる。
6. plugin が clean exit した後、spool を単一 DB transaction で ingest する。

`state_query` / `fetch_request` は `discover` 応答待ちの間だけ受け付ける。
`initialize` 中や discover 完了後の plugin→host request は protocol error として扱う。
JSON-RPC id は plugin namespace の `p-*` string を必須にする。

対応済み plugin→host traffic:

- `record`: single `record` または `records` batch（最大100）。host は `discover.limits.max_records`
  も memory spool 側で強制する。`max_pages` / `per_page` は plugin が page-based discovery に使う scope hint。
- `log`: host run result に保持する。
- `state_query`: typed op だけ受け、CLI では canonical DB 由来の read-only state
  provider で応答する。任意 SQL は受け付けない。provider 未注入・backend error・不正 op は
  JSON-RPC error と host failure になり、空配列/null を代替値として返さない。
- `fetch_request`: `manifest.allowed_domains` を使う safe host_fetch broker で応答する。
  http/https、redirect 再検査、DNS 後 IP 検査、timeout、body 上限、header policy を host が強制する。
  policy/network error は JSON-RPC error と host failure になり、DB ingest へ進まない。

`discover` result の `records` は spool 済み record 件数と一致しなければならない。
不一致は plugin bug として fail-closed し、DB ingest へ進まない。

## memory limits

host は plugin が守る `limits.max_records` とは別に、常に次の絶対上限を持つ。

- record 件数: 10,000
- record spool の serialized JSON 合計: 32 MiB
- log spool の UTF-8 byte 合計: 1 MiB
- reader thread → host loop の frame queue: 16 frames

上限超過は fail-closed とし、DB ingest へ進まない。

## lifecycle

stdout frame read は別スレッドに分離し、main loop が deadline を管理する。CLI の
既定 discover deadline は 60 秒で、operator は `--timeout-seconds N` で正の秒数を
明示できる。host は同じ deadline から plugin へ `remaining_ms` を渡す。timeout 時は Unix で
plugin を新 session/process group として起動しているため、process group に `SIGTERM`、
grace 後に `SIGKILL` を送る。これにより plugin の子孫プロセスも残さない。

`discover` response は単独では commit point ではない。host は response 後に plugin が
grace 内で clean exit（exit status 0）することを要求する。non-zero exit や grace 超過での
強制終了は fail-closed とし、spool 済み record も ingest しない。
grace は browser-backed plugin が Playwright/Chromium などの子孫プロセスを閉じる短い猶予を
持てる長さにするが、終了しない plugin は引き続き bounded に fail-closed する。
