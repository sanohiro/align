このディレクトリには、ロードマップの本文を超えた std モジュールの Opus 実装可能な設計仕様が置かれている。
メインループ (Fable) が執筆したもので、各モジュールを実装する際の source of truth である。

# std.process — implementation design (M11)

> 🌐 [English](../process.md) · **日本語**

## Overview

spawn、exec、exit(draft §18.2)。fork/exec/waitpid + 子プロセスを表す Move ハンドル。**このモジュールは
`process.exit` の Drop セマンティクスに関する Open な設問(open-questions)を確定させる。**

## Signatures

```text
ch := process.spawn(cmd: str, args: array<str>) -> Result<child, Error>   // fork+exec, child owns pid
ch.wait() -> Result<i64, Error>       // reap, return exit code (consumes the child's reapable state)
ch.kill(sig: i64) -> Result<(), Error>
process.exec(cmd: str, args: array<str>) -> Result<(), Error>   // replace current image (execvp; returns only on error)
process.exit(code: i64)               // run cleanup, then exit — see below
process.abort()                        // immediate _exit, NO cleanup
```

## Type & ownership classification

`child` は pid を所有する **Move 型** である。Drop は次のとおり: すでに wait 済みなら何もしない。まだ
wait していなければ、ブロッキングな `waitpid` で **reap する**(終了コードは破棄する)ため、ゾンビ化する
ことはない。明示的な `wait()` の呼び出しを推奨するが、これは終了コードを返す。wait しないまま Drop する
のは安全(ゾンビにはならない)だが、終了コードを失い、子プロセスが終了するまでブロックする可能性がある。

**なぜ `SA_NOCLDWAIT` を使わないか**(却下された代替案): `SIGCHLD` に対して init 時にグローバルに
`SA_NOCLDWAIT` を設定するとゾンビは自動的に reap されるが、POSIX の下ではそれ以降の特定の子プロセスに
対する `waitpid` が `ECHILD` で失敗するようになる — これは `ch.wait() -> Result<i64, Error>` を直接壊す
(明示的な wait が終了ステータスをもはや取得できなくなる)。したがって v1 は `SIGCHLD` のデフォルトの
ディスポジションを維持し、代わりに Drop の中で子プロセスごとに reap する。呼び出し側がブロックせずに
長寿命の子プロセスを drop したい場合は、先に `kill()` するべきである(あるいは将来の明示的な `detach()`
API を使う — これは記録のみで v1 には含まない)。

## `process.exit` Drop-semantics decision(ここで SETTLED)

`process.exit(code)` はトップレベルへの通常の return と同じように動作する — **保留中のすべての
Drop・arena の終了・バッファ済み writer のフラッシュを unwind して実行し**、その後 libc の `exit(code)`
を呼び出す。これは Nothing-hidden を尊重する(バッファ済みの出力が黙って失われることはない — まさに
io.md のバッファ済み writer の制限が警告している危険そのものである)。すべてのクリーンアップをスキップ
する即時のハードエグジットは、これとは別の明示的な API、`process.abort()`(→ `_exit`)であり、プログラ
ムが今すぐ死ななければならない場合のためのものである。理由: デフォルトは安全な方(クリーンアップが実行
される)でなければならず、危険な方には名前を付けなければならない。(open-questions の「process.exit
Drop semantics」の項目を解決する — デフォルトは run-Drops-then-exit、`abort()` はその避難ハッチである。)

## Effect classification

すべて impure である。

## Error policy

fork/exec/wait の失敗 → errno→Error テーブル(M9)。`exec` が(何であれ)戻ってきた場合はそれ自体が失敗
(errno)を意味する。`exit`/`abort` は戻らない。

## New machinery required

