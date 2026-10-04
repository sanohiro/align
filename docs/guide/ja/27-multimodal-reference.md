# 上限を明示したテキスト・音声・映像パイプライン

[`examples/multimodal/`](../../../examples/multimodal/) は、通常の Align で
書いた実行可能なアプリケーションです。HTTP、所有する JSON、構造化タスク、
保持したディレクトリ、`std.process.child_scope` を組み合わせます。
厳密な契約は [plan91](../../impl/91-multimodal-reference-composition.md) にあります。

固定の模擬テキスト、1秒のモノラル WAV 無音、16×16 の PPM 画像を生成し、
実際の FFmpeg で音声と映像を fragmented MP4 に結合します。所有権と転送の
参照実装です。モデル品質、音素アライメント、GPU に収まるか、GPU の解放は
実エンジンのアダプターで確認する必要があります。

## Linux または WSL2 で実行する

LLVM22 と FFmpeg/ffprobe を用意します。WSL2 は既存の child_scope の
カーネル機能検査を通る必要があります。管理プロセスを含む参照実装は、非対応の
カーネルと macOS ではワーカー開始前に拒否します。共通のソース・モデル検査は
macOS でも動作します。

リポジトリ直下でビルドします。制御用1ポートと配信用2ポートを、異なる明示的な
入力として渡します。配信ポートはそれぞれ1本の継続中の配信を扱います。
2本目の配信にはもう一方のポートを選びます。

```bash
scripts/cargo.sh build --workspace
./target/debug/alignc build examples/multimodal/main.align
./main selftest
root=$(mktemp -d)
"$(pwd)/main" serve "$root" 8080 8081 8082 /usr/bin/ffmpeg
```

ROOT は絶対パスで指定する、空の既存ディレクトリ（モード0700）です。
実行ファイルも絶対パスで起動します。終了要求までプロセスは動き続けます。

別の端末からパイプラインを投入します。

```bash
curl -sS http://127.0.0.1:8080/v1/jobs/pipeline \
  -H 'Content-Type: application/json' \
  -d '{"input":"A short scene","delay_ns":0}'
```

202 応答には `epoch`、`id`、`status:"queued"` が含まれます。
以下の EPOCH と ID にその値を入れます。

| 操作 | メソッドと経路 |
| --- | --- |
| 状態・成果物・音声メタデータ | 制御 GET `/v1/jobs/EPOCH/ID` |
| 進捗 SSE | 配信 GET `/v1/jobs/EPOCH/ID/events` |
| 完了した MP4 の取得 | 配信 GET `/v1/jobs/EPOCH/ID/artifacts/mux` |
| キャンセル | 制御 POST `/v1/jobs/EPOCH/ID/cancel`、空の本文 |
| ヘルス | 制御 GET `/health` |
| 正常終了 | 制御 POST `/shutdown`、空の本文 |

イベントには `curl -N`、成果物には `curl -o scene.mp4` を使います。
音声・テキスト・バッチの投入には対応するサフィックスを使います。
成果物のフェーズは `text`、`audio`、`batch`、`mux` です。
配信の `/v1/live/text` は直接 SSE、`/v1/live/pcm` は形式・レート・
チャンネル・フレーム数をヘッダーに明示した s16le 無音を送ります。
生 PCM と完成した WAV ファイルは区別します。

## 上限と所有権

一つの制御タスクが8個のスカラー SoA ジョブ枠、最大4件の待機ジョブ、
一つの管理フェーズを所有します。タスク間は認証付きでサイズを制限した
HTTP スナップショットを交換します。可変のジョブ表は共有しません。
全応答に明示的な書き込み時間制限があります。

出力は固定4KiB の作業領域と、ジョブ当たり最大1MiB の予約を使います。
成果物はファイルのまま保持します。完了済みジョブは3件を公開し、古いものを
退役させます。既存の読み手のリースがあれば、正確な ID の解放まで退役した
ファイルと予約を保持します。イベント切断はジョブをキャンセルしません。
JSON の None フィールドは省略し、不明な進捗率・未対応のアライメントも
省略します。模擬音声は既知の無音区間のみをサンプルフレームで報告し、
単語・音素の時刻を作りません。

各フェーズはパイプを読み、ネイティブのプロセス状態を確認し、child_scope を
明示的に解放してから公開・次フェーズ開始を行います。解放に失敗すると
Faulted の所有を保持し、次の起動を止めます。このプロセス不在の証明だけで
実エンジンのデバイスメモリ解放を証明することはできません。

正常終了は投入を止め、管理処理を解放し、配信タスクを join してから記録された
所有ファイルだけを削除し、最後に `controller.lock` を削除します。
異常終了ではフェンスを残します。残るプロセス・デバイスの所有を確認してから
手動で削除してください。ルートの終了や経過時間だけでは不十分です。

回帰検査は `scripts/cargo.sh test -p align_driver --test multimodal_reference`
です。Linux の統合検査は FFmpeg と ffprobe をインストールして実行します。
実際の結合、キャンセル、読み手の保持、遅れた解放、制御応答の停止、
生成器の異常を検査します。GPU 推論の動作確認は含みません。
