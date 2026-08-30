このディレクトリには、ロードマップの本文ではカバーしきれない `std` モジュールについて、Opus がそのまま実装に着手できる粒度の設計仕様を収めている。執筆はメインループ（Fable）が担当しており、各モジュールの実装においてこれが信頼できる情報源（source of truth）となる。

# std.crypto — implementation design (M11)

> 🌐 [English](../crypto.md) · **日本語**

> **ステータス:** M11 の symmetric/hash/KDF surface は完了済みです。post-pkg.db の asymmetric
> signature suite は以下で設計済み、実装は pending です。文書に記載した BLAKE3 の例外も
> 引き続き保留です。

## Overview

`crypto.random`、`sha256` / `sha512`、`blake3`、`hmac`、`hkdf`、`argon2id`、`aes_gcm`、`chacha20_poly1305`、`constant_time_equal`（draft §18.2）。**譲れない要件: 秘密情報に依存する処理経路はすべて constant-time（定数時間）でなければならない**（open-questions std.crypto — 秘密情報に依存する分岐やメモリのインデックスアクセスは禁止。CMOV やビット演算のみに制限する）。この領域は、Align の分岐なし機構がパフォーマンスのためだけでなく、**正しさの要件** そのものになる唯一の領域である。

**戦略**: **検証済みの計算エンジンを外部から借りる**。AEAD（aes_gcm、chacha20_poly1305）、ハッシュ（sha256/512）、KDF（hkdf、argon2id）、hmac は、constant-time 性が監査済みの C ライブラリを FFI でラップする — 暗号処理を自前で実装して constant-time を証明し直すよりも、実績あるライブラリの constant-time 保証をそのまま継承するほうがはるかに安全だからである。自前実装するのは `constant_time_equal` ただ一つである（分岐なしのバイト単位の差分 OR 縮約であり、Align の `where` / mask 機構に素直に乗るうえ、容易に監査できるほど単純であるため）。`crypto.random` は OS の CSPRNG（getrandom / getentropy — `rand.seed` のソースと同じものだが、ここでは鍵材料向けの crypto グレードとして公開する）。

**エンジン: OpenSSL libcrypto(EVP)、2026-07-07 決定**（`open-questions.md` Settled に記録。本ドキュメント当初の「libsodium 推奨」を置き換える）。独立したセキュリティレビューと依存関係レビューが収束した根拠は次のとおり。libcrypto は必要なプリミティブを *すべて* ネイティブにカバーする — HKDF と Argon2id も `EVP_KDF` 経由で提供される — 単一の信頼境界（trust surface）に収まり、エンジンの混在も、自前 HKDF の統合による隙間（継ぎ目）も生じない。これらの機能の大部分は **OpenSSL ≥ 3.0** で動作する。Argon2id には **OpenSSL ≥ 3.2** で追加された `ARGON2ID` provider が必要であり、存在しない場合は `Error.Code` を返す。ドライバは使用した Crypto / TLS capability が必要とする場合に限り `-lcrypto` をリンクとして追加する。このモジュールでは `crypto.random` と `constant_time_equal` はリンクを要求しない。さらに AES-GCM はサポート対象環境において constant-time である（AES-NI / PCLMULQDQ のハードウェア経路、constant-time な vpaes フォールバック — x86-64 / aarch64 で T-table AES を使うことは決してない）。そのうえ、libsodium の `crypto_aead_aes256gcm_*` と異なり、ハードウェア要件によって API が制限（ゲート）されることがない。libsodium は抽象的には依然として優れたエンジンだが、システム全体の統合面（継ぎ目）で劣る（1.0.18 クラスのリリースには HKDF が無く、AES-GCM がハードウェアゲートされる）。**blake3 は決定記録付きで見送る**: これを提供するシステムの基本エンジンが無く（Debian に `libblake3-dev` は無い。OpenSSL にも BLAKE3 は無い）、自前実装は原則 P5 に反する。また、BLAKE2b を `blake3` という別名として扱うことは禁止されている（誤解を招く API となるため）。システムライブラリとして普及すれば再び候補になるか、あるいは `pkg` レイヤーでの提供対象となる。

## Signatures

