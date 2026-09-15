// Mechanical exposure of LLVM's implicit return demotion. The module is fully
// built and verified before entry, and no Rust FunctionValue is used afterward.
#include "llvm-c/Core.h"
#include "llvm-c/TargetMachine.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/SmallPtrSet.h"
#include "llvm/CodeGen/MachineFunction.h"
#include "llvm/CodeGen/MachineModuleInfo.h"
#include "llvm/CodeGen/TargetLowering.h"
#include "llvm/CodeGen/TargetSubtargetInfo.h"
#include "llvm/IR/Attributes.h"
#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/Verifier.h"
#include "llvm/Support/raw_ostream.h"
#include "llvm/Target/TargetMachine.h"
#include <memory>
#include <string>
#include <vector>

using namespace llvm;

namespace {
constexpr StringLiteral OwnedCall = "align.program.return";
constexpr StringLiteral NativeCall = "align.native.return";

struct ResultLayout {
  Type *Ty;
  uint64_t Size;
  Align Alignment;
  unsigned AddressSpace;
};
struct FunctionPlan {
  Function *Old;
  ResultLayout Layout;
  Function *Replacement = nullptr;
};
struct CallPlan {
  CallInst *Old;
  ResultLayout Layout;
  StoreInst *Materialize;
  bool ProgramOwned;
};

// Match SelectionDAG's decision, including the actual function type and return
// attributes. Scratch functions/MachineFunctions never mutate the input module
// or outlive classification. No hand-maintained size/register threshold exists.
class Classifier {
  TargetMachine &TM;
  Module Scratch;
  MachineModuleInfo Machines;
  using Key = std::pair<FunctionType *, std::pair<unsigned, AttributeList>>;
  DenseMap<Key, std::optional<ResultLayout>> Results;
public:
  Classifier(TargetMachine &TM, Module &Input)
      : TM(TM), Scratch("return.classification", Input.getContext()),
        Machines(&TM) {
    Scratch.setDataLayout(Input.getDataLayout());
    Scratch.setTargetTriple(Input.getTargetTriple());
  }
  bool classify(FunctionType *FT, CallingConv::ID CC, AttributeList Attrs,
                Function &Context, std::optional<ResultLayout> &Result,
                std::string &Error) {
    Result.reset();
    Type *Ty = FT->getReturnType();
    if (Ty->isVoidTy()) return true;
    if (!Ty->isSized()) { Error = "unsized return type"; return false; }
    TypeSize Size = Scratch.getDataLayout().getTypeAllocSize(Ty);
    if (Size.isScalable()) { Error = "scalable return storage"; return false; }
    // Include the caller's effective subtarget attributes in the cache key.
    for (StringRef Name : {"target-cpu", "target-features", "tune-cpu"}) {
      if (Context.hasFnAttribute(Name))
        Attrs = Attrs.addFnAttribute(Scratch.getContext(),
                                     Context.getFnAttribute(Name));
    }
    Key K{FT, {CC, Attrs}};
    if (auto It = Results.find(K); It != Results.end()) {
      Result = It->second;
      return true;
    }
    Function *Probe = Function::Create(FT, GlobalValue::ExternalLinkage,
                                      "probe", Scratch);
    Probe->setCallingConv(CC);
    Probe->setAttributes(Attrs);
    auto &MF = Machines.getOrCreateMachineFunction(*Probe);
    const auto *TL = MF.getSubtarget().getTargetLowering();
    if (!TL) { Error = "missing target return classifier"; return false; }
    SmallVector<ISD::OutputArg, 8> Outs;
    GetReturnInfo(CC, Ty, Attrs, Outs, *TL, Scratch.getDataLayout());
    if (TL->CanLowerReturn(CC, MF, FT->isVarArg(), Outs,
                           Scratch.getContext(), Ty)) {
      Results[K] = std::nullopt;
      return true;
    }
    if (FT->isVarArg() || Attrs.hasFnAttr(Attribute::AllocSize) ||
        Attrs.hasFnAttr(Attribute::Naked)) {
      Error = "unsupported indirect return attributes"; return false;
    }
    for (unsigned I = 0; I < FT->getNumParams(); ++I) {
      if (Attrs.hasParamAttr(I, Attribute::InAlloca) ||
          Attrs.hasParamAttr(I, Attribute::Returned)) {
        Error = "incompatible indirect return parameter"; return false;
      }
    }
    Result = ResultLayout{Ty, Size.getFixedValue(),
                          Scratch.getDataLayout().getPrefTypeAlign(Ty),
                          Scratch.getDataLayout().getAllocaAddrSpace()};
    Results[K] = Result;
    return true;
  }
};

FunctionType *indirectType(FunctionType *FT, const ResultLayout &L) {
  SmallVector<Type *, 8> Params{PointerType::get(FT->getContext(), L.AddressSpace)};
  Params.append(FT->param_begin(), FT->param_end());
  return FunctionType::get(Type::getVoidTy(FT->getContext()), Params, false);
}

AttributeList indirectAttributes(LLVMContext &C, AttributeList Old,
                                 unsigned ParamCount, const ResultLayout &L,
                                 bool ProgramOwned = true) {
  AttrBuilder Out(C);
  Out.addStructRetAttr(L.Ty);
  Out.addAlignmentAttr(L.Alignment);
  if (ProgramOwned) Out.addCapturesAttr(CaptureInfo::none());
  SmallVector<AttributeSet, 8> Params{AttributeSet::get(C, Out)};
  for (unsigned I = 0; I < ParamCount; ++I)
    Params.push_back(Old.getParamAttrs(I));
  AttrBuilder Fn(C, Old.getFnAttrs());
  Fn.removeAttribute(OwnedCall);
  Fn.removeAttribute(NativeCall);
  // The new destination write is real even when the original function had no
  // memory effects. It does not change effects of any existing parameter.
  Fn.addMemoryAttr(Old.getMemoryEffects() |
                   MemoryEffects::argMemOnly(ModRefInfo::Mod));
  Fn.removeAttribute(Attribute::Speculatable);
  return AttributeList::get(C, AttributeSet::get(C, Fn), AttributeSet(), Params);
}

StoreInst *materializingStore(CallInst &Call, const ResultLayout &L) {
  if (!Call.hasOneUse()) return nullptr;
  auto *Store = dyn_cast<StoreInst>(*Call.user_begin());
  if (!Store || Store != Call.getNextNode() || !Store->isSimple() ||
      Store->getValueOperand() != &Call) return nullptr;
  // Only a complete local object, not a field, borrowed parameter or an
  // undersized destination. LLVM later decides whether old contents are
  // observable; memcpy here does not perform destination forwarding itself.
  auto *Slot = dyn_cast<AllocaInst>(Store->getPointerOperand());
  auto *Count = Slot ? dyn_cast<ConstantInt>(Slot->getArraySize()) : nullptr;
  if (!Slot || !Count || !Count->isOne() || Slot->getAllocatedType() != L.Ty ||
      Slot->getAddressSpace() != L.AddressSpace) return nullptr;
  return Store;
}

bool normalize(Module &M, TargetMachine &TM, ArrayRef<LLVMValueRef> Owned,
               std::string &Error) {
  if (M.getDataLayout() != TM.createDataLayout() ||
      M.getTargetTriple() != TM.getTargetTriple()) {
    Error = "module and target return context disagree"; return false;
  }
  SmallPtrSet<Function *, 32> Selected;
  for (auto Ref : Owned) {
    auto *F = Ref ? dyn_cast<Function>(unwrap(Ref)) : nullptr;
    if (!F || F->getParent() != &M) {
      Error = "return owner is not a function in this module"; return false;
    }
    Selected.insert(F);
  }
  std::vector<FunctionPlan> Functions;
  std::vector<CallPlan> Calls;
  DenseMap<Function *, size_t> Index;
  // All classification and refusal happens before mutation. Destruction of
  // MachineModuleInfo precedes moving/erasing the original IR functions.
  {
    Classifier Classify(TM, M);
    for (Function &F : M) {
      if (!Selected.contains(&F)) continue;
      std::optional<ResultLayout> L;
      if (!Classify.classify(F.getFunctionType(), F.getCallingConv(),
                             F.getAttributes(), F, L, Error)) return false;
      if (L) {
        Index[&F] = Functions.size();
        Functions.push_back(FunctionPlan{&F, *L});
      }
    }
    for (Function &F : M) for (BasicBlock &B : F) for (Instruction &I : B) {
      auto *CB = dyn_cast<CallBase>(&I);
      if (!CB) continue;
      Function *Callee = CB->getCalledFunction();
      if (!(Callee && Selected.contains(Callee)) &&
          !CB->hasFnAttr(OwnedCall) && !CB->hasFnAttr(NativeCall)) continue;
      std::optional<ResultLayout> L;
      if (!Classify.classify(CB->getFunctionType(), CB->getCallingConv(),
                             CB->getAttributes(), F, L, Error)) return false;
      if (Callee && Selected.contains(Callee) &&
          (L.has_value() != Index.contains(Callee))) {
        Error = "caller and callee disagree on return transport"; return false;
      }
      if (!L) continue;
      auto *Call = dyn_cast<CallInst>(CB);
      if (!Call || Call->isMustTailCall() || Call->hasOperandBundles()) {
        Error = "unsupported indirect return call edge"; return false;
      }
      bool ProgramOwned = !CB->hasFnAttr(NativeCall);
      Calls.push_back(CallPlan{Call, *L,
          ProgramOwned ? materializingStore(*Call, *L) : nullptr, ProgramOwned});
    }
  }
  for (auto &P : Functions) {
    Function &Old = *P.Old;
    auto *New = Function::Create(indirectType(Old.getFunctionType(), P.Layout),
                                 Old.getLinkage(), Old.getAddressSpace(), "", &M);
    New->copyAttributesFrom(&Old);
    New->setAttributes(indirectAttributes(M.getContext(), Old.getAttributes(),
                                          Old.arg_size(), P.Layout));
    New->copyMetadata(&Old, 0);
    New->takeName(&Old);
    auto NewArg = New->arg_begin();
    NewArg->setName("result.destination");
    ++NewArg;
    for (Argument &Arg : Old.args()) {
      NewArg->takeName(&Arg);
      Arg.replaceAllUsesWith(&*NewArg++);
    }
    New->splice(New->end(), &Old);
    for (BasicBlock &B : *New) {
      if (auto *Ret = dyn_cast<ReturnInst>(B.getTerminator())) {
        IRBuilder<> Builder(Ret);
        Builder.SetCurrentDebugLocation(Ret->getDebugLoc());
        Builder.CreateAlignedStore(Ret->getReturnValue(), New->getArg(0),
                                    P.Layout.Alignment);
        Builder.CreateRetVoid();
        Ret->eraseFromParent();
      }
    }
    P.Replacement = New;
  }
  for (auto &P : Calls) {
    CallInst &Old = *P.Old;
    IRBuilder<> Entry(&*Old.getFunction()->getEntryBlock().getFirstInsertionPt());
    auto *Slot = Entry.CreateAlloca(P.Layout.Ty, P.Layout.AddressSpace, nullptr,
                                    "call.result.storage");
    Slot->setAlignment(P.Layout.Alignment);
    IRBuilder<> Builder(&Old);
    Builder.SetCurrentDebugLocation(Old.getDebugLoc());
    if (P.ProgramOwned) Builder.CreateLifetimeStart(Slot);
    SmallVector<Value *, 8> Args{Slot};
    for (Use &Arg : Old.args()) Args.push_back(Arg.get());
    Value *Target = Old.getCalledOperand();
    if (auto It = Index.find(Old.getCalledFunction()); It != Index.end())
      Target = Functions[It->second].Replacement;
    auto *Call = Builder.CreateCall(indirectType(Old.getFunctionType(), P.Layout),
                                    Target, Args);
    Call->setCallingConv(Old.getCallingConv());
    Call->setAttributes(indirectAttributes(M.getContext(), Old.getAttributes(),
                                           Old.arg_size(), P.Layout, P.ProgramOwned));
    Call->setDebugLoc(Old.getDebugLoc());
    // A result pointer into this frame cannot carry the original tail marker.
    if (P.Materialize) {
      Builder.CreateMemCpy(P.Materialize->getPointerOperand(),
                           P.Materialize->getAlign(), Slot, P.Layout.Alignment,
                           P.Layout.Size);
      P.Materialize->eraseFromParent();
    } else if (!Old.use_empty()) {
      auto *Value = Builder.CreateAlignedLoad(P.Layout.Ty, Slot,
                                               P.Layout.Alignment, "call.result");
      Old.replaceAllUsesWith(Value);
    }
    if (P.ProgramOwned) Builder.CreateLifetimeEnd(Slot);
    Old.eraseFromParent();
  }
  for (auto &P : Functions) {
    P.Old->replaceAllUsesWith(P.Replacement);
    P.Old->eraseFromParent();
  }
  // Source-owned markers are invocation-local and never enter bitcode/cache.
  for (Function &F : M) for (BasicBlock &B : F) for (Instruction &I : B)
    if (auto *Call = dyn_cast<CallBase>(&I)) {
      Call->removeFnAttr(OwnedCall);
      Call->removeFnAttr(NativeCall);
    }
  return true;
}
} // namespace

// Input handles belong to the caller's live LLVM context. Only the error message
// escapes; LLVMDisposeMessage releases it. The module is unpublished on failure.
extern "C" int align_normalize_return_transport(
    LLVMModuleRef MRef, LLVMTargetMachineRef TMRef, const LLVMValueRef *Owned,
    size_t Count, char **ErrorOut) {
  if (!ErrorOut) return 1;
  *ErrorOut = nullptr;
  if (!MRef || !TMRef || (!Owned && Count)) {
    *ErrorOut = LLVMCreateMessage("invalid return transport input"); return 1;
  }
  Module &M = *unwrap(MRef);
  std::string Error;
  raw_string_ostream Stream(Error);
  if (verifyModule(M, &Stream)) {
    *ErrorOut = LLVMCreateMessage(Error.c_str()); return 1;
  }
  if (!normalize(M, *reinterpret_cast<TargetMachine *>(TMRef),
                  ArrayRef<LLVMValueRef>(Owned, Count), Error)) {
    *ErrorOut = LLVMCreateMessage(Error.c_str()); return 1;
  }
  Error.clear();
  if (verifyModule(M, &Stream)) {
    *ErrorOut = LLVMCreateMessage(Error.c_str()); return 1;
  }
  return 0;
}
