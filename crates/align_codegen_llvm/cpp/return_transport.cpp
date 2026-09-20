// Mechanical exposure of LLVM's target-selected aggregate transport. The
// module is fully built and verified before entry, and no Rust FunctionValue
// is used afterward.
#include "llvm-c/Core.h"
#include "llvm-c/TargetMachine.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/SmallPtrSet.h"
#include "llvm/CodeGen/MachineFunction.h"
#include "llvm/CodeGen/MachineModuleInfo.h"
#include "llvm/CodeGen/Analysis.h"
#include "llvm/CodeGen/FunctionLoweringInfo.h"
#include "llvm/CodeGen/SelectionDAG.h"
#include "llvm/CodeGen/TargetLowering.h"
#include "llvm/CodeGen/TargetSubtargetInfo.h"
#include "llvm/Analysis/OptimizationRemarkEmitter.h"
#include "llvm/Analysis/CFG.h"
#include "llvm/Analysis/TargetLibraryInfo.h"
#include "llvm/IR/Attributes.h"
#include "llvm/IR/Dominators.h"
#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/Verifier.h"
#include "llvm/Support/raw_ostream.h"
#include "llvm/Target/TargetMachine.h"
#include "llvm/Transforms/Utils/Cloning.h"
#include <memory>
#include <string>
#include <vector>

using namespace llvm;

namespace {
constexpr StringLiteral OwnedCall = "align.program.return";
constexpr StringLiteral NativeCall = "align.native.return";
constexpr StringLiteral CleanupCall = "align.program.cleanup";
constexpr StringLiteral ParameterOwner = "align.program.parameters";

struct ResultLayout {
  Type *Ty;
  uint64_t Size;
  Align Alignment;
  unsigned AddressSpace;
};

std::optional<ResultLayout> storageLayout(Type *Ty, const DataLayout &DL,
                                          std::string &Error) {
  if (!Ty->isSized()) { Error = "unsized transport type"; return std::nullopt; }
  TypeSize Size = DL.getTypeAllocSize(Ty);
  if (Size.isScalable()) { Error = "scalable transport storage"; return std::nullopt; }
  return ResultLayout{Ty, Size.getFixedValue(), DL.getPrefTypeAlign(Ty),
                      DL.getAllocaAddrSpace()};
}
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
struct CleanupFunctionPlan {
  Function *Old;
  StructType *PairTy;
  ResultLayout PairLayout;
  ResultLayout ValueLayout;
  bool ValueIndirect;
  Function *Replacement = nullptr;
};
struct CleanupCallPlan {
  CallInst *Old;
  StructType *PairTy;
  ResultLayout PairLayout;
  ResultLayout ValueLayout;
  bool ValueIndirect;
  bool ProgramOwned;
  StoreInst *Materialize;
};
struct ParamPlan {
  unsigned Index;
  ResultLayout Layout;
};

StructType *cleanupPair(FunctionType *FT, AttributeList Attrs) {
  if (!Attrs.hasFnAttr(CleanupCall)) return nullptr;
  auto *Pair = dyn_cast<StructType>(FT->getReturnType());
  if (!Pair || Pair->getNumElements() != 2 ||
      !Pair->getElementType(1)->isIntegerTy(1))
    return nullptr;
  return Pair;
}

// Match SelectionDAG's decision, including the actual function type and return
// attributes. Scratch functions/MachineFunctions never mutate the input module
// or outlive classification. No hand-maintained size/register threshold exists.
class Classifier {
  TargetMachine &TM;
  Module Scratch;
  MachineModuleInfo Machines;
  using Key = std::pair<FunctionType *, std::pair<unsigned, AttributeList>>;
  DenseMap<Key, std::optional<ResultLayout>> Results;
  DenseMap<Key, SmallVector<ParamPlan, 4>> ParameterResults;
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
  bool callFrameSize(FunctionType *FT, CallingConv::ID CC,
                     AttributeList Attrs, uint64_t &Bytes,
                     std::string &Error) {
    if (FT->isVarArg()) { Error = "variadic parameter transport"; return false; }
    auto *CallerTy = FunctionType::get(Type::getVoidTy(Scratch.getContext()),
                                       false);
    Function *Caller = Function::Create(CallerTy, GlobalValue::InternalLinkage,
                                        "argument.probe", Scratch);
    Caller->setCallingConv(CC);
    Caller->setAttributes(AttributeList::get(
        Scratch.getContext(), Attrs.getFnAttrs(), AttributeSet(), {}));
    BasicBlock *IRBlock = BasicBlock::Create(Scratch.getContext(), "entry", Caller);
    ReturnInst::Create(Scratch.getContext(), IRBlock);
    auto &MF = Machines.getOrCreateMachineFunction(*Caller);
    MachineBasicBlock *MBB = MF.CreateMachineBasicBlock(IRBlock);
    MF.push_back(MBB);
    OptimizationRemarkEmitter ORE(Caller);
    TargetLibraryInfoImpl LibImpl(Triple(Scratch.getTargetTriple()));
    TargetLibraryInfo LibInfo(LibImpl);
    SelectionDAG DAG(TM, CodeGenOptLevel::Default);
    DAG.init(MF, ORE, nullptr, &LibInfo, nullptr, nullptr, nullptr, Machines,
             nullptr);
    FunctionLoweringInfo FLI;
    FLI.set(*Caller, MF, &DAG);
    DAG.setFunctionLoweringInfo(&FLI);
    const TargetLowering *TL = MF.getSubtarget().getTargetLowering();
    if (!TL) { Error = "missing target parameter classifier"; return false; }
    TargetLowering::ArgListTy Args;
    for (unsigned I = 0; I < FT->getNumParams(); ++I) {
      Type *Ty = FT->getParamType(I);
      SmallVector<EVT, 8> VTs;
      ComputeValueVTs(*TL, Scratch.getDataLayout(), Ty, VTs);
      if (VTs.empty()) continue;
      SmallVector<SDValue, 8> Values;
      for (EVT VT : VTs) Values.push_back(DAG.getUNDEF(VT));
      SDValue Node = Values.size() == 1
                         ? Values.front()
                         : DAG.getNode(ISD::MERGE_VALUES, SDLoc(),
                                       DAG.getVTList(VTs), Values);
      TargetLowering::ArgListEntry Entry(Node, Ty);
      AttributeSet Param = Attrs.getParamAttrs(I);
      Entry.IsSExt = Param.hasAttribute(Attribute::SExt);
      Entry.IsZExt = Param.hasAttribute(Attribute::ZExt);
      Entry.IsNoExt = Param.hasAttribute(Attribute::NoExt);
      Entry.IsInReg = Param.hasAttribute(Attribute::InReg);
      Entry.IsSRet = Param.hasAttribute(Attribute::StructRet);
      Entry.IsNest = Param.hasAttribute(Attribute::Nest);
      Entry.IsByVal = Param.hasAttribute(Attribute::ByVal);
      Entry.IsByRef = Param.hasAttribute(Attribute::ByRef);
      Entry.IsInAlloca = Param.hasAttribute(Attribute::InAlloca);
      Entry.IsPreallocated = Param.hasAttribute(Attribute::Preallocated);
      Entry.IsReturned = Param.hasAttribute(Attribute::Returned);
      Entry.IsSwiftSelf = Param.hasAttribute(Attribute::SwiftSelf);
      Entry.IsSwiftAsync = Param.hasAttribute(Attribute::SwiftAsync);
      Entry.IsSwiftError = Param.hasAttribute(Attribute::SwiftError);
      Entry.Alignment = Param.getAlignment();
      if (Entry.IsByVal)
        Entry.IndirectType = Param.getByValType();
      else if (Entry.IsSRet)
        Entry.IndirectType = Param.getStructRetType();
      else if (Entry.IsByRef)
        Entry.IndirectType = Param.getByRefType();
      else if (Entry.IsPreallocated)
        Entry.IndirectType = Param.getPreallocatedType();
      Args.push_back(Entry);
    }
    auto PtrVT = TL->getPointerTy(Scratch.getDataLayout());
    TargetLowering::CallLoweringInfo CLI(DAG);
    CLI.setDebugLoc(SDLoc())
        .setChain(DAG.getEntryNode())
        .setCallee(CC, Type::getVoidTy(Scratch.getContext()),
                   DAG.getExternalSymbol("argument.target", PtrVT),
                   std::move(Args));
    TL->LowerCallTo(CLI);
    std::optional<uint64_t> Found;
    for (SDNode &Node : DAG.allnodes()) {
      if (Node.getOpcode() != ISD::CALLSEQ_START || Node.getNumOperands() < 2)
        continue;
      auto *Amount = dyn_cast<ConstantSDNode>(Node.getOperand(1));
      if (!Amount) { Error = "nonconstant target call frame"; return false; }
      if (Found) { Error = "multiple target call frames"; return false; }
      Found = Amount->getZExtValue();
    }
    if (!Found) { Error = "target call lowering omitted its frame"; return false; }
    Bytes = *Found;
    FLI.clear();
    DAG.clear();
    return true;
  }

