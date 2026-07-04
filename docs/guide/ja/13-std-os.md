# std: ファイル、I/O、そして OS 境界

> 🌐 [English](../13-std-os.md) · **日本語**

OS に関わるものはすべて `std` に置かれ、明示的なインポートの背後に隠れています。`std.io`、`std.fs`、`std.path`、`std.env`、`std.time`(この章)、それに `std.encoding` と `std.rand`(次章)です。これらのインポートはケイパビリティの宣言でもあります。ヘッダーに `std` インポートを持たないファイルは、OS に一切触れないことが証明できるのです。`std` 全体を貫く 3 つのルールがあります。

- 失敗しうるものはすべて `Result<T, Error>` を返します。errno からの変換は**唯一の固定テーブル**で決まります。`ENOENT` → `NotFound`、`EACCES`/`EPERM` → `Denied`、`EINVAL` → `Invalid`、それ以外はすべて `Code(errno)` です。
- リソースハンドルは**Move 型**で、ドロップ時に自分自身をクローズします(第 [05](05-memory.md) 章)。忘れうる `close()` はなく、エラーパスでのリークもありません。
- 何も隠されていません。グローバルなオープンファイルテーブルも、cwd 相対の魔法も、ロケール由来の驚きもありません。

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

`read_file` / `write_file` / `exists` / `remove` / `read_dir` はファイル全体を扱う階層です。1 回の呼び出しで済み、管理すべきハンドルはありません。`write_file` は `str`、`builder`、`buffer` のバイト列を受け取ります。`read_dir` は名前の `array<string>` を返します。テキスト読み込みは UTF-8 を検証し(バイナリのゴミには `Error.Invalid`)、バイナリデータは後述のストリーミング階層を通します。

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

`read_file_view` はファイルをマップし、その `str` ビューを渡します。これは**囲む `arena` を要求します**。マッピングの寿命はアリーナであり、アンマップはアリーナのクリーンアップであり、ビューは外へ逃がせません(一部を生き残らせたいなら `.clone()` します)。第 [05](05-memory.md) 章のメモリモデルは mmap のために特別ケースを増やしたりはしていません。mmap の方がモデルに収まったのです。

## ストリーム: `reader`、`writer`、`buffer`

メモリより大きなデータのためのストリーミング階層です。

```align
import std.fs

fn pump(r: reader, w: writer, buf: buffer) -> Result<(), Error> {
    n := r.read(buf)?               // fill buf to capacity; 0 = EOF
    if n == 0 { return Ok(()) }
    w.write(buf.bytes())?
    return pump(r, w, buf)          // tail call — the loop
}

pub fn main(args: array<str>) -> Result<(), Error> {
    r := fs.open(args[1])?          // reader — owns the fd, closes on drop
    w := fs.create(args[2])?        // writer
    buf := buffer(4096)             // reused across the whole loop
    pump(r, w, buf)?
    return Ok(())
}
```

そして、まさにこの形のための短縮形が `io.copy` です(ファイルサイズによらず一定メモリ)。

```align
import std.io

pub fn main() -> Result<(), Error> {
    n := io.copy(io.stdin, io.stdout)?      // the whole of `cat`
    return Ok(())
}
```

`io.stdin` / `io.stdout` / `io.stderr` は借用された標準ストリームです。出力の多い処理では `w := io.stdout.buffered()` … `w.flush()?` のようにラップします。

**一度は必ずつまずく v1 のルール:** *所有された*ハンドルは、使う前に**ローカル変数へ束縛**しなければなりません。`fs.create(p)?.write(d)?` は拒否されます。名前のない一時値はクリーンアップを実行しないままドロップされてしまう(今のところ)ため、コンパイラが名前を付けることを強制するのです。借用された標準ストリームは例外です(`io.stdout.write("ok\n")?` は問題ありません)。ただし `.buffered()` したライターは例外ではありません(最後の flush はドロップ時に走るので、名前を付ける必要があります)。この制限は、Move 一時値がドロップを得れば解除されます(実装中)。

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

設計上の重みを持つ注意点です。

- `path.base`/`dir`/`ext` は**入力へのビュー**を返します。割り当てはなく、リージョンのルールが適用されます(アリーナにマップされたパスのビューはそのアリーナより長生きできません)。
- `env.get` は `Result` ではなく `Option` を返します。未設定の変数は失敗ではなく通常の答えだからです。どちらの「ない」なのかは型が教えてくれます。
- 期間はただの `i64` ナノ秒です。`Duration` 型も、単位の enum も、変換 API もありません。区間には `instant()`、タイムスタンプには `now()` を使い、`i32` を渡すと型エラーになります(暗黙の拡張はありません。第 [02](02-language-basics.md) 章の通り)。
- プログラム引数は `main(args: array<str>)` です。`env.args` は存在せず、argv は 1 つの見える扉から流れ込みます。