```text
crypto.random(out: mut buffer)                                  // fill with CSPRNG bytes
crypto.sha256(data: bytes) -> array<u8>    // 32-byte digest (fixed-size)
crypto.sha512(data: bytes) -> array<u8>
crypto.blake3(data: bytes) -> array<u8>    // DEFERRED v1 (no system engine provides BLAKE3 — see Overview)
crypto.hmac_sha256(key: bytes, data: bytes) -> array<u8>
crypto.hkdf_sha256(salt: bytes, ikm: bytes, info: bytes, len: i64) -> Result<buffer, Error>
crypto.argon2id(password: bytes, salt: bytes, params: argon2_params) -> Result<buffer, Error>
crypto.aes_gcm_seal(key: bytes, nonce: bytes, plaintext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.aes_gcm_open(key: bytes, nonce: bytes, ciphertext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.chacha20_poly1305_seal(...) / _open(...)    // same shape as aes_gcm
crypto.constant_time_equal(a: bytes, b: bytes) -> bool          // CT — self-hosted
```

**実装された表層の詳細（実装記録、2026-07-07、PR #384–#388）:**
`argon2_params { m_cost: i64, t_cost: i64, parallelism: i64, len: i64 }` は **組み込み（builtin）構造体** である。non-entry module が local `argon2_params` を宣言しない限り bare alias を利用できる。collision がある module で builtin を明示する正確な spelling は `crypto.argon2_params` であり、`import std.crypto` が必要で、その import の use として数える。通常の構造体リテラルによる構築と型検査を使う。m_cost は KiB、t_cost は反復回数、parallelism はレーン数、len は出力バイト数。エンジンに渡す前に検証する（`parallelism 1..=2^24-1`、`t_cost 1..=u32max`、`m_cost 8*parallelism..=4 GiB-in-KiB`(= 4,194,304 KiB)、`len 4..=1 GiB` → 違反時は `Error.Invalid`。エンジンの `threads` は 1 に固定し、`OSSL_set_max_threads` の利用は見送り）。
AEAD: どちらの暗号も 32 バイトの鍵と 12 バイトの nonce を取る（公開パラメータとして検証し、違反時は `Error.Invalid`）。seal の出力は **結合された** `ciphertext || 16 バイトの tag` を単一のバッファに収めたものである。open は `len >= 16` を要求する。hkdf の `len` は `1..=8160` に制限される（RFC 5869 の L ≤ 255·HashLen）。ダイジェスト / タグの戻り値は、アルゴリズムで固定された長さを持つ動的な `array<u8>` である（固定長の `array<u8; N>` は現状のランタイム戻り値 ABI では表現できないため）。FFI 演算はすべて impure。
`constant_time_equal` は pure であり、その分岐なしの性質は **コンパイル済みの機械語に照らして検証済み** である（release + debug の逆アセンブル結果において、内容に依存する分岐も、memcmp のイディオムも存在しない）。

## Type & ownership classification

byte → byte、または byte →（所有権を持つ buffer か固定長 `array<u8>`）。新しい Move 型は必要ない（buffer / array を再利用する）。固定長のダイジェストは `array<u8>`（32 または 64 バイト）で表す。

## Effect classification

FFI でラップした演算は impure（extern 呼び出し）となる。`constant_time_equal` は pure だが、分岐なしの性質を保ち続けなければならない。

## Error policy

AEAD の open における認証失敗は `Error.Invalid`（タグの不一致か長さの不一致かを **決して** 漏らさない — 単一の不透明な失敗として扱う）。KDF / argon のパラメータエラーは `Error.Invalid`。エンジンのエラーは `Error.Code`。**重要**: `aes_gcm_open` は認証失敗時に部分的な平文を返しては **ならない** — 全か無か（all-or-nothing）の、不透明なエラーとする。

## New machinery required

**OpenSSL libcrypto** への capability-gated FFI リンク（EVP ベースの操作を使う場合だけ `-lcrypto` をリンクする。一般には OpenSSL ≥ 3.0、Argon2id には ≥ 3.2 — Overview 参照）。EVP の上に構築するおよそ 6 個のランタイムラッパー: 共有の一括ダイジェスト（`EVP_Q_digest`、`EVP_sha256/512` でパラメータを差し替える）、HMAC（`EVP_MAC` の "HMAC"）、HKDF（`EVP_KDF_fetch("HKDF")` + `OSSL_PARAM` の salt/key/info）、Argon2id（`EVP_KDF_fetch("ARGON2ID")` + `OSSL_KDF_PARAM_ARGON2_*`）、そして P2 に従う全か無かの共有 AEAD seal/open ペア（`EVP_CIPHER`、AES-256-GCM / ChaCha20-Poly1305 でパラメータを差し替える）。加えて `constant_time_equal` の自前実装（分岐なし、早期 return なし）、OS の CSPRNG の上に構築する `crypto.random`。そして Argon2 のパラメータ構造体。

