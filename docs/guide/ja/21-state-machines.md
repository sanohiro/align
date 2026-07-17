# 21. State machines

> 🌐 [English](../21-state-machines.md) · **日本語**

第19章で、私たちは「オブジェクト指向を忘れる」ことを学びました。隠蔽された `is_connected` フラグや、内部の可変状態を持つインスタンスを使うのをやめたのです。しかし、オブジェクトを使わずに、TCPコネクションやゲームのターン進行、UIコンポーネントのような「複雑なシステム」をどうやってモデル化すればよいのでしょうか？

Align では、**Sum Type（`enum`）** と **ステートマシン（状態遷移機械）** を使って状態をモデル化します。

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

この構造体は「ありえない状態（Invalid State）」を表現できてしまいます。もし `isConnected` が true なのに `socketId` が null だったらどういう意味でしょうか？ `isAuthenticating` と `errorMessage` の両方が true だったら？ コンパイラはここでは助けてくれません。これらのありえない状態が絶対に起きないことを保証するために、あなたは大量のテストを書かなければなりません。

## ありえない状態を、表現不可能にする

Align では `enum` を使って、「どの状態が存在しうるか」を正確に定義し、その特定の状態に関連するペイロード（データ）だけをそこに持たせます。

```align
ConnectionState {
    Disconnected,
    Connecting(url: string),
    Authenticating(socket: i64),
    Connected(socket: i64, user_id: i64),
    Failed(reason: string),
}
```

これで、`Disconnected`（切断状態）のときに `user_id` を持つことは物理的に不可能になりました。`socket` を持たずに `Authenticating`（認証中）になることも不可能です。データの「形」が、ドメインの「現実」と完全に一致したのです。

## 純粋関数としての「遷移」

OOP では、状態の遷移は内部のフィールドを書き換えるメソッド（`conn.connect()` など）を呼び出すことで起こります。Align では、遷移とは「現在の状態」と「イベント」を受け取り、「次の状態」を返す**純粋関数**です。

まず、起こりうるイベントを定義します。

```align
Event {
    Start(url: string),
    SocketOpened(socket: i64),
    AuthSuccess(user_id: i64),
    Error(reason: string),
}
```

次に、遷移関数を書きます。これは単に現在の状態とイベントを `match` するだけです。

```align
fn next(state: ConnectionState, event: Event) -> ConnectionState {
    match (state, event) {
        // 正常系
        (Disconnected, Start(url)) => 
            ConnectionState.Connecting(url),
            
        (Connecting(url), SocketOpened(socket)) => 
            ConnectionState.Authenticating(socket),
            
        (Authenticating(socket), AuthSuccess(user_id)) => 
            ConnectionState.Connected(socket, user_id),
            
        // エラー処理
        (_, Error(reason)) => 
            ConnectionState.Failed(reason),
            
        // 無効な遷移は無視する（あるいはエラーを返してもよい）
        _ => state
    }
}
```

## なぜこれが優れているのか

これが **有限オートマトン（Finite State Machine）** です。状態をオブジェクトの中に隠すのをやめ、明示的なデータとして表現することで、いくつかのスーパーパワーを手に入れることができます。

1. **バグを未然に防ぐ:** コンパイラは、状態とイベントのすべての組み合わせが処理されていること（網羅性）をチェックしてくれます。
2. **テストが容易:** このロジックをテストするのに、実際のソケットを立ち上げたりモックオブジェクトを作ったりする必要はありません。ただ `next(state, event)` を呼び出し、結果をアサートするだけです。
3. **データ指向との親和性:** 何千もの `ConnectionState` の配列（あるいは SoA）をメモリ上に並べ、パイプラインを使って一括で更新することができます: `states.map(fn s { next(s, ev) })`。

状態をオブジェクトの中に隠すのをやめたとき、あなたのシステムは「予測可能な遷移のパイプライン」へと生まれ変わるのです。
