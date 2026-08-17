このディレクトリには、ロードマップの本文ではカバーしきれない `std` モジュールについて、Opus がそのまま実装に着手できる粒度の
設計仕様を収めている。執筆はメインループ（Fable）が担当しており、各モジュールの実装においてこれが信頼できる情報源（source of truth）となる。

# std.process — implementation design (M11)

> 🌐 [English](../process.md) · **日本語**

> **ステータス:** M11 のコアは完了済みです（exit/abort、spawn/wait/kill、exec は実装済み）。**拡張は
> 2026-07-24 に設計済み**（align-llm Request 1）です。`process.command` ビルダ + `run_output` ハンドルに
> よる、出力キャプチャ + cwd / env / timeout — 本ファイル末尾の「Extension」を参照してください。**スライス
> 4〜6 は実装済み**（`process.command`/`c.cwd`/`c.run` のキャプチャ、`c.timeout_ns` とコアの `Error.Timeout`
> バリアント、`c.env`/`c.env_clear`）。align-llm Request 11 の有界 text/bytes 拡張も**実装済み**です。

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
process.cpu_count() -> i64            // parallelism available to THIS process (>= 1); see below
```

## `process.cpu_count()` — 出荷済み 2026-07-21

**このプロセスに**利用可能な並列度である。すなわち CPU affinity と cgroup クォータで制限された後の
コア数(`std::thread::available_parallelism`)であって、マシンの生のコア数では決してない。常に
`>= 1` — OS が答えられなければ `1` にフォールバックするので、エラー経路も `Option` も無い。
**Impure**(マシンの状態を観測する)。

`std.env` ではなくここに置くのは、これが環境ブロックの性質ではなく実行中プロセスの性質だからである。
存在理由は、**`task_group` の worker 数を照らし合わせるべき数がこれである**という点にある: ランタイムの
タスクプールはまさにこの値をもとにサイズが決まり、しかもグループのタスクをそのプール **+ 呼び出しスレッド**
の上で走らせるので、「決して戻らないタスク」の集合が `cpu_count() + 1` を超えると、あふれた分のタスクは
起動しないまま残る。`pkg.web.serve` の `workers` パラメータはその境界を超えると abort し、推奨サイジング
(`workers = process.cpu_count()`)はこれが存在して初めて書けるようになった。

**デプロイ上の注意。** この値がクォータを反映することこそが要点なのだが、それゆえここから導いた上限は
マシン依存になる: 固定の worker 数を書いたソース行は、16 コアのマシンでは動き、4 CPU のコンテナでは
即座に abort する。`cpu_count()` から導くのが移植性のある書き方である。

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

## Slice 1 — 実装済み (2026-07-06, ブランチ `feat/m11-process-slice1-exit`)

`process.exit(code)` / `process.abort()` はエンドツーエンドで実装済み(sema → HIR
`ProcessExit`/`ProcessAbort` → MIR → ランタイム `align_rt_process_exit`/`align_rt_process_abort`):

- **exit = クリーンアップしてから終了。** MIR ロワリングは、ランタイム呼び出しの *前に* 現在の関数の
  `emit_exit_cleanup`(`return` が使うのと同じヘルパ — 生存する所有ローカルの Drop、`task_group`/arena の
  終了)を実行し、その後ブロックを `Unreachable` で終端する。よってバッファ付き writer は `Drop` で
  flush + close され、arena はプロセス終了前に解放される。ランタイム側は `std::process::exit(code)` だけ。
- **abort = 名前付きの危険な脱出口。** クリーンアップを一切先行させない裸の `align_rt_process_abort()`、
  すなわち libc `_exit(1)` — Drop なし、flush なし、`atexit` なし。コンパイラの `panic_abort`
  (`SIGABRT`、算術トラップ / 不変条件違反用)とは別物。`abort()` は仕様どおりユーザ要求によるシグナル無しの
  即時終了(`abort` ではなく `_exit`)。終了ステータスは `1`(abort は code を取らない。意図的な異常終了は
  失敗)。
- **「グローバル flush」は結局何も要らなかった。** ランタイムはプロセス全体の出力バッファを持たない:
  `print` は毎回 `stdout` を flush し(生成された `main` は crt0 に直接戻るため `atexit` フックに頼れない)、
  すべての `writer` / バッファ付きシンクは Align の **Move** 値で、呼び出し側のクリーンアップ内の `Drop` で
  flush される。よって atexit 相当の登録機構を作る必要はない — 現時点では不要と記録する。将来ランタイム所有の
  グローバルバッファを導入するなら、その flush は `align_rt_process_exit` にフックする。
- **終了コードの切り詰め。** `i64 -> i32`、Unix の `wait` では下位 8 ビットのみが観測される
  (`WEXITSTATUS`):`exit(256)` → `0`、`exit(-1)` → `255`。`exit(3)` に一致、ドキュメント化済み。
- **divergence の型付け(v1 の制限)。** `Never` 型がまだ無いため `exit`/`abort` の公開結果型は `()` のまま。
  checked control flow と MIR では発散し(クリーンアップ + 呼び出し + `Unreachable`)、後続コードは死んでいて
  出力されない(`lower_block` が `is_terminated` で停止 — `return` 後のコードと同等、ICE なし)。am-f の
  return-completeness 検査はこの制御効果を使い、直接の completion 式または非 fallthrough 文経路として
  両操作を受理する。末尾値を捏造せず、eager parent を通した一般的な `Never` coercion でもない。
  first-class の発散/`Never` 型は引き続き deferred。
- **v1 のマルチフレーム gap(正確に記録)。** 現在の関数のクリーンアップのみが走る。スタックを遡って
  *すべての* 呼び出し側の Drop を実行する完全なマルチフレーム巻き戻しは理想形で、deferred。所有リソースが
  すべて `exit` を呼ぶフレーム内(あるいはそこに束縛された arena / バッファ付き writer)に存在するプログラムでは、
  現在フレームのクリーンアップで表現可能なものは全てカバーされる。gap が問題になるのは、スタック上位の呼び出し側が
  観測可能な `Drop` 効果を持つリソースを所有する場合のみ。

Slice 2(`child` / `spawn` / `wait`)は実装済み(2026-07-06, `feat/m11-process-slice2-*`, PR #377)。

## Slice 3 — 実装済み (2026-07-06, ブランチ `feat/m11-process-slice3-kill-exec`)

`ch.kill(sig)` / `process.exec(cmd, args)` はエンドツーエンドで実装済み(sema → HIR
`ChildKill`/`ProcessExec` → MIR → ランタイム `align_rt_child_kill`/`align_rt_process_exec`):

- **`ch.kill(sig: i64) -> Result<(), Error>`** — libc `kill(pid, sig)`。子プロセスを借用し(`wait` と
  同じく非消費。bound-receiver でゲートする)、シグナルを送る *前に* `reaped` フラグをガードする。すでに
  reap 済みの子を kill しようとすると、きれいに `Err`(`AL_INVALID`)を返し、リサイクルされ得る pid へ迷い
  シグナルを送ることは決してない。**`sig == 0` は許可する** — POSIX 標準の生存確認 / 権限確認のプローブ
  (シグナルは送らず存在チェックのみ)。負の値や範囲外の `sig`(`> 64`、Linux の `SIGRTMAX`)はシステム
  コールの *前に* `Error.Invalid` となる(よって `i64 → i32` の narrow は常に健全)。`EPERM`/`ESRCH` は
  共有 errno テーブル経由で表面化する。シグナルで殺された子はその後 `wait()` で `128 + sig` として観測される。
- **`process.exec(cmd, args) -> Result<(), Error>`** — **現在のプロセス内で**(`fork` せず)`execvp(cmd, argv)`
  を実行する。`args` は新しいイメージの `argv[0]` を含む完全な argv(P5 — `spawn` と同じ慣習。`cmd` は独立した
  ルックアップパス)。**成功するとプロセスイメージを置き換え、決して戻らない**ため、`Result` はその `Err` の
  腕としてしか観測されない(写された `execvp` の errno、あるいは不正な `cmd`/`argv` に対する `AL_INVALID`)。
  **⚠️ 成功パスではクリーンアップが一切走らない — これは明示的かつ意図的な仕様である:** `execvp` はアドレス空間全体を
  破棄するため、保留中の `Drop` / arena の終了 / **バッファ付き writer の flush は走らない**(ユーザ空間に
  まだ残っているバッファ済みバイトは失われる — 重要なら `exec` の前に flush すること)。これは `execvp` に
  本質的なもので、`exec` をクリーンアップの観点で **abort クラス** にする — `process.exit`(先にクリーン
  アップを走らせる)の鏡像であり、`process.abort`(クリーンアップなし)に近い。`process.exit`/`abort` と
  異なり、`exec` は型システム上では発散しない(失敗時に `Result` を返す)。MIR は普通の失敗可能ビルトイン
  呼び出しで、その成功パスが単にランタイムから戻らないだけなので、クリーンアップは発行されない(そもそも
  走り得ない)。**CLOEXEC との相互作用:** Align が所有する fd(reader / writer / socket / child)は
  `CLOEXEC`(Slice 2 の P3 スイープ)なので、exec 後のイメージはそれらを継承しない。継承される標準ストリーム
  (fd 0/1/2、`CLOEXEC` ではない)だけが生き残る — 通常の契約どおり。
- **マーシャリングは `spawn` と共有。** `cmd` + argv → C 文字列(内部 NUL / 空 argv / 非 UTF-8 の拒否)は
  単一のランタイムヘルパ `marshal_cmd_argv` で、`spawn`(親側で `fork` 前)と `exec`(置き換えられようと
  しているプロセス内)の両方が使う。重複はない。argv のソース 3 形式(`array<str>` / `slice<str>` /
  `ArrayToSlice` 経由の固定長配列リテラル)も 1 つの sema ヘルパを共有する。

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
化するにとどめ、v1 は現フレーム + グローバルフラッシュまでとする。(このギャップは正確に記録すること。)

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

# Extension — captured output + cwd / env / timeout (align-llm Request 1)

> **ステータス: 出荷済み(Slice 4〜6、#630/#631/#632、2026-07-24)。** `process.command` + `cwd` +
> `timeout_ns` + `env`/`env_clear` + `run` キャプチャがエンドツーエンドで構築済み。bytes 層と明示 cap は
> concrete consumer を得て、下記 Request 11 拡張で設計済み。実クライアントである `align-llm`(コアループが build/test/lint
> コマンドを走らせ、その**出力をパースする**)が動機である。出典:
> `../align-llm/docs/align-requests.md` の Request 1(優先度: critical — このループを塞いでいた)。

## Why this is genuinely new

上記のスライス 1–3 は `spawn`/`wait`/`kill`/`exec` を出荷しているが、`spawn` は素の `fork` + `execvp` を
**パイプも `dup2` も無し**で行う。子は親の fd を継承し、その出力はそのまま端末へ流れる。`child` ハンドルは
`{ pid, reaped }` だけである。したがって `stdout`/`stderr` を文字列としてキャプチャすること、指定した作業
ディレクトリで実行すること、子ごとの環境を渡すこと、タイムアウトで実行を打ち切ることは、いずれも**今日は
不可能**であり、そのどれも本モジュールの記録済み設計空間には無い(過去の先送りは `detach()` と `Never` 型
だけだった)。これは実ワークロードの要件であって、計画済みのギャップではない。

## Shared prerequisite — the `Error.Timeout` variant (canonical definition; `std.http`/`std.net` reuse it)

> **実装済み(スライス 5)。** 5 バリアントの `Error` enum が着地した。sema は `Denied` と `Code` の間に
> `Timeout` を登録し(`ERROR_VARIANT_CODE` は `4` になり、`Code` は末尾のまま)、ランタイムは
> `AL_TIMEOUT = 4` / `AL_CODE = 5` を持ち、MIR の `make_error_from_status` の分岐レス復号は
> `tag = min(status-1, 4)` / `Code = status-5` になった。一般の errno→`Error` の対応は不変で、タイムアウトは
> タイムアウト箇所で `AL_TIMEOUT` を**明示的に**返すことでのみ表面化する(無関係な `ETIMEDOUT`/`EAGAIN` errno
> は依然として `Error.Code` に対応づく)。`Error.Timeout` はユーザから見える。`NotFound`/`Invalid`/`Denied`/
> `Code(c)` と並んで `match` のアームを名指しできる。

タイムアウトは、非ゼロの終了とトランスポートエラーから**区別可能**でなければならない(align-llm は「テストが
ハングした」と「テストが失敗した」を区別する必要がある)。組み込みの `Error` enum は 4 つのバリアント
(`NotFound`, `Invalid`, `Denied`, `Code(i32)`)を持ち、timeout は無い。本拡張は 5 つ目の
**`Error.Timeout`**(ペイロード無し)を追加する。これは `std.http`/`std.net` の I/O タイムアウト作業
(http.md / net.md G3-1)と共有する。Request 1 が先に着地するため、ここで定義する。

5 バリアントの enum と、その分岐なしの status↔variant マッピング(唯一の非機械的な部分):

```text
variant order (must match ERROR_VARIANT_CODE):  NotFound, Invalid, Denied, Timeout, Code(i32)
AL_ status sentinels (align_runtime):            AL_NOT_FOUND=1  AL_INVALID=2  AL_DENIED=3
                                                 AL_TIMEOUT=4  (NEW)   AL_CODE=5  (was 4)