`child` の Move 型 + ランタイムの fork/execvp/waitpid/kill ラッパー;**child の Drop はブロッキングな
`waitpid` で reap する**(`SA_NOCLDWAIT` は使わない — 明示的な `wait()` を `ECHILD` で壊してしまうため);
**exit-runs-cleanup のパス** — `process.exit` は、トップレベルの return が使うのと同じ unwind/クリーン
アップの発行機構(全ての開いている arena に対する emit_exit_cleanup + drop_locals + writer のフラッシ
ュ)をフックしてから `exit()` を呼ばなければならない。これがこのモジュールで唯一非自明な codegen の要
素である: exit は単なるランタイム呼び出しではなく、先に関数の(理想的にはスタック全体の)保留中のクリー
ンアップを実行しなければならない。v1 の実務的なスコープ: CURRENT な関数のクリーンアップ + std ハンドル
の atexit 相当のフラッシュ登録を実行してから exit する — 完全なマルチフレームの unwind は理想として文
書化するにとどめ、v1 は current-frame + グローバルフラッシュにとどめる。(このギャップは正直に記録す
ること。)

## Slice breakdown

1. `process.exit`/`abort` + cleanup-then-exit のパス(確定した意味論)+ std ハンドルのグローバルフラッ
   シュ登録。
2. `child` の Move 型 + `spawn` + `wait` + waitpid 経由の Drop-reaps(`SA_NOCLDWAIT` は使わない)。
3. `kill` + `exec`。

## Pitfalls

- **P1 (exit skips cleanup = the hazard)**: このモジュール全体の要点は「exit がクリーンアップを実行す
  る」ことである。素朴な `process.exit` = libc の `exit()` では、バッファ済みの writer の出力が黙って
  失われ、arena の解放もスキップされてしまう — まさにそのバグを防ぐ。先にクリーンアップを発行しなけれ
  ばならない。最も価値の高い正しさのポイント。
- **P2 (zombie children)**: wait せずに Drop してもゾンビにしてはならない — Drop の中でブロッキングな
  `waitpid` により子プロセスごとに reap する。グローバルな `SA_NOCLDWAIT` は使わないこと: 自動 reap は
  されるが、明示的な `ch.wait()` が `ECHILD` で失敗するようになり、終了コードの契約が壊れる。トレード
  オフとして、まだ実行中の子プロセスを drop すると、その子プロセスが終了するまでブロックする(これは文
  書化する。避けたい場合は先に `kill()` する)。テスト: 短命なプロセスを 100 個 spawn し、wait せずにす
  べて drop し、ゾンビが残っていないこと(ps/proc)、および別の明示的な `wait()` が依然として終了コードを
  返すことを確認する。
- **P3 (fork+exec fd leak)**: 子プロセスは fd を継承する。Align が所有する fd(reader/writer/socket)には
  CLOEXEC を設定して子プロセスへリークしないようにする。あるいはこの継承を文書化する。v1: すべての
  Align の fd 所有ハンドルに CLOEXEC を設定する。
- **P4 (child Move sweep + bound-receiver)**: Gate-1 のスイープ;束縛されていない一時値をレシーバにする
  ことは拒否する。
- **P5 (exec argv[0])**: execvp の慣習 — args に argv[0] を含めるか、ランタイムが cmd を argv[0] として
  供給するか。どちらか一方を選ぶ(v1: 呼び出し側の args が argv[0] を含む完全な argv であり、cmd はルッ
  クアップ用のパスである)ことにし、それを文書化する。

## Test checklist

- `true`/`false` を spawn する → wait が 0/1 を返す
- wait せずに spawn + drop する → ゾンビにならない(P2)
- exec がイメージを置き換える(子プロセスが出力し、親プロセスは exec 成功後に決して処理を継続しない)
- バッファ済み stdout への書き込みの後の `process.exit(3)` → その書き込みがフラッシュ**される**こと
  (P1 — 決定的なテスト)+ 終了コード 3
- `process.abort()` → フラッシュせずに終了する
- kill がシグナルを送る
- child を array の要素にする → 拒否される
- CLOEXEC が子プロセスへの fd リークを防ぐ(P3)
- import が必須であること
