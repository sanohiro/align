//! ThinLTO S0 spike shim.
//!
//! A minimal C++ bridge exposing the three summary-based ThinLTO entry points
//! that the llvm-sys 221 C API cannot drive on its own (it has no way to emit a
//! module summary index, no combined-index/FunctionImporter surface). This is a
//! FEASIBILITY PROTOTYPE compiled by build.rs only when the `thinlto-spike`
//! cargo feature is enabled. It is NOT wired into the driver.
//!
//! All three entry points take/return plain C types so the Rust side declares
//! them with `extern "C"`. LLVMModuleRef values are inkwell/llvm-sys handles
//! that we `llvm::unwrap` back into `llvm::Module*` in the SAME libLLVM-22.so
//! the workspace already links (prefer-dynamic), so there is a single LLVM in
//! the process.

#include "llvm-c/Core.h"
#include "llvm-c/Target.h"
#include "llvm-c/TargetMachine.h"

#include "llvm/Analysis/ModuleSummaryAnalysis.h"
#include "llvm/Analysis/ProfileSummaryInfo.h"
#include "llvm/Bitcode/BitcodeReader.h"
#include "llvm/Bitcode/BitcodeWriter.h"
#include "llvm/IR/LLVMContext.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/ModuleSummaryIndex.h"
#include "llvm/IRReader/IRReader.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/FileSystem.h"
#include "llvm/Support/MemoryBuffer.h"
#include "llvm/Support/SourceMgr.h"
#include "llvm/Support/TargetSelect.h"
#include "llvm/Support/raw_ostream.h"
#include "llvm/Target/TargetMachine.h"
#include "llvm/Transforms/IPO/FunctionImport.h"

#include <cstddef>
#include <cstdint>
#include <map>
#include <memory>
#include <string>
#include <tuple>
#include <vector>

using namespace llvm;

