//! Structural enumeration of explicit MIR operand uses. An enum declaration emits both its
//! ordinary type and the traversal, so adding a field cannot omit it from the use proof.
use super::*;

pub(crate) trait OperandInputs {
    fn operands_satisfy(&self, check: &mut impl FnMut(&Operand) -> bool) -> bool;
}

impl OperandInputs for Operand {
    fn operands_satisfy(&self, check: &mut impl FnMut(&Operand) -> bool) -> bool {
        check(self)
            && match self {
                Operand::BorrowedElementPlace(place) => {
                    place.index.operands_satisfy(check) && place.guard.len.operands_satisfy(check)
                }
                Operand::Value(_)
                | Operand::Arg(_)
                | Operand::Const(_)
                | Operand::BorrowedPlace(_)
                | Operand::BorrowedCleanupArg(_)
                | Operand::BorrowedFixedElementPlace(_) => true,
            }
    }
}
impl<T: OperandInputs> OperandInputs for Vec<T> {
    fn operands_satisfy(&self, check: &mut impl FnMut(&Operand) -> bool) -> bool {
        self.iter().all(|v| v.operands_satisfy(check))
    }
}
impl<T: OperandInputs> OperandInputs for Option<T> {
    fn operands_satisfy(&self, check: &mut impl FnMut(&Operand) -> bool) -> bool {
        self.as_ref().is_none_or(|v| v.operands_satisfy(check))
    }
}
impl<T: OperandInputs> OperandInputs for Box<T> {
    fn operands_satisfy(&self, check: &mut impl FnMut(&Operand) -> bool) -> bool {
        self.as_ref().operands_satisfy(check)
    }
}
impl<A: OperandInputs, B: OperandInputs> OperandInputs for (A, B) {
    fn operands_satisfy(&self, check: &mut impl FnMut(&Operand) -> bool) -> bool {
        self.0.operands_satisfy(check) && self.1.operands_satisfy(check)
    }
}

// Scalar metadata and static declarations contain no executable operands. Numeric SSA
// definition fields (Let and cleanup results) belong to the definition inventory.
macro_rules! metadata {
    ($($ty:ty),+ $(,)?) => { $(
        impl OperandInputs for $ty {
            #[inline]
            fn operands_satisfy(&self, _: &mut impl FnMut(&Operand) -> bool) -> bool { true }
        }
    )+ }
}
metadata!(
    bool,
    u8,
    u16,
    u32,
    u64,
    i8,
    i32,
    i64,
    i128,
    usize,
    String,
    BinOp,
    UnOp,
    Ty,
    Scalar,
    ConstElem,
    FnSignatureFacts,
    DirectCall,
    ProgramCall,
    StaticData,
    SqliteCallbackDescriptor,
    FsTreeOutput,
    ParMapStageKind,
    align_sema::ArithMode,
    align_sema::MathFn,
    align_sema::SignatureAlgorithm,
    align_sema::fs_tree::FsTreeKind,
    align_sema::process_live::ProcessLiveKind,
    hir::AeadCipher,
    hir::AeadDir,
    hir::CliFlagKind,
    BitsetOp,
    hir::CodecPutKind,
    hir::CompressKind,
    hir::EncodingKind,
    hir::FloatMode,
    hir::FrameJoinKind,
    hir::GroupOp,
    hir::HashAlgo,
    hir::JsonDocCollection,
    hir::JsonOwnedRoot,
    hir::OwnedJsonGraphPlanV4,
    hir::PathComponentKind,
    hir::ResourceViewKind,
    hir::StrPredKind,
    hir::StrTrimKind,
    hir::TimeFormatKind
);

macro_rules! mir_inputs_struct {
    ($(#[$doc:meta])* pub struct $name:ident {
        $($(#[$attr:meta])* pub $field:ident: $ty:ty),* $(,)?
    }) => {
        $(#[$doc])* pub struct $name { $($(#[$attr])* pub $field: $ty),* }
        impl crate::operand_inputs::OperandInputs for $name {
            fn operands_satisfy(&self, check: &mut impl FnMut(&Operand) -> bool) -> bool {
                true $(&& self.$field.operands_satisfy(check))*
            }
        }
    };
}
macro_rules! mir_inputs_enum {
    ($(#[$doc:meta])* pub enum $name:ident {
        $($(#[$attr:meta])* $variant:ident
            $( ($($pos:ty),* $(,)?) )?
            $( { $($(#[$field_attr:meta])* $field:ident: $ty:ty),* $(,)? } )?
        ),* $(,)?
    }) => {
        $(#[$doc])* pub enum $name {
            $($(#[$attr])* $variant $( ($($pos),*) )? $( { $($(#[$field_attr])* $field: $ty),* } )?),*
        }
        impl crate::operand_inputs::OperandInputs for $name {
            fn operands_satisfy(&self, check: &mut impl FnMut(&Operand) -> bool) -> bool {
                match self {
                    $(Self::$variant $( ( $(mir_inputs_enum!(@ignore $pos)),* ) )? $( { $($field),* } )? => {
                        true $( && mir_inputs_enum!(@tuple self, Self::$variant, check; $($pos),*) )?
                             $( $(&& $field.operands_satisfy(check))* )?
                    }),*
                }
            }
        }
    };
    (@ignore $ty:ty) => { _ };
    (@tuple $s:ident, $v:path, $c:ident; $a:ty) => { matches!($s, $v(a) if a.operands_satisfy($c)) };
    (@tuple $s:ident, $v:path, $c:ident; $a:ty, $b:ty) => {
        matches!($s, $v(a,b) if a.operands_satisfy($c) && b.operands_satisfy($c))
    };
    (@tuple $s:ident, $v:path, $c:ident; $a:ty, $b:ty, $d:ty) => {
        matches!($s, $v(a,b,d) if a.operands_satisfy($c) && b.operands_satisfy($c) && d.operands_satisfy($c))
    };
    (@tuple $s:ident, $v:path, $c:ident; $a:ty, $b:ty, $d:ty, $e:ty) => {
        matches!($s, $v(a,b,d,e) if a.operands_satisfy($c) && b.operands_satisfy($c) && d.operands_satisfy($c) && e.operands_satisfy($c))
    };
}
