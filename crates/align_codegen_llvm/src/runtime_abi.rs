//! Typed LLVM ABI registry for the fixed native runtime surface.

use align_mir::RuntimeKey;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType};
use inkwell::values::FunctionValue;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeType {
    I32,
    I64,
    F32,
    F64,
    Ptr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeReturn {
    Void,
    I32,
    I64,
    F32,
    F64,
    Ptr,
    I64Pair,
    PtrLen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeAbiShape {
    A00,
    A01,
    A02,
    A03,
    A04,
    A05,
    A06,
    A07,
    A08,
    A09,
    A10,
    A12,
    A13,
    A15,
    A16,
    A17,
    A18,
    A19,
    A20,
    A21,
    A22,
    A23,
    A24,
    A25,
    A26,
    A27,
    A28,
    A29,
    A30,
    A31,
    A32,
    A33,
    A34,
    A35,
    A36,
    A37,
    A38,
    A39,
    A40,
    A41,
    A42,
    A43,
    A44,
    A45,
    A46,
    A47,
    A48,
    A49,
    A50,
    A51,
    A52,
    A53,
    A54,
    A55,
    A56,
    A57,
    A58,
    A59,
    A60,
    A61,
    A62,
    A63,
    A64,
    A65,
    A66,
    A67,
    A68,
    A69,
    A70,
    A71,
    A72,
    A73,
    A74,
    A75,
    A76,
    A77,
    A78,
    A79,
    A80,
    A81,
    A82,
    A83,
    A84,
    A85,
    A86,
    A87,
    A88,
    A89,
    A90,
    A91,
    A92,
    A93,
    A94,
    A95,
    A96,
    A97,
    A98,
    A99,
    A100,
    A101,
    A102,
    A103,
    A104,
    A105,
    A106,
    A107,
    A108,
    A109,
}

#[derive(Clone, Copy)]
struct RuntimeAbiShapeSpec {
    ret: NativeReturn,
    params: &'static [NativeType],
    return_noalias: bool,
    fn_attrs: &'static [&'static str],
    memory_argmem_read: bool,
    read_ptr_params: &'static [u32],
}

#[derive(Clone, Copy)]
pub(super) struct RuntimeAbi {
    pub(super) key: RuntimeAbiId,
    pub(super) symbol: &'static str,
    shape: RuntimeAbiShape,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub(super) enum UnkeyedRuntimeKey {
    ReportError = 0,
    ArgsBuild = 1,
    ArenaReset = 2,
    Realloc = 3,
    HttpSerialize = 4,
    F32ToBits = 5,
    F32FromBits = 6,
    F64ToBits = 7,
    F64FromBits = 8,
    F32TextLen = 9,
    F64TextLen = 10,
    F32TextWrite = 11,
    F64TextWrite = 12,
}

pub(super) const UNKEYED_RUNTIME_KEYS: [UnkeyedRuntimeKey; 13] = [
    UnkeyedRuntimeKey::ReportError,
    UnkeyedRuntimeKey::ArgsBuild,
    UnkeyedRuntimeKey::ArenaReset,
    UnkeyedRuntimeKey::Realloc,
    UnkeyedRuntimeKey::HttpSerialize,
    UnkeyedRuntimeKey::F32ToBits,
    UnkeyedRuntimeKey::F32FromBits,
    UnkeyedRuntimeKey::F64ToBits,
    UnkeyedRuntimeKey::F64FromBits,
    UnkeyedRuntimeKey::F32TextLen,
    UnkeyedRuntimeKey::F64TextLen,
    UnkeyedRuntimeKey::F32TextWrite,
    UnkeyedRuntimeKey::F64TextWrite,
];

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum RuntimeAbiId {
    Keyed(RuntimeKey),
    Unkeyed(UnkeyedRuntimeKey),
}

impl RuntimeAbi {
    pub(super) fn function_type<'c>(self, ctx: &'c Context) -> FunctionType<'c> {
        let spec = shape_spec(self.shape);
        let params: Vec<BasicMetadataTypeEnum<'c>> = spec
            .params
            .iter()
            .map(|ty| native_type(ctx, *ty).into())
            .collect();
        match spec.ret {
            NativeReturn::Void => ctx.void_type().fn_type(&params, false),
            ret => native_return_type(ctx, ret).fn_type(&params, false),
        }
    }

    pub(super) fn declare<'c>(self, ctx: &'c Context, module: &Module<'c>) -> FunctionValue<'c> {
        module.add_function(self.symbol, self.function_type(ctx), None)
    }

    pub(super) fn apply_attributes<'c>(self, ctx: &'c Context, function: FunctionValue<'c>) {
        let spec = shape_spec(self.shape);
        if spec.return_noalias {
            super::add_enum_attr(
                ctx,
                function,
                inkwell::attributes::AttributeLoc::Return,
                "noalias",
            );
        }
        for attr in spec.fn_attrs {
            super::add_enum_attr(
                ctx,
                function,
                inkwell::attributes::AttributeLoc::Function,
                attr,
            );
        }
        if spec.memory_argmem_read {
            super::add_valued_enum_attr(
                ctx,
                function,
                inkwell::attributes::AttributeLoc::Function,
                "memory",
                super::MEM_ARGMEM_READ,
            );
        }
        for param in spec.read_ptr_params {
            super::add_enum_attr(
                ctx,
                function,
                inkwell::attributes::AttributeLoc::Param(*param),
                "readonly",
            );
            super::add_valued_enum_attr(
                ctx,
                function,
                inkwell::attributes::AttributeLoc::Param(*param),
                "captures",
                super::CAPTURES_NONE,
            );
        }
    }

    pub(super) fn remove_attributes(self, function: FunctionValue<'_>) {
        use inkwell::attributes::AttributeLoc;
        let spec = shape_spec(self.shape);
        if spec.return_noalias {
            function.remove_enum_attribute(AttributeLoc::Return, super::enum_kind_id("noalias"));
        }
        for attr in spec.fn_attrs {
            function.remove_enum_attribute(AttributeLoc::Function, super::enum_kind_id(attr));
        }
        if spec.memory_argmem_read {
            function.remove_enum_attribute(AttributeLoc::Function, super::enum_kind_id("memory"));
        }
        for param in spec.read_ptr_params {
            function.remove_enum_attribute(
                AttributeLoc::Param(*param),
                super::enum_kind_id("readonly"),
            );
            function.remove_enum_attribute(
                AttributeLoc::Param(*param),
                super::enum_kind_id("captures"),
            );
        }
    }

    pub(super) fn is_rt_lto_guarded(self) -> bool {
        matches!(
            self.key,
            RuntimeAbiId::Keyed(
                RuntimeKey::StrEq
                    | RuntimeKey::StrStartsWith
                    | RuntimeKey::StrEndsWith
                    | RuntimeKey::StrEqIgnoreCase
            )
        )
    }

    pub(super) fn runtime_key(self) -> Option<RuntimeKey> {
        match self.key {
            RuntimeAbiId::Keyed(key) => Some(key),
            RuntimeAbiId::Unkeyed(_) => None,
        }
    }
}

