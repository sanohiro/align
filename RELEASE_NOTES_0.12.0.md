# Align v0.12.0 Release Notes

Align v0.12.0 completes the selected call-boundary view-safety repair, adds bounded HTTP and explicit memory-view capabilities, and reduces compiler cost on large ownership graphs.

## Views and ownership

- Non-owning views passed through ordinary value parameters, captures or function returns are read-only. Explicit mutable parameters retain proved writable backing; owned storage remains writable. Returns and mutable outputs validate view lifetime at publication. This uses the existing parameter modes without hidden copies, allocations or lifetime syntax.
- `builder.bytes()` exposes the initialized byte prefix as a read-only `slice<u8>` without allocation or copying. The view retains its builder owner and existing mutation/lifetime restrictions.
- Indexed fixed and dynamic array fields support retained read-only range views, including `rows[i].field[..end]`. The view borrows the selected backing rather than copying the field; existing owner, bounds and invalidation checks still apply.
- Unsafe `resource.view_mut_from_raw(owner, ptr, len)` forms an exclusive view tied to a mutable resource owner. The native wrapper must prove initialized, live, aligned, writable and uniquely owned backing, and remains responsible for external transfer completion. `File.read_at(offset, destination)` performs a synchronous positional read into an admitted initialized byte view.
- Compiler fixes preserve Copy records loaded through raw C-layout access, scope loop-generation advances to the loop actually exited, and avoid treating an inactive owned match temporary as a release of the selected value. Actual expired views and use after move remain rejected.

## HTTP

- `http_request_ctx.respond_timeout_ns(response, timeout_ns)` sends an ordinary response under one positive total timeout budget, including retries and connection restoration. HEAD sends the corresponding Content-Length without the body. Errors consume and close the response context; only a fully completed eligible response can return to keep-alive reuse.
- `http_request_ctx.peer_eof()` makes one nonblocking, nonconsuming receive observation and returns `Result<bool, Error>`. True includes a peer half-close; false does not prove liveness. Cancellation and GPU cleanup remain application policy.

## Compiler cost and documentation

- Builder reservation validation retains only live SSA/Boolean facts and sparse exact reservation states. Producer validation reuses completed exact graph-query results. The measured large consumer module now completes below its previous 3 GiB supervisor limit, and the measured helper project emits MIR in 44 seconds instead of reaching its 240-second stop. These are local macOS arm64 observations, not cross-machine performance guarantees.
- The specification and English/Japanese library designs now reflect supported bound-owner conditional results and owned-field replacement. Module and Web examples use current signatures and compile in whole-program and per-unit modes.

## Compatibility and known limits

- This release enforces the explicit writable-call boundary. Code that writes through an ordinary parameter or a returned read-only view must move the write into a helper with an admitted explicit mutable parameter. General writable returns are not introduced.
- Exclusive access through a partial owned subrecord (R132) or a matched owned payload (R142) remains a deliberate 1.0 language restriction. Use the supported complete-owner and field-operation rules. These are not pending implementation promises.
- Native operations and compiler artifact identities changed. Install `alignc`, `align-repl` and the matching runtime archive together, and rebuild generated programs; do not mix compiler/runtime versions.
- Provider delivery is separate from consumer adoption. The selected K1 compiler has recorded acceptance in all three consumers within their named native boundaries; adoption and application qualification for the later capabilities remain consumer-owned.

Release artifacts target Linux x86_64, Linux ARM64 and macOS Apple Silicon. Packages include the compiler, REPL, matching runtime archive and compiler-exact first-party package cache. Native dependencies remain explicit; versioned artifacts use the existing thin-LTO and compiler-PGO distribution workflow.

Implementation and verification: [changes since v0.11.0](https://github.com/sanohiro/align/compare/v0.11.0...v0.12.0). Capability owners, independent reviews and platform CI qualify the shipped boundaries; excluded capabilities and consumer-owned acceptance are not implied.
