//! LLVM C++ shim (production component of align_codegen_llvm).
//!
//! A minimal C++ bridge for surface the llvm-sys 221 C API lacks: the three
//! summary-based ThinLTO entry points (no way to emit a module summary index,
//! no combined-index/FunctionImporter surface), plus the instrument-PGO
//! pipeline entry `align_pgo_run_pipeline` (no `PGOOptions` C surface at all).
//! Compiled UNCONDITIONALLY by build.rs (the `--thin-lto` and `--pgo-*` driver
//! paths both need it in every `alignc`), against the SAME libLLVM-22.so the
//! workspace links (prefer-dynamic), so there is a single LLVM in the process.
//!
//! All entry points take/return plain C types so the Rust side declares
//! them with `extern "C"`. `LLVMModuleRef` / `LLVMTargetMachineRef` values are
//! inkwell/llvm-sys handles that we `llvm::unwrap` / `reinterpret_cast` back into
//! their C++ types in that same libLLVM.
//!
//! S1 final form (vs the S0 spike): (1) entry 1 stamps the module with a driver-
//! chosen STABLE id (not the incidental inkwell "align" name); (2) the combined
//! index is keyed by those stable ids (a MemoryBufferRef identifier override), so
//! import edges and the backend agree on module identity regardless of temp path;
//! (3) the backend RECEIVES the serialized import list computed by entry 2 and
//! reconstructs `ImportListsTy` — it never re-runs `ComputeCrossModuleImport`;
//! (4) the backend uses the driver's own `TargetMachine` (passed as a handle), so
//! cpu/features/reloc/code-model/opt exactly match `create_target_machine`.

#include "llvm-c/Core.h"
#include "llvm-c/TargetMachine.h"

#include "llvm/ADT/StringMap.h"
#include "llvm/Analysis/ModuleSummaryAnalysis.h"
#include "llvm/Analysis/ProfileSummaryInfo.h"
#include "llvm/Bitcode/BitcodeReader.h"
#include "llvm/Bitcode/BitcodeWriter.h"
#include "llvm/IR/Function.h"
#include "llvm/IR/GlobalValue.h"
#include "llvm/IR/LLVMContext.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/ModuleSummaryIndex.h"
#include "llvm/IRReader/IRReader.h"
#include "llvm/LTO/LTO.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/FileSystem.h"
#include "llvm/Support/MemoryBuffer.h"
#include "llvm/Support/PGOOptions.h"
#include "llvm/Support/SourceMgr.h"
#include "llvm/Support/VirtualFileSystem.h"
#include "llvm/Target/TargetMachine.h"
#include "llvm/Transforms/IPO/FunctionImport.h"
#include "llvm/Transforms/Utils/FunctionImportUtils.h"

#include <cstddef>
#include <cstdint>
#include <map>
#include <memory>
#include <string>
#include <tuple>
#include <vector>

using namespace llvm;

namespace {
// Map a driver opt-level int (0..3) to the middle-end OptimizationLevel. ThinLTO
// is legal only on release (O2) / fast (O3); anything else clamps to O2 rather
// than guessing a size level (the driver never passes one).
OptimizationLevel optLevel(int level) {
  switch (level) {
  case 0:
    return OptimizationLevel::O0;
  case 1:
    return OptimizationLevel::O1;
  case 3:
    return OptimizationLevel::O3;
  case 2:
  default:
    return OptimizationLevel::O2;
  }
}
} // namespace