## Slice breakdown

1. `constant_time_equal`（自前実装、分岐なし）+ `crypto.random`（OS の CSPRNG） — 外部依存なしで、CT の規律を検証する。
2. ハッシュ（sha256/512）を EVP 経由で。blake3 は見送り（Overview 参照）。
3. hmac + hkdf。
4. AEAD（aes_gcm、chacha20_poly1305） — 全か無かの認証。
5. argon2id（KDF、設計上コストが高い）。

## Pitfalls

- **P1 (constant-time is CORRECTNESS)**: `constant_time_equal` には早期 return も、秘密情報に依存する分岐もあってはならない — 全長にわたってバイト差分を OR 縮約し、最後に一度だけ 0 チェックする。早期 break のある `for` ループは、処理タイミングを通じて長さや内容を漏洩させる。self-review では、秘密情報に依存する制御フローが無いことを検証しなければならない。これはこのモジュールを定義づける根本の制約である。
  - **長さの扱い**: 入力の *長さ* は、意図された用途（MAC タグやダイジェストの比較 — 両側とも固定で、公開済みの長さ）においては秘密ではなく **公開情報** として扱う。したがって長さが違えば即座に `false` を返してよい。constant-time の保証は、**同じ長さ**の入力の *内容* に対して成り立つ。これは libsodium の `sodium_memcmp` の契約（長さが等しいことが事実上の前提条件）と一致する。呼び出し側が、長さ自体を隠蔽することを期待して秘密の長さの入力を渡すことのないよう、この点は明示的に文書化すること。
