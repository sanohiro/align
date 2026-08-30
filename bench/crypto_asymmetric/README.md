# Asymmetric crypto resource probe

This manual probe owns the resource claim in `docs/impl/std-design/crypto.md`:

- 64 concurrently live public keys produce exactly 64 published runtime shells and return to zero;
- private construction has at most two simultaneous wrapper-owned clear-free allocations (the
  decoded PKCS#8 input and its canonical re-encoding scratch), cleanses both, and leaves none live;
- Ed25519 signs borrowed 1-byte and 8-MiB message views. The wrapper source has no message-copy
  allocation; the timings are observation only and have no threshold.

Run from any directory:

```text
bench/crypto_asymmetric/run.sh
```

The `crypto-asymmetric-probe` runtime feature is off in normal builds. This probe is local evidence,
not a correctness, latency, or constant-time gate.