// ---------------------------------------------------------------------------
// Entry 1: run the ThinLTO pre-link pipeline and emit summary-bearing bitcode.
// ---------------------------------------------------------------------------
//
// This is THE capability llvm-sys lacks: LLVMWriteBitcodeToMemoryBuffer emits
// NO summary. Here we stamp the module with the driver's stable id, run
// buildThinLTOPreLinkDefaultPipeline, build a ModuleSummaryIndex, and
// WriteBitcodeToFile WITH that index + module hash.
//
// Returns 0 on success, nonzero on failure.
extern "C" int align_thinlto_write_prelink_bc(LLVMModuleRef Mref,
                                              const char *stable_id,
                                              int ir_opt_level,
                                              const char *out_path) {
  Module *M = unwrap(Mref);
  if (!M || !stable_id || !out_path)
    return 10;

  // Stamp the driver-chosen stable id so the module's own identity (used by the
  // ThinLTO backend passes) matches the combined-index key entries 2/3 build.
  M->setModuleIdentifier(stable_id);
  M->setSourceFileName(stable_id);

  {
    PassBuilder PB;
    LoopAnalysisManager LAM;
    FunctionAnalysisManager FAM;
    CGSCCAnalysisManager CGAM;
    ModuleAnalysisManager MAM;
    PB.registerModuleAnalyses(MAM);
    PB.registerCGSCCAnalyses(CGAM);
    PB.registerFunctionAnalyses(FAM);
    PB.registerLoopAnalyses(LAM);
    PB.crossRegisterProxies(LAM, FAM, CGAM, MAM);
    ModulePassManager MPM =
        PB.buildThinLTOPreLinkDefaultPipeline(optLevel(ir_opt_level));
    MPM.run(*M, MAM);
  }

  // Build the per-module summary index. No PGO profile: a plain PSI + a
  // BFI callback that returns null is what a non-instrumented build uses.
  ProfileSummaryInfo PSI(*M);
  std::function<BlockFrequencyInfo *(const Function &)> GetBFI =
      [](const Function &) -> BlockFrequencyInfo * { return nullptr; };
  ModuleSummaryIndex Index = buildModuleSummaryIndex(*M, GetBFI, &PSI);

  std::error_code EC;
  raw_fd_ostream OS(out_path, EC, sys::fs::OF_None);
  if (EC)
    return 11;
  WriteBitcodeToFile(*M, OS, /*ShouldPreserveUseListOrder=*/false, &Index,
                     /*GenerateHash=*/true);
  OS.flush();
  // EC only reflects the open; write failures surface via the stream state.
  return OS.has_error() ? 12 : 0;
}

// ---------------------------------------------------------------------------
// Shared: read every summary-bearing bitcode file into one combined index,
// keyed by the driver's stable ids (NOT the incidental temp-file path).
// ---------------------------------------------------------------------------
namespace {
struct LinkResult {
  ModuleSummaryIndex Index{/*HaveGVs=*/false};
  // Keeps MemoryBuffers alive: the combined index copies module-path keys, but
  // the GVSummary pointers still reference buffer-owned data.
  std::vector<std::unique_ptr<MemoryBuffer>> Buffers;
};

// Populate `LR.Index` from the given bitcode files, keying each module by
// `identifiers[i]` (a MemoryBufferRef identifier override). Returns 0 on
// success. `computeDeadSymbolsWithConstProp` runs with the driver's preserve
// set; Align's single-prevailing-copy model makes every definition prevailing.
int buildIndex(LinkResult &LR, const char *const *bc_paths,
               const char *const *identifiers, size_t n_modules,
               const char *const *preserve_syms, size_t n_preserve) {
  // Defensive FFI-boundary checks: a null array or element is a caller bug,
  // but fail with an rc instead of a segfault.
  if ((n_modules && (!bc_paths || !identifiers)) ||
      (n_preserve && !preserve_syms))
    return 19;
  for (size_t i = 0; i < n_modules; i++) {
    if (!bc_paths[i] || !identifiers[i])
      return 19;
    ErrorOr<std::unique_ptr<MemoryBuffer>> MB =
        MemoryBuffer::getFile(bc_paths[i]);
    if (!MB)
      return 20;
    // Override the buffer identifier with the driver's stable id so the combined
    // index (which keys modules by the buffer identifier) is path-independent.
    MemoryBufferRef Ref((*MB)->getBuffer(), identifiers[i]);
    if (Error E = readModuleSummaryIndex(Ref, LR.Index)) {
      consumeError(std::move(E));
      return 21;
    }
    LR.Buffers.push_back(std::move(*MB));
  }

  DenseSet<GlobalValue::GUID> Preserved;
  for (size_t i = 0; i < n_preserve; i++) {
    if (!preserve_syms[i])
      return 19;
    Preserved.insert(
        GlobalValue::getGUIDAssumingExternalLinkage(preserve_syms[i]));
  }

  // Single-copy world: every definition is prevailing (Align guarantees one
  // prevailing copy per external symbol — no ODR duplicates across units;
  // duplicate consumer-side generic monomorphs are internal linkage, so ThinLTO
  // treats them per-module, never as prevailing-conflicting externals).
  auto isPrevailingDead = [](GlobalValue::GUID) { return PrevailingType::Yes; };
  computeDeadSymbolsWithConstProp(LR.Index, Preserved, isPrevailingDead,
                                  /*ImportEnabled=*/true);
  return 0;
}
} // namespace

