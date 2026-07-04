このディレクトリには、ロードマップの本文には収まりきらない std モジュールの、Opus が実装できる粒度の
設計仕様を置いている。執筆はメインループ (Fable)。各モジュールを実装するときの source of truth である。

# std.crypto — implementation design (M11)

> 🌐 [English](../crypto.md) · **日本語**

## Overview

crypto.random、sha256/sha512、blake3、hmac、hkdf、argon2id、aes_gcm、chacha20_poly1305、
constant_time_equal(draft §18.2)。**譲れない要件: 秘密情報に依存する経路はすべて constant-time で
なければならない**(open-questions std.crypto — 秘密情報に依存する分岐やメモリインデックスは禁止。
CMOV/ビット演算のみ)。ここは、Align の分岐なし機構がパフォーマンス上の選択ではなく **正しさの要件** に
なる、唯一の領域である。

**戦略**: **検証済みのエンジンを借りる**。AEAD(aes_gcm、chacha20_poly1305)、ハッシュ(sha256/512、
blake3)、KDF(hkdf、argon2id)、hmac は、constant-time が監査済みの C ライブラリ(libsodium/BoringSSL
クラス)を FFI でラップする — 暗号を自前実装して constant-time を証明し直すよりも、そのライブラリの
constant-time 保証を継承するほうがはるかに安全だからだ。自前実装するのは `constant_time_equal` ただ一つ
(分岐なしのバイト差分 OR 縮約であり、Align の `where`/mask 機構に素直に乗るうえ、監査できるほど単純で
ある)。`crypto.random` は OS の CSPRNG(getrandom/getentropy — rand.seed のソースと同じものだが、ここでは
鍵材料向けに crypto グレードとして公開する)。

## Signatures

```text
crypto.random(out: mut buffer)                                  // fill with CSPRNG bytes
crypto.sha256(data: bytes) -> array<u8>    // 32-byte digest (fixed-size)
crypto.sha512(data: bytes) -> array<u8>
crypto.blake3(data: bytes) -> array<u8>
crypto.hmac_sha256(key: bytes, data: bytes) -> array<u8>
crypto.hkdf_sha256(salt: bytes, ikm: bytes, info: bytes, len: i64) -> Result<buffer, Error>
crypto.argon2id(password: bytes, salt: bytes, params: argon2_params) -> Result<buffer, Error>
crypto.aes_gcm_seal(key: bytes, nonce: bytes, plaintext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.aes_gcm_open(key: bytes, nonce: bytes, ciphertext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.chacha20_poly1305_seal(...) / _open(...)    // same shape as aes_gcm
crypto.constant_time_equal(a: bytes, b: bytes) -> bool          // CT — self-hosted
```

## Type & ownership classification

byte→byte、または byte→(所有権付き buffer か固定長 `array<u8>`)。新しい Move 型は要らない(buffer/array
を再利用する)。固定長のダイジェストは `array<u8>`(32/64)で表す。

## Effect classification

FFI でラップした演算は impure(extern 呼び出し)。`constant_time_equal` は pure だが、分岐なしを保ち続け
なければならない。

## Error policy

AEAD の open における認証失敗は `Error.Invalid`(タグの不一致か長さの不一致かを**決して**漏らさない —
単一の不透明な失敗にする)。KDF/argon のパラメータエラーは `Error.Invalid`。エンジンのエラーは
`Error.Code`。**重要**: `aes_gcm_open` は認証失敗時に部分的な平文を返しては**ならない** — 全か無か、かつ
不透明なエラーとする。

## New machinery required

暗号ライブラリへの FFI リンク(libsodium を推奨 — 単一の依存で、constant-time が監査済みであり、すべての
プリミティブをカバーする)。`constant_time_equal` の自前実装(分岐なし、早期 return なし)。OS の CSPRNG
の上に載せる `crypto.random`。そして Argon2 のパラメータ構造体。

## Slice breakdown

1. `constant_time_equal`(自前実装、分岐なし)+ `crypto.random`(OS の CSPRNG) — 外部依存なしで、CT の
   規律を検証する。
2. ハッシュ(sha256/512、blake3)を FFI 経由で。
3. hmac + hkdf。
4. AEAD(aes_gcm、chacha20_poly1305) — 全か無かの認証。
5. argon2id(KDF、設計上コストが高い)。

## Pitfalls

- **P1 (constant-time is CORRECTNESS)**: `constant_time_equal` には早期 return も、秘密情報に依存する分岐も
  あってはならない — 全長にわたってバイト差分を OR 縮約し、最後に一度だけ 0 チェックする。早期 break の
  ある `for` は、タイミングを通じて長さ/内容を漏らす。self-review では、秘密情報に依存する制御フローが
  無いことを検証しなければならない。これはこのモジュールを定義づける制約である。
  - **長さの扱い**: 入力の *長さ* は、意図された用途(MAC タグやダイジェストの比較 — 両側とも固定で、
    公開済みの長さ)においては秘密ではなく **公開情報** として扱う。したがって長さが違えば即座に `false`
    を返してよい。constant-time の保証は、**同じ長さ**の入力の *内容* に対して成り立つ。これは libsodium
    の `sodium_memcmp` の契約(長さが等しいことが事実上の前提条件)と一致する。呼び出し側が、長さ自体が
    隠れることを期待して秘密の長さの入力を渡すことのないよう、この点は明示的に文書化すること。
- **P2 (AEAD all-or-nothing)**: 認証失敗時の `open()` は `Error.Invalid` を返し、平文のバイトは**ゼロ**で
  なければならない — 部分的な平文も、区別のつくエラーも決して返さない。未検証の平文を外に出してしまうのは、
  AEAD の典型的な誤用である。
- **P3 (nonce reuse)**: 同じ鍵で nonce を再利用するのは(特に aes_gcm では)致命的であることを文書化する。
  v1 は nonce を自動生成しない(呼び出し側が渡す)が、文書には警告を書くこと。nonce を生成する補助関数は
  候補として記録しておく。nonce の長さ検証を必須にすることも検討する。
- **P4 (FFI memory safety, Gate 2)**: compress と同様に、FFI を越えるすべてのバッファに対して
  try_from/checked_mul/null ガードを適用する。
- **P5 (don't self-host the primitives)**: SHA/AES/argon を Align 内で再実装したくなるのを抑えること —
  constant-time と正しさを証明し直すのは、監査すべき面積が膨大になる。エンジンを借りる。自前実装するのは
  (自明な)`constant_time_equal` だけである。
- **P6 (key material zeroization)**: 鍵を保持する buffer は Drop 時にゼロクリアされるのが望ましい
  (zeroize-on-drop の buffer 亜種を用意するか、呼び出し側の責務として文書化する)。v1 の検討事項として
  記録する — buffer の Drop は現状ただ解放するだけだが、暗号の鍵に関しては理想としては先にゼロクリア
  すべきである。

## Test checklist

- sha256/512/blake3 を既知のテストベクタ(NIST/RFC)に照らして検証する
- hmac を RFC 4231 のベクタに照らして検証する
- hkdf を RFC 5869 に照らして検証する
- aes_gcm/chacha20 をそれぞれのテストベクタに照らして検証する
- タグを 1 ビット反転させた `aes_gcm_open` → `Error.Invalid` + ゼロの平文(P2)
- `constant_time_equal` の true/false、および(検査/監査により)早期 return が無いこと(P1)
- `crypto.random` が毎回異なるバイト列で埋めること
- argon2id の既知解答との照合
- import が必須であること
- (FFI のテストは libsodium の有無でゲートすること。)
