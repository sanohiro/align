# データベース: pkg.db の実践

> 🌐 [English](../24-database.md) · **日本語**

`pkg.db` は SQL を SQL のまま扱います。1つのクエリモジュールに、`.sql` ファイル、プレースホルダに対応する `Params` 構造体、結果の列に対応する `Row` 構造体を置きます。コンパイラは保存済みのスキーマメタデータを使い、データベースに接続せずに検査できます。実行時の引数のバインドと行のデコードは生成されたコードが行い、リフレクションや暗黙のステートメントキャッシュは使いません。

vendoring の手順は他のパッケージと同じです（第 [23](23-packages.md) 章）。[apps/db/pkg](../../../apps/db/pkg) をプロジェクトルートへコピーすれば、`pkg.db` に加えて `pkg.db.sqlite`、`pkg.db.postgres`、`pkg.db.pool` が使えます。以降の各節は、実際に必要になる作業1つに対応します。

## 最初のクエリ

```text
main.align
db/queries/user_by_id.align
db/queries/user_by_id.sql
pkg/db.align
pkg/db/…
```

```sql
SELECT id, name FROM users WHERE id = :id
```

```align
module db.queries.user_by_id

import pkg.db

pub Params { id: i64 }
pub Row { id: i64, name: str }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query_file([])
```

`query_file([])` は同じベース名の隣接する `.sql` を結び付けます。パス引数も glob もありません。SQL 中の `:name` はそれぞれちょうど1つの `Params` フィールドに対応し、`Row` は `SELECT` の順に並んだ結果カラムの厳密な契約です。短い SQL は `pkg.db.query("…", [])` でインラインにもできますが、ファイルのほうがレビューも整形も `EXPLAIN` も容易です。

```align
module main

import pkg.db
import pkg.db.sqlite
import db.queries.user_by_id

fn lookup(borrow connection: pkg.db.conn, id: i64) -> Result<string, pkg.db.Error> {
  q := db.queries.user_by_id.query()
  p := db.queries.user_by_id.Params { id: id }
  arena out {
    found := pkg.db.one(pkg.db.exec_conn(connection), q, p, out, [])?
    return Ok(found.name.clone())
  }
}

fn main() -> i32 {
  connection := pkg.db.sqlite.connect("app.db", []) else { return 1 }
  name := lookup(connection, 1) else { return 2 }
  print(name)
  return 0
}
```

呼び出し箇所で次の3つを指定しています。

- `pkg.db.exec_conn(connection)` は実行対象です。`pkg.db.exec_tx(transaction)` も同じ `pkg.db.exec` 型を返すので、トランザクションの内でも外でもクエリ側のコードは変わりません。
- `out` は行の文字列データをコピーする先のリージョンです。`one` が返す行はこの領域を参照するため、外に残す文字列は `.clone()` します。接続の `conn` は Move リソースで、drop 時に閉じられます（第 [05](05-memory.md) 章）。
- `[]` は実行オプションのスライスです。空なら、文書化された既定値を使います。

`one` はちょうど1行を要求します。0行でも2行でも `pkg.db.Error.Cardinality` が返り、どちらだったかは `observed_at_least` が示します。`maybe_one` も `all` も無いので、それ以外はすべてストリームで処理します。

`pkg.db.Error` は組み込みの `Error` とは別の直和型なので、失敗しうる `main` からそのまま返すことはできません（第 [04](04-errors.md) 章）。境界で `match` するか `map_err` します。

## 行数が多いとき

```align
fn total_ids(borrow connection: pkg.db.conn) -> Result<i64, pkg.db.Error> {
  q := db.queries.active_users.query()
  p := db.queries.active_users.Params { active: true }
  mut stream := pkg.db.rows(pkg.db.exec_conn(connection), q, p, [])?
  mut total := 0
  loop {
    row := pkg.db.next(stream)? else { break }
    total = total + row.id
  }
  return Ok(total)
}
```

`rows` は結果全体を実体化せず、一度だけ走査するストリームを返します。ストリームは Move リソースで、最後まで読み終えるか drop するまで接続を保持します。第 [02](02-language-basics.md) 章の `loop` と `else` アンラップで読み進めます。

列方向の処理には、1行ずつではなく上限付きのバッチを取ります。

