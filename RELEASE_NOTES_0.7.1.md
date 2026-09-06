# Align v0.7.1 Release Notes

Align v0.7.1 is a release-pipeline correction for v0.7.0. The v0.7.0 tag was
created, but no GitHub Release or downloadable archives were published because
all three platform builds rejected valid `pkg.web` MIR while generating the
prebuilt package cache.

The compiler now preserves lifted-lambda capture provenance through direct
pipeline calls by expressing capture roots as the lifted function's trailing
ABI parameters. Function-value closures convert those roots back to their
environment-relative form, retaining the existing closure contract. Producer
validation also recognizes the settled one-way `http_request_ctx` to
`http_headers` view conversion used by `ctx.headers()`; the reverse conversion
remains invalid.

There are no language, package, runtime ABI, or public API changes from v0.7.0.
The v0.7.1 archives supersede the unpublished v0.7.0 artifacts. Rebuild Align
programs and cache entries with v0.7.1.