MIR status→Error decode (make_error_from_status): tag = min(status-1, 4);  Code payload = status-5
```

タッチポイント(いずれも既に 4 バリアントを扱っている — 再構築せず拡張する):
- `crates/align_sema/src/lib.rs` ~`:2795` — `Denied` と `Code` の**間**に `Timeout` バリアントを追加し
  (ペイロード `Vec::new()`、`field_base: 1`)、`Code` を末尾に保つ。
- `crates/align_runtime/src/lib.rs` ~`:6803` — `AL_TIMEOUT = 4` を挿入し、`AL_CODE` を 4→5 へ繰り上げ、
  `io_error_to_status` 内の `AL_CODE.saturating_add(errno)` の基準値を更新する。汎用の errno マッピングは
  不変であり、タイムアウトは**タイムアウト箇所で明示的に** `AL_TIMEOUT` を返すことで表面化させる。errno の
  分類によるのではない(無関係な箇所からの `EAGAIN`/`ETIMEDOUT` は依然として `Code` を意味する)。
- `crates/align_mir/src/lib.rs` ~`:8117` の `make_error_from_status` — 分岐なしのクランプを
  `min(status-1, 3)` / `Code(status-4)` から `min(status-1, 4)` / `Code(status-5)` へ変更する。

これは言語コアの変更である(`Error` enum は std ではなくコア)。意図的に小さく、一方向的である(新バリアントを
足すだけで、リネームも削除もしない)。

## Surface

実行ごとの任意設定(cwd / env / timeout)は、末尾の `opts?` 引数にはできない — **Align には任意/デフォルト/
名前付き引数が無い**。任意設定の既存の唯一のイディオムは `std.http` のリクエストビルダ
(`r := http.request(...)`; `r.header(...)`; `r.body(...)` — それぞれ束縛ローカルの Move ハンドルへの in-place な
変更で、`()` を返す。連鎖する fluent 呼び出しでは*ない*)である。本拡張はこのイディオムに正確に従うので、二つ目の
機構ではなく同じ「一つのやり方」である:

```text
c := process.command(cmd: str, args: array<str>) -> command   // Move handle (opaque, Ty::Command)
c.cwd(dir: str)                    // set working directory       -> ()   (in-place, bound-local)
c.env(name: str, value: str)       // add/override one variable    -> ()
c.env_clear()                      // start the child env empty    -> ()
c.timeout_ns(ns: i64)              // kill + Err(Timeout) past ns   -> ()
out := c.run() -> Result<run_output, Error>   // fork+capture; borrows c (re-runnable)