// ---------------------------------------------------------------------------
// Entry 2: thin-link. Combine summaries and report BOTH the per-unit import
// edges and the per-unit export set.
// ---------------------------------------------------------------------------
//
// The thin-link is the single global step that computes cross-module import AND
// export decisions once. It reports:
//   * every import edge (destination module, source module, GUID, kind) via
//     `import_cb`, and
//   * every exported (module, GUID) pair via `export_cb`
// so the driver holds the complete decision set and threads it into each
// backend (entry 3) WITHOUT the backend recomputing thin-link. Both the import
// list (per destination) and the export set (per source, needed to promote a
// module's cross-module-referenced locals consistently) are required for a
// correct backend. Module identities are the driver's stable ids, reported as
// (ptr,len) slices (NOT null-terminated). Returns 0 on success.
typedef void (*align_thinlto_import_cb)(void *ctx, const char *dest_mod,
                                        size_t dest_len, const char *src_mod,
                                        size_t src_len, uint64_t guid,
                                        int is_definition);
typedef void (*align_thinlto_export_cb)(void *ctx, const char *mod,
                                        size_t mod_len, uint64_t guid);

extern "C" int align_thinlto_link(const char *const *bc_paths,
                                  const char *const *identifiers,
                                  size_t n_modules,
                                  const char *const *preserve_syms,
                                  size_t n_preserve,
                                  align_thinlto_import_cb import_cb,
                                  align_thinlto_export_cb export_cb, void *ctx) {
  LinkResult LR;
  if (int rc = buildIndex(LR, bc_paths, identifiers, n_modules, preserve_syms,
                          n_preserve))
    return rc;

  DenseMap<StringRef, GVSummaryMapTy> ModuleToDefinedGVSummaries;
  LR.Index.collectDefinedGVSummariesPerModule(ModuleToDefinedGVSummaries);

  auto isPrevailing = [](GlobalValue::GUID,
                         const GlobalValueSummary *) { return true; };
  FunctionImporter::ImportListsTy ImportLists;
  DenseMap<StringRef, FunctionImporter::ExportSetTy> ExportLists;
  ComputeCrossModuleImport(LR.Index, ModuleToDefinedGVSummaries, isPrevailing,
                           ImportLists, ExportLists);

  // Import edges. The per-module edge SET is deterministic (import decisions are
  // order-independent — proven at S0); the DenseSet iteration ORDER is not, so
  // the driver canonically sorts the collected edges. Both are fine:
  // reconstruction (entry 3) re-inserts into a set.
  if (import_cb) {
    for (size_t i = 0; i < n_modules; i++) {
      StringRef DestMod(identifiers[i]);
      const FunctionImporter::ImportMapTy &Map = ImportLists.lookup(DestMod);
      for (const auto &Imp : Map) {
        StringRef FromMod = std::get<0>(Imp);
        GlobalValue::GUID GUID = std::get<1>(Imp);
        GlobalValueSummary::ImportKind Kind = std::get<2>(Imp);
        import_cb(ctx, DestMod.data(), DestMod.size(), FromMod.data(),
                  FromMod.size(), static_cast<uint64_t>(GUID),
                  Kind == GlobalValueSummary::Definition ? 1 : 0);
      }
    }
  }

  // Export set per module (order-independent set; driver order-agnostic).
  if (export_cb) {
    for (size_t i = 0; i < n_modules; i++) {
      StringRef Mod(identifiers[i]);
      auto It = ExportLists.find(Mod);
      if (It == ExportLists.end())
        continue;
      for (const ValueInfo &VI : It->second)
        export_cb(ctx, Mod.data(), Mod.size(),
                  static_cast<uint64_t>(VI.getGUID()));
    }
  }
  return 0;
}