// ---------------------------------------------------------------------------
// Entry 1: run the ThinLTO pre-link pipeline and emit summary-bearing bitcode.
// ---------------------------------------------------------------------------
//
// This is THE capability llvm-sys lacks: LLVMWriteBitcodeToMemoryBuffer emits
// NO summary. Here we run buildThinLTOPreLinkDefaultPipeline, build a
// ModuleSummaryIndex, and WriteBitcodeToFile WITH that index + module hash.
//
// Returns 0 on success, nonzero on failure.
extern "C" int align_thinlto_write_prelink_bc(LLVMModuleRef Mref,
                                              const char *out_path) {
  Module *M = unwrap(Mref);
  if (!M || !out_path)
    return 10;

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
        PB.buildThinLTOPreLinkDefaultPipeline(OptimizationLevel::O2);
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
// Shared: read every summary-bearing bitcode file into one combined index and
// compute cross-module import lists.
// ---------------------------------------------------------------------------
namespace {
struct LinkResult {
  ModuleSummaryIndex Index{/*HaveGVs=*/false};
  FunctionImporter::ImportListsTy ImportLists;
  DenseMap<StringRef, GVSummaryMapTy> ModuleToDefinedGVSummaries;
  // Keeps MemoryBuffers alive: the combined index's module-path StringRefs and
  // the GVSummary pointers reference data owned here.
  std::vector<std::unique_ptr<MemoryBuffer>> Buffers;
};

// Populate `LR` from the given bitcode files (fed in caller order). Returns 0
// on success.
int buildLinkResult(LinkResult &LR, const char *const *bc_paths,
                    size_t n_modules, const char *const *preserve_syms,
                    size_t n_preserve) {
  for (size_t i = 0; i < n_modules; i++) {
    ErrorOr<std::unique_ptr<MemoryBuffer>> MB =
        MemoryBuffer::getFile(bc_paths[i]);
    if (!MB)
      return 20;
    MemoryBufferRef Ref = (*MB)->getMemBufferRef();
    if (Error E = readModuleSummaryIndex(Ref, LR.Index)) {
      consumeError(std::move(E));
      return 21;
    }
    LR.Buffers.push_back(std::move(*MB));
  }

  DenseSet<GlobalValue::GUID> Preserved;
  for (size_t i = 0; i < n_preserve; i++)
    Preserved.insert(
        GlobalValue::getGUIDAssumingExternalLinkage(preserve_syms[i]));

  // Single-copy world: every definition is prevailing.
  auto isPrevailingDead = [](GlobalValue::GUID) { return PrevailingType::Yes; };
  auto isPrevailing = [](GlobalValue::GUID,
                         const GlobalValueSummary *) { return true; };

  computeDeadSymbolsWithConstProp(LR.Index, Preserved, isPrevailingDead,
                                  /*ImportEnabled=*/true);

  LR.Index.collectDefinedGVSummariesPerModule(LR.ModuleToDefinedGVSummaries);

  DenseMap<StringRef, FunctionImporter::ExportSetTy> ExportLists;
  ComputeCrossModuleImport(LR.Index, LR.ModuleToDefinedGVSummaries, isPrevailing,
                           LR.ImportLists, ExportLists);
  return 0;
}
} // namespace

// ---------------------------------------------------------------------------
// Entry 2: thin-link. Combine summaries and report per-unit import edges.
// ---------------------------------------------------------------------------
//
// For every (destination module, source module, function GUID) import decision
// the shim invokes `cb`. Module identities are the modules' embedded
// identifiers (the string passed to create_module), reported as (ptr,len)
// slices (NOT null-terminated). Returns 0 on success.
typedef void (*align_thinlto_import_cb)(void *ctx, const char *dest_mod,
                                        size_t dest_len, const char *src_mod,
                                        size_t src_len, uint64_t guid,
                                        int is_definition);

extern "C" int align_thinlto_link(const char *const *bc_paths, size_t n_modules,
                                  const char *const *preserve_syms,
                                  size_t n_preserve, align_thinlto_import_cb cb,
                                  void *ctx) {
  LinkResult LR;
  if (int rc = buildLinkResult(LR, bc_paths, n_modules, preserve_syms,
                               n_preserve))
    return rc;

  if (cb) {
    for (const auto &Entry : LR.ImportLists) {
      StringRef DestMod = Entry.first;
      for (const auto &Imp : Entry.second) {
        StringRef FromMod = std::get<0>(Imp);
        GlobalValue::GUID GUID = std::get<1>(Imp);
        auto Kind = std::get<2>(Imp);
        cb(ctx, DestMod.data(), DestMod.size(), FromMod.data(), FromMod.size(),
           static_cast<uint64_t>(GUID),
           Kind == GlobalValueSummary::Definition ? 1 : 0);
      }
    }
  }
  return 0;
}

// ---------------------------------------------------------------------------
// Entry 3: per-unit backend. Import functions into one module, run the ThinLTO
// backend pipeline, and emit its object file.
// ---------------------------------------------------------------------------
//
// NOTE (spike deviation): rather than threading the serialized import list back
// in, the backend recomputes the combined index + import lists from the same
// inputs and looks up its own module's list. That keeps the prototype small and
// is faithful to what a real per-unit cached backend does (it reads the
// combined index anyway); S1 should pass the import list explicitly so the
// backend does not redo the thin-link work.
//
// Returns 0 on success, nonzero on failure.
extern "C" int
align_thinlto_backend(const char *own_identifier, const char *const *bc_paths,
                      size_t n_modules, const char *const *identifiers,
                      unsigned own_idx, const char *const *preserve_syms,
                      size_t n_preserve, const char *cpu, const char *features,
                      const char *out_obj) {
  if (own_idx >= n_modules)
    return 30;

  LinkResult LR;
  if (int rc = buildLinkResult(LR, bc_paths, n_modules, preserve_syms,
                               n_preserve))
    return rc;

  // Parse the own module into a fresh context; imported modules must be parsed
  // into the SAME context (IRMover requirement).
  LLVMContext Ctx;
  SMDiagnostic Err;
  std::unique_ptr<Module> Own =
      parseIRFile(bc_paths[own_idx], Err, Ctx);
  if (!Own)
    return 31;

  // identifier -> path map for the module loader.
  std::map<std::string, std::string> IdToPath;
  for (size_t i = 0; i < n_modules; i++)
    IdToPath[identifiers[i]] = bc_paths[i];

  FunctionImporter::ModuleLoaderTy Loader =
      [&](StringRef Identifier) -> Expected<std::unique_ptr<Module>> {
    auto It = IdToPath.find(Identifier.str());
    if (It == IdToPath.end())
      return createStringError(inconvertibleErrorCode(),
                               "unknown module identifier");
    SMDiagnostic LErr;
    std::unique_ptr<Module> Dep = parseIRFile(It->second, LErr, Ctx);
    if (!Dep)
      return createStringError(inconvertibleErrorCode(), "parse dep failed");
    return std::move(Dep);
  };

  FunctionImporter Importer(LR.Index, Loader,
                            /*ClearDSOLocalOnDeclarations=*/false);
  Expected<bool> Imported =
      Importer.importFunctions(*Own, LR.ImportLists.lookup(own_identifier));
  if (!Imported) {
    consumeError(Imported.takeError());
    return 32;
  }

  // Create a host TargetMachine via the C API (robust across LLVM minor
  // versions), then reinterpret to the C++ type for the PassBuilder.
  LLVMInitializeNativeTarget();
  LLVMInitializeNativeAsmPrinter();
  LLVMInitializeNativeAsmParser();

  char *triple = LLVMGetDefaultTargetTriple();
  LLVMTargetRef TargetC = nullptr;
  char *terr = nullptr;
  if (LLVMGetTargetFromTriple(triple, &TargetC, &terr)) {
    if (terr)
      LLVMDisposeMessage(terr);
    LLVMDisposeMessage(triple);
    return 33;
  }
  LLVMTargetMachineRef TMRef = LLVMCreateTargetMachine(
      TargetC, triple, cpu ? cpu : "", features ? features : "",
      LLVMCodeGenLevelDefault, LLVMRelocPIC, LLVMCodeModelDefault);
  LLVMDisposeMessage(triple);
  if (!TMRef)
    return 34;
  TargetMachine *TM = reinterpret_cast<TargetMachine *>(TMRef);

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
        PB.buildThinLTODefaultPipeline(OptimizationLevel::O2, &LR.Index);
    MPM.run(*Own, MAM);
  }

  // Emit the object via the C API on the same module handle.
  char *emit_err = nullptr;
  LLVMBool failed = LLVMTargetMachineEmitToFile(
      TMRef, wrap(Own.get()), out_obj, LLVMObjectFile, &emit_err);
  int rc = 0;
  if (failed) {
    if (emit_err)
      LLVMDisposeMessage(emit_err);
    rc = 35;
  }
  LLVMDisposeTargetMachine(TMRef);
  return rc;
}
