# std: ファイル、I/O、そして OS 境界

> 🌐 [English](../13-std-os.md) · **日本語**

必要な OS・サービスの API を `std` からインポートします。本章では `std.io`、`std.fs`、`std.path`、`std.env`、`std.time` を扱い、続く章でエンコーディング、乱数、その他のサービスを紹介します。インポートは直接使う API を示します。ただし、そのファイルに `std` のインポートがなくても、`print` や別モジュールへの呼び出しで I/O を行う場合はあります。`std` に共通するルールは3つです。

- 失敗しうる操作は `Result<T, Error>` を返します。`errno` からの変換規則は共通で、`ENOENT` は `NotFound`、`EACCES` / `EPERM` は `Denied`、`EINVAL` は `Invalid`、それ以外は `Code(errno)` です。
- リソースハンドルは **Move 型** で、drop 時に閉じられます（第 [05](05-memory.md) 章）。`close()` を呼ぶ必要はなく、エラーで早期 return する場合も解放されます。
- グローバルなファイル管理テーブル、カレントディレクトリに依存する暗黙の処理、ロケールに依存する挙動は持ち込みません。

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

`read_file`、`write_file`、`exists`、`remove`、`read_dir` は、ハンドルを管理せずに1回の呼び出しで使える API です。`write_file` は `str`、`builder`、`buffer` のバイト列を受け取ります。`read_dir` は名前の一覧を `array<string>` として返します。テキストの読み込みでは UTF-8 を検証し、不正なら `Error.Invalid` を返します。バイナリデータには後述のストリーミング API を使います。

## 既存のファイルを置き換えずに公開する

既存のエントリを暗黙に置き換えずに成果物を公開するため、`std.fs` には次の 2 つの
プリミティブがあります。

```text
fs.create_exclusive(path: str) -> Result<writer, Error>
fs.rename_no_replace(source: str, destination: str) -> Result<(), Error>
```

`create_exclusive` は OS の排他的ファイル作成を1回実行します。指定した場所にファイル、
ディレクトリ、シンボリックリンク、FIFO、デバイスなどがあれば `Error.Code(native EEXIST)` を
返し、既存のエントリは開かず、切り詰めも置換も削除もしません。返される `writer` は所有権を
持つ Move ハンドルです。drop 時にはファイルを閉じますが削除はしないため、書き込みや flush に
失敗して途中までのファイルが残った場合は、呼び出し側で後始末します。

`rename_no_replace` は同じファイルシステム内で、宛先を置き換えずに名前を変更します。
宛先をたどったり削除したりせず、移動元をディレクトリエントリとして移動します。エラーは
共通の errno 変換規則に従います。別のファイルシステムへのコピー、クラッシュ時の永続性、
2ファイルをまとめたトランザクション、自動的な後始末は提供しません。パスは呼び出し中だけ
借用され、記述したとおりに渡されます。空文字列、不正な UTF-8、NUL を含むパスは使えません。
パスの必要容量の計算がオーバーフローすると `Error.Invalid`、実際のメモリ不足ではプロセスを終了します。

たとえば結果ファイルと、その根拠を記録したファイルを別々に公開するなら、両方の writer を
閉じ、公開先が空いていることを確認してから、結果、根拠の順で名前を変更できます。後始末で
削除するのは自分が作ったパスだけです。この2回の名前変更は、ひとまとまりの不可分な操作ではありません。

## シンボリックリンクをたどらずに通常ファイルを開く

シンボリックリンクをたどる操作を拒否したい場合には、次の2つの関数を使います。

```text
fs.open_beneath(root: str, relative: str) -> Result<reader, Error>
fs.create_exclusive_beneath(root: str, relative: str) -> Result<writer, Error>
```

2番目の引数には、空でない相対パスを指定します。使えるパスの形式には制限があります。
どちらの関数も、起点と相対パスの全体を検証してからディレクトリのファイルディスクリプタを
保持して走査します。起点、途中のディレクトリ、末尾のいずれでもシンボリックリンクをたどりません。
`open_beneath` は、開く前後で同じ通常ファイルであることを確認し、所有権を持つ `reader` を
返します。この呼び出し自体はファイルの内容を読みません。`create_exclusive_beneath` は保持した
親ディレクトリ内に新しいファイルを1つ作り、所有権を持つ `writer` を返します。既存のエントリは変更しません。