// run_output — an opaque Move handle (Ty::RunOutput), NOT a by-value struct (see below).
out.code() -> i64        // exit code (decode_wait_status: WEXITSTATUS, or 128+signal)
out.stdout() -> str      // captured stdout, zero-copy view into out (region-bound to out)
out.stderr() -> str      // captured stderr, zero-copy view into out (region-bound to out)
```

使用例(align-llm の verify ループ):

```align
c := process.command("git", ["git", "status", "--porcelain"])
c.cwd(repo_dir)
c.timeout_ns(30_000_000_000)          // 30 s
out := c.run()?                        // Err(Timeout) if it overruns
if out.code() == 0 {
  parse_clean(out.stdout())
} else {
  report(out.stderr())
}
```

### Why `run_output` is a handle, not `{ code, stdout, stderr }`

リクエストは `output = { code, stdout, stderr }` を素描していた。**2 つ**のヒープ文字列を所有する値渡しの
組み込み構造体は、まさに net.md(`datagram { n, peer }`)と http.md(`response_builder` が深く所有して回避)が
ともに**先送り**と記録した「ファーストクラスの組み込み構造体の返却」である — `Result` の `Ok` ペイロードは
単一の `Scalar` であり、複数の所有アロケーションを集約した値を返す機構は無い。「ヒープを所有する返却物」の
実現済み Align イディオムは、内部にアロケーションを所有する単一の不透明な `*mut Handle` をアクセサ経由で読む
ことである — これはまさに `http.response` の動作そのもの(`resp.status()` / `resp.header()` / `resp.body()`)である。
よって `run_output` は `response` を鏡写しにする: 1 つの Move ハンドル、`.code()` / `.stdout()` / `.stderr()`
アクセサ、文字列アクセサはリージョン束縛のゼロコピービューを返す。これは Align の現在の一貫した設計の中での
**理想形**であって妥協ではない。値渡し構造体の綴りは、別のより大きな先送り機能を先に作る必要があり(その上で
同じことをする二つ目のやり方になってしまう)。

## Type & ownership classification

- `command` — **Move** 型(`Ty::Command`)。cmd + 完全な argv + 任意の cwd + env 上書きリスト + タイムアウトを
  所有する。`Ty::HttpRequest`(所有内部を持つビルダ Move ハンドル)を手本とする。Drop = free。設定メソッドは
  借用(束縛レシーバ、in-place)、`run()` も借用(再実行可能)。
- `run_output` — **Move** 型(`Ty::RunOutput`)。終了コード + 2 つの所有バイトバッファを所有する。
  `Ty::HttpResponse` を手本とする。`Result<run_output, Error>` の `Ok` 位置に乗る(`Scalar::RunOutput`)。
  `.stdout()`/`.stderr()` ビューは `region_of(out)` である(P3 スタイルの escape ゲート: ビューは `out` の Drop を
  越えて escape してはならない)。Drop = 両バッファを free。
- どちらも自身のコンストラクタの `Result` Ok スロットを除いて集約要素としては拒否される — 標準の Move ハンドル
  制限である。どちらも**新 Ty の完全なスイープ**が必要(New machinery を参照)。

## Runtime design (`align_rt_command_*` + `align_rt_command_run`)

この subsection は**現在 shipped 済みのスライス 4–7**を記録する。下の Request 11 ledger が親側の完全な
bounded capture/reap lifecycle を所有する。

ビルダハンドル `Command { argv: Vec<CString>, cwd: Option<CString>, env: Vec<(CString,CString)>,
env_clear: bool, timeout_ns: i64, max_capture_bytes: Option<i64> }` を
`align_rt_command_new(cmd, args)` で構築する(argv は
`marshal_cmd_argv` を再利用。内部 NUL / 空 argv / 非 UTF-8 の拒否も同じ)。`cwd` / `env` / `env_clear` /
`timeout_ns` は薄いセッタである(env のペアは `*const AlignStr, len` のスライス ABI で marshal、`env` 呼び出し
1 回につき 1 ペア)。`run_output` ハンドルは `RunOutput { code: i64, out: Vec<u8>, err: Vec<u8> }`。

`align_rt_command_run(c, out: *mut *mut RunOutput) -> i32`:

1. optional bound を検証し、2つの exact capture layout と output shell を割り当てる。2本の `O_CLOEXEC`
   pipe を作り、両 read end を fork 前に nonblocking にする。
2. `fork`。**子**(async-signal-safety の注意点は `spawn` と同一 — スレッド化された親から fork した後の
   `execvp`/`chdir`/`setenv` は既知の既存ハザードであり、`posix_spawn` が記録済みの理想的な修正):cwd が設定
   されていれば `chdir(cwd)`(失敗 → `_exit(127)`);`env_clear` なら `clearenv()`、続いて各上書きを `setenv`;
   2 つのパイプ write 端を fd 1 と 2 に `dup2`;すべてのパイプ fd を close;`execvp`;失敗時 `_exit(127)`。
3. **親**:write 端を close し、**両 read fd を一緒に `poll`** して、stdout、stderr の順に選択した store へ
   ドレインする。両方を同時にドレインするのは必須である。さもないと、親が
   stdout を読む間に子が stderr パイプを満たすと**デッドロック**する(古典的な 2 パイプキャプチャバグ)。両方が
   EOF に達するまでループする。
4. monotonic deadline は pipe drain から direct-child reap までを覆う。timed EOF/live-child は
   `waitpid(WNOHANG)` + zero-fd `poll`、untimed EOF は blocking `waitpid` を使える。timeout、overflow、hard
   capture/wait error は最初の status を保持し、owned group があればそこへ、direct child が waitable な間は
   direct pid へ signal し、両 read を close して直接の子を reap する。既に子を consume した wait は、再利用され得る
   pid へ signal しない。
5. 成功には両 EOF と direct-child reap が必要で、その後 `out.code = decode_wait_status(status)` を設定する。
6. **UTF-8**:`out.out` / `out.err` を UTF-8 として検証する。不正 → free して `AL_INVALID` を返す
   (`fs.read_file` の先例 — `string` 型のアクセサは非 UTF-8 バイトを晒せない)。下記参照。
7. 2つの store を preallocated `RunOutput` または `RunBytes` shell へ移し、公開して `0` を返す。

`.stdout()`/`.stderr()` は `AlignStr { ptr: out.out.as_ptr(), len }` を返す — 借用ビューであり、
`align_rt_http_resp_body` とまったく同じ。

## UTF-8 policy (decision + the bounded bytes tier)

v1 の `run()` は `str` アクセサを返すので、**UTF-8 を検証し、不正バイトでエラー(`Error.Invalid`)にする** —
`fs.read_file`(string、検証あり)対 `read_bytes_view`(bytes)と一貫する。build / test / lint の出力は実際上
UTF-8 であり、クライアントはそれをテキストとしてパースする。任意のバイナリなツール出力に対する堅牢性の逃げ道は、
Request 11 の bytes 層 `c.run_bytes() -> Result<run_bytes, Error>` であり、その `.stdout()`/`.stderr()` は
`slice<u8>` を返す(検証なし)— `read_file` 対 `read_bytes_view` を一対一で鏡写しにする。同じ capture engine
上の兄弟 handle + accessor なので string 層を乱さない。厳密な bound と ownership contract は下記 extension
ledger に定める。

## Effect classification

すべて impure(プロセスを fork する / マシンを観測する)。

## Error policy

fork/pipe/dup2/waitpid の失敗 → errno→`Error` テーブル(M9)。タイムアウトは `Error.Timeout`(タイムアウト箇所
での明示的な `AL_TIMEOUT` — errno から推論しない)。非 UTF-8 のキャプチャ出力 → `Error.Invalid`。子内での
`chdir`/`exec` 失敗は子の `_exit(127)` → `out.code()` の終了コード 127 として表面化する(`spawn` と同じ規約)。
`Err` では**ない** — fork 自体は成功しているため。

## New machinery required

- `Error.Timeout` + `AL_TIMEOUT`(上記の共有前提)。
- 2 つの新しい不透明 Move ハンドル型 `Ty::Command` / `Ty::RunOutput`(+ `Scalar::Command` /
  `Scalar::RunOutput`)。それぞれ、新しい Move ハンドルが飛ばしてはならない**新 Ty の完全なスイープ**を通す
  必要がある(最近の `Ty::Captures` / 既存の `Ty::HttpResponse` を手本とする):sema の `scalar_of`/逆写像、
  `needs_drop` / 4 つの move 分類 `matches!` リスト、`tracks_region`、`region_of`、要素借用インターセプト、
  `ty_name`、`scalar_arg` の Move 拒否チョークポイント;MIR の move 分類器 / owning-expr セット / 表示名 /
  `new_slot`;codegen の LLVM ポインタ型、`handle_free_fn`(`Ty::Command => "command_free"`、
  `Ty::RunOutput => "run_output_free"`)、move 時のゼロ初期化セット、ランタイム free-fn の extern 宣言。
  `handle_free_fn` は `is_field_ok` の許可集合と一致していなければならない。このスイープこそ soundness の穴が
  潜む場所である — `/align-self-review` gate 2(「新しい IR バリアントが解析パスを飛ばす」)。
- ランタイム:`align_rt_command_new/cwd/env/env_clear/timeout_ns/run/free` +
  `align_rt_run_output_code/stdout/stderr/free`。

## Slice breakdown

4. `process.command` + `c.cwd(dir)` + `run()` — **両方 must-have**(出力キャプチャ + 作業ディレクトリ)。
   `Command`/`RunOutput` ハンドル + 新 Ty の完全なスイープ(機構の大半)、pipe+fork+dup2+2 パイプ `poll`
   ドレイン、子側の `chdir`、`.code()`/`.stdout()`/`.stderr()`、UTF-8 検証。タイムアウト/env はまだ無し。`cwd` は
   must-have であり子側の `chdir` が些細なのでここに畳み込む。よって S4 は完結した must-have の提供になる(空の
   足場ではなく、実セッタを持つ `command`)。
5. `c.timeout_ns(ns)` + `Error.Timeout` のコア変更 — 「ハングしたテストがループを凍らせる」の修正
   (kill + `Err(Timeout)`)。**実装済み。** `c.timeout_ns(ns: i64)` はバインド済みローカルへのインプレース
   セッタ(`()`)。`ns == 0` = タイムアウト無し(スライス 4 の既定)、負の `ns` はビルド時に abort。`c.run()`
   は deadline を 2 パイプ drain から direct-child reap まで通す。期限切れ時、runtime は owned
   **プロセスグループ**があればそこへ、direct child が waitable な間は direct pid へ signal し、両 read を閉じ、直接の子を reap し、
   部分出力を捨てて `Err(Error.Timeout)` を返す。deadline は pipe EOF 後も有効である。
   `timeout_ns == 0` は blocking 動作を保つ。
6. `c.env(name,value)` + `c.env_clear()` — **実装済み。** どちらもその場で書き換える束縛ローカルのセッタ
   (`()`)。`c.env(name, value)` は `(name, value)` の上書きを記録し、`c.env_clear()` は子環境を空から
   始めるよう印を付ける。ランタイムの `Command` に `env: Vec<(CString, CString)>` と `env_clear: bool` を
   追加。フォークした子では、`chdir` の後・`dup2` の前に `if env_clear { clearenv() }` を実行し、続いて
   記録された各ペアを `setenv(name, value, 1)`(overwrite=1 — 同名の後発 `env` が勝ち、`env_clear` 後の
   `env` は残る)。name/value は**親側**で C 文字列にマーシャルするため、子は pair ごとの marshalling
   allocation を追加しない。`clearenv`/`setenv`/`execvp` は P11 の子側 allocation/async-signal-safety caveat を
   保つ。name/value の内部 NUL・非 UTF-8 は abort し、`=` を含む name も abort する(`setenv` が拒否するため)。
7. **実装済み:** bytes 層 `c.run_bytes()` とコマンドローカルな `max_capture_bytes` 上限 —
   align-llm Request 11。

## Pitfalls

- **P7(2 パイプデッドロック)** — 第 1 の正しさポイント。**両方**の read fd を `poll` し両方をドレインせよ。
  さもないと、子が一方のパイプを満たす間に親が他方を読むとデッドロックする。テスト:*両方*のストリームに
  >64 KiB を書き、非ゼロで終了する子 → 両方が完全にキャプチャされ、コードも正しい。
- **P8(タイムアウトは実際に kill + reap すべし)** — deadline は pipe drain と **pipe EOF 後の wait** の両方を
  覆う。期限切れ時は process group と direct pid を `SIGKILL`、両 capture read を閉じ、直接の子を `waitpid` する。
  fd 1/2 を閉じて走り続ける子でも zombie/leak/hang を起こさない。Acceptance test:100 ms timeout の `sleep 10` と
  `exec 1>&- 2>&-; sleep 10` は bounded tolerance 内に `Err(Timeout)`、direct-child zombie 無し。
- **P9(ビューのリージョン、http P3 と同様)** — `.stdout()`/`.stderr()` は `out` へのビュー;`region_of =
  region_of(out)`。`out` の Drop を越える escape は拒否。
- **P10(新 Ty スイープ)** — `Ty::Command`/`Ty::RunOutput` は New machinery の全パスを叩かねばならない;飛ばした
  パスは leak/double-free/UAF になる。`/align-self-review` gate 2 を回す。
- **P11(子の async-signal-safety)** — スレッド化された親から `fork` した後の `chdir`/`clearenv`/`setenv`/`execvp`
  は既存の `spawn` ハザードを持つ(文書化済み;`posix_spawn` が先送りの理想)。
- **P12(無制限キャプチャ)** — 暴走する子(`yes`)はキャプチャを無制限に増やす。既存呼び出しは無制限のまま
  保ち、下記 Request 11 拡張が明示的なコマンドローカル上限とバイナリ出力層を提供する。

## Test checklist / gate

- 子が **stdout と stderr の両方**に書いて非ゼロで終了する → 呼び出し側が完全な stdout 文字列、完全な stderr
  文字列、終了コードを回収する(Request-1 の受け入れゲート)。
- `c.cwd(dir)` → 子が `dir` を作業ディレクトリとして観測する。
- `timeout_ns` を超えるコマンド → `Err(Timeout)`(非ゼロ終了とは別)、kill 済み、ゾンビ無し。
- `c.env(n,v)` の上書き / `c.env_clear()` が空から始まる → 子が期待どおりの環境を見る。
- 非 UTF-8 出力 → `Error.Invalid`(string 層)。
- 2 パイプ各 >64 KiB → デッドロック無し(P7)。
- `.stdout()` ビューが `out` の Drop を越えて escape → 拒否(P9)。
- `command` / `run_output` を array の要素にする → 拒否。
- import が必須であること。

# 拡張 — 有界 text/byte キャプチャ（align-llm Request 11）

> **ステータス: 実装済み（2026-08-14）。** この拡張は上限を選択した呼び出しについて P12 を
> 閉じ、先送りしていたバイナリ出力層を出荷する。出典: `../align-llm/docs/align-requests.md` Request 11。

## 公開契約 ledger

次の ledger が本拡張の正本である。本文と実装は独立に範囲を広げてはならない。

| Surface | 厳密な契約 |
|---|---|
| `c.max_capture_bytes(limit: i64) -> ()` | 束縛されたローカル `command` をその場で変更する setter。`limit >= 0`。負値はプログラマエラーとして、子プロセス生成や割り当てより前に abort する。最後の呼び出しが前の上限を上書きする。明示的な `0` は stdout と stderr の両方について空だけを許す。未呼び出しのコマンドだけが既存の無制限動作を保つ。同じ上限を各ストリームへ独立に適用し、その後のすべての `run()` / `run_bytes()` で使う。 |
| `c.run() -> Result<run_output, Error>` | 既存の、借用され再実行可能な text キャプチャ。有界時は各ストリームを `limit` バイトまで許す。ちょうど上限は成功し、どちらかで最初に上限を超えるバイトを観測したら owned child process group があればそこへ signal し、直接の子を kill/reap し、両方の部分出力を捨て、`Error.Invalid` を返す。上限内で完了した出力は従来どおり UTF-8 検証し、不正なら `Error.Invalid`。非ゼロ終了は従来どおり `Ok(run_output)`。 |
| `c.run_bytes() -> Result<run_bytes, Error>` | 新しい、借用され再実行可能なバイナリキャプチャ。コマンド設定、両パイプ drain、上限、timeout、kill、reap は `run()` と同じ経路を使い、UTF-8 検証はしない。`run_bytes` は終了コードと2つの byte buffer を所有する単一の不透明 Move ハンドル。 |
| `out.code() -> i64` | `run_output` と `run_bytes` の両方が既存の終了コード復号を公開する: `WEXITSTATUS`、`128 + signal`、または子側 `chdir`/`execvp` 失敗時の `127`。純粋な Copy 読み出し。 |
| `out.stdout()`, `out.stderr()` | `run_output` は zero-copy `str` view、`run_bytes` は zero-copy `slice<u8>` view を返す。空出力は空 view。各 view は出力ハンドルの region に束縛され、Drop を越えて escape できない。埋め込み NUL は byte 層では通常データであり、text 層でもストリーム全体が正しい UTF-8 なら受理する。 |

上限は合計ではなく**ストリームごと**である。選択した上限 `L` は stdout を最大 `L` バイト、stderr を最大
`L` バイト、合計で最大 `2L` バイトの保持キャプチャとして許す。stdout が消費者指定の応答予算を全部使っても
診断を保持でき、スケジューリングに依存しない1つの規則を各パイプに与える。C6 は helper command に `65_536`、
measurement command に `262_144` を設定し、無制限実行後の長さ検査はしない。

### 入力、既定値、エラー、優先順位

- `max_capture_bytes` は `i64` のバイト数を受け取る。ambient な環境変数、グローバル既定値、package 設定は
  無い。未設定だけが無制限で、`0` は実在する0バイト上限である。
- 負の `limit` は割り当てやプロセス生成より前の setter で abort する。有界実行は最初に出力 slot を検証し、
  非負上限を platform allocation size へ変換し、2つの capture store と mode 固有の空 output-handle shell を
  生成する。表現不能な layout は `Error.Invalid`。物理 capture/output allocation failure は Align の locked fatal-OOM 方針に
  従い、unwind や recoverable `Error` 無しで即時 abort する。どちらでも子はまだ開始しておらず、fatal OOM 前の
  preallocation は process teardown が回収する。
- pipe/fork/nonblocking-setup failure は固定 errno 対応を保つ。pipe 作成と両 read-fd の `fcntl` は fork 前に
  完了し、hard setup error は開いた全 fd を閉じ、有界 preallocation を解放し、子を開始しない。子側
  `chdir`/`execvp` failure は code `127` の `Ok` のまま。非ゼロ終了は capture failure より優先されない。
- shared post-fork engine は最初の pipe poll から**両方**の stream が EOF かつ直接の子が reaped になるまでを
  1つの state machine として所有する。各 `poll` 前、stdout→stderr 順の各 descriptor/read result を解釈する前、
  pipe EOF 後の各 `waitpid(WNOHANG)` checkpoint 前に monotonic deadline を検査する。期限切れなら
  `Error.Timeout` が勝つ。そうでなければ cap を越える正の read は `Error.Invalid`、`POLLNVAL`、non-`EINTR`
  poll failure、non-`EINTR`/non-`EAGAIN` read failure は固定 `Error.Code(errno)` 対応となる。`POLLNVAL` は
  deterministic に `EBADF`。したがって観測済み timeout が勝ち、それ以外は stdout-before-stderr の最初の
  hard pipe error/overflow が勝つ。いずれも後の exit status や UTF-8 検証より優先する。
- 両 pipe EOF 後、untimed run は blocking `waitpid` を使える。timed run は deadline を再検査し、
  `waitpid(WNOHANG)` を呼び、allocation 無しの zero-fd `poll` で最大 `min(remaining, 1 ms)` だけ待つ。したがって
  stdout/stderr を閉じたまま走り続ける子でも unbounded wait に入らない。`EINTR` は deadline checkpoint から
  retry する。hard `waitpid` error は固定 errno 対応で、`ECHILD` は直接の子が既に reaped であることを意味し、
  partial success を許可しない。
- 両ストリーム EOF 後、text `run()` は stdout、stderr の順に検証する。どちらも同じ `Error.Invalid` なので、
  部分出力や失敗ストリームの識別は公開しない。
- post-fork timeout/overflow/hard-pipe/hard-wait failure は winning status を保存し、direct child が waitable な間は
  この run が作った owned process group と direct pid へ `SIGKILL`、両 read fd を close、direct-child `waitpid` を
  `EINTR` retry (`ECHILD` は already-reaped)、capture/output state を free し、元の status を result slot null のまま返す。
  successful wait または `ECHILD` は後続 cleanup より先に pid を consumed とし、再利用され得る pid へ signal しない。
  `ECHILD` は synthesized exit status ではなく hard `Error.Code` のままである。この caller が reap するのは直接の子
  だけで、group 内 descendant は signal され、`setsid` descendant は契約外である。fatal OOM に cleanup/unwind
  path は無く、有界 run では pipe 作成/fork より前に起きる。recoverable error は部分 bytes、exit code、
  truncation marker を返さない。

### 所有権、lifetime、allocation、並行性

`command` は単一所有の Move handle のままで、`run()` / `run_bytes()` はそれを借用する。optional bound は反復
実行と両 run mode を越えて持続する。`run_bytes` は新 Move type の完全 sweep に従う。constructor の `Result`
Ok slot だけに置け、aggregate/array element や task capture にはできず、move 時に source を null にし、2つの
buffer をちょうど1回 Drop する。その view は text view が `region_of(run_output)` を持つのと同じく
`region_of(run_bytes)` を持つ。

有界実行では、runtime が pipe 作成と fork の**前**に、ストリームごとに厳密な `L` バイトの capture layout と
mode 固有の空 output-handle shell を割り当てる。`L == 0` ならどちらの byte layout も割り当てない。1つの
capture allocation / reserved capacity は `L` より大きくなく、2つの live capture layout の合計は厳密に
`2L` である。既存の固定64 KiB stack read scratch、command の argv/cwd/environment storage、2本の pipe
descriptor、固定 stack `[PollFd; 2]`、小さい output handle はこの capture-store 上限の外である。**親側の有界
capture/reap state machine** は fork 後に heap allocation をしない。poll descriptor array はその場で埋め、
post-EOF wait は zero-fd `poll` を使い、read は固定 scratch に入り、chunk 全体が選択ストリームの残容量へ収まる
場合だけコピーする。成功時は既存 shell を埋め、追加 allocation 無しで2 layout を移す。すべての recoverable
failure で3つの preallocated object を解放する。これら capture/output allocation の fatal OOM は process-wide
allocation rule に従い pipe 作成/fork より前に終了する。fork された子は既存 P11 launch path のままであり、
`clearenv`/`setenv`/`execvp` は allocation し得る。この capture 契約はその caveat を強めも隠しもしない。両 read
end は fork 前に nonblocking 化し、`F_GETFL`/`F_SETFL` failure は recoverable setup error であって、best-effort
post-fork assumption ではない。無制限呼び出しは既存 growable `Vec` 動作を保ち、memory-bound claim を持たない。

正の timeout または明示的 capture bound を持つ command は、子を process-group leader にする。timeout、
overflow、hard post-fork capture/wait error は owned group があればそこへ、direct child が waitable な間は直接 pid へ
`SIGKILL` を送り、EOF の再 drain 無しに両 read end を閉じ、`EINTR` retry 付きで直接の子を待つ。既に子を consume
した wait result は、その pid への後続 signal を抑止する。意図的に `setsid` で escape した
descendant は process-group 契約外だが、read end を閉じるので呼び出し元を block し続けられない。この caller は
descendant を reap すると主張しない。別 command は mutable state を共有せず、並行実行は独立する。safe Align
では実行中の同一 command を再設定できず、同期 run が完了してから再利用する。

### Runtime と cache identity

実装は既存 signature を変更せず、次の ABI entry を追加する。

```text
align_rt_command_max_capture(command: ptr, limit: i64) -> void
align_rt_command_run_bytes(command: ptr, out: ptr<ptr>) -> i32
align_rt_run_bytes_code(out: ptr) -> i64
align_rt_run_bytes_stdout(out: ptr) -> { ptr, i64 }
align_rt_run_bytes_stderr(out: ptr) -> { ptr, i64 }
align_rt_run_bytes_free(out: ptr) -> void
```

`align_rt_command_run` は ABI を保ち、`Command` に保存した新 bound を読む。両 run entry は1つの runtime capture
engine へ委譲し、mode 固有の処理は完了 buffer の UTF-8 検証と output-handle construction だけである。

`run_bytes` は compiler-owned type format ごとに次の厳密な append-only encoding を持つ。

- canonical type codec version 3 は変更しない。`Ty::RunBytes` は既存最大 `59` の後の leaf tag `60`、
  `Scalar::RunBytes` は既存最大 `35` の後の leaf tag `36`。root の semantic-to-byte / byte-to-semantic golden は
  `RunBytes <-> [3, 0, 0, 0, 0, 60]`、scalar field golden は `RunBytes <-> [36]`。tag の挿入や renumber はしない。
  unknown tag `61`/`37` と truncated root `[3, 0, 0, 0, 0]` は cache publication 前に拒否する。
- shipped interface summary format 6 は全 source type を既存 `IType::Named` で表すため、`run_bytes` のための
  新 discriminator や version bump は無い。accepted Request 9 の codec 全体の format-7 bump でもこの
  named-type record は変更しない。`run_bytes` は既存 type tag `0`、UTF-8 path `run_bytes`、type argument 0個を使う。field-local golden は双方向で
  `[0, 9, 0, 0, 0, 114, 117, 110, 95, 98, 121, 116, 101, 115, 0, 0, 0, 0]`。unknown type tag `3`、
  truncated name/argument count、invalid UTF-8、trailing bytes は semantic import 前に拒否する。

HIR、MIR、LLVM lowering、checked-HIR validation は同じ新しい closed builtin type を使う。runtime reflection、
callback、別の user wire format、ambient cache input は無く、canonical type bytes と versioned interface summary が
persisted compiler record である。同じ source の whole-program/per-unit compile は同じ run mode と output type を
選ぶ。`run` と `run_bytes` の変更、source 上の bound call の追加・変更は通常の source-derived frontend/object cache
entry を無効化する。

## 実装 closure matrix

| Cell | Owner と必要な evidence |
|---|---|
| Setter formation と validation | Sema/HIR/MIR/codegen/runtime は束縛 `command` 上の `max_capture_bytes(i64)` だけを形成する。arity/type 違いと temporary receiver は診断し、負値は side effect 前に abort。`0`、overwrite、unset は別状態。Owner: `m11_process_command::command_capture_bound_formation_and_state`。 |
| Exact-limit text success | 共通 drain は empty、`L`、stdout-only、stderr-only、同時 `L`/`L` を受理し、`run_output` は既存 code/text view を保つ。Owner: `command_capture_exact_limit_and_reuse`。 |
| One-byte overflow | どちらかが `L + 1`、または両 pipe 同時圧力でも `Error.Invalid`、output 非公開、group/direct pid kill、fd close、direct child 1回 reap。Owner: `command_capture_overflow_kills_group_and_discards_partial`。 |
| Timeout/cap/exit/UTF-8 precedence | timeout-before-overflow、overflow-before-timeout、nonzero-overflow、in-bound invalid UTF-8、両 stream close 後も deadline 超過まで生存する子で上記 checkpoint order を検証。Owner: `command_capture_error_precedence` と `command_timeout_covers_post_eof_wait`。 |
| Hard pipe/wait error | non-`EINTR` poll、`POLLNVAL`、stdout/stderr hard read、post-EOF `waitpid` error を注入。観測済み timeout が勝ち、それ以外は stdout が stderr より先、元の固定 errno が cleanup 後も残り、partial result 無し、owned group (存在時)/direct pid kill、fd close、direct child reap または既に `ECHILD`。Owner: `command_capture_hard_io_errors_are_terminal`。 |
| Post-fork lifecycle | `{pipes open/EOF} × {child live/exited} × {untimed/timed/bounded}` を parameterize。成功には両 EOF と direct child reap が必要。timed EOF/live child は WNOHANG + allocation-free zero-fd poll で exit/deadline まで進む。Owner: `command_capture_lifecycle_state_matrix`。 |
| Binary tier | `run_bytes` は invalid UTF-8 と embedded NUL を byte-for-byte で保ち、region-bound byte view、nonzero exit、exact-cap 動作を共有。Owner: `command_run_bytes_preserves_arbitrary_output`。 |
| Move と Drop | formation、construction、`Result` move-in/out、`?`、`else`、`match`、`map_err`、replacement、return、source nulling、early exit は各 output を1回 Drop。aggregate/capture/temporary と escaped view を拒否。Owner: `m11_process_command` ownership matrix と checked-HIR variant tripwire。 |
| Allocation、descriptor setup、malformed limit | exact layout/shell を pipe/fork 前に割り当て、zero は byte store 無し、unrepresentable layout は `Invalid`、物理 capture/output allocation failure は子が存在する前に no-unwind abort、両 read fd は fork 前に nonblocking または setup failure、固定 poll/scratch/wait storage により親側 bounded capture/reap state machine の post-fork heap allocation は0、capture capacity は `L` 以下。Owner: 第1/第2/shell allocation の subprocess fatal-OOM failpoint、`fcntl` failpoint、`command_capture_allocation_bound`。child marker で fork 未到達を証明。 |
| Child launch boundary | bounded terminal は既存の post-fork child `chdir`/environment/`execvp` path を再利用し、新しい child-side operation を追加しない。`clearenv`/`setenv`/`execvp` は allocation し得て P11 を保ち、親側 capture bound/fatal-OOM-before-fork claim は適用しない。Owner: `m11_process_command` 内の既存 cwd/env/env_clear/exit-127 regression。 |
| Reuse と concurrency | 1 command が保持/上書き bound で text/byte run を反復し、2つの独立 command は shared state 無しで並行実行。Owner: `command_capture_reuse_and_independent_concurrency`。 |
| Generic/interface/per-unit/cache parity | `Result<run_bytes, Error>` を返す関数は whole-program/per-unit で同じ type/ABI。interface round-trip と exact edit/revert cache identity が一致。Owner: process interface/per-unit/cache tests。 |
| Existing behavior | setter 無し `run()` は無制限で byte-for-byte compatible。cwd/env/env_clear/timeout、large dual-pipe、nonzero exit、text view owner は green のまま。Owner: 完全な `m11_process_command` target。 |

明示的な memory promise のため、local `bench/process_capture` measurement も必要である。65,536 と 262,144 の
consumer limit について、有界 text/byte throughput と最大 live capture-layout bytes を既存無制限 path と比較して
記録する。resource contract の evidence であり correctness gate ではない。

### Closure matrix reopened: post-fork lifecycle

2回目の review で、最初の matrix は bounded pipe capture までで direct-child termination を含まないと判明した。
その後の projection review で、target-facing overview が古い unconditional post-EOF wait を記述し、既存 child
launcher まで親側の no-allocation promise に含めていたと判明した。さらに status audit で、corrected target を
pending replacement のまま、still-shipped スライス 5 behavior と分離した。reopen した軸は
`{parent capture/child launch} × {pipe state} ×
{direct-child state} × {deadline state} × {terminal trigger}`。shared parent engine は pre-fork setup から EOF、
direct-child wait、terminal cleanup まで1つの indivisible capability とし、既存 P11 child launcher は明示的な隣接
boundary とする。新しい producer/type work をこの runtime consumer から分けると既存 timeout hang と
truncated-success path が reachable のままなので、design/implementation は1つの mergeable capability を保つ。

## Design review finding の closure

| Finding | Ledger-first closure |
|---|---|
| P1 recoverable OOM が locked allocation model と矛盾 | error/allocation row は no-unwind fatal OOM を維持する。表現不能 layout だけが `Error.Invalid` で、subprocess failpoint が abort-before-fork を証明する。 |
| P1 HIR/native owner ledger に新 surface が無い | `docs/impl/19-hir-validation-ledger.md` が5つの厳密な expression row と malformed fixture を予約し、`docs/impl/20-runtime-abi-ledger.md` が6つの keyed symbol、declaration、attribute、count、registry owner を予約する。 |
| P2 compiler type encoding が実装依存 | canonical codec version 3 に exact `Ty`/`Scalar` tag 60/36 と双方向/malformed vector を追加し、shipped interface format 6 は既存 named-type record を exact byte vector 付きで使い、Request 9 format 7 もその record を維持する。 |
| P2 external request register が proposed のまま | sibling register に accepted per-stream/text/bytes/ownership/error contract と final reviewed design commit を記録し、指示どおりその repository では uncommitted のままにする。 |
| P1 deadline が pipe EOF で終わっていた | reopen した lifecycle は direct-child reap まで deadline を維持する。EOF/live は `waitpid(WNOHANG)` + allocation-free zero-fd `poll` と専用 owner を使う。 |
| P1 bounded poll が fork 後に allocation | allocation row は exact store/shell を pipe 前に用意し、fixed stack poll/scratch state、pre-fork nonblocking setup、親側 bounded post-fork heap allocation 0を要求する。 |
| P2 hard poll/read error が partial success | precedence/hard-I/O row が最初の deterministic errno を map し、同じ kill/close/direct-reap cleanup、元 status 保持、output 非公開を要求する。 |
| P2 normative group reaping が過大 | specification は process group を signal し、reap するのは直接の子だけと明記する。escaped descendant は契約外。 |
| P1 target overview が pipe EOF で deadline を終えていた | Request 11 lifecycle は direct-child reap まで deadline を維持し、timed EOF/live-child state は exit/expiry まで `waitpid(WNOHANG)` + zero-fd `poll` を使う。 |
| P1 no-allocation promise が既存 child launcher を含んだ | allocation promise/owner row は親側 bounded capture/reap state machine だけを対象とし、別の child-launch row が `clearenv`/`setenv`/`execvp` の P11 を維持する。 |
| P2 target timeout overview に direct-pid fallback が無い | すべての Request 11 記述は status を保存し、owned group があればそこへ、direct child が waitable な間は direct pid へ signal、両 read を close、`EINTR` retry 付きで直接の子だけを reap する。successful wait または `ECHILD` は、再利用され得る pid への後続 signal を抑止する。 |
| P1 pending lifecycle を shipped スライス 5 に帰属 | 設計時の status 分離は、完全な Request 11 state machine が atomic に有効化されるまで旧 Slice-5 behavior と区別した。 |
| P2 pending direct-pid fallback を shipped スライス 5 に帰属 | 設計時 ledger は waitable child への direct-pid fallback を implementation に予約し、shipped runtime と owner が現在それを検証する。 |
| P2 `code()` を region-bound view と記述 | condensed specification は `code()` を Copy `i64` とし、region-bound zero-copy view を `stdout()`/`stderr()` だけに限定した。 |

## Acceptance gate

implementation acceptance と consumer adoption には次が必要である:

1. 各 matrix row が implementation と regression owner を指し、新 type が exhaustive variant tripwire に含まれる。
2. 英日 process design、`draft.md`、condensed language spec、design notes、Settled decisions、checked-HIR ledger、
   runtime ABI ledger、align-llm request register が一致する。
3. focused process owner、bounded PR gate、library/binary Clippy、whole/per-unit/cache owner、allocation failpoint、
   local resource measurement が final candidate で通る。
4. align-llm は、named merged implementation commit を pin し、focused helper/adapter target が 65,536/262,144
   bound、timeout/cap precedence、process-group cleanup、arbitrary-byte tier を証明し、その capability wave の final
   `make ci` が通った後だけ request を進められる。
