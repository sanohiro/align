# std: ファイル、I/O、そして OS 境界

> 🌐 [English](../13-std-os.md) · **日本語**

OS に関連する機能はすべて `std` 名前空間に配置され、明示的なインポートの背後に隔離されています。具体的には `std.io`、`std.fs`、`std.path`、`std.env`、`std.time`（本章で解説）、および `std.encoding` と `std.rand`（次章で解説）などです。これらのインポートは、そのモジュールが持つ**ケイパビリティ（権限）の宣言**としても機能します。ファイル冒頭に `std` のインポートがないモジュールは、OS のリソースに一切アクセスしないことが証明可能です。`std` ライブラリ全体を貫く設計ルールは以下の3つです。

- 失敗する可能性のある操作は、すべて `Result<T, Error>` を返します。OS の `errno` から Align のエラー型への変換は、単一の固定テーブルに従って一意に決定されます。例えば `ENOENT` は `NotFound` に、`EACCES` や `EPERM` は `Denied` に、`EINVAL` は `Invalid` にマッピングされ、それ以外はすべて `Code(errno)` として扱われます。
- リソースハンドルは **Move 型** として扱われ、スコープを抜けてドロップされる際に自動的に自身をクローズ（解放）します（[05](05-memory.md) 章参照）。プログラマが `close()` の呼び出しを忘れることはなく、エラー発生時に早期リターンする経路（エラーパス）でもリソースリークは起こりません。
- 隠れた状態はありません。グローバルなファイルディスクリプタのテーブルや、カレントディレクトリ（CWD）に依存した暗黙の解決、システムのロケール設定によって挙動が変わるような「驚き」は一切排除されています。

## ファイルを 1 回の呼び出しで: `std.fs`

```align
import std.fs

pub fn main(args: array<str>) -> Result<(), Error> {
    fs.write_file(args[1], "hello, disk\n")?
    if fs.exists(args[1]) { print("written") }
    data := fs.read_file(args[1])?      // whole file → owned string
    print(data.len())                   // 12
    fs.remove(args[1])?
    return Ok(())
}
```

`read_file`、`write_file`、`exists`、`remove`、`read_dir` は、ファイル全体を一括で扱うための高レベルな API です。1回の関数呼び出しで処理が完了し、プログラマがハンドルを管理する必要はありません。`write_file` は `str`、`builder`、`buffer` などのデータを受け取ります。`read_dir` はディレクトリ内のファイル名を `array<string>` として返します。テキストを読み込む関数は入力が正しい UTF-8 であるかを検証し（不正なバイナリが含まれる場合は `Error.Invalid` を返します）、純粋なバイナリデータの読み書きは後述するストリーミング API やメモリマップ API を使用します。

## ゼロコピー読み込み: `read_file_view`

```align
import std.fs
import std.io

pub fn main(args: array<str>) -> Result<(), Error> {
    arena {
        v := fs.read_file_view(args[1])?    // mmap — no read loop, no copy
        print(v.len())
        io.stdout.write(v)?
    }
    return Ok(())
}
```

`read_file_view` はファイルをメモリ上にマップ（mmap）し、そのデータに対する `str` ビューを返します。この関数を呼び出すには、**周囲を `arena` ブロックで囲む必要があります**。マッピングの有効期間（ライフタイム）はこのアリーナと同一であり、アリーナがクリーンアップされるタイミングでアンマップ（`munmap`）が実行されます。このため、取得したビューをアリーナの外へ持ち出すことはできません（一部のデータをスコープ外で使い続けたい場合は `.clone()` でディープコピーします）。[05](05-memory.md) 章で解説したメモリモデルは、mmap をサポートするために特別なルールを追加したわけではありません。mmap の仕組み自体が、Align のアリーナ・ベースのライフタイムモデルに完璧に適合したのです。

`read_file_view` は戻り値として `str` を返すため、内容が正しい UTF-8 であることを検証し、不正なバイナリデータを含むファイルは拒否します。GGUF 形式の機械学習モデルやパック済みの検索インデックスといったバイナリ資産を扱う場合は、兄弟関数である `read_bytes_view` を使用します。こちらは UTF-8 の検証を行わずに同じアリーナベースの mmap を実行し、純粋なバイト列のビューである `bytes`（内部的には `slice<u8>`）を返します。

```align
import std.fs
import std.io

pub fn main(args: array<str>) -> Result<(), Error> {
    arena {
        raw := fs.read_bytes_view(args[1])?   // バイナリ mmap — 検証なし、ゼロコピー
        print(raw.len())
        io.stdout.write(raw)?
    }
    return Ok(())
}
```

アリーナのライフタイム規則はここでも同じです（`bytes` ビューはアリーナよりも長生きできません）。また、v1 の実装における制限も `read_file_view` と同様です。例えば、特殊ファイル（デバイスファイルなど）やサイズが 0 のファイルに対しては、真のゼロコピーのメモリマップは行われず、アリーナ内への通常のデータコピーへフォールバックします。さらに、マップ中のファイルが別のプロセスによって並行して切り詰められた（truncate された）場合、OS レベルで `SIGBUS` シグナルが発生する可能性があります。（Align のランタイムはシグナルハンドラを一切インストールしません。プロセス全体に影響を与えるシグナルハンドラは、Align が最も嫌う「隠れた副作用」そのものだからです。）

