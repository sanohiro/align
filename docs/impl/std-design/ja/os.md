# std.os

**実装済みの identity 観測:**
`os.identity() -> Result<os.identity_info, Error>` は getuid/getgid の順で現在の実 UID/GID を観測する。
std.os を必要とする Impure 操作で、メモリ確保も資格情報の変更もない。
修飾名のみの通常の Copy レコードは `real_uid: i64`、`real_gid: i64` の順に2フィールドを持ち、
成功値はどちらも非負。表現不能な変換や非対応環境は Invalid。組は原子的な観測や認証を保証しない。
正確な ABI と検証条件は [Plan 54](../../54-r69-r76-prerequisite-batch-plan.md) が所有する。
以下は実装済みの host 操作である。

`os.host() -> Result<os.host_info, Error>` は `import std.os` が必要な Impure 操作。
引数・既定値はない。修飾名のみの通常の Move レコード `os.host_info` は、宣言順に
`system: string`、`release: string`、`machine: string`、`cpu: Option<string>`、
`logical_cpu_count: Option<i64>` を持つ。通常の構築・部分移動・再帰 Drop を使う。
成功時の文字列は独立した所有値で、arena 内でも領域に依存せず、OS のバッファを保持しない。

対応する 64-bit Linux/macOS では system/release/machine は uname の
sysname/release/machine そのもの。順に空でないこと、ネイティブ配列内の NUL 終端、
厳密な UTF-8 を検証してから任意の CPU 数を問い合わせ、文字列を割り当てる。
終端の欠落・空文字列・不正な UTF-8 は `Error.Invalid`。最初の NUL 後のパディングは無視する。
uname の失敗は既存の errno 対応を使う。`cpu` はこの実装では常に None。
`sysconf(_SC_NPROCESSORS_ONLN)` の正で i64 に収まる値だけを Some にする。
取得不能・非正・表現不能は None で、取得済みの必須フィールドは失わない。
非対応環境では `Error.Invalid`。OOM は既存の即時停止規則を使う。

環境変数・子プロセス・ディストリビューションファイル・CPU 名の推測・キャッシュ・
グローバル状態の変更はない。認証済みの識別子でも原子的なスナップショットでもない。
CPU 数は物理ホスト全体・affinity・quota に関する保証を持たない。
正本と受け入れテストは [host observation](../../42-host-observation-plan.md) を参照。
