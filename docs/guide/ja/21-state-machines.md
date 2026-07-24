# 21. State machines

> 🌐 [English](../21-state-machines.md) · **日本語**

第19章で、私たちは「オブジェクト指向を忘れる」ことを学びました。隠蔽された `is_connected` フラグや、内部の可変状態を持つインスタンスを使うのをやめたのです。しかし、オブジェクトを使わずに、TCPコネクションやゲームのターン進行、UIコンポーネントのような「複雑なシステム」をどうやってモデル化すればよいのでしょうか？

Align では、**直和型（Sum Type）** と **ステートマシン（状態遷移機械）** を使って状態をモデル化します。

## 真偽値（Boolean）の罠

オブジェクト指向のコードでは、状態を複数のフィールドの組み合わせで表現することがよくあります。

```typescript
// アンチパターン
class Connection {
    isConnected: boolean;
    isAuthenticating: boolean;
    socketId: number | null;
    errorMessage: string | null;
}
```

この構造体は「ありえない状態（Invalid State）」を表現できてしまいます。もし `isConnected` が true なのに `socketId` が null だったらどういう意味でしょうか？ `isAuthenticating` と `errorMessage` の両方がセットされていたら？ コンパイラはここでは助けてくれません。これらのありえない状態が絶対に起きないことを保証するために、あなたは大量のテストを書かなければなりません。

## ありえない状態を、表現不可能にする

Align では直和型を使って、「どの状態が存在しうるか」を正確に定義し、その特定の状態に関連するペイロード（データ）だけをそこに持たせます。

```align
ConnectionState {
    Disconnected,
    Connecting,
    Authenticating(i64),   // socket
    Connected(i64, i64),   // socket, user_id
    Failed(i64),           // error code
}
```

これで、`Disconnected`（切断状態）のときに `user_id` を持つことは物理的に不可能になりました。ソケットを持たずに `Authenticating`（認証中）になることも不可能です。データの「形」が、ドメインの「現実」と完全に一致したのです。ペイロードは位置指定です。複数の値にドメイン上の名前を与えたいなら、`Connected(Session)` のように構造体へまとめます。借用テキストや対応済みの所有配列もペイロードにでき、第05章の通常のregion規則とMove規則がそのまま適用されます。

## 純粋関数としての「遷移」

OOP では、状態の遷移は内部のフィールドを書き換えるメソッド（`conn.connect()` など）を呼び出すことで起こります。Align では、遷移とは「現在の状態」と「イベント」を受け取り、「次の状態」を返す**純粋関数**です。

まず、起こりうるイベントを定義します。

```align
Event {
    Start,
    SocketOpened(i64),   // socket
    AuthSuccess(i64),    // user_id
    Failure(i64),        // error code
}
```

次に、遷移関数を書きます。`match` は一度に1つの値を検査するので、この関数は「表」として読めます。外側の `match` が行（状態）を選び、内側の `match` が列（イベント）を選ぶのです。

```align
fn next(state: ConnectionState, event: Event) -> ConnectionState {
    return match state {
        Disconnected => match event {
            Start => ConnectionState.Connecting,
            Failure(code) => ConnectionState.Failed(code),
            _ => state,
        },
        Connecting => match event {
            SocketOpened(s) => ConnectionState.Authenticating(s),
            Failure(code) => ConnectionState.Failed(code),
            _ => state,
        },
        Authenticating(s) => match event {
            AuthSuccess(user_id) => ConnectionState.Connected(s, user_id),
            Failure(code) => ConnectionState.Failed(code),
            _ => state,
        },
        // Connected と Failed は以降のイベントを無視する（エラーを返す設計も可）
        _ => state,
    }
}
```

## なぜこれが優れているのか

これが **有限ステートマシン（Finite State Machine）** です。状態をオブジェクトの中に隠すのをやめ、明示的なデータとして表現することで、いくつかのスーパーパワーを手に入れることができます。

1. **バグを未然に防ぐ:** `match` はすべてのバリアントをカバーしなければならず、アームの抜けはコンパイラが拒否します。「このイベントは無視する」という判断は、あなたが*意図して書いた*ワイルドカードであって、書き忘れたケースではありません。
2. **テストが容易:** このロジックをテストするのに、実際のソケットを立ち上げたりモックオブジェクトを作ったりする必要はありません。ただ `next(state, event)` を呼び出し、結果をアサートするだけです。
3. **データ指向との親和性:** 何千もの `ConnectionState` の配列をメモリ上に並べ、パイプラインで一括更新できます: `states.map(fn s { next(s, ev) }).to_array()`。

状態をオブジェクトの中に隠すのをやめたとき、あなたのシステムは「予測可能な遷移のパイプライン」へと生まれ変わるのです。