```align
fn total_in_batches(target: pkg.db.exec) -> Result<i64, pkg.db.Error> {
  q := db.queries.user_ids.query()
  mut stream := pkg.db.rows(target, q, db.queries.user_ids.Params { min_id: 0 }, [])?
  mut total := 0
  loop {
    chunk := pkg.db.next_batch(stream, 64)? else { break }
    columns := pkg.db.batch_soa(chunk)?
    total = total + columns.id.sum()
  }
  return Ok(total)
}
```

`next_batch` は最大 `max_rows` 行を、独立して所有する1つの列指向バッチに読み込みます。`batch_len` と `batch_row` で長さと各行を取得し、`batch_soa` で第 [11](11-data-oriented.md) 章の `soa<R>` として参照できます。呼び出し側が上限を決めるので、メモリ使用量を制限できます。

実行のデッドラインには `pkg.db.ExecuteOption.TimeoutNs(ns)` を使います。PostgreSQL はノンブロッキングの待機とキャンセルで期限を守り、期限切れは `pkg.db.Error.Timeout`、データベース側のキャンセルは `Cancelled` を返します。SQLite はこのオプションに対応せず、SQL を送る前に `db.execute.timeout_ns` を示す `Unsupported` を返します。SQLite 用の `pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(ns)` は `pkg.db.sqlite.rows_native` で渡せますが、制限するのはロック待ちだけで、クエリ全体の実行時間ではありません。

## 書き込みを安全に

行を返さない文は `pkg.db.command<P>` です。

```align
pub fn command() -> pkg.db.command<Params> = pkg.db.command_file([])
```

```align
outcome := pkg.db.execute(target, command(), params, [])?
affected := outcome.rows_affected else { return Ok(-1) }
```

件数を報告しない文もあるため、`rows_affected` は `Option<i64>` です。`RETURNING` を伴う DML は `command` ではなく `query` になります。

トランザクションは次の3操作で接続の所有権を移します。

```align
fn seed(connection: pkg.db.conn) -> Result<pkg.db.conn, pkg.db.Error> {
  transaction := pkg.db.begin(connection, [])?
  attempt := add(pkg.db.exec_tx(transaction), 1, "ada")
  match attempt {
    Ok(_) => {}
    Err(failure) => {
      _ := pkg.db.rollback(transaction)?
      return Err(failure)
    }
  }
  return pkg.db.commit(transaction)
}
```

`begin` は `conn` を消費して `tx` を作り、`commit` と `rollback` は `tx` を消費して `conn` を返します。この例のエラー分岐で `?` の代わりに `match` を使うのは、接続を受け取り直すためです。トランザクション内で `?` を使っても、`tx` の drop でロールバックするため安全です。ただし、早期 return すると接続も手放します。接続を取り戻す必要があれば `match`、なければ `?` を使えます。

`[]` のトランザクションオプションは、SQLite では `DEFERRED`、PostgreSQL では `READ COMMITTED READ WRITE` を意味します。より強いモードはドライバの `begin_native` を通します。共通オプションのスライスにネイティブオプションのスライスを加えた形で、`pkg.db.sqlite.TxOption.Immediate` や `pkg.db.postgres.TxOption.Isolation(pkg.db.postgres.Isolation.Serializable)` を渡します。透過的なリトライはありません。`Serialization` や `Deadlock` を `match` して、見える形で再試行します。

## スキーマを変える

マイグレーションは `NNNN_snake_name.sql` という名前の素の SQL ファイルで、バージョンは連番です。

```text
db/migrations/0001_create_users.sql
db/migrations/0002_create_groups.sql
```

```text
alignc db migrate --entry main.align --migrations db/migrations --driver sqlite --sqlite-path dev.sqlite
alignc db status  --entry main.align --migrations db/migrations --driver sqlite --sqlite-path dev.sqlite
alignc db check   --entry main.align --migrations db/migrations --driver sqlite --sqlite-path dev.sqlite
```

PostgreSQL では対象を `--postgres-url-env NAME` に置き換えます。コマンドラインに載るのは環境変数の*名前*だけで、URL とそのパスワードは決して載りません。各コマンドはマイグレーション1件につき1行と、サマリ（`applied=1 pending=0 dirty=0 mismatched=0 history_only=0`）を出力します。`status` は報告するだけ、`check` はさらに、実際の状態がカタログと厳密に一致しなければ失敗します。CI のゲートはこちらです。

