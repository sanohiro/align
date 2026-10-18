# Align v0.10.2 Release Notes

Align v0.10.2 restores `pkg.web` and `pkg.ws` after the structured-task callback regression in v0.10.1.

- Full package imports, concurrent web workers, middleware, HTTP streams and WebSocket applications compile again. Callback access requirements preserve shared-task safety across ordinary calls, separate compilation and cached interfaces. Request-local mutation remains supported; callbacks that write shared captured backing remain rejected.
- Both package trees return to the prebuilt first-party cache, whose complete corpus now contains 30 compilation units. The bounded compiler gate checks the real web/ws import graph.
- On macOS, an upgraded connection reset by its peer preserves the native transport error when setting the next I/O timeout fails. The connection retains its existing close-on-failure and sticky-error behavior.

Compiler interface format 21 invalidates earlier cached interfaces. Install the compiler and matching runtime together and rebuild generated programs. The public web/ws APIs and native runtime ABI are unchanged by this patch. General higher-order effect polymorphism remains outside the supported surface: task callback requirements must be satisfied at direct calls, and cannot be hidden in an unconstrained function value.

Release artifacts target Linux x86_64, Linux ARM64 and macOS Apple Silicon.

Implementation and verification: [PR #1384](https://github.com/sanohiro/align/pull/1384).
