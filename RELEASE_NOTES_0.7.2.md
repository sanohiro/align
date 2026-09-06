# Align v0.7.2 Release Notes

Align v0.7.2 is a release-pipeline correction for v0.7.1. The v0.7.1 tag was
created, but no GitHub Release or downloadable archives were published because
all three platform jobs detected a mismatch between the prebuilt-cache warm
project and the project used to measure that cache.

Cache generation and release measurement now consume one shared project
definition covering the complete first-party package corpus: `pkg.db`,
`pkg.web`, `pkg.frame`, `pkg.auth`, `pkg.kv`, `pkg.csv`, `pkg.ws`, and
`pkg.template`. This removes the duplicated package/import inventory that
allowed the measurement input to fall behind the shipped cache input.

There are no language, compiler, package, runtime ABI, or public API changes
from v0.7.1. The v0.7.2 archives supersede the unpublished v0.7.0 and v0.7.1
artifacts.