見つからない場合は `Error.NotFound`、権限がない場合は `Error.Denied` です。許可されないパス形式、
シンボリックリンクやディレクトリでない途中要素、通常ファイルでない入力、開く前後のファイルの
入れ替わりは `Error.Invalid`、その他は共通の errno 変換規則に従います。カレントディレクトリの
変更やグローバルな起点の保持は行いません。ディレクトリハンドルや正規化済みパスを返す機能、
親ディレクトリの作成、ロールバック、永続性、トランザクションも提供しません。同じファイルの
読み書きを同期する仕組みもないため、作成と同時に開こうとすると、まだ存在しないと判定される
場合も、書き込み中の新しい通常ファイルを開ける場合もあります。

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

`read_file_view` はファイルをメモリにマップし、その内容への `str` ビューを返します。呼び出しには**外側の `arena` ブロックが必要です**。マッピングはアリーナの終了時に解除されるため、ビューを外へ持ち出すことはできません。文字列を残したい場合は `.clone()` でコピーします。第 [05](05-memory.md) 章のメモリモデルがそのまま適用されます。

`read_file_view` は `str` を返すため、内容が有効な UTF-8 かを検証します。GGUF モデルや検索インデックスなどのバイナリデータには `read_bytes_view` を使います。UTF-8 の検証をせずに同じアリーナ内でファイルをマップし、`bytes`（`slice<u8>`）のビューを返します。

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

`bytes` ビューもアリーナより長く保持することはできません。特殊ファイルや空のファイルでは、メモリマップの代わりにアリーナ内へデータをコピーします。マップ中のファイルが並行して切り詰められると、`SIGBUS` が発生する可能性があります。Align はプロセス全体に作用するシグナルハンドラを設置しません。

`bytes.clone()` はまだないため、アリーナの外へデータを残すには、ビューを保持する代わりにファイルや `buffer` へ書き出します。

## ストリーム: `reader`、`writer`、`buffer`

メモリに収まらないデータはストリームで処理します。第 [02](02-language-basics.md) 章の `loop` 式を使うと、次のようになります。

```align
import std.fs

fn pump(r: reader, w: writer) -> Result<(), Error> {
    mut buf := buffer(4096)
    loop {
        n := r.read(buf)?           // fill buf to capacity; 0 = EOF
        if n == 0 { break Ok(()) }  // break carries the loop's value out
        w.write(buf.bytes())?
    }
}

pub fn main(args: array<str>) -> Result<(), Error> {
    r := fs.open(args[1])?          // reader — owns the fd, closes on drop
    w := fs.create(args[2])?        // writer
    pump(r, w)?
    return Ok(())
}
```

読み込んだ内容をそのまま書き出すなら、`io.copy` で同じ処理を行えます。ファイルサイズによらず、一定量のメモリを使います。

```align
import std.io

pub fn main() -> Result<(), Error> {
    n := io.copy(io.stdin, io.stdout)?      // the whole of `cat`
    return Ok(())
}
```

`io.stdin`、`io.stdout`、`io.stderr` は標準ストリームを借用するハンドルです。細かい出力が多い場合は、`w := io.stdout.buffered()` でバッファ付き writer を作り、最後に `w.flush()?` を呼びます。

**所有権を持つハンドルは、メソッドを呼ぶ前にローカル変数へ束縛してください。** `fs.create(p)?.write(d)?` は拒否されるので、先にハンドルを変数へ格納します。借用された標準ストリームには `io.stdout.write("ok\n")?` のように直接呼び出せますが、`.buffered()` が返す writer には変数が必要です。これは現在のメソッドレシーバの制限です。名前のない Move 値を後始末する機能自体は実装されています。

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

各 API を使う際には、次の点にも注意してください。

- `path.base`、`path.dir`、`path.ext` は**入力文字列へのビュー**を返し、新しいメモリを確保しません。アリーナに属する文字列へのビューは、そのアリーナより長く保持できません。
- `env.get` は `Option` を返します。環境変数が未設定であることを、操作の失敗を表す `Result` と区別しています。
- 時間の間隔は `i64` のナノ秒で表します。`Duration` 型、単位を表す enum、変換 API はありません。経過時間には `instant()`、タイムスタンプには `now()` を使います。`i32` を渡すと型エラーになります。暗黙の型拡張は行われません（第 [02](02-language-basics.md) 章）。
- コマンドライン引数は `main(args: array<str>)` で受け取ります。`env.args` API はありません。