// ---------------------------------------------------------------------------
// Entry 3: per-unit backend. Import functions into one module, run the ThinLTO
// backend pipeline, and emit its object file.
// ---------------------------------------------------------------------------
//
// The import list AND the global export map computed by entry 2 are threaded back
// in — the backend NEVER re-runs ComputeCrossModuleImport (S1 final form):
//   * `imp_*` = this module's import list (src stable id, GUID, kind), reconstructed
//     into an ImportListsTy for `own_id` and used by `importFunctions`.
//   * `exp_*` = the (module id, GUID) export pairs for ALL modules, used to build
//     the `isExported` predicate for `thinLTOInternalizeAndPromoteInIndex`, which
//     marks every cross-module-referenced local for promotion. Without this step a
//     module's exported locals (e.g. a private string constant behind an imported
//     function) are NOT emitted under their promoted `.llvm.<hash>` names, and the
//     importing unit fails to link. Promotion + internalization are then applied to
//     the module by `importFunctions` (via FunctionImportGlobalProcessing), reading
//     the index flags this call set.
// The combined index is read (FunctionImporter + the backend pipeline consult it),
// keyed by the same stable ids so identities agree. `tm` is the driver's own
// TargetMachine handle, so cpu/features/reloc/code-model match create_target_machine.
//
// Returns 0 on success, nonzero on failure.
extern "C" int align_thinlto_backend(
    const char *own_id, const char *const *bc_paths,
    const char *const *identifiers, size_t n_modules, unsigned own_idx,
    const char *const *preserve_syms, size_t n_preserve,
    const char *const *imp_src_ids, const uint64_t *imp_guids,
    const int *imp_kinds, size_t n_imports, const char *const *exp_mods,
    const uint64_t *exp_guids, size_t n_exports, LLVMTargetMachineRef tm_ref,
    int ir_opt_level, const char *out_obj) {
  if (own_idx >= n_modules || !tm_ref || !own_id || !out_obj)
    return 30;

  LinkResult LR;
  if (int rc = buildIndex(LR, bc_paths, identifiers, n_modules, preserve_syms,
                          n_preserve))
    return rc;

  // Reconstruct this module's import list from the serialized edges (entry 2's
  // decisions), instead of recomputing cross-module import here.
  FunctionImporter::ImportListsTy ImportLists;
  FunctionImporter::ImportMapTy &OwnList = ImportLists[own_id];
  for (size_t i = 0; i < n_imports; i++) {
    GlobalValueSummary::ImportKind Kind =
        imp_kinds[i] ? GlobalValueSummary::Definition
                     : GlobalValueSummary::Declaration;
    OwnList.addGUID(imp_src_ids[i],
                    static_cast<GlobalValue::GUID>(imp_guids[i]), Kind);
  }

  // Reconstruct the global export map (module id -> set of exported GUIDs) from
  // the serialized export pairs, and mark exported locals for promotion /
  // non-exported for internalization in the index. Single-copy world: everything
  // is prevailing.
  // StringMap allows allocation-free StringRef lookup in the hot isExported
  // callback (a std::map<std::string,...> key would heap-allocate per query).
  StringMap<DenseSet<GlobalValue::GUID>> ExportMap;
  for (size_t i = 0; i < n_exports; i++)
    ExportMap[exp_mods[i]].insert(
        static_cast<GlobalValue::GUID>(exp_guids[i]));
  auto isExported = [&](StringRef ModuleId, ValueInfo VI) -> bool {
    auto It = ExportMap.find(ModuleId);
    return It != ExportMap.end() && It->second.contains(VI.getGUID());
  };
  auto isPrevailing = [](GlobalValue::GUID,
                         const GlobalValueSummary *) { return true; };
  thinLTOInternalizeAndPromoteInIndex(LR.Index, isExported, isPrevailing);

  // Parse the own module into a fresh context; imported modules must be parsed
  // into the SAME context (IRMover requirement).
  LLVMContext Ctx;
  SMDiagnostic Err;
  std::unique_ptr<Module> Own = parseIRFile(bc_paths[own_idx], Err, Ctx);
  if (!Own)
    return 31;
  // Defensive: the id was stamped by entry 1 and stored in the bitcode, but pin
  // it again so the backend passes' self-lookup keys match the combined index.
  Own->setModuleIdentifier(own_id);

  // stable id -> path map for the module loader (StringMap: StringRef lookup).
  StringMap<std::string> IdToPath;
  for (size_t i = 0; i < n_modules; i++)
    IdToPath[identifiers[i]] = bc_paths[i];

  FunctionImporter::ModuleLoaderTy Loader =
      [&](StringRef Identifier) -> Expected<std::unique_ptr<Module>> {
    auto It = IdToPath.find(Identifier);
    if (It == IdToPath.end())
      return createStringError(inconvertibleErrorCode(),
                               "unknown module identifier");
    SMDiagnostic LErr;
    std::unique_ptr<Module> Dep = parseIRFile(It->second, LErr, Ctx);
    if (!Dep)
      return createStringError(inconvertibleErrorCode(), "parse dep failed");
    // parseIRFile sets the module identifier from the buffer path, NOT the id
    // stamped in entry 1 (bitcode stores only the source filename). Restamp the
    // stable id so promotion (`getPromotedName` -> `getModuleHash`) keys match the
    // combined index (keyed by stable id) — otherwise a promoted local's source
    // module hash lookup misses and segfaults in release LLVM (asserts compiled out).
    Dep->setModuleIdentifier(Identifier.str());
    return std::move(Dep);
  };

  // Promote THIS module's own exported locals (per the index flags set above) to
  // external `.llvm.<hash>` symbols BEFORE importing. This must run even for a unit
  // that imports nothing (e.g. a leaf library): `importFunctions` on an empty import
  // list does not promote the module's own exports, so a local referenced by another
  // unit's imported copy would be left undefined at final link. Called with
  // GlobalsToImport=nullptr, this is the "promote my exports" pass; `importFunctions`
  // below then handles the "import from others" pass.
  renameModuleForThinLTO(*Own, LR.Index, /*ClearDSOLocalOnDeclarations=*/false,
                         /*GlobalsToImport=*/nullptr);

  FunctionImporter Importer(LR.Index, Loader,
                            /*ClearDSOLocalOnDeclarations=*/false);
  Expected<bool> Imported =
      Importer.importFunctions(*Own, ImportLists.lookup(own_id));
  if (!Imported) {
    consumeError(Imported.takeError());
    return 32;
  }

  // The driver's own TargetMachine: reinterpret the C handle to the C++ type for
  // the PassBuilder, and reuse the handle for object emission.
  TargetMachine *TM = reinterpret_cast<TargetMachine *>(tm_ref);
  {
    PassBuilder PB(TM);
    LoopAnalysisManager LAM;
    FunctionAnalysisManager FAM;
    CGSCCAnalysisManager CGAM;
    ModuleAnalysisManager MAM;
    PB.registerModuleAnalyses(MAM);
    PB.registerCGSCCAnalyses(CGAM);
    PB.registerFunctionAnalyses(FAM);
    PB.registerLoopAnalyses(LAM);
    PB.crossRegisterProxies(LAM, FAM, CGAM, MAM);
    ModulePassManager MPM =
        PB.buildThinLTODefaultPipeline(optLevel(ir_opt_level), &LR.Index);
    MPM.run(*Own, MAM);
  }

  // Emit the object via the C API on the same module handle + driver TM.
  char *emit_err = nullptr;
  LLVMBool failed = LLVMTargetMachineEmitToFile(
      tm_ref, wrap(Own.get()), out_obj, LLVMObjectFile, &emit_err);
  if (failed) {
    if (emit_err)
      LLVMDisposeMessage(emit_err);
    return 35;
  }
  return 0;
}

