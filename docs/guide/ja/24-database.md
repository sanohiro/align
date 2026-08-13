# データベース: pkg.db の実践

> 🌐 [English](../24-database.md) · **日本語**

`pkg.db` は SQL を SQL のまま扱います。クエリは小さなモジュール1つです。`.sql` ファイル、プレースホルダに対応する `Params` 構造体、結果カラムに対応する `Row` 構造体。その SQL は実スキーマに対してオフラインで検査でき、実行時のパラメータバインドと行デコードは生成コードが行います。リフレクションも、隠れたステートメントキャッシュもありません。

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

呼び出し箇所には3つのものが見えたまま残ります。

- `pkg.db.exec_conn(connection)` は実行対象です。`pkg.db.exec_tx(transaction)` も同じ `pkg.db.exec` 型を返すので、トランザクションの内でも外でもクエリ側のコードは変わりません。
- `out` は行の文字列ビューをクローンする先です。`one` はこの region に実体化し、行はそれより長くは生きられないので、外へ持ち出すものは `.clone()` します。`conn` も同様に、drop 時に自分で閉じる Move リソースです（第 [05](05-memory.md) 章）。忘れうる `close()` はありません。
- `[]` は実行オプションのスライスです。空は「文書化された既定」であって、推論された既定ではありません。

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

`rows` は暗黙の実体化を伴わない1パスです。ストリームは Move リソースで、尽きるか drop されるまで接続を握り続けます。`loop` と `else` アンラップの形は第 [02](02-language-basics.md) 章そのままで、これを隠すためのコールバック ABI をパッケージは持ち込みません。

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

`next_batch` は最大 `max_rows` 行を、独立して所有される列指向のバッチ1つに実体化します。`batch_len` と `batch_row` で添字アクセスでき、`batch_soa` は第 [11](11-data-oriented.md) 章の `soa<R>` として射影します。上限を決めるのは呼び出し側なので、メモリコストが目に見えます。

デッドラインは共通オプション `pkg.db.ExecuteOption.TimeoutNs(ns)` で、ドライバの対応状況について正直です。PostgreSQL はノンブロッキング待機とネイティブなキャンセルで実際に強制し、期限切れは `pkg.db.Error.Timeout`、エンジン側都合のキャンセルは `Cancelled` に対応付けます。SQLite は受け取って無視するのではなく、SQL を送る前に `db.execute.timeout_ns` を名指しした `Unsupported` で拒否します。SQLite 側の `pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(ns)`（`pkg.db.sqlite.rows_native` 経由）が制御するのはロック待ちだけで、クエリ全体のデッドラインだとは意図的に称しません。

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

トランザクションは、名前の付いた3ステップで接続を移動させます。

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

`begin` は `conn` を消費して `tx` を作り、`commit` と `rollback` は `tx` を消費して `conn` を返します。失敗側の腕が `?` ではなく `match` なのはそのためです。トランザクションの中で `?` を使うこと自体は*安全*で、`tx` が drop されればフェイルセーフなロールバックが走ります。ただし早期 return は接続ごと手放します。呼び出し元が接続を受け取り直す必要があるなら `match`、その必要がないなら `?` をそのまま使ってかまいません。

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

データベースに接続するワークフローは `alignc db prepare` だけです。エントリから到達できるすべてのクエリをエンジンに記述させ、決定的なメタデータを `.align-db/` 以下へ書き出します。

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

既定の `DeclaredOnly` は、SQL と構造体が well-formed であることだけを見ます。`CheckedOptional` はドライバごとに、現存する最新メタデータがあればそれを使います。`CheckedRequired` はメタデータの欠落や陳腐化をビルドエラーにします。しかもそのクエリが許すすべてのドライバ分を要求し、既定では両方なので、SQLite だけ prepare した状態では `checked metadata for PostgreSQL is stale: checked metadata is missing` で失敗します。片方のエンジンしか実在しないなら、`pkg.db.sqlite.query_file(options, native_options)` か PostgreSQL 版でクエリを固定します。

