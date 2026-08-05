use std::fmt;

use crate::{CanonicalCodecError, CanonicalFnAbi, CanonicalTy, ProgramCall, RuntimeKey};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DirectCall {
    Program(ProgramCall),
    Runtime(RuntimeKey),
}

impl fmt::Display for DirectCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Program(target) => target.fmt(f),
            Self::Runtime(key) => f.write_str(key.logical_name()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum ParallelKernelMode {
    Materialize = 0,
    Reduce = 1,
    FilterCount = 2,
    FilterScatter = 3,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ParallelStageId {
    Map {
        target: ProgramCall,
        abi: CanonicalFnAbi,
        input: CanonicalTy,
        output: CanonicalTy,
        captures: Vec<CanonicalTy>,
    },
    Filter {
        target: ProgramCall,
        abi: CanonicalFnAbi,
        input: CanonicalTy,
        output: CanonicalTy,
        captures: Vec<CanonicalTy>,
    },
    FilterStrContains {
        input: CanonicalTy,
        output: CanonicalTy,
        needle: CanonicalTy,
    },
    Project {
        input: CanonicalTy,
        output: CanonicalTy,
        field: u32,
    },
    FilterField {
        input: CanonicalTy,
        output: CanonicalTy,
        field: u32,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ParallelGeneratedId {
    pub mode: ParallelKernelMode,
    pub source: CanonicalTy,
    pub terminal_input: CanonicalTy,
    pub terminal_output: CanonicalTy,
    pub terminal: ProgramCall,
    pub terminal_abi: CanonicalFnAbi,
    pub terminal_captures: Vec<CanonicalTy>,
    pub stages: Vec<ParallelStageId>,
    pub work_weight: u8,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GeneratedId {
    FnValue {
        target: ProgramCall,
        signature: CanonicalFnAbi,
    },
    Closure {
        lifted: ProgramCall,
        explicit_signature: CanonicalFnAbi,
        captures: Vec<CanonicalTy>,
    },
    Task {
        fallible: bool,
        result: CanonicalTy,
    },
    Parallel(ParallelGeneratedId),
}

impl GeneratedId {
    pub fn to_canonical_bytes(&self) -> Result<Box<[u8]>, CanonicalCodecError> {
        validate_generated(self)?;
        let mut out = Vec::new();
        out.push(1);
        match self {
            Self::FnValue { target, signature } => {
                out.push(0);
                encode_call(&mut out, target)?;
                out.extend(signature.as_bytes());
            }
            Self::Closure {
                lifted,
                explicit_signature,
                captures,
            } => {
                out.push(1);
                encode_call(&mut out, lifted)?;
                out.extend(explicit_signature.as_bytes());
                encode_types(&mut out, captures)?;
            }
            Self::Task { fallible, result } => {
                out.push(2);
                out.push(u8::from(*fallible));
                out.extend(result.as_bytes());
            }
            Self::Parallel(parallel) => {
                out.push(3);
                encode_parallel(&mut out, parallel)?;
            }
        }
        Ok(out.into_boxed_slice())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalCodecError> {
        let mut cursor = IdentityCursor::new(bytes);
        if cursor.byte()? != 1 {
            return Err(CanonicalCodecError::UnsupportedVersion);
        }
        let value = match cursor.byte()? {
            0 => Self::FnValue {
                target: cursor.call()?,
                signature: cursor.abi()?,
            },
            1 => Self::Closure {
                lifted: cursor.call()?,
                explicit_signature: cursor.abi()?,
                captures: cursor.types()?,
            },
            2 => Self::Task {
                fallible: cursor.boolean()?,
                result: cursor.ty()?,
            },
            3 => Self::Parallel(cursor.parallel()?),
            _ => return Err(CanonicalCodecError::UnknownTag),
        };
        validate_generated(&value)?;
        if cursor.offset != bytes.len() {
            return Err(CanonicalCodecError::TrailingBytes);
        }
        Ok(value)
    }
}

fn checked_count(len: usize) -> Result<u32, CanonicalCodecError> {
    u32::try_from(len).map_err(|_| CanonicalCodecError::InvalidCount)
}

fn encode_call(out: &mut Vec<u8>, value: &ProgramCall) -> Result<(), CanonicalCodecError> {
    out.extend(checked_count(value.as_bytes().len())?.to_le_bytes());
    out.extend(value.as_bytes());
    Ok(())
}

fn encode_types(out: &mut Vec<u8>, values: &[CanonicalTy]) -> Result<(), CanonicalCodecError> {
    out.extend(checked_count(values.len())?.to_le_bytes());
    for value in values {
        out.extend(value.as_bytes());
    }
    Ok(())
}

fn encode_parallel(
    out: &mut Vec<u8>,
    value: &ParallelGeneratedId,
) -> Result<(), CanonicalCodecError> {
    out.push(value.mode as u8);
    out.extend(value.source.as_bytes());
    out.extend(value.terminal_input.as_bytes());
    out.extend(value.terminal_output.as_bytes());
    encode_call(out, &value.terminal)?;
    out.extend(value.terminal_abi.as_bytes());
    encode_types(out, &value.terminal_captures)?;
    out.extend(checked_count(value.stages.len())?.to_le_bytes());
    for stage in &value.stages {
        encode_stage(out, stage)?;
    }
    out.push(value.work_weight);
    Ok(())
}

fn encode_stage(out: &mut Vec<u8>, value: &ParallelStageId) -> Result<(), CanonicalCodecError> {
    match value {
        ParallelStageId::Map {
            target,
            abi,
            input,
            output,
            captures,
        }
        | ParallelStageId::Filter {
            target,
            abi,
            input,
            output,
            captures,
        } => {
            out.push(u8::from(matches!(value, ParallelStageId::Filter { .. })));
            encode_call(out, target)?;
            out.extend(abi.as_bytes());
            out.extend(input.as_bytes());
            out.extend(output.as_bytes());
            encode_types(out, captures)?;
        }
        ParallelStageId::FilterStrContains {
            input,
            output,
            needle,
        } => {
            out.push(2);
            out.extend(input.as_bytes());
            out.extend(output.as_bytes());
            out.extend(needle.as_bytes());
        }
        ParallelStageId::Project {
            input,
            output,
            field,
        }
        | ParallelStageId::FilterField {
            input,
            output,
            field,
        } => {
            out.push(if matches!(value, ParallelStageId::Project { .. }) {
                3
            } else {
                4
            });
            out.extend(input.as_bytes());
            out.extend(output.as_bytes());
            out.extend(field.to_le_bytes());
        }
    }
    Ok(())
}

fn validate_generated(value: &GeneratedId) -> Result<(), CanonicalCodecError> {
    match value {
        GeneratedId::FnValue { target, .. } => validate_call(target),
        GeneratedId::Closure { lifted, .. } => validate_call(lifted),
        GeneratedId::Task { .. } => Ok(()),
        GeneratedId::Parallel(value) => validate_parallel(value),
    }
}

fn validate_call(value: &ProgramCall) -> Result<(), CanonicalCodecError> {
    if value.as_bytes().is_empty() || value.as_bytes().contains(&0) {
        Err(CanonicalCodecError::InvalidGraph)
    } else if u32::try_from(value.as_bytes().len()).is_err() {
        Err(CanonicalCodecError::InvalidCount)
    } else {
        Ok(())
    }
}

fn validate_parallel(value: &ParallelGeneratedId) -> Result<(), CanonicalCodecError> {
    validate_call(&value.terminal)?;
    checked_count(value.terminal_captures.len())?;
    checked_count(value.stages.len())?;
    let has_filter = value.stages.iter().any(|stage| {
        matches!(
            stage,
            ParallelStageId::Filter { .. }
                | ParallelStageId::FilterStrContains { .. }
                | ParallelStageId::FilterField { .. }
        )
    });
    let mode_valid = match value.mode {
        ParallelKernelMode::Materialize => !has_filter,
        ParallelKernelMode::Reduce => value.stages.is_empty(),
        ParallelKernelMode::FilterCount | ParallelKernelMode::FilterScatter => has_filter,
    };
    if !mode_valid || !matches!(value.work_weight, 1 | 2 | 4) {
        return Err(CanonicalCodecError::InvalidGraph);
    }
    for stage in &value.stages {
        match stage {
            ParallelStageId::Map {
                target, captures, ..
            }
            | ParallelStageId::Filter {
                target, captures, ..
            } => {
                validate_call(target)?;
                checked_count(captures.len())?;
            }
            ParallelStageId::FilterStrContains { .. }
            | ParallelStageId::Project { .. }
            | ParallelStageId::FilterField { .. } => {}
        }
    }
    Ok(())
}

struct IdentityCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> IdentityCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn byte(&mut self) -> Result<u8, CanonicalCodecError> {
        let value = self
            .bytes
            .get(self.offset)
            .copied()
            .ok_or(CanonicalCodecError::Truncated)?;
        self.offset += 1;
        Ok(value)
    }

    fn boolean(&mut self) -> Result<bool, CanonicalCodecError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CanonicalCodecError::InvalidBool),
        }
    }

    fn u32(&mut self) -> Result<u32, CanonicalCodecError> {
        let end = self
            .offset
            .checked_add(4)
            .ok_or(CanonicalCodecError::Truncated)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(CanonicalCodecError::Truncated)?;
        self.offset = end;
        Ok(u32::from_le_bytes(
            bytes
                .try_into()
                .map_err(|_| CanonicalCodecError::Truncated)?,
        ))
    }

    fn count(&mut self, minimum_bytes: usize) -> Result<usize, CanonicalCodecError> {
        let count = self.u32()? as usize;
        if minimum_bytes != 0
            && count > self.bytes.len().saturating_sub(self.offset) / minimum_bytes
        {
            return Err(CanonicalCodecError::Truncated);
        }
        Ok(count)
    }

    fn call(&mut self) -> Result<ProgramCall, CanonicalCodecError> {
        let len = self.count(1)?;
        let end = self
            .offset
            .checked_add(len)
            .ok_or(CanonicalCodecError::Truncated)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(CanonicalCodecError::Truncated)?;
        self.offset = end;
        let value = std::str::from_utf8(bytes).map_err(|_| CanonicalCodecError::InvalidUtf8)?;
        ProgramCall::try_from_logical(value).map_err(|error| match error {
            crate::ProgramCallError::Empty => CanonicalCodecError::InvalidGraph,
            crate::ProgramCallError::EmbeddedNul => CanonicalCodecError::EmbeddedNul,
            crate::ProgramCallError::TooLong => CanonicalCodecError::InvalidCount,
        })
    }

    fn ty(&mut self) -> Result<CanonicalTy, CanonicalCodecError> {
        let bytes = self
            .bytes
            .get(self.offset..)
            .ok_or(CanonicalCodecError::Truncated)?;
        let len = crate::canonical_graph::canonical_type_record_len(bytes)?;
        let end = self
            .offset
            .checked_add(len)
            .ok_or(CanonicalCodecError::Truncated)?;
        let value = CanonicalTy::decode(
            self.bytes
                .get(self.offset..end)
                .ok_or(CanonicalCodecError::Truncated)?,
        )?;
        self.offset = end;
        Ok(value)
    }

    fn abi(&mut self) -> Result<CanonicalFnAbi, CanonicalCodecError> {
        let bytes = self
            .bytes
            .get(self.offset..)
            .ok_or(CanonicalCodecError::Truncated)?;
        let len = crate::canonical_graph::canonical_fn_abi_record_len(bytes)?;
        let end = self
            .offset
            .checked_add(len)
            .ok_or(CanonicalCodecError::Truncated)?;
        let value = CanonicalFnAbi::decode(
            self.bytes
                .get(self.offset..end)
                .ok_or(CanonicalCodecError::Truncated)?,
        )?;
        self.offset = end;
        Ok(value)
    }

    fn types(&mut self) -> Result<Vec<CanonicalTy>, CanonicalCodecError> {
        let count = self.count(6)?;
        let mut values = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            values.push(self.ty()?);
        }
        Ok(values)
    }

    fn parallel(&mut self) -> Result<ParallelGeneratedId, CanonicalCodecError> {
        let mode = match self.byte()? {
            0 => ParallelKernelMode::Materialize,
            1 => ParallelKernelMode::Reduce,
            2 => ParallelKernelMode::FilterCount,
            3 => ParallelKernelMode::FilterScatter,
            _ => return Err(CanonicalCodecError::UnknownTag),
        };
        let source = self.ty()?;
        let terminal_input = self.ty()?;
        let terminal_output = self.ty()?;
        let terminal = self.call()?;
        let terminal_abi = self.abi()?;
        let terminal_captures = self.types()?;
        let count = self.count(1)?;
        let mut stages = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            stages.push(self.stage()?);
        }
        let work_weight = self.byte()?;
        Ok(ParallelGeneratedId {
            mode,
            source,
            terminal_input,
            terminal_output,
            terminal,
            terminal_abi,
            terminal_captures,
            stages,
            work_weight,
        })
    }

    fn stage(&mut self) -> Result<ParallelStageId, CanonicalCodecError> {
        let tag = self.byte()?;
        match tag {
            0 | 1 => {
                let target = self.call()?;
                let abi = self.abi()?;
                let input = self.ty()?;
                let output = self.ty()?;
                let captures = self.types()?;
                if tag == 0 {
                    Ok(ParallelStageId::Map {
                        target,
                        abi,
                        input,
                        output,
                        captures,
                    })
                } else {
                    Ok(ParallelStageId::Filter {
                        target,
                        abi,
                        input,
                        output,
                        captures,
                    })
                }
            }
            2 => Ok(ParallelStageId::FilterStrContains {
                input: self.ty()?,
                output: self.ty()?,
                needle: self.ty()?,
            }),
            3 => Ok(ParallelStageId::Project {
                input: self.ty()?,
                output: self.ty()?,
                field: self.u32()?,
            }),
            4 => Ok(ParallelStageId::FilterField {
                input: self.ty()?,
                output: self.ty()?,
                field: self.u32()?,
            }),
            _ => Err(CanonicalCodecError::UnknownTag),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(value: &str) -> Vec<u8> {
        assert_eq!(value.len() % 2, 0);
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }

    fn ty(value: &str) -> CanonicalTy {
        CanonicalTy::decode(&hex(value)).unwrap()
    }

    fn abi(value: &str) -> CanonicalFnAbi {
        CanonicalFnAbi::decode(&hex(value)).unwrap()
    }

    fn call(value: &str) -> ProgramCall {
        ProgramCall::try_from_logical(value).unwrap()
    }

    fn roundtrip(value: GeneratedId) -> Vec<u8> {
        let bytes = value.to_canonical_bytes().unwrap();
        assert_eq!(GeneratedId::decode(&bytes).unwrap(), value);
        bytes.into()
    }

    #[test]
    fn generated_identity_codec() {
        let unit = ty("010000000038");
        let bool_ty = ty("010000000002");
        let i64_ty = ty("0100000000000140");
        let slice_i64 = ty("01000000000d000140");
        let empty_abi = abi("0100000000010000000038000000");
        let i64_abi = abi("01010000000001000000000001400100000000000140000000");

        let goldens = [
            (
                GeneratedId::FnValue {
                    target: call("f"),
                    signature: empty_abi.clone(),
                },
                "010001000000660100000000010000000038000000",
            ),
            (
                GeneratedId::Closure {
                    lifted: call("l"),
                    explicit_signature: empty_abi.clone(),
                    captures: vec![bool_ty.clone()],
                },
                "0101010000006c010000000001000000003800000001000000010000000002",
            ),
            (
                GeneratedId::Task {
                    fallible: false,
                    result: unit.clone(),
                },
                "010200010000000038",
            ),
            (
                GeneratedId::Task {
                    fallible: true,
                    result: i64_ty.clone(),
                },
                "0102010100000000000140",
            ),
        ];
        for (value, expected) in goldens {
            let expected = hex(expected);
            assert_eq!(roundtrip(value.clone()), expected);
            assert_eq!(GeneratedId::decode(&expected).unwrap(), value);
        }

        let parallel = GeneratedId::Parallel(ParallelGeneratedId {
            mode: ParallelKernelMode::Materialize,
            source: slice_i64,
            terminal_input: i64_ty.clone(),
            terminal_output: i64_ty.clone(),
            terminal: call("f"),
            terminal_abi: i64_abi,
            terminal_captures: vec![],
            stages: vec![],
            work_weight: 1,
        });
        let expected = hex(
            "01030001000000000d00014001000000000001400100000000000140010000006601010000000001000000000001400100000000000140000000000000000000000001",
        );
        assert_eq!(roundtrip(parallel.clone()), expected);
        assert_eq!(GeneratedId::decode(&expected).unwrap(), parallel);

        let stages = vec![
            ParallelStageId::Map {
                target: call("map"),
                abi: empty_abi.clone(),
                input: unit.clone(),
                output: bool_ty.clone(),
                captures: vec![i64_ty.clone()],
            },
            ParallelStageId::Filter {
                target: call("filter"),
                abi: empty_abi,
                input: bool_ty.clone(),
                output: bool_ty.clone(),
                captures: vec![],
            },
            ParallelStageId::FilterStrContains {
                input: bool_ty.clone(),
                output: bool_ty.clone(),
                needle: bool_ty.clone(),
            },
            ParallelStageId::Project {
                input: bool_ty.clone(),
                output: i64_ty.clone(),
                field: 3,
            },
            ParallelStageId::FilterField {
                input: i64_ty.clone(),
                output: i64_ty.clone(),
                field: 4,
            },
        ];
        for mode in [
            ParallelKernelMode::FilterCount,
            ParallelKernelMode::FilterScatter,
        ] {
            roundtrip(GeneratedId::Parallel(ParallelGeneratedId {
                mode,
                source: unit.clone(),
                terminal_input: i64_ty.clone(),
                terminal_output: i64_ty.clone(),
                terminal: call("terminal"),
                terminal_abi: abi("0100000000010000000038000000"),
                terminal_captures: vec![bool_ty.clone()],
                stages: stages.clone(),
                work_weight: 4,
            }));
        }
    }

    #[test]
    fn generated_identity_error_precedence() {
        assert_eq!(
            GeneratedId::decode(&[]),
            Err(CanonicalCodecError::Truncated)
        );
        assert_eq!(
            GeneratedId::decode(&[2]),
            Err(CanonicalCodecError::UnsupportedVersion)
        );
        assert_eq!(
            GeneratedId::decode(&[1, 0xff]),
            Err(CanonicalCodecError::UnknownTag)
        );
        assert_eq!(
            GeneratedId::decode(&[1, 2, 2]),
            Err(CanonicalCodecError::InvalidBool)
        );
        assert_eq!(
            GeneratedId::decode(&[1, 0, 0, 0, 0, 0]),
            Err(CanonicalCodecError::InvalidGraph)
        );
        assert_eq!(
            GeneratedId::decode(&[1, 0, 1, 0, 0, 0, 0xff]),
            Err(CanonicalCodecError::InvalidUtf8)
        );
        assert_eq!(
            GeneratedId::decode(&[1, 0, 1, 0, 0, 0, 0]),
            Err(CanonicalCodecError::EmbeddedNul)
        );

        let valid = GeneratedId::Task {
            fallible: false,
            result: ty("010000000038"),
        }
        .to_canonical_bytes()
        .unwrap();
        let mut trailing = valid.to_vec();
        trailing.push(0);
        assert_eq!(
            GeneratedId::decode(&trailing),
            Err(CanonicalCodecError::TrailingBytes)
        );

        let invalid_parallel = GeneratedId::Parallel(ParallelGeneratedId {
            mode: ParallelKernelMode::FilterCount,
            source: ty("010000000038"),
            terminal_input: ty("010000000038"),
            terminal_output: ty("010000000038"),
            terminal: call("f"),
            terminal_abi: abi("0100000000010000000038000000"),
            terminal_captures: vec![],
            stages: vec![],
            work_weight: 3,
        });
        assert_eq!(
            invalid_parallel.to_canonical_bytes(),
            Err(CanonicalCodecError::InvalidGraph)
        );
    }

    #[test]
    fn deep_generated_identity_codec_is_stack_bounded() {
        let value = GeneratedId::Closure {
            lifted: call("deep"),
            explicit_signature: abi("0100000000010000000038000000"),
            captures: vec![ty("010000000038"); 4096],
        };
        let bytes = value.to_canonical_bytes().unwrap();
        assert_eq!(GeneratedId::decode(&bytes).unwrap(), value);
        assert_eq!(
            GeneratedId::decode(&bytes[..bytes.len() - 1]),
            Err(CanonicalCodecError::Truncated)
        );
    }
}