// ===========================================================================
// Instrument-PGO pipeline entry (production; compiled unconditionally).
// ===========================================================================
//
// Why a shim at all: llvm-sys 221 exposes NO PGO surface. LLVMPassBuilderOptions
// has setters for LTO/loop/SLP tuning but nothing for PGOOptions, and the textual
// pipeline (`LLVMRunPasses(module, "default<O2>", ...)`) can express instr-GEN as
// a bare pass name but NOT instr-USE (there is no pass-parameter form for the
// profile-use action; only a process-global test `cl::opt`). The ONE capability
// the C API cannot reach is constructing a `PassBuilder` with a populated
// `std::optional<PGOOptions>` and running the default per-module pipeline, which
// is exactly what clang's `-fprofile-generate` / `-fprofile-use` do under the new
// pass manager. This entry is that construction and nothing more.
//
// It runs the pipeline IN PLACE on `Mref`; it does NOT emit an object (the Rust
// side owns emission via the same C API `LLVMTargetMachineEmitToFile`, so the
// spike proves Align's real emit path survives instrumentation).
//
//   kind == 0  -> IRInstr (gen): insert __profc_/__profd_ counters, the
//                 __llvm_profile_runtime anchor, and llvm.used pinning.
//   kind == 1  -> IRUse  (use): read `profdata_path` (a merged .profdata) and
//                 attach !prof branch_weights.
//
// `profdata_path` is required for USE, ignored (may be null) for GEN — the GEN
// default output filename is governed by LLVM_PROFILE_FILE at runtime.
//
// Returns 0 on success, nonzero on argument errors. CONTRACT CAVEAT (measured):
// a missing/unreadable/corrupt profdata does NOT surface through this return
// code — libLLVM diagnoses it on the LLVMContext (and without a diagnostic
// handler installed, exits the process). The caller MUST validate the profdata
// (existence/readability/magic) BEFORE this call and install a context
// diagnostic handler to observe use-phase degradation; the S1 driver owns that
// fail-loud policy (roadmap "Instrument-PGO design SETTLED").
//
// `out_matched` / `out_total` (either may be null) report the PROFILE-MATCH signal
// after the pipeline: `total` = defined (non-declaration) functions in the module,
// `matched` = how many of those carry a PGO entry count (`Function::getEntryCount`).
// A function is given an entry count iff the USE pass FOUND it in the profile with a
// matching structural hash; a hash mismatch or an absent record leaves it unset. So
// `matched == 0 && total > 0` after a USE run means NONE of this module's functions
// matched the profile — the "0%-match" wrong-program/incompatible-profile signal the
// driver turns into a hard error (the settled fail-loud policy). For a GEN run the
// pass sets no entry counts, so `matched` is 0 and the driver ignores it (GEN reads
// no profile). The count is taken AFTER the default pipeline; inlining/DCE can drop a
// matched callee, but the ZERO-vs-NONZERO boundary the driver keys on is robust
// (`main` is never inlined away and keeps its entry count on any real match).
extern "C" int align_pgo_run_pipeline(LLVMModuleRef Mref,
                                      LLVMTargetMachineRef tm_ref, int opt_level,
                                      int kind, const char *profdata_path,
                                      int *out_matched, int *out_total) {
  if (out_matched)
    *out_matched = 0;
  if (out_total)
    *out_total = 0;
  Module *M = unwrap(Mref);
  if (!M || !tm_ref)
    return 40;
  // USE needs a profile; GEN does not.
  if (kind == 1 && !profdata_path)
    return 41;

  TargetMachine *TM = reinterpret_cast<TargetMachine *>(tm_ref);

  // PGOOptions in LLVM 22 no longer carries the VFS (it moved to the PassBuilder
  // ctor's last arg); its ctor is:
  //   PGOOptions(ProfileFile, CSProfileGenFile, ProfileRemappingFile,
  //              MemoryProfile, PGOAction, CSPGOAction, ColdFuncOpt,
  //              DebugInfoForProfiling, PseudoProbeForProfiling,
  //              AtomicCounterUpdate)
  // For GEN, ProfileFile is the baked default output name (empty => the runtime's
  // own default, overridden by LLVM_PROFILE_FILE); for USE it is the .profdata.
  PGOOptions::PGOAction action =
      (kind == 1) ? PGOOptions::IRUse : PGOOptions::IRInstr;
  std::string profileFile = profdata_path ? std::string(profdata_path) : "";
  PGOOptions pgo(profileFile, /*CSProfileGenFile=*/"",
                 /*ProfileRemappingFile=*/"", /*MemoryProfile=*/"", action,
                 PGOOptions::NoCSAction, PGOOptions::ColdFuncOpt::Default,
                 /*DebugInfoForProfiling=*/false,
                 /*PseudoProbeForProfiling=*/false,
                 /*AtomicCounterUpdate=*/false);

  PassBuilder PB(TM, PipelineTuningOptions(), std::optional<PGOOptions>(pgo),
                 /*PIC=*/nullptr, vfs::getRealFileSystem());
  LoopAnalysisManager LAM;
  FunctionAnalysisManager FAM;
  CGSCCAnalysisManager CGAM;
  ModuleAnalysisManager MAM;
  PB.registerModuleAnalyses(MAM);
  PB.registerCGSCCAnalyses(CGAM);
  PB.registerFunctionAnalyses(FAM);
  PB.registerLoopAnalyses(LAM);
  PB.crossRegisterProxies(LAM, FAM, CGAM, MAM);
  ModulePassManager MPM = PB.buildPerModuleDefaultPipeline(optLevel(opt_level));
  MPM.run(*M, MAM);

  // Profile-match tally (see the contract note above): of the OPTIMIZED module's defined
  // functions, how many carry a PGO entry count (`matched`) — i.e. were found in the profile
  // and given profile data that survived to the final IR — out of the total defined (`total`).
  // Measured AFTER the pipeline deliberately: the USE pass's function hashes are computed on
  // the SIMPLIFIED CFG (post the early passes that precede `addPGOInstrPasses`), so the only
  // faithful match check is inside the real pipeline — a separate un-simplified measurement
  // mis-hashes and matches nothing. `matched == 0 && total > 0` after a USE run therefore means
  // 0% of the optimized module got profile data: the profile is for a different program, or
  // every function it could have applied to changed shape since it was collected — either way
  // it contributes nothing to this build, which the driver escalates to a hard error. A build
  // with any surviving matched function (`matched > 0`) is at most partially stale and proceeds.
  // For a GEN run the pass sets no entry counts (`matched == 0`) and the driver ignores it.
  if (out_matched || out_total) {
    int matched = 0;
    int total = 0;
    for (Function &F : M->functions()) {
      if (F.isDeclaration())
        continue;
      ++total;
      if (F.getEntryCount().has_value())
        ++matched;
    }
    if (out_matched)
      *out_matched = matched;
    if (out_total)
      *out_total = total;
  }
  return 0;
}