また、現時点では `bytes.clone()` が未実装であるため、バイナリデータの一部をアリーナの外へ残したい場合は、ビューから直接コピーするのではなく、ファイルや `buffer` へ書き出す（`fs.write_file` などを使用する）必要があります。

## ストリーム: `reader`、`writer`、`buffer`

ファイルのサイズがメモリ容量を超えるような大きなデータを扱うための、ストリーミング操作の階層です。この制御構造は、[02](02-language-basics.md) 章で解説した `loop` 式と相性が良く、典型的なパターンとして利用されます。

```align
import std.fs

fn pump(r: reader, w: writer, buf: buffer) -> Result<(), Error> {
    loop {
        n := r.read(buf)?           // fill buf to capacity; 0 = EOF
        if n == 0 { break Ok(()) }  // break carries the loop's value out
        w.write(buf.bytes())?
    }
}

pub fn main(args: array<str>) -> Result<(), Error> {
    r := fs.open(args[1])?          // reader — owns the fd, closes on drop
    w := fs.create(args[2])?        // writer
    buf := buffer(4096)             // reused across the whole loop
    pump(r, w, buf)?
    return Ok(())
}
```

そして、このような「読み込んでそのまま書き出す」処理の専用の短縮形として `io.copy` が提供されています（ファイルサイズに関わらず、一定量のメモリしか消費しません）。

```align
import std.io

pub fn main() -> Result<(), Error> {
    n := io.copy(io.stdin, io.stdout)?      // the whole of `cat`
    return Ok(())
}
```

`io.stdin`、`io.stdout`、`io.stderr` は、プロセスの標準ストリームに対する借用されたハンドルです。出力が頻繁に行われる処理では、パフォーマンスを向上させるために `w := io.stdout.buffered()` としてバッファリング層でラップし、最後に `w.flush()?` で強制出力します。

**v1 において開発者が一度は必ずつまずくルール：**
所有権を持つハンドルに対してメソッドを呼び出す場合、必ず**事前にローカル変数へ束縛（代入）**しなければなりません。つまり、`fs.create(p)?.write(d)?` のようなメソッドチェーンはコンパイラに拒否されるため、先にハンドルに名前（変数名）を付ける必要があります。

（※ただし、借用された標準ストリームは例外であり、`io.stdout.write("ok\n")?` のように直接呼び出しても問題ありません。一方、`.buffered()` が返す writer は所有権を持つため変数への代入が必要です。）

一般の無名 Move 値のクリーンアップ処理自体はすでに実装されていますが、メソッドチェーンのレシーバとして直接受け入れるように制限を緩和するには、変更や消費を伴うハンドルメソッドに対して安定したメモリアドレスを保証する仕組みが必要となるため、現状では意図的な制限として残されています。

## `std.path`、`std.env`、`std.time`

```align
import std.path
import std.env
import std.time

pub fn main() -> Result<(), Error> {
    j := path.join("logs/app", "run.tar.gz")    // owned string
    print(path.dir(j))                          // logs/app     — zero-copy view
    print(path.base(j))                         // run.tar.gz   — view
    print(path.ext(j))                          // .gz          — view
    print(path.normalize("a/./b/../c"))         // a/c — lexical only, no filesystem touch

    env.set("ALIGN_GUIDE", "yes")?
    match env.get("ALIGN_GUIDE") {              // Option<string> — absence isn't an error
        Some(v) => print(v),
        None    => print("unset"),
    }

    t0 := time.instant()                        // monotonic ns — for measuring
    time.sleep(1000000)                         // 1 ms; the argument is ns, exactly i64
    t1 := time.instant()
    if t1 > t0 { print("time moved") }
    // time.now() — wall-clock UNIX ns — for timestamps
    return Ok(())
}
```

設計上の重要な判断が反映されている注意点は以下の通りです。

- `path.base`、`path.dir`、`path.ext` は**入力文字列へのビュー**を返します。新しい文字列のメモリ確保は行われず、リージョンのルールが適用されます（アリーナ内に確保されたパス文字列から取得したビューは、そのアリーナより長生きできません）。
- `env.get` は `Result` ではなく `Option` を返します。環境変数が未設定であることは「エラー（失敗）」ではなく、「値が存在しない」という正常な状態の1つだからです。「エラーによる取得失敗」と「値が存在しない」の違いは、型システムが明確に教えてくれます。
- 時間の間隔（期間）は、単なる `i64` 型のナノ秒として表現されます。複雑な `Duration` 型や、単位を表す enum、暗黙の変換 API などはありません。経過時間の計測には `instant()`（モノトニックなナノ秒）を、現在時刻のタイムスタンプ取得には `now()`（実時間の UNIX ナノ秒）を使用します。関数に `i32` の値を渡そうとすると型エラーになります（[02](02-language-basics.md) 章で解説した通り、暗黙の型拡張は行われません）。
- コマンドライン引数は、エントリポイントの引数 `main(args: array<str>)` として直接渡されます。グローバルな `env.args` のようなものは存在せず、`argv` は必ず「1つの目に見える扉（引数）」を通ってプログラムに流れ込みます。
