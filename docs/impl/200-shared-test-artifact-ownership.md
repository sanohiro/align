# Shared driver test artifact ownership

The common driver harness still deletes predictable project paths before
acquiring them, and `build_exe` arms cleanup only after emission and linking.
Independent reviews #1292, #1333, and #1347 repeatedly reached this boundary
through otherwise unrelated owner tests. Close the shared construction path
with the existing production `ArtifactStage` authority.

This is a test-harness capability. It changes no language, runtime, ABI, or
compiler cache contract. Keep checking, code generation, linking, child inputs,
and returned output unchanged. Existing `owned_fixture` supervision continues
to own native-child deadlines; this work does not add supervision to every
legacy test.

## Implementation closure matrix

| Invariant | Implementation | Acceptance owner |
| --- | --- | --- |
| Every temporary root is acquired atomically before its first write; caller labels have no path authority | Constant-label `ArtifactStage` in C, single-file, `TempProject`, and `Proj` helpers; fixed artifact basenames | `shared_artifact_ownership`: historical file/directory/symlink sentinels and path-like labels |
| Cleanup is armed before fallible source writes, emission, or linking | Keep the stage local or transfer it into a project before populating sources | Source-write conflict, C compilation/link failure, and post-construction unwind owners |
| Same-name live owners cannot overwrite each other's sources or executables | One stage per invocation | Simultaneous `Proj`, single-file and multi-file executable guards |
| A returned executable, per-unit build, or project keeps its artifacts alive until Drop | Private stage field in `BuiltExe`, `TempProject`, and `Proj`; existing wrapper ownership retained | Execute returned binaries, inspect per-unit objects, then assert stage removal |
| Cleanup follows the acquired stage, never a caller-editable path | Remove legacy path-based Drop implementations | Retarget public project/executable paths and preserve unrelated sentinel |
| Fixture-relative file names and entry paths cannot lexically escape their root | Shared nonempty relative-path check rejects root/parent components; nested module paths remain supported | Invalid path matrix and nested-module differential check |
| Existing directly constructed `Proj` callers preserve acquisition, canonical paths, and explicit permission controls | Migrate literal construction to the stage-owning constructors | Compile all driver test targets; run changed owners and the required DB suites |
| Existing nested ThinLTO staging stays alive with its enclosing project | Retain parent ownership and nested staging behavior | Existing ThinLTO cache owner |

This reuses an already-reviewed acquisition and cleanup strategy. No new
temporary-path protocol or public safety strategy needs a separate plan review.
The author matrix pass and one fresh preflight inspection cover the integration.
Generic monomorphization, serialization, runtime allocation, and language
control-flow closure cells are unchanged. Source fixtures may deliberately
create symlinks later; this lexical filename check is not a new sandbox for
arbitrary test code.

The focused target reuses `owned_fixture` for private TMPDIR and bounded native
descendants without mutating process-global environment. Shared harness changes
also run local PostgreSQL verification, the bounded gate, and Clippy. No
performance claim or benchmark is introduced.
