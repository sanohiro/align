このディレクトリには、ロードマップの本文には収まりきらない std モジュールの、Opus が実装できる粒度の
設計仕様を置いている。執筆はメインループ (Fable)。各モジュールを実装するときの source of truth である。

# std.process — implementation design (M11)

> 🌐 [English](../process.md) · **日本語**

## Overview

spawn / exec / exit(draft §18.2)。fork/exec/waitpid と、子プロセスを表す child Move ハンドルで構成する。
**`process.exit` の Drop セマンティクスをめぐる Open question(open-questions)は、このモジュールで確定させる。**

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

`child` は pid を所有する **Move 型**である。Drop の挙動は wait 済みかどうかで分かれる。すでに wait して
いれば何もしない。まだなら、ブロッキングな `waitpid` で **reap し**(終了コードは捨てる)、ゾンビ化を防ぐ。
終了コードが欲しければ明示的に `wait()` を呼ぶのが推奨で、これは終了コードを返す。wait せずに Drop しても
ゾンビにはならず安全だが、終了コードは失われ、子プロセスが終わるまでブロックすることがある。

**`SA_NOCLDWAIT` を使わない理由**(検討のうえ却下した案): init 時に `SIGCHLD` へグローバルに
`SA_NOCLDWAIT` を設定すればゾンビは自動的に reap されるが、POSIX ではその後、特定の子プロセスを
`waitpid` しようとすると `ECHILD` で失敗するようになる。これは `ch.wait() -> Result<i64, Error>` を真正面
から壊す(明示的な wait で終了ステータスを取り出せなくなる)。そこで v1 では `SIGCHLD` のデフォルトの
ディスポジションを保ち、代わりに Drop の中で子プロセスごとに reap する。長寿命の子プロセスをブロックせずに
drop したい場合は、先に `kill()` するとよい(あるいは将来の明示的な `detach()` API を使う — これは記録の
みで v1 には入れない)。

## `process.exit` Drop-semantics decision(ここで SETTLED)

`process.exit(code)` はトップレベルへの通常の return とまったく同じように振る舞う。**保留中の Drop・arena
の終了・バッファ済み writer のフラッシュをすべて unwind して実行し**、そのうえで libc の `exit(code)` を呼ぶ。
これは Nothing-hidden を守る(バッファ済みの出力が黙って失われない — io.md のバッファ済み writer の制限が
警告しているのは、まさにこの危険である)。クリーンアップを一切せず即座に落とすハードエグジットは、これとは
分けて `process.abort()`(→ `_exit`)という別の明示的な API に切り出してある。プログラムを今すぐ終わらせ
なければならないとき用だ。理由: デフォルトは安全な側(クリーンアップが走る)であるべきで、危険な側にこそ
名前を付けるべきである。(open-questions の「process.exit Drop semantics」を解決する — デフォルトは
run-Drops-then-exit、`abort()` が避難ハッチ。)

## Effect classification

すべて impure。

## Error policy

fork/exec/wait の失敗は errno→Error テーブル(M9)に写す。`exec` が戻ってきたということ自体が失敗(errno)
を意味する。`exit`/`abort` は戻らない。

## New machinery required

`child` の Move 型と、fork/execvp/waitpid/kill を包むランタイムラッパー。**child の Drop はブロッキングな
`waitpid` で reap する**(`SA_NOCLDWAIT` は使わない — 明示的な `wait()` を `ECHILD` で壊すため)。そして
**exit がクリーンアップを走らせる経路**。`process.exit` は、トップレベルの return が使うのと同じ
unwind/クリーンアップの発行機構(開いている全 arena に対する emit_exit_cleanup + drop_locals + writer の
フラッシュ)をフックしてから `exit()` を呼ぶ必要がある。ここがこのモジュール唯一の非自明な codegen だ。
exit は単なるランタイム呼び出しではなく、先に関数(理想的にはスタック全体)の保留中クリーンアップを走ら
せてからでなければならない。v1 の現実的なスコープは、現在の関数のクリーンアップ + std ハンドルの atexit
相当のフラッシュ登録を実行してから exit する、というもの。完全なマルチフレームの unwind は理想として文書
化するにとどめ、v1 は現フレーム + グローバルフラッシュまでとする。(このギャップは正直に記録すること。)

## Slice breakdown

1. `process.exit`/`abort` と cleanup-then-exit の経路(確定した意味論)、および std ハンドルのグローバル
   フラッシュ登録。
2. `child` の Move 型 + `spawn` + `wait` + waitpid 経由の Drop-reaps(`SA_NOCLDWAIT` は使わない)。
3. `kill` + `exec`。

## Pitfalls

- **P1 (exit skips cleanup = the hazard)**: このモジュールの眼目は「exit がクリーンアップを走らせる」こと
  そのものである。素朴に `process.exit` = libc の `exit()` としてしまうと、バッファ済み writer の出力が
  黙って捨てられ、arena の解放もスキップされる — 防ぎたいのはまさにこのバグだ。先にクリーンアップを発行
  しなければならない。正しさの観点で最も価値の高いポイント。
- **P2 (zombie children)**: wait せずに Drop してもゾンビを残してはならない — Drop の中でブロッキングな
  `waitpid` を使い、子プロセスごとに reap する。グローバルな `SA_NOCLDWAIT` は使わないこと。自動 reap は
  効くが、明示的な `ch.wait()` が `ECHILD` で失敗するようになり、終了コードの契約が壊れる。トレードオフ
  として、まだ動いている子プロセスを drop すると、それが終わるまでブロックする(これは文書化する。避けたい
  なら先に `kill()`)。テスト: 短命なプロセスを 100 個 spawn し、wait せず全部 drop して、ゾンビが残らない
  こと(ps/proc)と、別の明示的な `wait()` が依然として終了コードを返すことを確認する。
- **P3 (fork+exec fd leak)**: 子プロセスは fd を継承する。Align が所有する fd(reader/writer/socket)には
  CLOEXEC を立て、子プロセスへ漏れないようにする。あるいはこの継承を文書化する。v1 では Align が fd を
  所有する全ハンドルに CLOEXEC を立てる。
- **P4 (child Move sweep + bound-receiver)**: Gate-1 のスイープ。束縛されていない一時値をレシーバにする
  ことは拒否する。
- **P5 (exec argv[0])**: execvp の慣習について — args に argv[0] を含めるのか、それともランタイムが cmd を
  argv[0] として補うのか、どちらか一方に決める(v1: 呼び出し側の args を argv[0] 込みの完全な argv とし、
  cmd はルックアップ用のパスとする)。決めたら文書化する。

## Test checklist

- `true`/`false` を spawn → wait が 0/1 を返す
- wait せずに spawn + drop → ゾンビにならない(P2)
- exec がイメージを置き換える(子プロセスが出力し、親プロセスは exec 成功後に処理を続けない)
- バッファ済み stdout への書き込みの後で `process.exit(3)` → その書き込みがフラッシュ**される**こと
  (P1 — 決定的なテスト)、および終了コード 3
- `process.abort()` → フラッシュせずに終了する
- kill がシグナルを送る
- child を array の要素にすると拒否される
- CLOEXEC が子プロセスへの fd リークを防ぐ(P3)
- import が必須であること