  bool classifyParams(FunctionType *FT, CallingConv::ID CC,
                      AttributeList Attrs, Function &Context,
                      SmallVectorImpl<ParamPlan> &Plans,
                      std::string &Error) {
    Plans.clear();
    for (StringRef Name : {"target-cpu", "target-features", "tune-cpu"})
      if (Context.hasFnAttribute(Name))
        Attrs = Attrs.addFnAttribute(Scratch.getContext(),
                                     Context.getFnAttribute(Name));
    Key K{FT, {CC, Attrs}};
    if (auto It = ParameterResults.find(K); It != ParameterResults.end()) {
      Plans.append(It->second.begin(), It->second.end());
      return true;
    }
    SmallVector<Type *, 8> Prefix;
    uint64_t Before = 0;
    auto *Void = Type::getVoidTy(Scratch.getContext());
    for (unsigned I = 0; I < FT->getNumParams(); ++I) {
      Type *Ty = FT->getParamType(I);
      Prefix.push_back(Ty);
      auto *PrefixFT = FunctionType::get(Void, Prefix, false);
      uint64_t After = 0;
      if (!callFrameSize(PrefixFT, CC, Attrs, After, Error))
        return false;
      if (Ty->isAggregateType() && After > Before) {
        auto Layout = storageLayout(Ty, Scratch.getDataLayout(), Error);
        if (!Layout) return false;
        if (Layout->Size != 0)
          Plans.push_back(ParamPlan{I, *Layout});
      }
      Before = After;
    }
    ParameterResults[K].append(Plans.begin(), Plans.end());
    return true;
  }
};

FunctionType *indirectType(FunctionType *FT, const ResultLayout &L) {
  SmallVector<Type *, 8> Params{PointerType::get(FT->getContext(), L.AddressSpace)};
  Params.append(FT->param_begin(), FT->param_end());
  return FunctionType::get(Type::getVoidTy(FT->getContext()), Params, false);
}

FunctionType *cleanupType(FunctionType *FT, const ResultLayout &Value,
                          bool ValueIndirect) {
  SmallVector<Type *, 8> Params;
  if (ValueIndirect)
    Params.push_back(PointerType::get(FT->getContext(), Value.AddressSpace));
  Params.push_back(PointerType::get(FT->getContext(),
                                   Value.AddressSpace));
  Params.append(FT->param_begin(), FT->param_end());
  Type *Return = ValueIndirect ? Type::getVoidTy(FT->getContext()) : Value.Ty;
  return FunctionType::get(Return, Params, false);
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
  // The new destination write is real even when the original function had no
  // memory effects. It does not change effects of any existing parameter.
  Fn.addMemoryAttr(Old.getMemoryEffects() |
                   MemoryEffects::argMemOnly(ModRefInfo::Mod));
  Fn.removeAttribute(Attribute::Speculatable);
  return AttributeList::get(C, AttributeSet::get(C, Fn), AttributeSet(), Params);
}

AttributeList cleanupAttributes(LLVMContext &C, AttributeList Old,
                                unsigned ParamCount,
                                const ResultLayout &Value,
                                bool ValueIndirect,
                                bool ProgramOwned = true) {
  SmallVector<AttributeSet, 8> Params;
  if (ValueIndirect) {
    AttrBuilder Result(C);
    Result.addStructRetAttr(Value.Ty);
    Result.addAlignmentAttr(Value.Alignment);
    if (ProgramOwned) Result.addCapturesAttr(CaptureInfo::none());
    Params.push_back(AttributeSet::get(C, Result));
  }
  AttrBuilder Cleanup(C);
  Cleanup.addAlignmentAttr(Align(1));
  if (ProgramOwned) Cleanup.addCapturesAttr(CaptureInfo::none());
  Params.push_back(AttributeSet::get(C, Cleanup));
  for (unsigned I = 0; I < ParamCount; ++I)
    Params.push_back(Old.getParamAttrs(I));
  AttrBuilder Fn(C, Old.getFnAttrs());
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

StoreInst *cleanupMaterializingStore(CallInst &Call,
                                     const ResultLayout &Value) {
  ExtractValueInst *Extract = nullptr;
  for (User *User : Call.users()) {
    auto *Candidate = dyn_cast<ExtractValueInst>(User);
    if (!Candidate || Candidate->getNumIndices() != 1) return nullptr;
    if (*Candidate->idx_begin() != 0) continue;
    if (Extract) return nullptr;
    Extract = Candidate;
  }
  if (!Extract) return nullptr;
  StoreInst *Store = nullptr;
  for (User *User : Extract->users()) {
    auto *Candidate = dyn_cast<StoreInst>(User);
    if (!Candidate || !Candidate->isSimple() ||
        Candidate->getValueOperand() != Extract)
      continue;
    if (Store) return nullptr;
    Store = Candidate;
  }
  if (!Store) return nullptr;
  auto *Slot = dyn_cast<AllocaInst>(Store->getPointerOperand());
  auto *Count = Slot ? dyn_cast<ConstantInt>(Slot->getArraySize()) : nullptr;
  if (!Slot || !Count || !Count->isOne() ||
      Slot->getAllocatedType() != Value.Ty ||
      Slot->getAddressSpace() != Value.AddressSpace)
    return nullptr;
  if (Store->getParent() != Call.getParent() || !Call.comesBefore(Store))
    return nullptr;
  DominatorTree Dominators(*Call.getFunction());
  for (User *User : Slot->users()) {
    auto *Use = dyn_cast<Instruction>(User);
    if (!Use) return nullptr;
    if (Use == Store || Use->isLifetimeStartOrEnd()) continue;
    if (auto *OtherStore = dyn_cast<StoreInst>(Use)) {
      if (!OtherStore->isSimple() ||
          !isa<ConstantAggregateZero>(OtherStore->getValueOperand()))
        return nullptr;
      if ((OtherStore->getParent() == Call.getParent() &&
           OtherStore->comesBefore(&Call)) ||
          Dominators.dominates(Store, OtherStore))
        continue;
      return nullptr;
    }
    // The destination must be fresh until this result is committed. Every
    // ordinary use must observe the completed store, and a later replacement
    // store keeps the general temporary path.
    if (!Dominators.dominates(Store, Use)) return nullptr;
  }
  return Store;
}

const ParamPlan *findParam(ArrayRef<ParamPlan> Plans, unsigned Index) {
  auto It = llvm::find_if(Plans, [Index](const ParamPlan &P) {
    return P.Index == Index;
  });
  return It == Plans.end() ? nullptr : &*It;
}

FunctionType *parameterType(FunctionType *FT, ArrayRef<ParamPlan> Plans) {
  SmallVector<Type *, 8> Params;
  for (unsigned I = 0; I < FT->getNumParams(); ++I) {
    if (const ParamPlan *P = findParam(Plans, I))
      Params.push_back(PointerType::get(FT->getContext(), P->Layout.AddressSpace));
    else
      Params.push_back(FT->getParamType(I));
  }
  return FunctionType::get(FT->getReturnType(), Params, false);
}

AttributeList parameterAttributes(LLVMContext &C, AttributeList Old,
                                  unsigned ParamCount,
                                  ArrayRef<ParamPlan> Plans) {
  SmallVector<AttributeSet, 8> Params;
  for (unsigned I = 0; I < ParamCount; ++I) {
    if (const ParamPlan *P = findParam(Plans, I)) {
      AttrBuilder Param(C, Old.getParamAttrs(I));
      Param.addByValAttr(P->Layout.Ty);
      Param.addAlignmentAttr(P->Layout.Alignment);
      Param.addCapturesAttr(CaptureInfo::none());
      Params.push_back(AttributeSet::get(C, Param));
    } else {
      Params.push_back(Old.getParamAttrs(I));
    }
  }
  AttrBuilder Fn(C, Old.getFnAttrs());
  // The former entry alloca is now the callee-private byval copy. Even though
  // ordinary immutable parameters only read it, retaining ModRef is required
  // for a body that mutates its private copy before optimization refines it.
  Fn.addMemoryAttr(Old.getMemoryEffects() |
                   MemoryEffects::argMemOnly(ModRefInfo::ModRef));
  Fn.removeAttribute(Attribute::Speculatable);
  return AttributeList::get(C, AttributeSet::get(C, Fn), Old.getRetAttrs(),
                            Params);
}

bool sameParams(ArrayRef<ParamPlan> A, ArrayRef<ParamPlan> B) {
  if (A.size() != B.size()) return false;
  for (unsigned I = 0; I < A.size(); ++I)
    if (A[I].Index != B[I].Index || A[I].Layout.Ty != B[I].Layout.Ty ||
        A[I].Layout.Size != B[I].Layout.Size ||
        A[I].Layout.Alignment != B[I].Layout.Alignment ||
        A[I].Layout.AddressSpace != B[I].Layout.AddressSpace)
      return false;
  return true;
}

struct ParameterFunctionPlan {
  Function *Old;
  SmallVector<ParamPlan, 4> Params;
  Function *Replacement = nullptr;
};
struct ParameterCallPlan {
  CallInst *Old;
  SmallVector<ParamPlan, 4> Params;
  bool ProgramOwned;
};

LoadInst *underlyingAggregateLoad(Value *ValueArg, CallInst &Call,
                                  const ResultLayout &Layout) {
  auto *Load = dyn_cast<LoadInst>(ValueArg);
  if (Load && Load->isSimple() &&
      Load->getPointerAddressSpace() == Layout.AddressSpace) {
    // Already found the direct aggregate load.
  } else {
    auto *Extract = dyn_cast<ExtractValueInst>(ValueArg);
    if (!Extract || Extract->getNumIndices() != 1 || *Extract->idx_begin() != 0)
      return nullptr;
    auto *WithCleanup =
        dyn_cast<InsertValueInst>(Extract->getAggregateOperand());
    if (!WithCleanup || WithCleanup->getNumIndices() != 1 ||
        *WithCleanup->idx_begin() != 1)
      return nullptr;
    auto *WithValue =
        dyn_cast<InsertValueInst>(WithCleanup->getAggregateOperand());
    if (!WithValue || WithValue->getNumIndices() != 1 ||
        *WithValue->idx_begin() != 0)
      return nullptr;
    Load = dyn_cast<LoadInst>(WithValue->getInsertedValueOperand());
    if (!Load || !Load->isSimple() ||
        Load->getPointerAddressSpace() != Layout.AddressSpace)
      return nullptr;
  }
  auto *Slot = dyn_cast<AllocaInst>(Load->getPointerOperand()->stripPointerCasts());
  if (!Slot || Slot->getAllocatedType() != Layout.Ty ||
      Slot->getAlign() < Layout.Alignment || Load->getParent() != Call.getParent())
    return nullptr;
  for (Instruction *I = Load->getNextNode(); I && I != &Call;
       I = I->getNextNode())
    if (I->mayWriteToMemory()) return nullptr;
  return Load->comesBefore(&Call) ? Load : nullptr;
}

bool normalizeParameters(Module &M, TargetMachine &TM,
                         ArrayRef<Function *> Selected,
                         std::string &Error) {
  SmallPtrSet<Function *, 32> Owners(Selected.begin(), Selected.end());
  std::vector<ParameterFunctionPlan> Functions;
  std::vector<ParameterCallPlan> Calls;
  DenseMap<Function *, size_t> Index;
  {
    Classifier Classify(TM, M);
    for (Function *F : Selected) {
      SmallVector<ParamPlan, 4> Plans;
      if (!Classify.classifyParams(F->getFunctionType(), F->getCallingConv(),
                                   F->getAttributes(), *F, Plans, Error))
        return false;
      if (Plans.empty()) continue;
      for (const ParamPlan &P : Plans) {
        Argument *Arg = F->getArg(P.Index);
        if (F->isDeclaration()) continue;
        if (!Arg->hasOneUse()) {
          Error = "target-indirect parameter lacks one entry materialization";
          return false;
        }
        auto *Store = dyn_cast<StoreInst>(*Arg->user_begin());
        auto *Slot = Store ? dyn_cast<AllocaInst>(Store->getPointerOperand()) : nullptr;
        auto *Count = Slot ? dyn_cast<ConstantInt>(Slot->getArraySize()) : nullptr;
        if (!Store || !Store->isSimple() || Store->getValueOperand() != Arg ||
            !Slot || Slot->getFunction() != F ||
            Slot->getParent() != &F->getEntryBlock() ||
            Store->getParent() != &F->getEntryBlock() ||
            !Count || !Count->isOne() || !Slot->comesBefore(Store) ||
            Slot->getAllocatedType() != P.Layout.Ty ||
            Slot->getAddressSpace() != P.Layout.AddressSpace ||
            Slot->getAlign() < P.Layout.Alignment ||
            Store->getAlign() < P.Layout.Alignment) {
          Error = "target-indirect parameter has noncanonical entry storage";
          return false;
        }
      }
      Index[F] = Functions.size();
      Functions.push_back(ParameterFunctionPlan{F, std::move(Plans)});
    }
    for (Function &F : M) for (BasicBlock &B : F) for (Instruction &I : B) {
      auto *CB = dyn_cast<CallBase>(&I);
      if (!CB || CB->hasFnAttr(NativeCall)) continue;
      Function *Callee = CB->getCalledFunction();
      if (!(Callee && Owners.contains(Callee)) && !CB->hasFnAttr(OwnedCall))
        continue;
      SmallVector<ParamPlan, 4> Plans;
      if (!Classify.classifyParams(CB->getFunctionType(), CB->getCallingConv(),
                                   CB->getAttributes(), F, Plans, Error))
        return false;
      if (Callee && Index.contains(Callee) &&
          !sameParams(Plans, Functions[Index[Callee]].Params)) {
        Error = "caller and callee disagree on parameter transport";
        return false;
      }
      if (Callee && Owners.contains(Callee) &&
          (Plans.empty() != !Index.contains(Callee))) {
        Error = "caller and callee disagree on direct parameter transport";
        return false;
      }
      if (Plans.empty()) continue;
      auto *Call = dyn_cast<CallInst>(CB);
      if (!Call || Call->isMustTailCall() || Call->hasOperandBundles()) {
        Error = "unsupported target-indirect parameter call edge";
        return false;
      }
      Calls.push_back(ParameterCallPlan{Call, std::move(Plans), true});
    }
  }
  for (auto &P : Functions) {
    Function &Old = *P.Old;
    Function *New = Function::Create(parameterType(Old.getFunctionType(), P.Params),
                                     Old.getLinkage(), Old.getAddressSpace(), "", &M);
    New->copyAttributesFrom(&Old);
    New->setAttributes(parameterAttributes(M.getContext(), Old.getAttributes(),
                                           Old.arg_size(), P.Params));
    New->copyMetadata(&Old, 0);
    New->takeName(&Old);
    auto NewArg = New->arg_begin();
    for (unsigned I = 0; I < Old.arg_size(); ++I, ++NewArg) {
      Argument *OldArg = Old.getArg(I);
      NewArg->takeName(OldArg);
      if (!findParam(P.Params, I)) {
        OldArg->replaceAllUsesWith(&*NewArg);
        continue;
      }
      if (Old.isDeclaration()) continue;
      auto *Store = cast<StoreInst>(*OldArg->user_begin());
      auto *Slot = cast<AllocaInst>(Store->getPointerOperand());
      SmallVector<Instruction *, 2> Lifetimes;
      for (User *User : Slot->users())
        if (auto *Use = dyn_cast<Instruction>(User);
            Use && Use->isLifetimeStartOrEnd())
          Lifetimes.push_back(Use);
      for (Instruction *Lifetime : Lifetimes) Lifetime->eraseFromParent();
      Slot->replaceAllUsesWith(&*NewArg);
      Store->eraseFromParent();
      Slot->eraseFromParent();
    }
    New->splice(New->end(), &Old);
    P.Replacement = New;
  }
  for (auto &P : Calls) {
    CallInst &Old = *P.Old;
    IRBuilder<> Entry(&*Old.getFunction()->getEntryBlock().getFirstInsertionPt());
    IRBuilder<> Builder(&Old);
    Builder.SetCurrentDebugLocation(Old.getDebugLoc());
    SmallVector<Value *, 8> Args;
    SmallVector<AllocaInst *, 4> Scratch;
    for (unsigned I = 0; I < Old.arg_size(); ++I) {
      Value *ValueArg = Old.getArgOperand(I);
      const ParamPlan *Plan = findParam(P.Params, I);
      if (!Plan) { Args.push_back(ValueArg); continue; }
      if (auto *Load = underlyingAggregateLoad(ValueArg, Old, Plan->Layout);
          Load &&
          Load->getType() == Plan->Layout.Ty &&
          Load->getPointerAddressSpace() == Plan->Layout.AddressSpace) {
        Args.push_back(Load->getPointerOperand());
      } else {
        auto *Slot = Entry.CreateAlloca(Plan->Layout.Ty,
                                        Plan->Layout.AddressSpace, nullptr,
                                        "call.byval.storage");
        Slot->setAlignment(Plan->Layout.Alignment);
        Builder.CreateLifetimeStart(Slot);
        Builder.CreateAlignedStore(ValueArg, Slot, Plan->Layout.Alignment);
        Scratch.push_back(Slot);
        Args.push_back(Slot);
      }
    }
    Value *Target = Old.getCalledOperand();
    if (auto It = Index.find(Old.getCalledFunction()); It != Index.end())
      Target = Functions[It->second].Replacement;
    auto *Call = Builder.CreateCall(parameterType(Old.getFunctionType(), P.Params),
                                    Target, Args);
    Call->setCallingConv(Old.getCallingConv());
    Call->setAttributes(parameterAttributes(M.getContext(), Old.getAttributes(),
                                            Old.arg_size(), P.Params));
    Call->setDebugLoc(Old.getDebugLoc());
    if (!Old.getType()->isVoidTy()) Old.replaceAllUsesWith(Call);
    for (AllocaInst *Slot : llvm::reverse(Scratch)) Builder.CreateLifetimeEnd(Slot);
    Old.eraseFromParent();
  }
  for (auto &P : Functions) {
    P.Old->replaceAllUsesWith(P.Replacement);
    P.Old->eraseFromParent();
  }
  return true;
}

bool splitTaggedResultLoads(Module &M, std::string &Error) {
  SmallVector<LoadInst *, 16> Loads;
  for (Function &F : M) for (BasicBlock &B : F) for (Instruction &I : B) {
    auto *Load = dyn_cast<LoadInst>(&I);
    auto *Struct = Load ? dyn_cast<StructType>(Load->getType()) : nullptr;
    if (!Load || !Load->isSimple() || !Struct || Struct->getNumElements() < 1 ||
        !(Struct->getElementType(0)->isIntegerTy(8) ||
          Struct->getElementType(0)->isIntegerTy(32)))
      continue;
    Value *Pointer = Load->getPointerOperand();
    SmallVector<CallBase *, 2> Producers;
    for (User *User : Pointer->users()) {
      auto *Call = dyn_cast<CallBase>(User);
      if (!Call) continue;
      bool ThisSRet = false;
      for (unsigned A = 0; A < Call->arg_size(); ++A)
        if (Call->getArgOperand(A) == Pointer &&
            Call->paramHasAttr(A, Attribute::StructRet) &&
            Call->getParamStructRetType(A) == Struct &&
            Call->getParent() == Load->getParent() &&
            Call->comesBefore(Load))
          ThisSRet = true;
      if (ThisSRet)
        Producers.push_back(Call);
    }
    if (Producers.size() != 1) continue;
    CallBase *Producer = Producers.front();
    SmallVector<StoreInst *, 4> LaterWrites;
    bool UnsupportedPointerUse = false;
    for (User *User : Pointer->users()) {
      if (User == Load || User == Producer) continue;
      auto *Use = dyn_cast<Instruction>(User);
      if (Use && Use->isLifetimeStartOrEnd()) continue;
      auto *Store = dyn_cast<StoreInst>(Use);
      if (!Store || !Store->isSimple() ||
          !isa<ConstantAggregateZero>(Store->getValueOperand())) {
        UnsupportedPointerUse = true;
        continue;
      }
      if (Store->getParent() == Producer->getParent() &&
          Store->comesBefore(Producer))
        continue;
      LaterWrites.push_back(Store);
    }
    DominatorTree Dominators(*Load->getFunction());
    for (User *User : Load->users()) {
      auto *Use = dyn_cast<Instruction>(User);
      if (!Use) {
        UnsupportedPointerUse = true;
        continue;
      }
      for (StoreInst *Write : LaterWrites)
        if (isPotentiallyReachable(Write, Use, nullptr, &Dominators))
          UnsupportedPointerUse = true;
    }
    // Moving payload reloads to selected arms is valid only while the result
    // slot has no alias or write other than its one producer call.
    if (UnsupportedPointerUse) continue;
    bool HasTag = llvm::any_of(Load->users(), [](User *User) {
      auto *Extract = dyn_cast<ExtractValueInst>(User);
      return Extract && Extract->getNumIndices() == 1 &&
             *Extract->idx_begin() == 0;
    });
    if (HasTag) Loads.push_back(Load);
  }
  for (LoadInst *Load : Loads) {
    auto *Struct = cast<StructType>(Load->getType());
    Value *Pointer = Load->getPointerOperand();
    SmallVector<User *, 8> Users(Load->user_begin(), Load->user_end());
    for (User *User : Users) {
      auto *InstructionUser = dyn_cast<Instruction>(User);
      if (!InstructionUser || isa<PHINode>(InstructionUser)) {
        Error = "tagged indirect result has a nonlocal SSA user";
        return false;
      }
      if (auto *Extract = dyn_cast<ExtractValueInst>(InstructionUser);
          Extract && Extract->getNumIndices() == 1 &&
          *Extract->idx_begin() == 0) {
        IRBuilder<> Builder(Extract);
        Value *TagPointer = Builder.CreateStructGEP(Struct, Pointer, 0,
                                                    "call.tag.pointer");
        Type *TagTy = Struct->getElementType(0);
        Value *Tag = Builder.CreateAlignedLoad(
            TagTy, TagPointer, M.getDataLayout().getABITypeAlign(TagTy),
            "call.tag");
        Extract->replaceAllUsesWith(Tag);
        Extract->eraseFromParent();
      } else {
        IRBuilder<> Builder(InstructionUser);
        Value *Reload = Builder.CreateAlignedLoad(
            Struct, Pointer, Load->getAlign(), "call.payload");
        InstructionUser->replaceUsesOfWith(Load, Reload);
      }
    }
    if (!Load->use_empty()) {
      Error = "tagged indirect result retained a pre-branch aggregate use";
      return false;
    }
    Load->eraseFromParent();
  }
  return true;
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
  std::vector<CleanupFunctionPlan> CleanupFunctions;
  std::vector<CleanupCallPlan> CleanupCalls;
  DenseMap<Function *, size_t> Index;
  DenseMap<Function *, size_t> CleanupIndex;
  // All classification and refusal happens before mutation. Destruction of
  // MachineModuleInfo precedes moving/erasing the original IR functions.
  {
    Classifier Classify(TM, M);
    for (Function &F : M) {
      if (!Selected.contains(&F)) continue;
      StructType *Pair = cleanupPair(F.getFunctionType(), F.getAttributes());
      std::optional<ResultLayout> L;
      if (!Classify.classify(F.getFunctionType(), F.getCallingConv(),
                             F.getAttributes(), F, L, Error)) return false;
      if (Pair) {
        if (!L) continue;
        Type *ValueTy = Pair->getElementType(0);
        auto ValueLayout = storageLayout(ValueTy, M.getDataLayout(), Error);
        if (!ValueLayout) return false;
        auto *ValueFT = FunctionType::get(ValueTy,
                                          F.getFunctionType()->params(), false);
        std::optional<ResultLayout> ValueIndirect;
        if (!Classify.classify(ValueFT, F.getCallingConv(), F.getAttributes(),
                               F, ValueIndirect, Error)) return false;
        CleanupIndex[&F] = CleanupFunctions.size();
        CleanupFunctions.push_back(CleanupFunctionPlan{
            &F, Pair, *L, *ValueLayout, ValueIndirect.has_value()});
        continue;
      }
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
      AttributeList EffectiveAttrs = CB->getAttributes();
      if (Callee && Callee->hasFnAttribute(CleanupCall))
        EffectiveAttrs = EffectiveAttrs.addFnAttribute(M.getContext(), CleanupCall);
      StructType *Pair = cleanupPair(CB->getFunctionType(), EffectiveAttrs);
      std::optional<ResultLayout> L;
      if (!Classify.classify(CB->getFunctionType(), CB->getCallingConv(),
                             EffectiveAttrs, F, L, Error)) return false;
      if (Pair) {
        if (Callee && Selected.contains(Callee) &&
            (L.has_value() != CleanupIndex.contains(Callee))) {
          Error = "caller and callee disagree on cleanup transport"; return false;
        }
        if (!L) continue;
        auto *Call = dyn_cast<CallInst>(CB);
        if (!Call || Call->isMustTailCall() || Call->hasOperandBundles()) {
          Error = "unsupported split cleanup call edge"; return false;
        }
        Type *ValueTy = Pair->getElementType(0);
        auto ValueLayout = storageLayout(ValueTy, M.getDataLayout(), Error);
        if (!ValueLayout) return false;
        auto *ValueFT = FunctionType::get(ValueTy,
                                           CB->getFunctionType()->params(), false);
        std::optional<ResultLayout> ValueIndirect;
        if (!Classify.classify(ValueFT, CB->getCallingConv(), EffectiveAttrs,
                               F, ValueIndirect, Error)) return false;
        if (Callee && CleanupIndex.contains(Callee) &&
            ValueIndirect.has_value() !=
                CleanupFunctions[CleanupIndex[Callee]].ValueIndirect) {
          Error = "caller and callee disagree on cleanup value transport";
          return false;
        }
        CleanupCalls.push_back(CleanupCallPlan{
            Call, Pair, *L, *ValueLayout, ValueIndirect.has_value(),
            !CB->hasFnAttr(NativeCall),
            ValueIndirect ? cleanupMaterializingStore(*Call, *ValueLayout)
                          : nullptr});
        continue;
      }
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
  for (auto &P : CleanupFunctions) {
    Function &Old = *P.Old;
    auto *New = Function::Create(
        cleanupType(Old.getFunctionType(), P.ValueLayout, P.ValueIndirect),
        Old.getLinkage(), Old.getAddressSpace(), "", &M);
    New->copyAttributesFrom(&Old);
    New->setAttributes(cleanupAttributes(
        M.getContext(), Old.getAttributes(), Old.arg_size(), P.ValueLayout,
        P.ValueIndirect));
    New->copyMetadata(&Old, 0);
    New->takeName(&Old);
    unsigned Hidden = P.ValueIndirect ? 2 : 1;
    if (P.ValueIndirect) New->getArg(0)->setName("result.destination");
    New->getArg(Hidden - 1)->setName("cleanup.destination");
    auto NewArg = New->arg_begin();
    std::advance(NewArg, Hidden);
    for (Argument &Arg : Old.args()) {
      NewArg->takeName(&Arg);
      Arg.replaceAllUsesWith(&*NewArg++);
    }
    New->splice(New->end(), &Old);
    for (BasicBlock &B : *New) {
      auto *Ret = dyn_cast<ReturnInst>(B.getTerminator());
      if (!Ret) continue;
      Value *Pair = Ret->getReturnValue();
      if (!Pair) { Error = "cleanup return omitted its pair"; return false; }
      IRBuilder<> Builder(Ret);
      Builder.SetCurrentDebugLocation(Ret->getDebugLoc());
      Value *ValuePart = Builder.CreateExtractValue(Pair, 0, "return.value");
      Value *CleanupBit = Builder.CreateExtractValue(Pair, 1, "return.cleanup");
      Value *CleanupByte = Builder.CreateZExt(CleanupBit,
                                              Type::getInt8Ty(M.getContext()),
                                              "return.cleanup.byte");
      Builder.CreateAlignedStore(CleanupByte, New->getArg(Hidden - 1), Align(1));
      if (P.ValueIndirect) {
        Builder.CreateAlignedStore(ValuePart, New->getArg(0),
                                   P.ValueLayout.Alignment);
        Builder.CreateRetVoid();
      } else {
        Builder.CreateRet(ValuePart);
      }
      Ret->eraseFromParent();
    }
    P.Replacement = New;
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
  for (auto &P : CleanupCalls) {
    CallInst &Old = *P.Old;
    IRBuilder<> Entry(&*Old.getFunction()->getEntryBlock().getFirstInsertionPt());
    auto *CleanupSlot = Entry.CreateAlloca(Type::getInt8Ty(M.getContext()),
                                           P.ValueLayout.AddressSpace, nullptr,
                                           "call.cleanup.storage");
    CleanupSlot->setAlignment(Align(1));
    AllocaInst *OwnedValueSlot = nullptr;
    Value *ValueSlot = P.Materialize ? P.Materialize->getPointerOperand() : nullptr;
    if (P.ValueIndirect && !ValueSlot) {
      ValueSlot = Entry.CreateAlloca(P.ValueLayout.Ty,
                                     P.ValueLayout.AddressSpace, nullptr,
                                     "call.value.storage");
      OwnedValueSlot = cast<AllocaInst>(ValueSlot);
      OwnedValueSlot->setAlignment(P.ValueLayout.Alignment);
    }
    IRBuilder<> Builder(&Old);
    Builder.SetCurrentDebugLocation(Old.getDebugLoc());
    if (P.ProgramOwned) {
      Builder.CreateLifetimeStart(CleanupSlot);
      if (OwnedValueSlot) Builder.CreateLifetimeStart(OwnedValueSlot);
    }
    SmallVector<Value *, 8> Args;
    if (ValueSlot) Args.push_back(ValueSlot);
    Args.push_back(CleanupSlot);
    for (Use &Arg : Old.args()) Args.push_back(Arg.get());
    Value *Target = Old.getCalledOperand();
    if (auto It = CleanupIndex.find(Old.getCalledFunction());
        It != CleanupIndex.end())
      Target = CleanupFunctions[It->second].Replacement;
    auto *Call = Builder.CreateCall(
        cleanupType(Old.getFunctionType(), P.ValueLayout, P.ValueIndirect),
        Target, Args);
    Call->setCallingConv(Old.getCallingConv());
    Call->setAttributes(cleanupAttributes(
        M.getContext(), Old.getAttributes(), Old.arg_size(), P.ValueLayout,
        P.ValueIndirect, P.ProgramOwned));
    Call->setDebugLoc(Old.getDebugLoc());
    Value *CleanupByte = Builder.CreateAlignedLoad(Type::getInt8Ty(M.getContext()),
                                                   CleanupSlot, Align(1),
                                                   "call.cleanup.byte");
    Value *CleanupBit = Builder.CreateTrunc(CleanupByte,
                                            Type::getInt1Ty(M.getContext()),
                                            "call.cleanup.bit");
    if (P.Materialize) {
      auto *ValueExtract = cast<ExtractValueInst>(P.Materialize->getValueOperand());
      P.Materialize->eraseFromParent();
      SmallVector<User *, 8> ValueUsers(ValueExtract->user_begin(),
                                        ValueExtract->user_end());
      for (User *User : ValueUsers) {
        auto *InstructionUser = dyn_cast<Instruction>(User);
        if (!InstructionUser || isa<PHINode>(InstructionUser)) {
          Error = "materialized cleanup value has a nonlocal SSA user";
          return false;
        }
        if (auto *Field = dyn_cast<ExtractValueInst>(InstructionUser);
            Field && Field->getNumIndices() == 1 &&
            *Field->idx_begin() == 0 &&
            isa<StructType>(P.ValueLayout.Ty)) {
          IRBuilder<> AtField(Field);
          auto *Struct = cast<StructType>(P.ValueLayout.Ty);
          Value *Pointer = AtField.CreateStructGEP(Struct, ValueSlot, 0,
                                                   "call.tag.pointer");
          Value *Tag = AtField.CreateAlignedLoad(Struct->getElementType(0), Pointer,
                                                 Align(1), "call.tag");
          Field->replaceAllUsesWith(Tag);
          Field->eraseFromParent();
          continue;
        }
        IRBuilder<> AtUse(InstructionUser);
        Value *Reload = AtUse.CreateAlignedLoad(P.ValueLayout.Ty, ValueSlot,
                                                P.ValueLayout.Alignment,
                                                "call.value");
        InstructionUser->replaceUsesOfWith(ValueExtract, Reload);
      }
      if (!ValueExtract->use_empty()) {
        Error = "materialized cleanup value retained an SSA use";
        return false;
      }
      ValueExtract->eraseFromParent();
      SmallVector<ExtractValueInst *, 4> CleanupExtracts;
      for (User *User : Old.users()) {
        auto *Extract = dyn_cast<ExtractValueInst>(User);
        if (!Extract || Extract->getNumIndices() != 1 ||
            *Extract->idx_begin() != 1) {
          Error = "materialized cleanup pair has an unsupported use";
          return false;
        }
        CleanupExtracts.push_back(Extract);
      }
      for (ExtractValueInst *Extract : CleanupExtracts) {
        Extract->replaceAllUsesWith(CleanupBit);
        Extract->eraseFromParent();
      }
    } else {
      Value *ValuePart = P.ValueIndirect
                             ? Builder.CreateAlignedLoad(P.ValueLayout.Ty, ValueSlot,
                                                         P.ValueLayout.Alignment,
                                                         "call.value")
                             : static_cast<Value *>(Call);
      Value *Pair = PoisonValue::get(P.PairTy);
      Pair = Builder.CreateInsertValue(Pair, ValuePart, 0, "call.pair.value");
      Pair = Builder.CreateInsertValue(Pair, CleanupBit, 1, "call.pair.cleanup");
      Old.replaceAllUsesWith(Pair);
    }
    if (P.ProgramOwned) {
      if (OwnedValueSlot) Builder.CreateLifetimeEnd(OwnedValueSlot);
      Builder.CreateLifetimeEnd(CleanupSlot);
    }
    Old.eraseFromParent();
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
  SmallVector<Function *, 32> ParameterOwners;
  for (Function *F : Selected) {
    if (!F->hasFnAttribute(ParameterOwner)) continue;
    if (auto It = Index.find(F); It != Index.end())
      ParameterOwners.push_back(Functions[It->second].Replacement);
    else if (auto It = CleanupIndex.find(F); It != CleanupIndex.end())
      ParameterOwners.push_back(CleanupFunctions[It->second].Replacement);
    else
      ParameterOwners.push_back(F);
  }
  for (auto &P : Functions) {
    P.Old->replaceAllUsesWith(P.Replacement);
    P.Old->eraseFromParent();
  }
  for (auto &P : CleanupFunctions) {
    P.Old->replaceAllUsesWith(P.Replacement);
    P.Old->eraseFromParent();
  }
  if (!normalizeParameters(M, TM, ParameterOwners, Error)) return false;
  if (!splitTaggedResultLoads(M, Error)) return false;
  // Source-owned markers are invocation-local and never enter bitcode/cache.
  for (Function &F : M) for (BasicBlock &B : F) for (Instruction &I : B)
    if (auto *Call = dyn_cast<CallBase>(&I)) {
      Call->removeFnAttr(OwnedCall);
      Call->removeFnAttr(NativeCall);
      Call->removeFnAttr(CleanupCall);
    }
  for (Function &F : M) F.removeFnAttr(CleanupCall);
  for (Function &F : M) F.removeFnAttr(ParameterOwner);
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
  ValueToValueMapTy Map;
  std::unique_ptr<Module> Candidate = CloneModule(M, Map);
  SmallVector<LLVMValueRef, 32> CandidateOwned;
  CandidateOwned.reserve(Count);
  for (size_t I = 0; I < Count; ++I) {
    auto *Original = Owned[I] ? dyn_cast<Function>(unwrap(Owned[I])) : nullptr;
    auto It = Original ? Map.find(Original) : Map.end();
    auto *Cloned = It == Map.end() ? nullptr : dyn_cast<Function>(It->second);
    if (!Cloned) {
      *ErrorOut = LLVMCreateMessage("return owner was not cloned"); return 1;
    }
    CandidateOwned.push_back(wrap(Cloned));
  }
  if (!normalize(*Candidate, *reinterpret_cast<TargetMachine *>(TMRef),
                  CandidateOwned, Error)) {
    *ErrorOut = LLVMCreateMessage(Error.c_str()); return 1;
  }
  Error.clear();
  if (verifyModule(*Candidate, &Stream)) {
    *ErrorOut = LLVMCreateMessage(Error.c_str()); return 1;
  }
  // The Rust side deliberately drops every FunctionValue/Argument handle before
  // this last module operation. Moving the verified candidate into the stable
  // Module object makes every refusal transactional: malformed input cannot
  // leave a half-rewritten module behind.
  M = std::move(*Candidate);
  return 0;
}