- **P2 (AEAD all-or-nothing)**: 認証失敗時の `open()` は `Error.Invalid` を返し、平文のバイトは **ゼロ（空）** でなければならない — 部分的な平文も、区別のつくエラーも決して返さない。未検証の平文を外に出してしまうのは、AEAD の典型的な誤用である。**EVP 固有の必須の対応**: `EVP_DecryptUpdate` はタグが `EVP_DecryptFinal_ex` で検査される **前** に平文を放出するため、ラッパーは暗号文全体を内部の所有バッファへ復号し（決して外部へストリームしない）、`EVP_CIPHER_CTX_ctrl(EVP_CTRL_AEAD_SET_TAG)` で期待するタグを設定し、`EVP_DecryptFinal_ex` を呼び、`Final == 1` のときに **限って** バッファを呼び出し側へ渡さなければならない。失敗時はバッファを `OPENSSL_cleanse` してから解放し、単一の不透明な `Error.Invalid` を返す（タグ不一致か、長さ/パラメータのエラーかは区別がつかないようにする）。nonce / タグの長さは *公開* 値として事前に検証する（P1）。タグは固定で 16 バイトである。
- **P3 (nonce reuse)**: 同じ鍵で nonce を再利用するのは（特に aes_gcm では）致命的であることを文書化する。v1 は nonce を自動生成しない（呼び出し側が渡す）が、文書には強い警告を書くこと。nonce を生成する補助関数は将来の候補として記録しておく。nonce の長さ検証を必須にすることも検討する。
- **P4 (FFI memory safety, Gate 2)**: compress と同様に、FFI を越えるすべてのバッファに対して try_from / checked_mul / null ガードを適用する。
- **P5 (don't self-host the primitives)**: SHA / AES / argon を Align 内で再実装したくなる誘惑を抑えること — constant-time と正しさを証明し直すのは、監査すべき面積が膨大になる。エンジンを借りる。自前実装するのは（自明な）`constant_time_equal` だけである。
- **P6 (key material zeroization)**: 鍵を保持する buffer は Drop 時にゼロクリアされるのが望ましい（zeroize-on-drop の buffer 亜種を用意するか、呼び出し側の責務として文書化する）。v1 の検討事項として記録する — buffer の Drop は現状ただ解放するだけだが、暗号の鍵に関しては理想としては先にゼロクリアすべきである。

## Test checklist

- sha256/512 を既知のテストベクタ（NIST/RFC）に照らして検証する。blake3 は見送り。
- hmac を RFC 4231 のベクタに照らして検証する。
- hkdf を RFC 5869 に照らして検証する。
- aes_gcm / chacha20 をそれぞれのテストベクタに照らして検証する。
- タグを 1 ビット反転させた `aes_gcm_open` → `Error.Invalid` + 空の平文（P2）。
- `constant_time_equal` の true/false、および（検査/監査により）早期 return が無いこと（P1）。
- `crypto.random` が毎回異なるバイト列で埋めること。
- argon2id の既知解答との照合。
- import が必須であること。
- capability-linking テストで、EVP ベースの暗号を使う場合は libcrypto がリンクされ、Crypto / TLS capability のないプログラムにはリンクされないことを確認する。

## Asymmetric signature suite（post-pkg.db）

この節は RS256、ES256、Ed25519 の sign/verify に関する authoritative public-contract ledger
である。shipped M11 engine を拡張し、第二の crypto provider は追加しない。function と key
type の両方を algorithm-specific にするため、string/enum algorithm selector や generic key
handle によって RSA key が EC/Ed25519 operation に到達することはない。

normative algorithm と wire signature を次で固定する。

| Name | Exact operation | Public signature bytes |
|---|---|---|
| RS256 | SHA-256 を使う RSASSA-PKCS1-v1_5。RSA-PSS ではない | admitted 2048..=8192-bit key の modulus width と exact に等しい 256..=1024 bytes |
| ES256 | named curve P-256 上の ECDSA + SHA-256 | exact 64 bytes。leading zero を含む 32-byte big-endian `r` の直後に 32-byte big-endian `s`。DER は internal のみ |
| Ed25519 | caller digest、context、prehash のない complete message 上の pure Ed25519 | exact 64 bytes |

RS256/ES256 は RFC 7518 sections 3.3/3.4、Ed25519 は RFC 8032、PKCS#8/SPKI identifier は
RFC 8410 に従う。実装は exact digest/padding/group parameter を OpenSSL EVP へ渡し、provider
default に選択させない。

### Public surface ledger

6つの compiler-provided nominal type は互いに異なる **Move** owner である。

```text
rs256_private_key       rs256_public_key
es256_private_key       es256_public_key
ed25519_private_key     ed25519_public_key
```

各 bare spelling は `import std.crypto` 後に利用できる。explicit builtin spelling は
`crypto.rs256_private_key` のように `crypto.` を前置し、同じ import を require し use として
count する。同一 module の declaration は bare lookup で勝ち、explicit spelling は常に builtin
を指す。entry module は衝突する bare name を宣言できない。public interface producer/import
validator は bare/qualified の両方を認識し、required source import `std.crypto` を保持する。

以下が exact constructor/operation であり、default/optional argument はない。`bytes` は既存の
borrowed byte-view meaning を持つ。明示した `borrow` mode は stable bound key place を要求し、
key を consume しない。

```text
crypto.rs256_private_key_from_pem(pem: str) -> Result<rs256_private_key, Error>
crypto.es256_private_key_from_pem(pem: str) -> Result<es256_private_key, Error>
crypto.ed25519_private_key_from_pem(pem: str) -> Result<ed25519_private_key, Error>

crypto.rs256_public_key_from_pem(pem: str) -> Result<rs256_public_key, Error>
crypto.es256_public_key_from_pem(pem: str) -> Result<es256_public_key, Error>
crypto.ed25519_public_key_from_pem(pem: str) -> Result<ed25519_public_key, Error>

crypto.rs256_public_key_from_jwk(n: bytes, e: bytes) -> Result<rs256_public_key, Error>
crypto.es256_public_key_from_jwk(x: bytes, y: bytes) -> Result<es256_public_key, Error>
crypto.ed25519_public_key_from_jwk(x: bytes) -> Result<ed25519_public_key, Error>

crypto.rs256_sign(borrow key: rs256_private_key, message: bytes) -> Result<buffer, Error>
crypto.es256_sign(borrow key: es256_private_key, message: bytes) -> Result<buffer, Error>
crypto.ed25519_sign(borrow key: ed25519_private_key, message: bytes) -> Result<buffer, Error>

crypto.rs256_verify(borrow key: rs256_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
crypto.es256_verify(borrow key: es256_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
crypto.ed25519_verify(borrow key: ed25519_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
```

`_from_jwk` は JSON/encoded text ではなく、base64url decode 済み JWK field を受け取る。caller
は construction 前に唯一の既存 path `encoding.base64url_decode` を使う。private JWK、key
generation/export、certificate parser、encrypted PEM、password callback、RSA-PSS、
Ed25519ctx/ph、generic `sign`/`verify`、implicit algorithm selection は追加しない。

### Key input and validation contract

PEM constructor は bounded な RFC 7468 textual block を1つ受け取る。input は `str` により
UTF-8 であり、各 constructor は最初に `1..=65,536` bytes を要求し embedded NUL を reject
する。block は byte zero から exact `-----BEGIN PRIVATE KEY-----` または
`-----BEGIN PUBLIC KEY-----` line で始まる。base64 body は standard alphabet、canonical final
padding、LF/CRLF line ending を使う。non-final body line は exact 64 base64 characters、final
body line は 4..=64 characters かつ4の倍数である。対応する exact END line が直後に続く。
final LF/CRLF は高々1つ。leading text、horizontal whitespace、second block、その他の trailing
byte は reject する。decoded payload は trailing octet のない complete BER/DER object 1つで
なければならない。`PRIVATE KEY` は unencrypted PKCS#8 `PrivateKeyInfo` /
`OneAsymmetricKey`、`PUBLIC KEY` は `SubjectPublicKeyInfo` である。したがって
`ENCRYPTED PRIVATE KEY`、traditional `RSA PRIVATE KEY` / `EC PRIVATE KEY`、certificate、
OpenSSH key は password/terminal/file/network/environment lookup より前に reject する。

decode 後、constructor は advertised key class/algorithm との exact 一致を要求する。RSA key
は 2048..=8192-bit の odd modulus と、unsigned 64-bit に収まる odd public exponent `>= 3`
を持つ。private key は provider の complete private/pairwise check、public key は public check
を通る。ES256 は exact P-256 named group と full EC private/public check を要求する。Ed25519
は parameter absent の id-Ed25519 と provider private/public check を要求する。異なる class、
curve、size、algorithm は `Error.Invalid` であり、変換しない。

JWK component は borrowed binary integer/point である。

| Constructor | Exact decoded fields and validation |
|---|---|
| `rs256_public_key_from_jwk` | `n`/`e` は leading zero のない minimal unsigned big-endian。`n` は odd かつ actual bit width 2048..=8192。`e` は 1..=8 bytes、`u64` に収まり、odd、`>= 3`。constructed RSA key は provider public check を通る。 |
| `es256_public_key_from_jwk` | `x`/`y` は各 exact 32-byte big-endian P-256 coordinate。uncompressed point `0x04 || x || y` は canonical、on-curve、non-infinite で provider public check を通る。 |
| `ed25519_public_key_from_jwk` | `x` は exact 32-byte RFC 8037 Ed25519 public value で canonical/provider public-key validation を通る。 |

cheap structural check はすべて provider key construction より前に行う。decode、algorithm
match、bounds、complete key validation が成功するまで handle を publish しない。

### Ownership, allocation, and effects

各 successful constructor は provider-managed `EVP_PKEY` 1つを所有する fresh one-word opaque
owner を返す。Move transfer は pointer を copy して complete source を null にする。replacement
と active aggregate Drop は null-safe な `align_rt_crypto_key_free` を exact once 呼ぶ。type は
ordinary independent builtin-Move carrier rule を使う。local、by-value/shared-borrow parameter、
return、recursively admitted struct/sum/Option/Result field は valid。fixed/dynamic array と、既存
owned handle rule が reject する他の placement は invalid のままである。key は input borrow
lifetime や process-global registration を持たない。borrowed sign/verify 後も owner は利用でき、
message/signature view を retain しない。

private scalar material は provider-owned key 内に留まり `EVP_PKEY_free` で release される。
runtime は Align `buffer` へ copy しない。caller-owned PEM `str` は caller storage のままで
zeroize しない。PEM scan は高々65,536-byte input を borrow し、memory BIO は explicit length
を使う。JWK construction は bounded component view と 65-byte stack EC point だけを使う。
provider object/context allocation と key shell 1つは fallible で、failure path ごとに release
する。

sign/verify は complete message を Align-side copy なしで borrow する。RS256/ES256 は
incremental EVP digest-sign/verify、Ed25519 は null digest name の required one-shot pure-EdDSA
call を使う。successful sign は fresh `buffer` 1つだけを allocate/publish する。RS256 は exact
modulus width、ES256/Ed25519 は64 bytes。ES256 の EVP DER signature は raw result publish 前に
bounded internal `r`/`s` storage へ decode する。failure は buffer を publish しない。verify は
public result allocation をせず、ES256 は fixed raw input を bounded internal DER へ変換する。
exact-length だが mathematical invalid/malformed な signature は `Ok(false)`。RS256 の
wrong-modulus-width、ES256/Ed25519 の non-64-byte signature も provider verify 前に
`Ok(false)`。`Ok(true)` は exact named algorithm だけでの verification を意味する。

constructor/sign/verify はすべて EVP を越えるため **Impure**。shared key cache や mutable
wrapper-global state はなく、call は自由に overlap できる。named ambient platform dependency
は既存の linked OpenSSL default library context/provider availability のみ。wrapper は exact
algorithm parameter を渡し、それ自身は configuration/path/environment/terminal/network/clock
を読まない。ES256 sign は provider randomness を消費し得る。RS256/Ed25519 も標準構成が
deterministic な場合を含め observable determinism contract を約束しない。

### Errors and deterministic precedence

public validation/format/key rejection は `Error.Invalid`。provider fetch/context/allocation または
non-verification engine failure は既存の opaque `Error.Code(0)`。OpenSSL error-stack number/text
は公開しない。signature mismatch は error でなく data (`Ok(false)`)。allocation/parse/sign
failure は partial key/signature を publish しない。

multi-invalid runtime call の validation order は exact に次のとおり。

1. ABI output slot を validate/zero し、closed algorithm/key-kind tag を validate する
   （malformed checked HIR のみ）。
2. public length/structure rule を left-to-right に validate する（`pem`、`n` then `e`、`x` then
   `y`、または signature length）。
3. PEM envelope/base64/complete decoded object、または JWK numeric/point encoding を validate。
4. exact key algorithm/class/size/group を要求し、applicable complete provider key check を実行。
5. operation context を作り exact digest/padding/group parameter を設定して engine を実行。
6. produced length/ES256 conversion を validate し、その後だけ owner/result を publish。

verify の signature-length failure は typed key handle check 後、message 処理前に
`Ok(false)`。malformed HIR でも invalid key を invalid signature で mask できない。cleanup は
winning result を返す前に実行し、置き換えない。

### Runtime ABI and compiler identity

internal `SignatureAlgorithm` byte は closed `0=RS256`, `1=ES256`, `2=Ed25519`。
`SignatureKeyKind` byte は closed `0=RS256-private`, `1=RS256-public`, `2=ES256-private`,
`3=ES256-public`, `4=Ed25519-private`, `5=Ed25519-public`。他の値は EVP call 前に reject。
runtime shell は kind を再保持し、各 operation が check するため malformed MIR でも static
type confusion は unsafe provider call にならない。

実装は次の exact internal declaration を追加する（C ABI の `algorithm` は `i32`。1-byte
range を validate してから narrow する）。

```text
i32 @align_rt_crypto_private_key_from_pem(i32, ptr, i64, ptr)
i32 @align_rt_crypto_public_key_from_pem(i32, ptr, i64, ptr)
i32 @align_rt_crypto_public_key_from_jwk(i32, ptr, i64, ptr, i64, ptr)
i32 @align_rt_crypto_sign(i32, ptr, ptr, i64, ptr)
i32 @align_rt_crypto_verify(i32, ptr, ptr, i64, ptr, i64, ptr)
void @align_rt_crypto_key_free(ptr)
```

JWK ABI は Ed25519 の absent second component を null/zero で渡す。constructor/sign result
slot は null initialize する pointer-sized handle slot、verify final slot は zero initialize する
`i32`。status `0` は success、`AL_INVALID` は `Error.Invalid`、`AL_CODE` は
`Error.Code(0)` に map する。

checked HIR/MIR は6つの unrelated enum arm でなく payloaded key type 1つを使う。canonical
type record version 3 は `Scalar::SignatureKey(kind)` に leaf tag 39 + exact one-byte kind、
`Ty::SignatureKey(kind)` に leaf tag 63 +同 byte を割り当てる。kind は payload なので next
tag 40/64 は unknown のまま。interface format 8 は不変で、nominal path は既存の
length-prefixed UTF-8 type record を使う。producer/importer は12個の bare/qualified path を認識し、
required source import `std.crypto` と Move/return-cleanup identity を再構築する。各 key
fingerprint は `EVP_PKEY` layout/structural definition graph
ではなく exact closed kind という nominal identity である。runtime inspection field、descriptor
thunk、source/artifact read はない。operation/helper discriminant は compiler build fingerprint、
in-process memo、frontend/object cache key、
whole/per-unit parity に一度だけ入り、exact source edit/revert は prior key を復元する。

OpenSSL libcrypto はこれら operation のいずれかが reachable なときだけ capability-link する。
new artifact/file format/CLI flag/environment variable/provider selector/package dependency はない。

### Implementation closure matrix

実装前の author-side matrix であり、reviewer の後続 discovery だけで row を close してはならない。

| Axis | Required closure | Owner evidence |
|---|---|---|
| Type formation and interface | bare 6 + qualified 6 name、local-shadow/entry-collision/import-use、Copy reject、Move/return-cleanup reconstruction、canonical kind/tag round-trip と exact next-unknown reject | `align_interface::summary` builtin sweep、`align_mir::canonical_graph` exact golden、`crypto_asymmetric::type_identity_matrix` whole/per-unit |
| Construction | private PEM 3、public PEM 3、decoded-JWK 3 constructor。success は owner 1つを initialize、failure は null。wrong label/algorithm/class/curve/size/component と exact 65,536/65,537 PEM boundary | runtime RFC/PEM/JWK vector + `crypto_asymmetric::constructor_matrix` |
| Move-in/out and cleanup | local bind、by-value parameter/return、shared borrow、struct/sum/Option/Result construction、`?`、`else`、`match`、`map_err`、branch/loop join、replacement、early return、ordinary/malformed Drop が kind 1つと exactly-one free を保持。source nulling は later Drop より先 | parameterized `crypto_asymmetric::ownership_matrix`、runtime free counter/failpoint、checked-HIR one-field negative |
| Sign/verify semantics | empty/binary/large message、RS256 padding+digest/modulus-width result、leading zero/invalid r/s を含む ES256 DER/raw、Ed25519 one-shot no-digest、valid/wrong-message/wrong-key/wrong-length signature、key reusable | runtime と `crypto_asymmetric` の RFC 7515/7518、RFC 8032/8410、OpenSSL cross-check vector |
| FFI/allocation/cleanup | 全 length checked conversion、work 前 output zero、injected failure ごとの ctx/key/BIO/BIGNUM/signature storage free、partial publish なし、runtime kind recheck、reachable 時だけ libcrypto retain | runtime failpoint sweep、ABI declaration golden、capability-linking twin |
| Compilation paths | direct/imported call、public key-bearing signature、function value、concrete key 周辺 generic monomorphization、whole/per-unit、object/frontend cache edit/revert、optimized/unoptimized LLVM、malformed HIR で identical algorithm/kind/effect/cleanup fact | `crypto_asymmetric` driver owner、interface/cache owner、checked-HIR validator matrix |
| Resource claim | PEM exact limit、RSA size bound、fixed ES/Ed temporary、1-byte/8-MiB message で Align-side message copy なし。benchmark は local evidence で correctness gate ではない | `bench/crypto_asymmetric` peak-wrapper-allocation record + deterministic limit test |

実装は hand-written changed lines が roughly 1,000 を超える見込みでも capability PR 1つとする。
6 static type、constructor、runtime kind check、最初の sign/verify consumer は1つの proof boundary
であり、分割すると dormant key producer を land するか type/Drop/interface/ABI proof を algorithm
ごとに重複させる。RS256/ES256/Ed25519 は同一 boundary の parameterized cell であり、3つの
independent ownership mechanism ではない。

review 前に author は applicable matrix cell を implementation/regression に map し、またはここで
明示的に defer する。P1 または strategy-changing finding は implementation 続行前に matrix を reopen
する。

### Acceptance and synchronized sources

acceptance は complete matrix、`scripts/cargo.sh test -p align_runtime`、focused
`crypto_asymmetric` driver owner、interface/canonical/ABI golden、capability-linking twin、bounded
PR gate、Clippy を要求する。local benchmark は ledger が explicit message-copy/resource claim を
持つためだけに実行し、correctness gate でも latency target でもない。

この節が source of truth である。public type/signature と algorithm/error/ownership contract は
`draft.md` §18.2、`docs/language-spec.md`、`docs/open-questions.md`、
`docs/impl/07-roadmap.md`、`docs/impl/19-hir-validation-ledger.md`、
`docs/impl/20-runtime-abi-ledger.md`、English original と一致させる。implementation status は
capability boundary だけで変更し、この contract を書き換えない。