トランザクションポリシーはファイルごとに1つです。既定は all-or-nothing で、先頭行の `-- align:migration transaction=forbidden` は、同時実行のインデックス構築のように、トランザクション外で動かすほかない単一文であることを示します。forbidden なマイグレーションが途中で中断されると **dirty** な履歴行が残り、以降のマイグレーションをすべて止めます。その解消は意図的に手動で、`alignc db repair … --version N --accept-applied|--clear-dirty --expect-checksum HASH` を使います。その文が効いたかどうかをツールが推測することはありません。マイグレーションファイルに `BEGIN`/`COMMIT`/`ROLLBACK`/`SAVEPOINT` は書けません。境界を所有するのはランナーです。

## スキーマをコンパイル時に検査させる

コンパイル時のスキーマ検査に使うメタデータは、`alignc db prepare` で作成します。このコマンドがデータベースに接続し、エントリから到達する各クエリの情報を取得して、決定的な形式で `.align-db/` 以下に保存します。

```text
alignc db prepare main.align --driver sqlite --database dev.sqlite --schema-id dev-v1
alignc db prepare main.align --driver sqlite --memory --migrations db/migrations
alignc db prepare main.align --driver postgres --url-env ALIGN_DB_URL --schema-id dev-v1
alignc db prepare main.align --driver postgres --url-env ALIGN_DB_URL --schema-id dev-v1 --check
```

`--memory` 形式はマイグレーションカタログから使い捨ての SQLite データベースを組み立てるので、検査済みビルドに稼働中のサーバーは要りません。`--check` は何も再生成せず、コミット済みメタデータが古ければ非ゼロ終了します。これも CI ゲートの1つです。通常の `alignc build` がデータベースを開くことはなく、読むのはコミット済みメタデータだけです。

検査を要求するかどうかはクエリ単位のオプションです。

```align
pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query_file(
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
)
```

既定の `DeclaredOnly` は、SQL と構造体の形式だけを検査します。`CheckedOptional` は、ドライバごとに有効なメタデータがあれば使います。`CheckedRequired` は、そのクエリが許可するすべてのドライバでメタデータを要求し、欠落や古い内容をビルドエラーにします。既定では両方を許可するため、SQLite だけ prepare すると `checked metadata for PostgreSQL is stale: checked metadata is missing` で失敗します。片方しか使わないなら `pkg.db.sqlite.query_file(options, native_options)`、または PostgreSQL 版でドライバを限定してください。

実行時にも `pkg.db.meta_tables`、`meta_columns`、`meta_keys`、`meta_indexes` でカタログを調べ、`pkg.db.explain` でクエリプランを確認できます。これらは読み取り専用で、マイグレーションは行いません。

## SQLite と PostgreSQL の両方に対応する

`query` / `command`、`execute`、`one`、`rows` / `next`、`next_batch`、`prepare` / `rows_stmt`、`begin` / `commit` / `rollback`、`pkg.db.Error` は両エンジンで共通です。共通の SQL は `:name` プレースホルダを使い、各ドライバが自分のプロトコル形式へ変換します。エンジン固有の操作はモジュール名で区別します。

```align
sqlite := pkg.db.sqlite.connect("app.db", [
  pkg.db.sqlite.ConnectOption.Create,
  pkg.db.sqlite.ConnectOption.Pragma("journal_mode", "WAL"),
])?

postgres := pkg.db.postgres.connect(url, [
  pkg.db.postgres.ConnectOption.ApplicationName("align-guide"),
  pkg.db.postgres.ConnectOption.ConnectTimeoutNs(5000000000),
])?
```

`*_native` の実行関数はドライバオプションのスライスを1つ追加で取るだけで、他は何も変わりません。

```align
mut stream := pkg.db.postgres.rows_native(
  pkg.db.exec_conn(connection),
  db.queries.user_ids.query(),
  db.queries.user_ids.Params { min_id: 0 },
  [],
  [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(64))],
)?
```

`Delivery` は libpq が行を渡す方式の選択です。既定は結果全体をバッファし、`SingleRow` は1行ずつ流し、`PortalBatch(n)` は固定サイズで取り出します。変わるのはメモリの挙動だけで、書くループは同じです。要求したオプションが黙って無視されることはありません。対応していないものは、対象を名指しした `Unsupported` エラーになります。

## 接続を使い回す

`pkg.db.pool` は固定容量・待機なしのプールです。接続はすべて事前に開くので、取得時にネットワーク・ファイルシステム・認証の作業は発生しません。

```align
owner := pkg.db.pool.open_sqlite("app.db", 8, [])?
connection := pkg.db.pool.try_acquire(owner)?
snapshot := pkg.db.pool.info(owner)?
```

