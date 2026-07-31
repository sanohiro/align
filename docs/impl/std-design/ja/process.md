このディレクトリには、ロードマップの本文ではカバーしきれない `std` モジュールについて、Opus がそのまま実装に着手できる粒度の
設計仕様を収めている。執筆はメインループ（Fable）が担当しており、各モジュールの実装においてこれが信頼できる情報源（source of truth）となる。

# std.process — implementation design (M11)

> 🌐 [English](../process.md) · **日本語**

> **ステータス:** M11 のコアは完了済みです（exit/abort、spawn/wait/kill、exec は実装済み）。**拡張は
> 2026-07-24 に設計済み**（align-llm Request 1）です。`process.command` ビルダ + `run_output` ハンドルに
> よる、出力キャプチャ + cwd / env / timeout — 本ファイル末尾の「Extension」を参照してください。**スライス
> 4〜6 は実装済み**（`process.command`/`c.cwd`/`c.run` のキャプチャ、`c.timeout_ns` とコアの `Error.Timeout`
> バリアント、`c.env`/`c.env_clear`）。これで拡張は後回しの bytes 層（`c.run_bytes()`、需要に応じて出荷）を
> 除いて**完了**です。

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
> `timeout_ns` + `env`/`env_clear` + `run` キャプチャがエンドツーエンドで構築済み。後回しは bytes 層
> (`run_bytes`、Slice 7、需要待ち)のみ。実クライアントである `align-llm`(コアループが build/test/lint
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
match out.code() {
  0 => parse_clean(out.stdout()),
  _ => report(out.stderr()),
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

ビルダハンドル `Command { argv: Vec<CString>, cwd: Option<CString>, env: Vec<(CString,CString)>,
env_clear: bool, timeout_ns: i64 }` を `align_rt_command_new(cmd, args)` で構築する(argv は
`marshal_cmd_argv` を再利用。内部 NUL / 空 argv / 非 UTF-8 の拒否も同じ)。`cwd` / `env` / `env_clear` /
`timeout_ns` は薄いセッタである(env のペアは `*const AlignStr, len` のスライス ABI で marshal、`env` 呼び出し
1 回につき 1 ペア)。`run_output` ハンドルは `RunOutput { code: i64, out: Vec<u8>, err: Vec<u8> }`。

`align_rt_command_run(c, out: *mut *mut RunOutput) -> i32`:

1. パイプを 2 本(`stdout`, `stderr`)作り、両端を `O_CLOEXEC` にする(P3 — 子へリークせず、read 端が exec 後の
   イメージに届かない)。
2. `fork`。**子**(async-signal-safety の注意点は `spawn` と同一 — スレッド化された親から fork した後の
   `execvp`/`chdir`/`setenv` は既知の既存ハザードであり、`posix_spawn` が記録済みの理想的な修正):cwd が設定
   されていれば `chdir(cwd)`(失敗 → `_exit(127)`);`env_clear` なら `clearenv()`、続いて各上書きを `setenv`;
   2 つのパイプ write 端を fd 1 と 2 に `dup2`;すべてのパイプ fd を close;`execvp`;失敗時 `_exit(127)`。
3. **親**:write 端を close する。両 read fd を non-blocking にする。**両 read fd を一緒に `poll`** し、データが
   届くたびに `out.out` / `out.err` へドレインする。両方を同時にドレインするのは必須である。さもないと、親が
   stdout を読む間に子が stderr パイプを満たすと**デッドロック**する(古典的な 2 パイプキャプチャバグ)。両方が
   EOF に達するまでループする。
4. **タイムアウト**:`timeout_ns > 0` なら、残りのデッドラインで `poll` する(ns→ms、≥1 にクランプ)。期限切れ時:
   `kill(pid, SIGKILL)`、EOF までドレインし続け(子は死につつある — 有界の non-blocking なドレインなので、パイプが
   reap を詰まらせない)、`waitpid` の後 **`AL_TIMEOUT`** を返す(部分出力は捨てる — 「タイムアウトを報告せよ、
   半端な答えを返すな」)。`timeout_ns == 0` = タイムアウト無し(ブロック)。負の `timeout_ns` は
   `c.timeout_ns()` の構築時に拒否する(`kill` のシグナル範囲と同じく abort)。
5. 子を `waitpid`(ここで reap — ゾンビ無し);`out.code = decode_wait_status(status)`。
6. **UTF-8**:`out.out` / `out.err` を UTF-8 として検証する。不正 → free して `AL_INVALID` を返す
   (`fs.read_file` の先例 — `string` 型のアクセサは非 UTF-8 バイトを晒せない)。下記参照。
7. `*out = Box::into_raw(Box::new(RunOutput{...}))`;`0` を返す。

`.stdout()`/`.stderr()` は `AlignStr { ptr: out.out.as_ptr(), len }` を返す — 借用ビューであり、
`align_rt_http_resp_body` とまったく同じ。

## UTF-8 policy (decision + the deferred bytes tier)

v1 の `run()` は `str` アクセサを返すので、**UTF-8 を検証し、不正バイトでエラー(`Error.Invalid`)にする** —
`fs.read_file`(string、検証あり)対 `read_bytes_view`(bytes)と一貫する。build / test / lint の出力は実際上
UTF-8 であり、クライアントはそれをテキストとしてパースする。任意のバイナリなツール出力に対する堅牢性の逃げ道は、
**先送りの bytes 層** `c.run_bytes() -> Result<run_bytes, Error>` であり、その `.stdout()`/`.stderr()` は
`slice<u8>` を返す(検証なし)— `read_file` 対 `read_bytes_view` を一対一で鏡写しにする。最初のスライスでは
出荷しない先送りだが、string 層を乱さず落とし込めるよう設計する(兄弟ハンドル + アクセサ)。非 UTF-8 のツール
出力が消費者にとって現実だと判明したら出荷する。

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
   はデッドラインを 2 パイプのドレインに通す(残り時間 ns→ms、`>= 1` にクランプして `poll`)。期限切れ時は子の
   **プロセスグループ**全体を `SIGKILL` し(子は自分のグループに `setpgid` するので、`sh -c "sleep 10"` の孫
   プロセスも回収される。さもないとキャプチャパイプを開いたままドレインをハングさせる)、EOF までドレイン、
   `waitpid`、そして `Err(Error.Timeout)` を返す(部分出力は破棄)。`poll` の `EINTR` は残りデッドラインを
   再計算する。`timeout_ns == 0` は無限の `-1` `poll`(スライス 4 の挙動そのまま)を保つ。
6. `c.env(name,value)` + `c.env_clear()` — **実装済み。** どちらもその場で書き換える束縛ローカルのセッタ
   (`()`)。`c.env(name, value)` は `(name, value)` の上書きを記録し、`c.env_clear()` は子環境を空から
   始めるよう印を付ける。ランタイムの `Command` に `env: Vec<(CString, CString)>` と `env_clear: bool` を
   追加。フォークした子では、`chdir` の後・`dup2` の前に `if env_clear { clearenv() }` を実行し、続いて
   記録された各ペアを `setenv(name, value, 1)`(overwrite=1 — 同名の後発 `env` が勝ち、`env_clear` 後の
   `env` は残る)。name/value は**親側**で C 文字列にマーシャルする(子は割り当てをしない。`spawn`/スライス 4 の
   非同期シグナル安全性の規律)。name/value の内部 NUL・非 UTF-8 は abort し、`=` を含む name も abort する
   (`setenv` が拒否するため)。
7. *(先送り)* bytes 層 `c.run_bytes()` — 需要に応じて出荷。

## Pitfalls

- **P7(2 パイプデッドロック)** — 第 1 の正しさポイント。**両方**の read fd を `poll` し両方をドレインせよ。
  さもないと、子が一方のパイプを満たす間に親が他方を読むとデッドロックする。テスト:*両方*のストリームに
  >64 KiB を書き、非ゼロで終了する子 → 両方が完全にキャプチャされ、コードも正しい。
- **P8(タイムアウトは実際に kill + reap すべし)** — 期限切れ時 `SIGKILL`、続いて EOF までドレインして
  `waitpid`;ゾンビをリークせず、満杯のパイプで詰まらせない。テスト:100 ms タイムアウトの `sleep 10` →
  ~100 ms 以内に `Err(Timeout)`、ゾンビ無し。
- **P9(ビューのリージョン、http P3 と同様)** — `.stdout()`/`.stderr()` は `out` へのビュー;`region_of =
  region_of(out)`。`out` の Drop を越える escape は拒否。
- **P10(新 Ty スイープ)** — `Ty::Command`/`Ty::RunOutput` は New machinery の全パスを叩かねばならない;飛ばした
  パスは leak/double-free/UAF になる。`/align-self-review` gate 2 を回す。
- **P11(子の async-signal-safety)** — スレッド化された親から `fork` した後の `chdir`/`clearenv`/`setenv`/`execvp`
  は既存の `spawn` ハザードを持つ(文書化済み;`posix_spawn` が先送りの理想)。
- **P12(無制限キャプチャ)** — 暴走する子(`yes`)はキャプチャを無制限に増やす。v1 は無制限(`read_file` が
  ファイル全体を読むのと同じ);`max_capture` の上限は記録済みの将来ノブであって v1 ではない。

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