pub(super) fn runtime_abi(key: RuntimeKey) -> RuntimeAbi {
    let runtime_key = key;
    let key = RuntimeAbiId::Keyed(key);
    match runtime_key {
        RuntimeKey::Alloc => RuntimeAbi {
            key,
            symbol: "align_rt_alloc",
            shape: RuntimeAbiShape::A43,
        },
        RuntimeKey::AllocSizeFail => RuntimeAbi {
            key,
            symbol: "align_rt_alloc_size_fail",
            shape: RuntimeAbiShape::A54,
        },
        RuntimeKey::ArenaAlloc => RuntimeAbi {
            key,
            symbol: "align_rt_arena_alloc",
            shape: RuntimeAbiShape::A45,
        },
        RuntimeKey::ArenaBegin => RuntimeAbi {
            key,
            symbol: "align_rt_arena_begin",
            shape: RuntimeAbiShape::A42,
        },
        RuntimeKey::ArenaEnd => RuntimeAbi {
            key,
            symbol: "align_rt_arena_end",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::ArrayBuilderAppend => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_append",
            shape: RuntimeAbiShape::A73,
        },
        RuntimeKey::ArrayBuilderBuild => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_build",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::ArrayBuilderBuildStack => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_build_stack",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::ArrayBuilderFree => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::ArrayBuilderFreeStack => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_free_stack",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::ArrayBuilderFreeStrings => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_free_strings",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::ArrayBuilderFreeStringsStack => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_free_strings_stack",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::ArrayBuilderInitStack => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_init_stack",
            shape: RuntimeAbiShape::A51,
        },
        RuntimeKey::ArrayBuilderNew => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_new",
            shape: RuntimeAbiShape::A43,
        },
        RuntimeKey::ArrayBuilderNewIn => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_new_in",
            shape: RuntimeAbiShape::A45,
        },
        RuntimeKey::ArrayBuilderPush => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_push",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::ArrayBuilderPushBytes => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_push_bytes",
            shape: RuntimeAbiShape::A72,
        },
        RuntimeKey::ArrayBuilderPushStr => RuntimeAbi {
            key,
            symbol: "align_rt_array_builder_push_str",
            shape: RuntimeAbiShape::A73,
        },
        RuntimeKey::Base64Decode => RuntimeAbi {
            key,
            symbol: "align_rt_base64_decode",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::Base64Encode => RuntimeAbi {
            key,
            symbol: "align_rt_base64_encode",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::Base64urlDecode => RuntimeAbi {
            key,
            symbol: "align_rt_base64url_decode",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::Base64urlEncode => RuntimeAbi {
            key,
            symbol: "align_rt_base64url_encode",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::BoundsFail => RuntimeAbi {
            key,
            symbol: "align_rt_bounds_fail",
            shape: RuntimeAbiShape::A60,
        },
        RuntimeKey::BufferAppend => RuntimeAbi {
            key,
            symbol: "align_rt_buffer_append",
            shape: RuntimeAbiShape::A73,
        },
        RuntimeKey::BufferBytes => RuntimeAbi {
            key,
            symbol: "align_rt_buffer_bytes",
            shape: RuntimeAbiShape::A72,
        },
        RuntimeKey::BufferCapacity => RuntimeAbi {
            key,
            symbol: "align_rt_buffer_capacity",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::BufferFree => RuntimeAbi {
            key,
            symbol: "align_rt_buffer_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::BufferLen => RuntimeAbi {
            key,
            symbol: "align_rt_buffer_len",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::BufferNew => RuntimeAbi {
            key,
            symbol: "align_rt_buffer_new",
            shape: RuntimeAbiShape::A49,
        },
        RuntimeKey::BufferPut => RuntimeAbi {
            key,
            symbol: "align_rt_buffer_put",
            shape: RuntimeAbiShape::A67,
        },
        RuntimeKey::BuilderFinish => RuntimeAbi {
            key,
            symbol: "align_rt_builder_finish",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::BuilderFinishBoundedStack => RuntimeAbi {
            key,
            symbol: "align_rt_builder_finish_bounded_stack",
            shape: RuntimeAbiShape::A19,
        },
        RuntimeKey::BuilderFinishStack => RuntimeAbi {
            key,
            symbol: "align_rt_builder_finish_stack",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::BuilderFree => RuntimeAbi {
            key,
            symbol: "align_rt_builder_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::BuilderFreeStack => RuntimeAbi {
            key,
            symbol: "align_rt_builder_free_stack",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::BuilderInitStack => RuntimeAbi {
            key,
            symbol: "align_rt_builder_init_stack",
            shape: RuntimeAbiShape::A53,
        },
        RuntimeKey::BuilderInitBoundedStack => RuntimeAbi {
            key,
            symbol: "align_rt_builder_init_bounded_stack",
            shape: RuntimeAbiShape::A51,
        },
        RuntimeKey::BuilderIntoString => RuntimeAbi {
            key,
            symbol: "align_rt_builder_into_string",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::BuilderIntoStringStack => RuntimeAbi {
            key,
            symbol: "align_rt_builder_into_string_stack",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::BuilderNew => RuntimeAbi {
            key,
            symbol: "align_rt_builder_new",
            shape: RuntimeAbiShape::A44,
        },
        RuntimeKey::BuilderPopComma => RuntimeAbi {
            key,
            symbol: "align_rt_builder_pop_comma",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::BuilderWrite => RuntimeAbi {
            key,
            symbol: "align_rt_builder_write",
            shape: RuntimeAbiShape::A73,
        },
        RuntimeKey::BuilderWriteBool => RuntimeAbi {
            key,
            symbol: "align_rt_builder_write_bool",
            shape: RuntimeAbiShape::A65,
        },
        RuntimeKey::BuilderWriteChar => RuntimeAbi {
            key,
            symbol: "align_rt_builder_write_char",
            shape: RuntimeAbiShape::A65,
        },
        RuntimeKey::BuilderWriteF32 => RuntimeAbi {
            key,
            symbol: "align_rt_builder_write_f32",
            shape: RuntimeAbiShape::A64,
        },
        RuntimeKey::BuilderWriteF64 => RuntimeAbi {
            key,
            symbol: "align_rt_builder_write_f64",
            shape: RuntimeAbiShape::A63,
        },
        RuntimeKey::BuilderWriteInt => RuntimeAbi {
            key,
            symbol: "align_rt_builder_write_int",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::BuilderWriteUint => RuntimeAbi {
            key,
            symbol: "align_rt_builder_write_uint",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::BuilderWriteJsonStr => RuntimeAbi {
            key,
            symbol: "align_rt_builder_write_json_str",
            shape: RuntimeAbiShape::A73,
        },
        RuntimeKey::BuilderWriteStrIntStr => RuntimeAbi {
            key,
            symbol: "align_rt_builder_write_str_int_str",
            shape: RuntimeAbiShape::A76,
        },
        RuntimeKey::BytesAsStr => RuntimeAbi {
            key,
            symbol: "align_rt_bytes_as_str",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::ChildFree => RuntimeAbi {
            key,
            symbol: "align_rt_child_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::ChildKill => RuntimeAbi {
            key,
            symbol: "align_rt_child_kill",
            shape: RuntimeAbiShape::A04,
        },
        RuntimeKey::ChildWait => RuntimeAbi {
            key,
            symbol: "align_rt_child_wait",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::Chunks => RuntimeAbi {
            key,
            symbol: "align_rt_chunks",
            shape: RuntimeAbiShape::A85,
        },
        RuntimeKey::CliCommand => RuntimeAbi {
            key,
            symbol: "align_rt_cli_command_new",
            shape: RuntimeAbiShape::A51,
        },
        RuntimeKey::CliCommandFree => RuntimeAbi {
            key,
            symbol: "align_rt_cli_command_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::CliFlagBool => RuntimeAbi {
            key,
            symbol: "align_rt_cli_flag_bool",
            shape: RuntimeAbiShape::A73,
        },
        RuntimeKey::CliFlagI64 => RuntimeAbi {
            key,
            symbol: "align_rt_cli_flag_i64",
            shape: RuntimeAbiShape::A75,
        },
        RuntimeKey::CliFlagStr => RuntimeAbi {
            key,
            symbol: "align_rt_cli_flag_str",
            shape: RuntimeAbiShape::A77,
        },
        RuntimeKey::CliGetBool => RuntimeAbi {
            key,
            symbol: "align_rt_cli_get_bool",
            shape: RuntimeAbiShape::A20,
        },
        RuntimeKey::CliGetI64 => RuntimeAbi {
            key,
            symbol: "align_rt_cli_get_i64",
            shape: RuntimeAbiShape::A37,
        },
        RuntimeKey::CliGetStr => RuntimeAbi {
            key,
            symbol: "align_rt_cli_get_str",
            shape: RuntimeAbiShape::A87,
        },
        RuntimeKey::CliParse => RuntimeAbi {
            key,
            symbol: "align_rt_cli_parse",
            shape: RuntimeAbiShape::A22,
        },
        RuntimeKey::CliParsedFree => RuntimeAbi {
            key,
            symbol: "align_rt_cli_parsed_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::CliUsage => RuntimeAbi {
            key,
            symbol: "align_rt_cli_usage",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::CommandCwd => RuntimeAbi {
            key,
            symbol: "align_rt_command_cwd",
            shape: RuntimeAbiShape::A73,
        },
        RuntimeKey::CommandEnv => RuntimeAbi {
            key,
            symbol: "align_rt_command_env",
            shape: RuntimeAbiShape::A77,
        },
        RuntimeKey::CommandEnvClear => RuntimeAbi {
            key,
            symbol: "align_rt_command_env_clear",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::CommandFree => RuntimeAbi {
            key,
            symbol: "align_rt_command_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::CommandMaxCapture => RuntimeAbi {
            key,
            symbol: "align_rt_command_max_capture",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::CommandNew => RuntimeAbi {
            key,
            symbol: "align_rt_command_new",
            shape: RuntimeAbiShape::A52,
        },
        RuntimeKey::CommandRun => RuntimeAbi {
            key,
            symbol: "align_rt_command_run",
            shape: RuntimeAbiShape::A19,
        },
        RuntimeKey::CommandRunBytes => RuntimeAbi {
            key,
            symbol: "align_rt_command_run_bytes",
            shape: RuntimeAbiShape::A19,
        },
        RuntimeKey::CommandTimeout => RuntimeAbi {
            key,
            symbol: "align_rt_command_timeout",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::CompressGzipCompress => RuntimeAbi {
            key,
            symbol: "align_rt_compress_gzip_compress",
            shape: RuntimeAbiShape::A07,
        },
        RuntimeKey::CompressGzipDecompress => RuntimeAbi {
            key,
            symbol: "align_rt_compress_gzip_decompress",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::CompressZstdCompress => RuntimeAbi {
            key,
            symbol: "align_rt_compress_zstd_compress",
            shape: RuntimeAbiShape::A07,
        },
        RuntimeKey::CompressZstdDecompress => RuntimeAbi {
            key,
            symbol: "align_rt_compress_zstd_decompress",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::CryptoAesGcmOpen => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_aes_gcm_open",
            shape: RuntimeAbiShape::A15,
        },
        RuntimeKey::CryptoAesGcmSeal => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_aes_gcm_seal",
            shape: RuntimeAbiShape::A15,
        },
        RuntimeKey::CryptoArgon2id => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_argon2id",
            shape: RuntimeAbiShape::A10,
        },
        RuntimeKey::CryptoChacha20Poly1305Open => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_chacha20_poly1305_open",
            shape: RuntimeAbiShape::A15,
        },
        RuntimeKey::CryptoChacha20Poly1305Seal => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_chacha20_poly1305_seal",
            shape: RuntimeAbiShape::A15,
        },
        RuntimeKey::CryptoCtEqual => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_ct_equal",
            shape: RuntimeAbiShape::A09,
        },
        RuntimeKey::CryptoHkdfSha256 => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_hkdf_sha256",
            shape: RuntimeAbiShape::A13,
        },
        RuntimeKey::CryptoHmacSha256 => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_hmac_sha256",
            shape: RuntimeAbiShape::A86,
        },
        RuntimeKey::CryptoKeyFree => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_key_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::CryptoPrivateKeyFromPem => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_private_key_from_pem",
            shape: RuntimeAbiShape::A106,
        },
        RuntimeKey::CryptoPublicKeyFromJwk => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_public_key_from_jwk",
            shape: RuntimeAbiShape::A107,
        },
        RuntimeKey::CryptoPublicKeyFromPem => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_public_key_from_pem",
            shape: RuntimeAbiShape::A106,
        },
        RuntimeKey::CryptoRandom => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_random",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::CryptoSha256 => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_sha256",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::CryptoSha512 => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_sha512",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::CryptoSign => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_sign",
            shape: RuntimeAbiShape::A108,
        },
        RuntimeKey::CryptoVerify => RuntimeAbi {
            key,
            symbol: "align_rt_crypto_verify",
            shape: RuntimeAbiShape::A109,
        },
        RuntimeKey::DictEncodeStr => RuntimeAbi {
            key,
            symbol: "align_rt_dict_encode_str",
            shape: RuntimeAbiShape::A34,
        },
        RuntimeKey::DictLookup => RuntimeAbi {
            key,
            symbol: "align_rt_dict_lookup",
            shape: RuntimeAbiShape::A70,
        },
        RuntimeKey::DivFail => RuntimeAbi {
            key,
            symbol: "align_rt_div_fail",
            shape: RuntimeAbiShape::A54,
        },
        RuntimeKey::DnsResolve => RuntimeAbi {
            key,
            symbol: "align_rt_dns_resolve",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::EnvGet => RuntimeAbi {
            key,
            symbol: "align_rt_env_get",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::EnvSet => RuntimeAbi {
            key,
            symbol: "align_rt_env_set",
            shape: RuntimeAbiShape::A09,
        },
        RuntimeKey::FormDecode => RuntimeAbi {
            key,
            symbol: "align_rt_form_decode",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::FormEncode => RuntimeAbi {
            key,
            symbol: "align_rt_form_encode",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::Free => RuntimeAbi {
            key,
            symbol: "align_rt_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::FreeResponseArray => RuntimeAbi {
            key,
            symbol: "align_rt_free_response_array",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::FreeStringArray => RuntimeAbi {
            key,
            symbol: "align_rt_free_string_array",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::FsExists => RuntimeAbi {
            key,
            symbol: "align_rt_fs_exists",
            shape: RuntimeAbiShape::A04,
        },
        RuntimeKey::FsReadBytesView => RuntimeAbi {
            key,
            symbol: "align_rt_fs_read_bytes_view",
            shape: RuntimeAbiShape::A17,
        },
        RuntimeKey::FsReadDir => RuntimeAbi {
            key,
            symbol: "align_rt_fs_read_dir",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::FsReadFile => RuntimeAbi {
            key,
            symbol: "align_rt_fs_read_file",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::FsReadFileView => RuntimeAbi {
            key,
            symbol: "align_rt_fs_read_file_view",
            shape: RuntimeAbiShape::A17,
        },
        RuntimeKey::FsRemove => RuntimeAbi {
            key,
            symbol: "align_rt_fs_remove",
            shape: RuntimeAbiShape::A04,
        },
        RuntimeKey::FsRenameNoReplace => RuntimeAbi {
            key,
            symbol: "align_rt_fs_rename_no_replace",
            shape: RuntimeAbiShape::A09,
        },
        RuntimeKey::FsWriteFile => RuntimeAbi {
            key,
            symbol: "align_rt_fs_write_file",
            shape: RuntimeAbiShape::A09,
        },
        RuntimeKey::FsWriteFileBuilder => RuntimeAbi {
            key,
            symbol: "align_rt_fs_write_file_builder",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::GatherI64 => RuntimeAbi {
            key,
            symbol: "align_rt_gather_i64",
            shape: RuntimeAbiShape::A68,
        },
        RuntimeKey::GroupCountI64 => RuntimeAbi {
            key,
            symbol: "align_rt_group_count_i64",
            shape: RuntimeAbiShape::A35,
        },
        RuntimeKey::GroupCountStr => RuntimeAbi {
            key,
            symbol: "align_rt_group_count_str",
            shape: RuntimeAbiShape::A32,
        },
        RuntimeKey::GroupCountStrCols => RuntimeAbi {
            key,
            symbol: "align_rt_group_count_str_cols",
            shape: RuntimeAbiShape::A41,
        },
        RuntimeKey::GroupMaxI64 => RuntimeAbi {
            key,
            symbol: "align_rt_group_max_i64",
            shape: RuntimeAbiShape::A41,
        },
        RuntimeKey::GroupMaxStr => RuntimeAbi {
            key,
            symbol: "align_rt_group_max_str",
            shape: RuntimeAbiShape::A32,
        },
        RuntimeKey::GroupMaxStrCols => RuntimeAbi {
            key,
            symbol: "align_rt_group_max_str_cols",
            shape: RuntimeAbiShape::A41,
        },
        RuntimeKey::GroupMinI64 => RuntimeAbi {
            key,
            symbol: "align_rt_group_min_i64",
            shape: RuntimeAbiShape::A41,
        },
        RuntimeKey::GroupMinStr => RuntimeAbi {
            key,
            symbol: "align_rt_group_min_str",
            shape: RuntimeAbiShape::A32,
        },
        RuntimeKey::GroupMinStrCols => RuntimeAbi {
            key,
            symbol: "align_rt_group_min_str_cols",
            shape: RuntimeAbiShape::A41,
        },
        RuntimeKey::GroupMultiStr => RuntimeAbi {
            key,
            symbol: "align_rt_group_multi_str",
            shape: RuntimeAbiShape::A33,
        },
        RuntimeKey::GroupSumI64 => RuntimeAbi {
            key,
            symbol: "align_rt_group_sum_i64",
            shape: RuntimeAbiShape::A41,
        },
        RuntimeKey::GroupSumStr => RuntimeAbi {
            key,
            symbol: "align_rt_group_sum_str",
            shape: RuntimeAbiShape::A32,
        },
        RuntimeKey::GroupSumStrCols => RuntimeAbi {
            key,
            symbol: "align_rt_group_sum_str_cols",
            shape: RuntimeAbiShape::A41,
        },
        RuntimeKey::Hash128 => RuntimeAbi {
            key,
            symbol: "align_rt_hash128",
            shape: RuntimeAbiShape::A82,
        },
        RuntimeKey::Hash64 => RuntimeAbi {
            key,
            symbol: "align_rt_hash64",
            shape: RuntimeAbiShape::A26,
        },
        RuntimeKey::HexDecode => RuntimeAbi {
            key,
            symbol: "align_rt_hex_decode",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::HexEncode => RuntimeAbi {
            key,
            symbol: "align_rt_hex_encode",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::HtmlEscape => RuntimeAbi {
            key,
            symbol: "align_rt_html_escape",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::HttpAccept => RuntimeAbi {
            key,
            symbol: "align_rt_http_accept",
            shape: RuntimeAbiShape::A19,
        },
        RuntimeKey::HttpBody => RuntimeAbi {
            key,
            symbol: "align_rt_http_body",
            shape: RuntimeAbiShape::A73,
        },
        RuntimeKey::HttpClientFree => RuntimeAbi {
            key,
            symbol: "align_rt_http_client_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::HttpClientGet => RuntimeAbi {
            key,
            symbol: "align_rt_http_client_get",
            shape: RuntimeAbiShape::A22,
        },
        RuntimeKey::HttpClientMaxResponseBodyBytes => RuntimeAbi {
            key,
            symbol: "align_rt_http_client_max_response_body_bytes",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::HttpClientNew => RuntimeAbi {
            key,
            symbol: "align_rt_http_client_new",
            shape: RuntimeAbiShape::A47,
        },
        RuntimeKey::HttpClientPost => RuntimeAbi {
            key,
            symbol: "align_rt_http_client_post",
            shape: RuntimeAbiShape::A23,
        },
        RuntimeKey::HttpClientRequest => RuntimeAbi {
            key,
            symbol: "align_rt_http_client_request",
            shape: RuntimeAbiShape::A24,
        },
        RuntimeKey::HttpClientRequestStream => RuntimeAbi {
            key,
            symbol: "align_rt_http_client_request_stream",
            shape: RuntimeAbiShape::A24,
        },
        RuntimeKey::HttpClientTimeout => RuntimeAbi {
            key,
            symbol: "align_rt_http_client_timeout",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::HttpCtxBody => RuntimeAbi {
            key,
            symbol: "align_rt_http_ctx_body",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::HttpCtxFree => RuntimeAbi {
            key,
            symbol: "align_rt_http_ctx_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::HttpCtxHeader => RuntimeAbi {
            key,
            symbol: "align_rt_http_ctx_header",
            shape: RuntimeAbiShape::A22,
        },
        RuntimeKey::HttpCtxMethod => RuntimeAbi {
            key,
            symbol: "align_rt_http_ctx_method",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::HttpCtxPath => RuntimeAbi {
            key,
            symbol: "align_rt_http_ctx_path",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::HttpGetMany => RuntimeAbi {
            key,
            symbol: "align_rt_http_get_many",
            shape: RuntimeAbiShape::A21,
        },
        RuntimeKey::HttpHeader => RuntimeAbi {
            key,
            symbol: "align_rt_http_header",
            shape: RuntimeAbiShape::A77,
        },
        RuntimeKey::HttpMaxResponseBodyBytes => RuntimeAbi {
            key,
            symbol: "align_rt_http_max_response_body_bytes",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::HttpParse => RuntimeAbi {
            key,
            symbol: "align_rt_http_parse",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::HttpRbBody => RuntimeAbi {
            key,
            symbol: "align_rt_http_rb_body",
            shape: RuntimeAbiShape::A73,
        },
        RuntimeKey::HttpRbHeader => RuntimeAbi {
            key,
            symbol: "align_rt_http_rb_header",
            shape: RuntimeAbiShape::A77,
        },
        RuntimeKey::HttpRequest => RuntimeAbi {
            key,
            symbol: "align_rt_http_request_new",
            shape: RuntimeAbiShape::A52,
        },
        RuntimeKey::HttpRequestFree => RuntimeAbi {
            key,
            symbol: "align_rt_http_request_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::HttpReadStreamFree => RuntimeAbi {
            key,
            symbol: "align_rt_http_read_stream_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::HttpReadStreamHeader => RuntimeAbi {
            key,
            symbol: "align_rt_http_read_stream_header",
            shape: RuntimeAbiShape::A22,
        },
        RuntimeKey::HttpReadStreamRead => RuntimeAbi {
            key,
            symbol: "align_rt_http_read_stream_read",
            shape: RuntimeAbiShape::A24,
        },
        RuntimeKey::HttpReadStreamSse => RuntimeAbi {
            key,
            symbol: "align_rt_http_read_stream_sse",
            shape: RuntimeAbiShape::A50,
        },
        RuntimeKey::HttpReadStreamStatus => RuntimeAbi {
            key,
            symbol: "align_rt_http_read_stream_status",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::HttpSseStreamLastEventId => RuntimeAbi {
            key,
            symbol: "align_rt_http_sse_stream_last_event_id",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::HttpSseStreamNext => RuntimeAbi {
            key,
            symbol: "align_rt_http_sse_stream_next",
            shape: RuntimeAbiShape::A24,
        },
        RuntimeKey::HttpSseStreamRetryMs => RuntimeAbi {
            key,
            symbol: "align_rt_http_sse_stream_retry_ms",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::HttpRespBody => RuntimeAbi {
            key,
            symbol: "align_rt_http_resp_body",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::HttpRespFree => RuntimeAbi {
            key,
            symbol: "align_rt_http_resp_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::HttpRespHeader => RuntimeAbi {
            key,
            symbol: "align_rt_http_resp_header",
            shape: RuntimeAbiShape::A22,
        },
        RuntimeKey::HttpRespStatus => RuntimeAbi {
            key,
            symbol: "align_rt_http_resp_status",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::HttpRespond => RuntimeAbi {
            key,
            symbol: "align_rt_http_respond",
            shape: RuntimeAbiShape::A19,
        },
        RuntimeKey::HttpRespondStream => RuntimeAbi {
            key,
            symbol: "align_rt_http_respond_stream",
            shape: RuntimeAbiShape::A24,
        },
        RuntimeKey::HttpResponseFree => RuntimeAbi {
            key,
            symbol: "align_rt_http_response_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::HttpResponseNew => RuntimeAbi {
            key,
            symbol: "align_rt_http_response_new",
            shape: RuntimeAbiShape::A49,
        },
        RuntimeKey::HttpServe => RuntimeAbi {
            key,
            symbol: "align_rt_http_serve",
            shape: RuntimeAbiShape::A07,
        },
        RuntimeKey::HttpServeShared => RuntimeAbi {
            key,
            symbol: "align_rt_http_serve_shared",
            shape: RuntimeAbiShape::A07,
        },
        RuntimeKey::HttpServerFree => RuntimeAbi {
            key,
            symbol: "align_rt_http_server_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::HttpStreamFinish => RuntimeAbi {
            key,
            symbol: "align_rt_http_stream_finish",
            shape: RuntimeAbiShape::A03,
        },
        RuntimeKey::HttpStreamFree => RuntimeAbi {
            key,
            symbol: "align_rt_http_stream_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::HttpStreamReject => RuntimeAbi {
            key,
            symbol: "align_rt_http_stream_reject",
            shape: RuntimeAbiShape::A19,
        },
        RuntimeKey::HttpStreamSend => RuntimeAbi {
            key,
            symbol: "align_rt_http_stream_send",
            shape: RuntimeAbiShape::A20,
        },
        RuntimeKey::HttpStreamSendEvent => RuntimeAbi {
            key,
            symbol: "align_rt_http_stream_send_event",
            shape: RuntimeAbiShape::A20,
        },
        RuntimeKey::HttpTimeout => RuntimeAbi {
            key,
            symbol: "align_rt_http_timeout",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::IoCopy => RuntimeAbi {
            key,
            symbol: "align_rt_io_copy",
            shape: RuntimeAbiShape::A36,
        },
        RuntimeKey::IoFileCreate => RuntimeAbi {
            key,
            symbol: "align_rt_io_file_create",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::IoFileFree => RuntimeAbi {
            key,
            symbol: "align_rt_io_file_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::IoFileLen => RuntimeAbi {
            key,
            symbol: "align_rt_io_file_len",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::IoFileOpen => RuntimeAbi {
            key,
            symbol: "align_rt_io_file_open",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::IoFilePread => RuntimeAbi {
            key,
            symbol: "align_rt_io_file_pread",
            shape: RuntimeAbiShape::A37,
        },
        RuntimeKey::IoFilePwrite => RuntimeAbi {
            key,
            symbol: "align_rt_io_file_pwrite",
            shape: RuntimeAbiShape::A38,
        },
        RuntimeKey::IoReaderBuffered => RuntimeAbi {
            key,
            symbol: "align_rt_io_reader_buffered",
            shape: RuntimeAbiShape::A50,
        },
        RuntimeKey::IoReaderFree => RuntimeAbi {
            key,
            symbol: "align_rt_io_reader_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::IoReaderOpen => RuntimeAbi {
            key,
            symbol: "align_rt_io_reader_open",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::IoReaderOpenBeneath => RuntimeAbi {
            key,
            symbol: "align_rt_io_reader_open_beneath",
            shape: RuntimeAbiShape::A12,
        },
        RuntimeKey::IoReaderRead => RuntimeAbi {
            key,
            symbol: "align_rt_io_reader_read",
            shape: RuntimeAbiShape::A36,
        },
        RuntimeKey::IoReaderReadLine => RuntimeAbi {
            key,
            symbol: "align_rt_io_reader_read_line",
            shape: RuntimeAbiShape::A36,
        },
        RuntimeKey::IoReaderStdin => RuntimeAbi {
            key,
            symbol: "align_rt_io_reader_stdin",
            shape: RuntimeAbiShape::A47,
        },
        RuntimeKey::IoWriterCreate => RuntimeAbi {
            key,
            symbol: "align_rt_io_writer_create",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::IoWriterCreateExclusive => RuntimeAbi {
            key,
            symbol: "align_rt_io_writer_create_exclusive",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::IoWriterCreateExclusiveBeneath => RuntimeAbi {
            key,
            symbol: "align_rt_io_writer_create_exclusive_beneath",
            shape: RuntimeAbiShape::A12,
        },
        RuntimeKey::IoWriterFlush => RuntimeAbi {
            key,
            symbol: "align_rt_io_writer_flush",
            shape: RuntimeAbiShape::A03,
        },
        RuntimeKey::IoWriterFree => RuntimeAbi {
            key,
            symbol: "align_rt_io_writer_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::IoWriterStd => RuntimeAbi {
            key,
            symbol: "align_rt_io_writer_std",
            shape: RuntimeAbiShape::A48,
        },
        RuntimeKey::IoWriterWrite => RuntimeAbi {
            key,
            symbol: "align_rt_io_writer_write",
            shape: RuntimeAbiShape::A20,
        },
        RuntimeKey::IoWriterWriteBuilder => RuntimeAbi {
            key,
            symbol: "align_rt_io_writer_write_builder",
            shape: RuntimeAbiShape::A19,
        },
        RuntimeKey::JsonDecode => RuntimeAbi {
            key,
            symbol: "align_rt_json_decode",
            shape: RuntimeAbiShape::A103,
        },
        RuntimeKey::JsonDecodeArray => RuntimeAbi {
            key,
            symbol: "align_rt_json_decode_array",
            shape: RuntimeAbiShape::A05,
        },
        RuntimeKey::JsonDecodeScalar => RuntimeAbi {
            key,
            symbol: "align_rt_json_decode_scalar",
            shape: RuntimeAbiShape::A05,
        },
        RuntimeKey::JsonDecodeSoa => RuntimeAbi {
            key,
            symbol: "align_rt_json_decode_soa",
            shape: RuntimeAbiShape::A16,
        },
        RuntimeKey::JsonDecodeStructArray => RuntimeAbi {
            key,
            symbol: "align_rt_json_decode_struct_array",
            shape: RuntimeAbiShape::A104,
        },
        RuntimeKey::JsonDecodeUnion => RuntimeAbi {
            key,
            symbol: "align_rt_json_decode_union",
            shape: RuntimeAbiShape::A105,
        },
        RuntimeKey::JsonDocAsBool => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_as_bool",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::JsonDocAsF64 => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_as_f64",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::JsonDocAsI64 => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_as_i64",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::JsonDocAsStr => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_as_str",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::JsonDocAt => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_at",
            shape: RuntimeAbiShape::A69,
        },
        RuntimeKey::JsonDocElems => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_elems",
            shape: RuntimeAbiShape::A71,
        },
        RuntimeKey::JsonDocGet => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_get",
            shape: RuntimeAbiShape::A70,
        },
        RuntimeKey::JsonDocKey => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_key",
            shape: RuntimeAbiShape::A07,
        },
        RuntimeKey::JsonDocKind => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_kind",
            shape: RuntimeAbiShape::A04,
        },
        RuntimeKey::JsonDocLen => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_len",
            shape: RuntimeAbiShape::A30,
        },
        RuntimeKey::JsonDocParse => RuntimeAbi {
            key,
            symbol: "align_rt_json_doc_parse",
            shape: RuntimeAbiShape::A17,
        },
        RuntimeKey::JsonEncodeObject => RuntimeAbi {
            key,
            symbol: "align_rt_json_encode_object",
            shape: RuntimeAbiShape::A80,
        },
        RuntimeKey::JsonEncodeScalarArray => RuntimeAbi {
            key,
            symbol: "align_rt_json_encode_scalar_array",
            shape: RuntimeAbiShape::A74,
        },
        RuntimeKey::JsonEncodeStructArray => RuntimeAbi {
            key,
            symbol: "align_rt_json_encode_struct_array",
            shape: RuntimeAbiShape::A78,
        },
        RuntimeKey::JsonEncodeUnion => RuntimeAbi {
            key,
            symbol: "align_rt_json_encode_union",
            shape: RuntimeAbiShape::A79,
        },
        RuntimeKey::JsonScanNext => RuntimeAbi {
            key,
            symbol: "align_rt_json_scan_next",
            shape: RuntimeAbiShape::A18,
        },
        RuntimeKey::LenMismatchFail => RuntimeAbi {
            key,
            symbol: "align_rt_len_mismatch_fail",
            shape: RuntimeAbiShape::A60,
        },
        RuntimeKey::ParMap => RuntimeAbi {
            key,
            symbol: "align_rt_par_map",
            shape: RuntimeAbiShape::A46,
        },
        RuntimeKey::ParMapFilter => RuntimeAbi {
            key,
            symbol: "align_rt_par_map_filter",
            shape: RuntimeAbiShape::A89,
        },
        RuntimeKey::ParMapReduce => RuntimeAbi {
            key,
            symbol: "align_rt_par_map_reduce",
            shape: RuntimeAbiShape::A39,
        },
        RuntimeKey::PathBase => RuntimeAbi {
            key,
            symbol: "align_rt_path_base",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::PathDir => RuntimeAbi {
            key,
            symbol: "align_rt_path_dir",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::PathExt => RuntimeAbi {
            key,
            symbol: "align_rt_path_ext",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::PathJoin => RuntimeAbi {
            key,
            symbol: "align_rt_path_join",
            shape: RuntimeAbiShape::A86,
        },
        RuntimeKey::PathNormalize => RuntimeAbi {
            key,
            symbol: "align_rt_path_normalize",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::PercentDecode => RuntimeAbi {
            key,
            symbol: "align_rt_percent_decode",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::PercentEncode => RuntimeAbi {
            key,
            symbol: "align_rt_percent_encode",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::Print => RuntimeAbi {
            key,
            symbol: "align_rt_print_i64",
            shape: RuntimeAbiShape::A58,
        },
        RuntimeKey::PrintBool => RuntimeAbi {
            key,
            symbol: "align_rt_print_bool",
            shape: RuntimeAbiShape::A57,
        },
        RuntimeKey::PrintChar => RuntimeAbi {
            key,
            symbol: "align_rt_print_char",
            shape: RuntimeAbiShape::A57,
        },
        RuntimeKey::PrintF32 => RuntimeAbi {
            key,
            symbol: "align_rt_print_f32",
            shape: RuntimeAbiShape::A56,
        },
        RuntimeKey::PrintF64 => RuntimeAbi {
            key,
            symbol: "align_rt_print_f64",
            shape: RuntimeAbiShape::A55,
        },
        RuntimeKey::PrintStr => RuntimeAbi {
            key,
            symbol: "align_rt_print_str",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::ProcessAbort => RuntimeAbi {
            key,
            symbol: "align_rt_process_abort",
            shape: RuntimeAbiShape::A54,
        },
        RuntimeKey::ProcessCpuCount => RuntimeAbi {
            key,
            symbol: "align_rt_process_cpu_count",
            shape: RuntimeAbiShape::A25,
        },
        RuntimeKey::ProcessExec => RuntimeAbi {
            key,
            symbol: "align_rt_process_exec",
            shape: RuntimeAbiShape::A09,
        },
        RuntimeKey::ProcessExit => RuntimeAbi {
            key,
            symbol: "align_rt_process_exit",
            shape: RuntimeAbiShape::A59,
        },
        RuntimeKey::ProcessSpawn => RuntimeAbi {
            key,
            symbol: "align_rt_process_spawn",
            shape: RuntimeAbiShape::A12,
        },
        RuntimeKey::RangeFail => RuntimeAbi {
            key,
            symbol: "align_rt_range_fail",
            shape: RuntimeAbiShape::A61,
        },
        RuntimeKey::RegexCaptures => RuntimeAbi {
            key,
            symbol: "align_rt_regex_captures",
            shape: RuntimeAbiShape::A22,
        },
        RuntimeKey::RegexCapturesFree => RuntimeAbi {
            key,
            symbol: "align_rt_regex_captures_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::RegexCapturesGroup => RuntimeAbi {
            key,
            symbol: "align_rt_regex_captures_group",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::RegexCompile => RuntimeAbi {
            key,
            symbol: "align_rt_regex_compile",
            shape: RuntimeAbiShape::A08,
        },
        RuntimeKey::RegexFind => RuntimeAbi {
            key,
            symbol: "align_rt_regex_find",
            shape: RuntimeAbiShape::A21,
        },
        RuntimeKey::RegexFindAll => RuntimeAbi {
            key,
            symbol: "align_rt_regex_find_all",
            shape: RuntimeAbiShape::A22,
        },
        RuntimeKey::RegexFree => RuntimeAbi {
            key,
            symbol: "align_rt_regex_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::RegexGroupCount => RuntimeAbi {
            key,
            symbol: "align_rt_regex_group_count",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::RegexGroupIndex => RuntimeAbi {
            key,
            symbol: "align_rt_regex_group_index",
            shape: RuntimeAbiShape::A37,
        },
        RuntimeKey::RegexIsMatch => RuntimeAbi {
            key,
            symbol: "align_rt_regex_is_match",
            shape: RuntimeAbiShape::A20,
        },
        RuntimeKey::RegexReplace => RuntimeAbi {
            key,
            symbol: "align_rt_regex_replace",
            shape: RuntimeAbiShape::A90,
        },
        RuntimeKey::RegexSplit => RuntimeAbi {
            key,
            symbol: "align_rt_regex_split",
            shape: RuntimeAbiShape::A22,
        },
        RuntimeKey::RngNext => RuntimeAbi {
            key,
            symbol: "align_rt_rng_next",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::RngRange => RuntimeAbi {
            key,
            symbol: "align_rt_rng_range",
            shape: RuntimeAbiShape::A31,
        },
        RuntimeKey::RngSample => RuntimeAbi {
            key,
            symbol: "align_rt_rng_sample",
            shape: RuntimeAbiShape::A88,
        },
        RuntimeKey::RngSeedOs => RuntimeAbi {
            key,
            symbol: "align_rt_rng_seed_os",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::RngSeedWith => RuntimeAbi {
            key,
            symbol: "align_rt_rng_seed_with",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::RngShuffle => RuntimeAbi {
            key,
            symbol: "align_rt_rng_shuffle",
            shape: RuntimeAbiShape::A75,
        },
        RuntimeKey::RunBytesCode => RuntimeAbi {
            key,
            symbol: "align_rt_run_bytes_code",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::RunBytesFree => RuntimeAbi {
            key,
            symbol: "align_rt_run_bytes_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::RunBytesStderr => RuntimeAbi {
            key,
            symbol: "align_rt_run_bytes_stderr",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::RunBytesStdout => RuntimeAbi {
            key,
            symbol: "align_rt_run_bytes_stdout",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::RunOutputCode => RuntimeAbi {
            key,
            symbol: "align_rt_run_output_code",
            shape: RuntimeAbiShape::A29,
        },
        RuntimeKey::RunOutputFree => RuntimeAbi {
            key,
            symbol: "align_rt_run_output_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::RunOutputStderr => RuntimeAbi {
            key,
            symbol: "align_rt_run_output_stderr",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::RunOutputStdout => RuntimeAbi {
            key,
            symbol: "align_rt_run_output_stdout",
            shape: RuntimeAbiShape::A83,
        },
        RuntimeKey::StrClone => RuntimeAbi {
            key,
            symbol: "align_rt_str_clone",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::StrCmp => RuntimeAbi {
            key,
            symbol: "align_rt_str_cmp",
            shape: RuntimeAbiShape::A01,
        },
        RuntimeKey::StrContains => RuntimeAbi {
            key,
            symbol: "align_rt_str_contains",
            shape: RuntimeAbiShape::A02,
        },
        RuntimeKey::StrEndsWith => RuntimeAbi {
            key,
            symbol: "align_rt_str_ends_with",
            shape: RuntimeAbiShape::A01,
        },
        RuntimeKey::StrEq => RuntimeAbi {
            key,
            symbol: "align_rt_str_eq",
            shape: RuntimeAbiShape::A01,
        },
        RuntimeKey::StrEqIgnoreCase => RuntimeAbi {
            key,
            symbol: "align_rt_str_eq_ignore_case",
            shape: RuntimeAbiShape::A01,
        },
        RuntimeKey::StrFind => RuntimeAbi {
            key,
            symbol: "align_rt_str_find",
            shape: RuntimeAbiShape::A27,
        },
        RuntimeKey::StrFinderFind => RuntimeAbi {
            key,
            symbol: "align_rt_str_finder_find",
            shape: RuntimeAbiShape::A28,
        },
        RuntimeKey::StrFinderFree => RuntimeAbi {
            key,
            symbol: "align_rt_str_finder_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::StrFinderNew => RuntimeAbi {
            key,
            symbol: "align_rt_str_finder_new",
            shape: RuntimeAbiShape::A44,
        },
        RuntimeKey::StrRfind => RuntimeAbi {
            key,
            symbol: "align_rt_str_rfind",
            shape: RuntimeAbiShape::A27,
        },
        RuntimeKey::StrStartsWith => RuntimeAbi {
            key,
            symbol: "align_rt_str_starts_with",
            shape: RuntimeAbiShape::A01,
        },
        RuntimeKey::StrTrim => RuntimeAbi {
            key,
            symbol: "align_rt_str_trim",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::StrTrimEnd => RuntimeAbi {
            key,
            symbol: "align_rt_str_trim_end",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::StrTrimStart => RuntimeAbi {
            key,
            symbol: "align_rt_str_trim_start",
            shape: RuntimeAbiShape::A84,
        },
        RuntimeKey::TcpAccept => RuntimeAbi {
            key,
            symbol: "align_rt_tcp_accept",
            shape: RuntimeAbiShape::A19,
        },
        RuntimeKey::TcpConnFree => RuntimeAbi {
            key,
            symbol: "align_rt_tcp_conn_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::TcpConnReader => RuntimeAbi {
            key,
            symbol: "align_rt_tcp_conn_reader",
            shape: RuntimeAbiShape::A50,
        },
        RuntimeKey::TcpConnWriter => RuntimeAbi {
            key,
            symbol: "align_rt_tcp_conn_writer",
            shape: RuntimeAbiShape::A50,
        },
        RuntimeKey::TcpConnect => RuntimeAbi {
            key,
            symbol: "align_rt_tcp_connect",
            shape: RuntimeAbiShape::A06,
        },
        RuntimeKey::TcpListen => RuntimeAbi {
            key,
            symbol: "align_rt_tcp_listen",
            shape: RuntimeAbiShape::A07,
        },
        RuntimeKey::TcpListenerFree => RuntimeAbi {
            key,
            symbol: "align_rt_tcp_listener_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::TcpReadTimeout => RuntimeAbi {
            key,
            symbol: "align_rt_tcp_read_timeout",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::TcpWriteTimeout => RuntimeAbi {
            key,
            symbol: "align_rt_tcp_write_timeout",
            shape: RuntimeAbiShape::A66,
        },
        RuntimeKey::TgAlloc => RuntimeAbi {
            key,
            symbol: "align_rt_tg_alloc",
            shape: RuntimeAbiShape::A45,
        },
        RuntimeKey::TgBegin => RuntimeAbi {
            key,
            symbol: "align_rt_tg_begin",
            shape: RuntimeAbiShape::A42,
        },
        RuntimeKey::TgEnd => RuntimeAbi {
            key,
            symbol: "align_rt_tg_end",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::TgRegister => RuntimeAbi {
            key,
            symbol: "align_rt_tg_register",
            shape: RuntimeAbiShape::A81,
        },
        RuntimeKey::TgWait => RuntimeAbi {
            key,
            symbol: "align_rt_tg_wait",
            shape: RuntimeAbiShape::A50,
        },
        RuntimeKey::TimeInstant => RuntimeAbi {
            key,
            symbol: "align_rt_time_instant",
            shape: RuntimeAbiShape::A25,
        },
        RuntimeKey::TimeNow => RuntimeAbi {
            key,
            symbol: "align_rt_time_now",
            shape: RuntimeAbiShape::A25,
        },
        RuntimeKey::TimeSleep => RuntimeAbi {
            key,
            symbol: "align_rt_time_sleep",
            shape: RuntimeAbiShape::A58,
        },
        RuntimeKey::UdpBind => RuntimeAbi {
            key,
            symbol: "align_rt_udp_bind",
            shape: RuntimeAbiShape::A07,
        },
        RuntimeKey::UdpRecvFrom => RuntimeAbi {
            key,
            symbol: "align_rt_udp_recv_from",
            shape: RuntimeAbiShape::A36,
        },
        RuntimeKey::UdpSendTo => RuntimeAbi {
            key,
            symbol: "align_rt_udp_send_to",
            shape: RuntimeAbiShape::A40,
        },
        RuntimeKey::UdpSocketFree => RuntimeAbi {
            key,
            symbol: "align_rt_udp_socket_free",
            shape: RuntimeAbiShape::A62,
        },
        RuntimeKey::Utf8BoundaryFail => RuntimeAbi {
            key,
            symbol: "align_rt_utf8_boundary_fail",
            shape: RuntimeAbiShape::A60,
        },
        RuntimeKey::Utf8Valid => RuntimeAbi {
            key,
            symbol: "align_rt_utf8_valid",
            shape: RuntimeAbiShape::A00,
        },
    }
}

pub(super) fn keyed_runtime_abis() -> impl ExactSizeIterator<Item = RuntimeAbi> {
    RuntimeKey::ALL.into_iter().map(runtime_abi)
}

pub(super) fn runtime_abis() -> impl Iterator<Item = RuntimeAbi> {
    keyed_runtime_abis().chain(UNKEYED_RUNTIME_KEYS.into_iter().map(unkeyed_runtime_abi))
}

pub(super) fn validate_registry() -> Result<(), String> {
    if RuntimeKey::ALL.len() != 314 || keyed_runtime_abis().len() != 314 {
        return Err("runtime ABI registry invariant: key-count".to_string());
    }
    if runtime_abis().count() != 327 {
        return Err("runtime ABI registry invariant: base-count".to_string());
    }

    let mut keys = HashSet::with_capacity(RuntimeKey::ALL.len());
    let mut symbols = HashSet::with_capacity(327);
    for abi in keyed_runtime_abis() {
        let key = abi
            .runtime_key()
            .expect("keyed runtime iterator yielded an unkeyed row");
        if !keys.insert(key) {
            return Err(format!(
                "runtime ABI registry invariant: duplicate-key:{}",
                super::lowercase_hex(key.logical_name().as_bytes()),
            ));
        }
        if !symbols.insert(abi.symbol) {
            return Err(format!(
                "runtime ABI registry invariant: duplicate-symbol:{}",
                super::lowercase_hex(abi.symbol.as_bytes()),
            ));
        }
    }
    for abi in UNKEYED_RUNTIME_KEYS.into_iter().map(unkeyed_runtime_abi) {
        if !symbols.insert(abi.symbol) {
            return Err(format!(
                "runtime ABI registry invariant: duplicate-symbol:{}",
                super::lowercase_hex(abi.symbol.as_bytes()),
            ));
        }
    }
    for (key, symbol) in [
        (RuntimeKey::Print, "align_rt_print_i64"),
        (RuntimeKey::CliCommand, "align_rt_cli_command_new"),
        (RuntimeKey::HttpRequest, "align_rt_http_request_new"),
    ] {
        if runtime_abi(key).symbol != symbol {
            return Err(format!(
                "runtime ABI registry invariant: key-symbol:{}",
                super::lowercase_hex(key.logical_name().as_bytes()),
            ));
        }
    }
    Ok(())
}

pub(super) fn unkeyed_function_type<'c>(
    key: UnkeyedRuntimeKey,
    ctx: &'c Context,
) -> FunctionType<'c> {
    unkeyed_runtime_abi(key).function_type(ctx)
}

pub(super) fn unkeyed_runtime_abi(key: UnkeyedRuntimeKey) -> RuntimeAbi {
    let id = RuntimeAbiId::Unkeyed(key);
    match key {
        UnkeyedRuntimeKey::ReportError => RuntimeAbi {
            key: id,
            symbol: "align_rt_report_error",
            shape: RuntimeAbiShape::A91,
        },
        UnkeyedRuntimeKey::ArgsBuild => RuntimeAbi {
            key: id,
            symbol: "align_rt_args_build",
            shape: RuntimeAbiShape::A92,
        },
        UnkeyedRuntimeKey::ArenaReset => RuntimeAbi {
            key: id,
            symbol: "align_rt_arena_reset",
            shape: RuntimeAbiShape::A93,
        },
        UnkeyedRuntimeKey::Realloc => RuntimeAbi {
            key: id,
            symbol: "align_rt_realloc",
            shape: RuntimeAbiShape::A94,
        },
        UnkeyedRuntimeKey::HttpSerialize => RuntimeAbi {
            key: id,
            symbol: "align_rt_http_serialize",
            shape: RuntimeAbiShape::A95,
        },
        UnkeyedRuntimeKey::F32ToBits => RuntimeAbi {
            key: id,
            symbol: "align_rt_f32_to_bits",
            shape: RuntimeAbiShape::A96,
        },
        UnkeyedRuntimeKey::F32FromBits => RuntimeAbi {
            key: id,
            symbol: "align_rt_f32_from_bits",
            shape: RuntimeAbiShape::A97,
        },
        UnkeyedRuntimeKey::F64ToBits => RuntimeAbi {
            key: id,
            symbol: "align_rt_f64_to_bits",
            shape: RuntimeAbiShape::A98,
        },
        UnkeyedRuntimeKey::F64FromBits => RuntimeAbi {
            key: id,
            symbol: "align_rt_f64_from_bits",
            shape: RuntimeAbiShape::A99,
        },
        UnkeyedRuntimeKey::F32TextLen => RuntimeAbi {
            key: id,
            symbol: "align_rt_f32_text_len",
            shape: RuntimeAbiShape::A100,
        },
        UnkeyedRuntimeKey::F64TextLen => RuntimeAbi {
            key: id,
            symbol: "align_rt_f64_text_len",
            shape: RuntimeAbiShape::A98,
        },
        UnkeyedRuntimeKey::F32TextWrite => RuntimeAbi {
            key: id,
            symbol: "align_rt_f32_text_write",
            shape: RuntimeAbiShape::A101,
        },
        UnkeyedRuntimeKey::F64TextWrite => RuntimeAbi {
            key: id,
            symbol: "align_rt_f64_text_write",
            shape: RuntimeAbiShape::A102,
        },
    }
}

pub(super) fn unkeyed_symbol(key: UnkeyedRuntimeKey) -> &'static str {
    unkeyed_runtime_abi(key).symbol
}

pub(super) fn runtime_abi_for_symbol(symbol: &str) -> Option<RuntimeAbiId> {
    runtime_abis()
        .find(|abi| abi.symbol == symbol)
        .map(|abi| abi.key)
}

pub(super) fn function_type<'c>(id: RuntimeAbiId, ctx: &'c Context) -> FunctionType<'c> {
    runtime_abi_by_id(id).function_type(ctx)
}

pub(super) fn runtime_abi_by_id(id: RuntimeAbiId) -> RuntimeAbi {
    match id {
        RuntimeAbiId::Keyed(key) => runtime_abi(key),
        RuntimeAbiId::Unkeyed(key) => unkeyed_runtime_abi(key),
    }
}

/// Whether a source-derived extern type is compatible with the fixed native row of the same
/// physical name. Unknown names are ordinary user externs and therefore have no fixed row to
/// compare. Fixed names compare the complete LLVM function type, including the return and every
/// parameter ordinal.
pub(super) fn native_extern_abi_matches<'c>(
    symbol: &str,
    actual: FunctionType<'c>,
    ctx: &'c Context,
) -> bool {
    match runtime_abi_for_symbol(symbol) {
        Some(id) => actual == function_type(id, ctx),
        None => true,
    }
}

fn native_type<'c>(ctx: &'c Context, ty: NativeType) -> BasicTypeEnum<'c> {
    match ty {
        NativeType::I32 => ctx.i32_type().into(),
        NativeType::I64 => ctx.i64_type().into(),
        NativeType::F32 => ctx.f32_type().into(),
        NativeType::F64 => ctx.f64_type().into(),
        NativeType::Ptr => ctx.ptr_type(inkwell::AddressSpace::default()).into(),
    }
}

fn native_return_type<'c>(ctx: &'c Context, ret: NativeReturn) -> BasicTypeEnum<'c> {
    match ret {
        NativeReturn::Void => unreachable!("void has no BasicTypeEnum"),
        NativeReturn::I32 => ctx.i32_type().into(),
        NativeReturn::I64 => ctx.i64_type().into(),
        NativeReturn::F32 => ctx.f32_type().into(),
        NativeReturn::F64 => ctx.f64_type().into(),
        NativeReturn::Ptr => ctx.ptr_type(inkwell::AddressSpace::default()).into(),
        NativeReturn::I64Pair => ctx
            .struct_type(&[ctx.i64_type().into(), ctx.i64_type().into()], false)
            .into(),
        NativeReturn::PtrLen => ctx
            .struct_type(
                &[
                    ctx.ptr_type(inkwell::AddressSpace::default()).into(),
                    ctx.i64_type().into(),
                ],
                false,
            )
            .into(),
    }
}

fn shape_spec(shape: RuntimeAbiShape) -> RuntimeAbiShapeSpec {
    match shape {
        RuntimeAbiShape::A00 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[0],
        },
        RuntimeAbiShape::A01 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: true,
            read_ptr_params: &[0, 2],
        },
        RuntimeAbiShape::A02 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[0, 2],
        },
        RuntimeAbiShape::A03 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A04 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A05 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I32,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A06 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A07 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A08 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::Ptr, NativeType::I64, NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A09 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A10 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A12 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A13 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A15 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A16 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A17 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A18 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A19 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::Ptr, NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A20 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::Ptr, NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A21 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A22 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A23 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A24 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::Ptr, NativeType::Ptr, NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A25 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A26 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: true,
            read_ptr_params: &[0],
        },
        RuntimeAbiShape::A27 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[0, 2],
        },
        RuntimeAbiShape::A28 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::Ptr, NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[0, 1],
        },
        RuntimeAbiShape::A29 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A30 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A31 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::Ptr, NativeType::I64, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A32 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A33 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A34 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A35 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A36 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::Ptr, NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A37 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::Ptr, NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A38 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A39 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A40 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A41 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A42 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[],
            return_noalias: true,
            fn_attrs: &["nofree", "nounwind"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A43 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[NativeType::I64],
            return_noalias: true,
            fn_attrs: &["nofree", "nounwind"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A44 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[NativeType::Ptr, NativeType::I64],
            return_noalias: true,
            fn_attrs: &["nofree", "nounwind"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A45 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[NativeType::Ptr, NativeType::I64, NativeType::I64],
            return_noalias: true,
            fn_attrs: &["nounwind"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A46 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: true,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A47 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A48 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[NativeType::I32, NativeType::I32],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A49 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A50 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A51 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A52 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A53 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[NativeType::Ptr, NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A54 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[],
            return_noalias: false,
            fn_attrs: &["noreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A55 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::F64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A56 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::F32],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A57 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::I32],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A58 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A59 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::I64],
            return_noalias: false,
            fn_attrs: &["noreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A60 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::I64, NativeType::I64],
            return_noalias: false,
            fn_attrs: &["noreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A61 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::I64, NativeType::I64, NativeType::I64],
            return_noalias: false,
            fn_attrs: &["noreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A62 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A63 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::Ptr, NativeType::F64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A64 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::Ptr, NativeType::F32],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A65 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::Ptr, NativeType::I32],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A66 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A67 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I32,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A68 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A69 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A70 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A71 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A72 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::Ptr, NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A73 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::Ptr, NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A74 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I32,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A75 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A76 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A77 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A78 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A79 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::Ptr, NativeType::Ptr, NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A80 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A81 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A82 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64Pair,
            params: &[NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: true,
            read_ptr_params: &[0],
        },
        RuntimeAbiShape::A83 => RuntimeAbiShapeSpec {
            ret: NativeReturn::PtrLen,
            params: &[NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A84 => RuntimeAbiShapeSpec {
            ret: NativeReturn::PtrLen,
            params: &[NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A85 => RuntimeAbiShapeSpec {
            ret: NativeReturn::PtrLen,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A86 => RuntimeAbiShapeSpec {
            ret: NativeReturn::PtrLen,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A87 => RuntimeAbiShapeSpec {
            ret: NativeReturn::PtrLen,
            params: &[NativeType::Ptr, NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A88 => RuntimeAbiShapeSpec {
            ret: NativeReturn::PtrLen,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A89 => RuntimeAbiShapeSpec {
            ret: NativeReturn::PtrLen,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A90 => RuntimeAbiShapeSpec {
            ret: NativeReturn::PtrLen,
            params: &[
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I32,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A91 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::I32],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A92 => RuntimeAbiShapeSpec {
            ret: NativeReturn::PtrLen,
            params: &[NativeType::I32, NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A93 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Void,
            params: &[NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A94 => RuntimeAbiShapeSpec {
            ret: NativeReturn::Ptr,
            params: &[NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A95 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::Ptr, NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A96 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::F32],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A97 => RuntimeAbiShapeSpec {
            ret: NativeReturn::F32,
            params: &[NativeType::I32],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A98 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::F64],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A99 => RuntimeAbiShapeSpec {
            ret: NativeReturn::F64,
            params: &[NativeType::I64],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A100 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::F32],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A101 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::F32, NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A102 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I64,
            params: &[NativeType::F64, NativeType::Ptr, NativeType::I64],
            return_noalias: false,
            fn_attrs: &["nofree", "nosync", "willreturn"],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A103 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A104 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A105 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::Ptr, NativeType::I64, NativeType::Ptr, NativeType::Ptr, NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A106 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[NativeType::I32, NativeType::Ptr, NativeType::I64, NativeType::Ptr],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A107 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::I32,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A108 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::I32,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
        RuntimeAbiShape::A109 => RuntimeAbiShapeSpec {
            ret: NativeReturn::I32,
            params: &[
                NativeType::I32,
                NativeType::Ptr,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
                NativeType::I64,
                NativeType::Ptr,
            ],
            return_noalias: false,
            fn_attrs: &[],
            memory_argmem_read: false,
            read_ptr_params: &[],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RuntimeAbiId, UNKEYED_RUNTIME_KEYS, keyed_runtime_abis, native_extern_abi_matches,
        runtime_abi, runtime_abi_by_id, runtime_abi_for_symbol, runtime_abis, unkeyed_symbol,
        validate_registry,
    };
    use align_mir::RuntimeKey;
    use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType};
    use std::collections::HashSet;
    use std::fmt::Write;

    fn rebuilt_function_type<'c>(
        ctx: &'c inkwell::context::Context,
        ret: Option<BasicTypeEnum<'c>>,
        params: &[BasicMetadataTypeEnum<'c>],
    ) -> FunctionType<'c> {
        match ret {
            Some(ret) => ret.fn_type(params, false),
            None => ctx.void_type().fn_type(params, false),
        }
    }

    #[test]
    fn runtime_abi_registry_is_complete_and_unique() {
        fn assert_ord<T: Ord>() {}

        assert_ord::<RuntimeAbiId>();
        assert_eq!(
            UNKEYED_RUNTIME_KEYS.map(|key| key as u8),
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
        );
        validate_registry().unwrap();
        let rows: Vec<_> = runtime_abis().collect();
        assert_eq!(rows.len(), 327);
        assert_eq!(
            rows.iter().map(|row| row.key).collect::<HashSet<_>>().len(),
            327
        );
        assert_eq!(
            rows.iter()
                .map(|row| row.symbol)
                .collect::<HashSet<_>>()
                .len(),
            327
        );
        for (key, row) in RuntimeKey::ALL.into_iter().zip(keyed_runtime_abis()) {
            assert_eq!(row.key, RuntimeAbiId::Keyed(key));
            assert_eq!(runtime_abi(key).symbol, row.symbol);
        }
        assert_eq!(runtime_abi(RuntimeKey::Print).symbol, "align_rt_print_i64");
        assert_eq!(
            runtime_abi(RuntimeKey::CliCommand).symbol,
            "align_rt_cli_command_new"
        );
        assert_eq!(
            runtime_abi(RuntimeKey::HttpRequest).symbol,
            "align_rt_http_request_new"
        );

        for key in UNKEYED_RUNTIME_KEYS {
            let symbol = unkeyed_symbol(key);
            let id = RuntimeAbiId::Unkeyed(key);
            assert_eq!(runtime_abi_by_id(id).key, id);
            assert_eq!(runtime_abi_by_id(id).symbol, symbol);
            assert_eq!(runtime_abi_for_symbol(symbol), Some(id));
        }
        assert!(runtime_abi_for_symbol("align_rt_not_a_fixed_row").is_none());
    }

    #[test]
    fn runtime_abi_extern_type_matrix_is_exact_for_every_row_and_ordinal() {
        let ctx = inkwell::context::Context::create();
        let rows: Vec<_> = runtime_abis().collect();
        assert_eq!(rows.len(), 327);

        for row in rows {
            let symbol = row.symbol;
            let expected = row.function_type(&ctx);
            assert!(
                native_extern_abi_matches(symbol, expected, &ctx),
                "rejected exact native extern type for {symbol}",
            );

            let params = expected.get_param_types();
            let wrong_return = match expected.get_return_type() {
                Some(_) => rebuilt_function_type(&ctx, None, &params),
                None => rebuilt_function_type(&ctx, Some(ctx.i32_type().into()), &params),
            };
            assert!(
                !native_extern_abi_matches(symbol, wrong_return, &ctx),
                "accepted mutated native extern return for {symbol}",
            );

            for ordinal in 0..params.len() {
                let mut wrong_params = params.clone();
                wrong_params[ordinal] = if wrong_params[ordinal] == ctx.i32_type().into() {
                    ctx.i64_type().into()
                } else {
                    ctx.i32_type().into()
                };
                let wrong_param =
                    rebuilt_function_type(&ctx, expected.get_return_type(), &wrong_params);
                assert!(
                    !native_extern_abi_matches(symbol, wrong_param, &ctx),
                    "accepted mutated native extern parameter {ordinal} for {symbol}",
                );
            }
        }

        let ordinary = ctx.void_type().fn_type(&[], false);
        assert!(native_extern_abi_matches(
            "align_rt_not_a_fixed_row",
            ordinary,
            &ctx,
        ));
    }

    #[test]
    fn runtime_abi_registry_matches_checked_in_declaration_golden() {
        let ctx = inkwell::context::Context::create();
        let module = ctx.create_module("runtime_abi_golden");
        for abi in runtime_abis() {
            let function = abi.declare(&ctx, &module);
            abi.apply_attributes(&ctx, function);
        }

        let ir = module.print_to_string().to_string();
        let declaration = |symbol: &str| {
            let needle = format!("@{symbol}(");
            ir.lines()
                .find(|line| line.starts_with("declare ") && line.contains(&needle))
                .unwrap_or_else(|| panic!("missing golden declaration for {symbol}"))
        };
        let mut actual = String::new();
        for key in RuntimeKey::ALL {
            let abi = runtime_abi(key);
            writeln!(
                actual,
                "key|{key:?}|{}|{}",
                key.logical_name(),
                declaration(abi.symbol),
            )
            .unwrap();
        }
        for key in UNKEYED_RUNTIME_KEYS {
            writeln!(
                actual,
                "unkeyed|{key:?}|{}",
                declaration(unkeyed_symbol(key)),
            )
            .unwrap();
        }
        let guarded: Vec<_> = runtime_abis()
            .filter(|abi| abi.is_rt_lto_guarded())
            .map(|abi| abi.symbol)
            .collect();
        writeln!(actual, "rt-lto-guarded|{}", guarded.join(",")).unwrap();
        for line in ir.lines().filter(|line| line.starts_with("attributes #")) {
            writeln!(actual, "{line}").unwrap();
        }

        let golden_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/runtime_abi_declarations.txt"
        );
        if std::env::var_os("ALIGN_UPDATE_RUNTIME_ABI_GOLDEN").is_some() {
            std::fs::write(golden_path, &actual).unwrap();
            return;
        }
        assert_eq!(
            actual,
            include_str!("../tests/golden/runtime_abi_declarations.txt"),
            "runtime ABI declarations changed; compare the ledger before updating the golden",
        );
    }
}