`try_acquire` は待機せず、空きがなければ即座に `pkg.db.Error.PoolExhausted` を返します。再試行するか、要求を断るかは呼び出し側で決めます。取得する値は通常の `pkg.db.conn` なので、クエリ、トランザクション、プリペアドステートメントをそのまま使え、drop するとプールに戻ります。容量は `1..=1024` の範囲で固定です。drop 時に未完了のトランザクションがないと確認できない接続は閉じ、スロットも廃棄します。`info` の `capacity` / `idle` / `checked_out` で状態を確認できます。利用者が変わってもセッションはリセットされず、`PRAGMA` や `SET` はその物理接続に残ります。

## どうしても動的な SQL が要るとき

SQL が実行時に決まる場合は、動的 API を使います。結果は静的に検査された `Row` 型ではなく、`pkg.db.value` で受け取ります。

```align
fn counted(borrow connection: pkg.db.conn, out: region) -> Result<i64, pkg.db.Error> {
  params := [pkg.db.value.Bool(true)]
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT count(*) FROM users WHERE active = ?1", params[..], [],
  )?
  first := pkg.db.dynamic_next(stream, out)? else { return Ok(0) }
  return match first.values[0] {
    I64(total) => Ok(total)
    _ => Ok(-1)
  }
}
```

ドライバは引数で指定し、実行ハンドルと一致しなければ SQL を送る前に `DriverMismatch` を返します。プレースホルダは書き換えないので、SQLite なら `?1`、PostgreSQL なら `$1` を使います。引数は `pkg.db.value` で渡し、行はサーバーの列順に並んだ `array<pkg.db.value>` で受け取ります。結果は指定したリージョンにコピーされ、添字で参照できます。列名のテーブルや構造体へのリフレクションによるデコードはありません。行を返さない場合は `pkg.db.dynamic_execute` を使います。

識別子は依然としてバインドできません。名前付きの静的クエリ2つを分岐で使い分けるのが正攻法で、識別子を SQL に文字列連結するのはサポート対象外です。

## SQLite のスカラー関数を登録する

SQLite のスカラー関数には、キャプチャを持たず、コンパイラが検査したコールバックを登録します。

```align
fn twice(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  if args.values.len() != 1 { return Err("twice expects one argument") }
  return match args.values[0] {
    I64(value) => Ok(pkg.db.value.I64(value * 2))
    _ => Err("twice expects an i64")
  }
}

fn install(borrow mut connection: pkg.db.conn) -> Result<(), pkg.db.Error> {
  callback := pkg.db.sqlite.function(twice)
  return pkg.db.sqlite.register_function(
    connection, "align_twice", 1, callback,
    [pkg.db.sqlite.FunctionOption.Deterministic],
  )
}
```

登録する関数の引数の数は固定で、その接続内だけで有効です。SQLite 3.30 以降が必要で、常に `DIRECTONLY` が付きます。`Deterministic` を指定できるのは、コンパイラが Pure と判定したコールバックだけです。登録には、使用中の処理がない直接の SQLite 接続が必要です。コールバックの状態が再利用されるプールのスロットに残らないよう、プールから取得した接続は拒否します。`remove_function` は同じ名前と引数の数を持つ関数を削除します。コールバックの失敗は SQLite の関数エラーになり、言語のハードエラーはプロセスを終了します。

## まだ無いもの

- `maybe_one` も `all` もありません。`one` かストリームを使います。
- デッドラインは PostgreSQL の実行だけに対応します。`PrepareOption.TimeoutNs` と `TxOption.BeginTimeoutNs` は、v1 では両ドライバとも `Unsupported` を返します。
- プリペアドステートメントは1つの接続に属し、プール内の別接続へ移ることはありません。ステートメントキャッシュはグローバルにもローカルにも存在しません。
- プールは待機も再接続もヘルスチェックもセッションのリセットも行いません。
- 動的 SQL に prepare 形式や一括実体化の形式はありません。
- SQLite の照合順序、busy handler、拡張ローダー、PostgreSQL のコールバックはまだありません。具体的な利用者の要件に基づき、安全性や永続化を別途設計する必要があります。

契約の正本は `docs/impl/pkg-design/db.md` です。[apps/db](../../../apps/db) はパッケージ作者用ワークスペースで、`app/user_groups.align` に一対多の結果を組み立てる例があります。1本のクエリを順番に1回走査し、子のコレクションごとに1つの `array_builder` で蓄積します。
