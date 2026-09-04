> 🌐 [English](../25-vector-search.md) · **日本語**

# 25 — `pkg.db` を通じたベクトル検索

データベースがベクトルをテキストとして受け取り、`pkg.db` が扱える型の列を返すなら、
`pkg.db` からベクトル検索を使えます。アプリケーション側で埋め込みベクトルを生成し、
`Params` と `Row` を定義します。距離の計算、絞り込み、インデックスの使用、調整は各クエリの
SQL で指定します。データベース間で共通のベクトル API はありません。

この方法は PostgreSQL 16 と pgvector 0.8.6 で検証済みです。拡張は `CREATE EXTENSION vector`
で有効にしてください。`pkg.db` や `alignc` が拡張をインストール・更新することはありません。

```align
module app.search
import pkg.db
import pkg.db.postgres

pub Params {
  embedding: str,
  category: str,
}

pub Row {
  id: i64,
  label: str,
  distance: f64,
}

pub fn nearest() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT id, label, embedding <-> CAST(:embedding AS text)::vector(3) AS distance FROM items WHERE category = CAST(:category AS text) ORDER BY distance, id LIMIT 10",
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [],
)
```

アプリケーションが渡したベクトルのテキストを、サーバーが解析します。既存の文字列のバインドを
利用する方法であり、ベクトルをゼロコピーで送るものではありません。このクエリ定義は
`pkg.db.rows`、`pkg.db.prepare` と `pkg.db.rows_stmt` によるプリペアドステートメント、
検査用メタデータ、`pkg.db.meta_query`、`pkg.db.explain` で使えます。PostgreSQL の検査用
メタデータには、拡張のスキーマ、名前、バージョンを記録します。pgvector がなければ prepare は
失敗し、バージョンが変われば記録されるスキーマの識別情報も変わります。

## データベースごとの違い

次の表は、各データベースのベクトル機能を比較したものです。`pkg.db` がすべての製品に対応して
いるという意味ではありません。本番のバージョンやインデックスの調整を決めるときは、リンク先の
公式文書を確認してください。