同じ読み取り専用のカタログ面は実行時にも使えます。`pkg.db.meta_tables`、`meta_columns`、`meta_keys`、`meta_indexes`、そしてクエリプラン用の `pkg.db.explain` です。これらは調べるだけで、移行はしません。

## SQLite と PostgreSQL の両方に対応する

共通面 —— `query`/`command`、`execute`、`one`、`rows`/`next`、`next_batch`、`prepare`/`rows_stmt`、`begin`/`commit`/`rollback`、`pkg.db.Error` —— は両エンジンで同一で、ポータブルな SQL は `:name` プレースホルダを使い、各ドライバがそれぞれのプロトコル形式へ落とします。エンジン固有のものはすべて修飾されるので、レビューで一目で分かります。

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

`try_acquire` はブロックもスリープもしません。空きがなければ即座に `pkg.db.Error.PoolExhausted` を返し、バックプレッシャーの取り方は呼び出し側の判断のままです。取得できる値はごく普通の `pkg.db.conn` で、クエリもトランザクションもプリペアドステートメントもそのまま動きます。プールへ返すのはその drop です。容量は `1..=1024` で固定です。drop 時にトランザクションが idle だと証明できない接続は、黙って再利用されるのではなく閉じられ、そのスロットは廃棄されます。それは `info` の `capacity`／`idle`／`checked_out` で見えます。セッションは利用者間でリセットされません。適用した `PRAGMA` や `SET` は、その物理接続に残ります。

## どうしても動的な SQL が要るとき

脱出口は明示的で、弱く、名前が付いています。

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

ドライバは推論ではなく引数です。実行ハンドルと突き合わされ、食い違えば SQL を送る前に `DriverMismatch` で失敗します。ここではプレースホルダの書き換えを行わないので、SQL はそのエンジンの方言そのものです。SQLite なら `?1`、PostgreSQL なら `$1` を書きます。値は `pkg.db.value` として渡し、サーバーのカラム順どおりに添字アクセスできる `array<pkg.db.value>` として、指定した region にコピーされて返ります。カラム名のテーブルも、構造体へのリフレクティブなデコードもありません。行を返さない形は `pkg.db.dynamic_execute` です。

識別子は依然としてバインドできません。名前付きの静的クエリ2つを分岐で使い分けるのが正攻法で、識別子を SQL に文字列連結するのはサポート対象外です。

## SQLite scalar function を登録する

SQLite scalar function は raw function pointer や保持される closure environment ではなく、コンパイラが証明した noncapturing callback を使います。

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

登録は固定 arity、接続ローカル、SQLite 3.30 以降で、常に `DIRECTONLY` です。`Deterministic` は callback が Pure だとコンパイラが証明した場合だけ受理されます。idle な direct SQLite connection が必要で、callback state が再利用される pool slot を追わないよう pooled connection は拒否されます。`remove_function` は同じ name/arity pair を削除します。callback の失敗は SQLite function error になり、言語の hard error は従来どおり process を終了します。

## まだ無いもの

- `maybe_one` も `all` もありません。`one` かストリームを使います。
- デッドラインは実行時のみ、しかも PostgreSQL のみです。`PrepareOption.TimeoutNs` と `TxOption.BeginTimeoutNs` は v1 ではどちらのドライバも受け付けず、できるふりをせず `Unsupported` で拒否します。
- プリペアドステートメントは1つの接続に属し、プール内の別接続へ移ることはありません。ステートメントキャッシュはグローバルにもローカルにも存在しません。
- プールは待機も再接続もヘルスチェックもセッションのリセットも行いません。
- 動的 SQL に prepare 形式や一括実体化の形式はありません。
- 出荷済みの SQLite scalar function 以外に、SQLite collation、busy handler、extension loader、PostgreSQL callback surface はありません。それぞれ固有の安全性または永続化契約が必要なため、consumer-gated のままです。

契約の正本は `docs/impl/pkg-design/db.md` です。[apps/db](../../../apps/db) はパッケージ作者用ワークスペースで、その `app/user_groups.align` は一対多シェイピングの実例です。クエリ1本、順序付きの1パス、子コレクション1つにつき `array_builder` 1つ、という形になっています。