| データベース | 格納形式と提供条件 | 厳密検索と距離 | 近似インデックス・検索 | 設定する場所 |
|---|---|---|---|---|
| PostgreSQL + [pgvector 0.8.6](https://github.com/pgvector/pgvector/tree/v0.8.6) | 別途インストールする拡張型 `vector(N)`。結合テストでは公式の `pgvector/pgvector:0.8.6-pg16-bookworm` イメージを指定。 | インデックスなしで `<->`（L2）、`<#>`（負の内積）、`<=>`（コサイン）、`<+>`（L1）で並べると厳密検索になる。 | HNSW と IVFFlat の演算子クラス別インデックス。クエリの演算子とインデックスを一致させる。 | HNSW の構築・検索、IVFFlat の lists/probes は拡張の SQL・設定で指定する。 |
| SQLite + [vec1 0.7](https://sqlite.org/vec1/doc/version-0.7/doc/vec1.md) | 別途ビルドする仮想テーブル拡張。ベクトルは有限の IEEE-754 `f32` をマシンのバイト順で詰めた BLOB。SQLite の組み込み型ではない。 | `none` と `flat` は完全なベクトルを走査する。`vec1_l2_distance` は二乗 L2、`vec1_cos_distance` は文書で定義されたコサイン距離を使う。JSON 変換は SQL で明示する。 | 学習済みの IVFADC モデルと PQ/OPQ/BQ 圧縮。最近傍検索では仮想テーブルをテーブル値関数として使う。 | 学習、バケット、量子化器、コードサイズ、残差、`nprobe`、スレッド数は vec1 の設定で指定する。 |
| MySQL Community [9.7](https://dev.mysql.com/doc/refman/9.7/en/vector.html) と HeatWave | Community は `f32` 要素の組み込み `VECTOR(N)` 型を持つが、`VECTOR` 列はキーにできない。 | Community は `DISTANCE` を提供しない。[関数リファレンス](https://dev.mysql.com/doc/refman/9.7/en/vector-functions.html) では、コサイン・内積・ユークリッドの `DISTANCE` は HeatWave on OCI と MySQL AI に限定される。 | Community はベクトルインデックスや検索の機能を提供しない。[HeatWave](https://dev.mysql.com/doc/heatwave/en/mys-hw-genai-vector-index-creation.html) では、対象の処理に HNSW インデックスを自動生成する。 | 距離の計算、インデックス作成、メモリ上限、構築タイムアウト、HNSW 検索の制御は HeatWave のサービス設定で行い、Community と共通の SQL オプションではない。 |
| [MariaDB 11.7.1+](https://mariadb.com/docs/server/reference/sql-structure/vectors/vector-overview) | テキスト変換関数を伴う組み込みの `VECTOR(N)` 型。 | `VEC_DISTANCE` はユークリッド距離かコサイン距離。インデックスなしの評価は厳密。 | `VECTOR INDEX` は HNSW の改変版を使い、対応する `ORDER BY ... LIMIT` を高速化する。 | `M`、既定の距離、`mhnsw_ef_search`、キャッシュ上限は MariaDB のインデックス・セッション・サーバーで設定する。 |
| [SQL Server 2025 (17.x)](https://learn.microsoft.com/en-us/sql/sql-server/ai/vectors?view=sql-server-ver17) | `f32` のバイナリ表現を持ち、JSON 配列として公開する組み込み `VECTOR(N)` 型。Azure SQL の製品ごとに提供状況が異なる。 | `VECTOR_DISTANCE` は常に厳密で、コサイン、ユークリッド、負の内積に対応。 | `CREATE VECTOR INDEX` と `VECTOR_SEARCH` は DiskANN を使う。製品によっては、公式文書でインデックス・検索機能がプレビュー扱いとされている。 | 距離、インデックス構築の並列度、プレビューの有効化、検索オプションは T-SQL やサービスで設定する。 |
| [Oracle AI Database 26ai](https://docs.oracle.com/en/database/oracle/oracle-database/26/vecse/vector_distance.html) | 要素形式と次元を Oracle SQL で宣言する、組み込みの `VECTOR` 型。 | `VECTOR_DISTANCE` と短縮形の関数は、適用可能なコサイン、内積、ユークリッド、二乗ユークリッド、マンハッタン、ハミング、ジャッカードに対応する。`FETCH EXACT` は全件走査を指定する。 | HNSW と IVF のベクトルインデックス。クエリとインデックスの距離の種類が異なると、厳密検索に切り替える。 | 目標精度、ベクトルプール、近傍パーティション、グラフ・インデックス、距離の選択は Oracle の DDL やセッションで設定する。 |

共通して使えるのは、埋め込みベクトルを `Params` に、識別子・ペイロード・距離のスカラー値を
`Row` に持たせるという構成です。クエリ自体はデータベースごとにドライバを限定して定義します。
`pkg.db` は、埋め込みの生成、ネイティブ拡張の読み込み、ベクトル SQL の書き換え、インデックス
の選択、製品間の距離値の正規化を行いません。

## SQLite vec1 を `pkg.db` から使えない理由

x86-64 Linux の別環境で、sqlite.org の `version-0.7` ソースを使った検証も行いました。
`vec1.c` を読み込み可能な拡張としてビルドし、SQLite CLI に読み込んだ結果は次のとおりです。

```text
vec1_info()                                      version 0.7 (Scalar, multi-threaded)
hex(vec1_from_json('[1,2,3]'))                   0000803F0000004000004040
vec1_l2_distance([1,2,3], [2,2,3])              1.0
flat L2 top-2 with category = 'keep'             row 1: 0.0; row 2: 1.0
```

この検証環境では、バイト列はリトルエンディアンの `f32` です。vec1 はマシンのバイト順を使う
ため、このまま異なる環境に持ち運べる保存形式ではありません。拡張を読み込んだのは `pkg.db` の
外です。Align の SQLite 接続は拡張の読み込みを無効にしているため、この検証だけで Align が
SQLite のベクトル検索に対応したことにはなりません。vec1 の同梱や静的リンクには、依存元、
各環境でのビルド、初期化、マイグレーション、公開 API を含む別の設計が必要です。

ネイティブのベクトル値を直接バインドする機能も今後の課題です。型の識別、次元、有限値、
エンコーディング、所有権、メモリ確保、エラーの優先順位、行の有効期間をドライバごとに定める
必要があります。SQL の名前が似ていても、ベクトルの表現が共通とは限りません。
