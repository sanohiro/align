//! Bounded reader-to-writer codecs. Each native step consumes one borrowed input window
//! and writes its output before the next read. Only the codec context spans calls.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Deflate(DeflateFormat),
    Zstd,
}

#[link(name = "zstd")]
unsafe extern "C" {
    fn ZSTD_compressStream2(
        ctx: *mut c_void,
        output: *mut ZstdOutBuffer,
        input: *mut ZstdInBuffer,
        end_directive: c_int,
    ) -> usize;
}

struct Step {
    consumed: usize,
    produced: usize,
    complete: bool,
}

// Static dispatch keeps libz and libzstd in separate native link closures.
trait Codec {
    fn step(&mut self, input: &[u8], output: &mut [u8], finish: bool) -> Result<Step, i32>;
    fn next_member(&mut self) -> Result<(), i32> {
        Ok(())
    }
}

struct DeflateCodec<'a> {
    stream: &'a mut ZStream,
    encode: bool,
    dictionary: &'a [u8],
}

impl Drop for DeflateCodec<'_> {
    fn drop(&mut self) {
        unsafe {
            if self.encode {
                deflateEnd(self.stream);
            } else {
                inflateEnd(self.stream);
            }
        }
    }
}

impl<'a> DeflateCodec<'a> {
    fn open(
        slot: &'a mut ZStream,
        format: DeflateFormat,
        encode: bool,
        level: c_int,
        dictionary: &'a [u8],
    ) -> Result<Self, i32> {
        let version = ZLIB_VERSION.as_ptr().cast();
        let size = c_int::try_from(core::mem::size_of::<ZStream>()).map_err(|_| AL_INVALID)?;
        let width = c_uint::try_from(dictionary.len()).map_err(|_| AL_INVALID)?;
        let status = unsafe {
            if encode {
                deflateInit2_(
                    slot,
                    level,
                    Z_DEFLATED,
                    format.window_bits(),
                    GZIP_MEM_LEVEL,
                    Z_DEFAULT_STRATEGY,
                    version,
                    size,
                )
            } else {
                inflateInit2_(slot, format.window_bits(), version, size)
            }
        };
        if status != Z_OK {
            return Err(zlib_error_to_status(status));
        }
        // The provider's state retains the address of slot. Only its reference moves.
        let codec = Self {
            stream: slot,
            encode,
            dictionary,
        };
        if !dictionary.is_empty() && (encode || format == DeflateFormat::Raw) {
            let status = unsafe {
                if encode {
                    deflateSetDictionary(codec.stream, dictionary.as_ptr(), width)
                } else {
                    inflateSetDictionary(codec.stream, dictionary.as_ptr(), width)
                }
            };
            if status != Z_OK {
                return Err(zlib_error_to_status(status));
            }
        }
        Ok(codec)
    }
}

impl Codec for DeflateCodec<'_> {
    fn next_member(&mut self) -> Result<(), i32> {
        let status = unsafe { inflateReset(self.stream) };
        if status == Z_OK {
            Ok(())
        } else {
            Err(zlib_error_to_status(status))
        }
    }
    fn step(&mut self, input: &[u8], output: &mut [u8], finish: bool) -> Result<Step, i32> {
        let stream = &mut *self.stream;
        stream.next_in = input.as_ptr();
        stream.avail_in = c_uint::try_from(input.len()).map_err(|_| AL_INVALID)?;
        stream.next_out = output.as_mut_ptr();
        stream.avail_out = c_uint::try_from(output.len()).map_err(|_| AL_INVALID)?;
        let mut status = unsafe {
            if self.encode {
                deflate(stream, if finish { Z_FINISH } else { Z_NO_FLUSH })
            } else {
                inflate(stream, Z_NO_FLUSH)
            }
        };
        if status == Z_NEED_DICT && !self.dictionary.is_empty() {
            let width = c_uint::try_from(self.dictionary.len()).map_err(|_| AL_INVALID)?;
            status = unsafe { inflateSetDictionary(stream, self.dictionary.as_ptr(), width) };
            if status != Z_OK {
                return Err(zlib_error_to_status(status));
            }
            status = unsafe { inflate(stream, Z_NO_FLUSH) };
        }
        if !matches!(status, Z_OK | Z_BUF_ERROR | Z_STREAM_END) {
            return Err(zlib_error_to_status(status));
        }
        Ok(Step {
            consumed: input
                .len()
                .checked_sub(usize::try_from(stream.avail_in).map_err(|_| AL_INVALID)?)
                .ok_or(AL_INVALID)?,
            produced: output
                .len()
                .checked_sub(usize::try_from(stream.avail_out).map_err(|_| AL_INVALID)?)
                .ok_or(AL_INVALID)?,
            complete: status == Z_STREAM_END,
        })
    }
}

struct ZstdCodec {
    ctx: *mut c_void,
    encode: bool,
}
impl Drop for ZstdCodec {
    fn drop(&mut self) {
        unsafe {
            if self.encode {
                ZSTD_freeCCtx(self.ctx);
            } else {
                ZSTD_freeDStream(self.ctx);
            }
        }
    }
}
impl ZstdCodec {
    fn open(encode: bool, level: c_int, dictionary: &[u8]) -> Result<Self, i32> {
        let ctx = unsafe {
            if encode {
                ZSTD_createCCtx()
            } else {
                ZSTD_createDStream()
            }
        };
        if ctx.is_null() {
            return Err(AL_CODE);
        }
        let codec = Self { ctx, encode };
        unsafe {
            if encode {
                zstd_operation_result(ZSTD_CCtx_setParameter(
                    ctx,
                    ZSTD_C_COMPRESSION_LEVEL,
                    level,
                ))?;
                zstd_operation_result(ZSTD_CCtx_loadDictionary(
                    ctx,
                    dictionary.as_ptr().cast(),
                    dictionary.len(),
                ))?;
            } else {
                zstd_operation_result(ZSTD_initDStream(ctx))?;
                zstd_operation_result(ZSTD_DCtx_loadDictionary(
                    ctx,
                    dictionary.as_ptr().cast(),
                    dictionary.len(),
                ))?;
            }
        }
        Ok(codec)
    }
}
impl Codec for ZstdCodec {
    fn step(&mut self, input: &[u8], output: &mut [u8], finish: bool) -> Result<Step, i32> {
        let mut source = ZstdInBuffer {
            src: input.as_ptr().cast(),
            size: input.len(),
            pos: 0,
        };
        let mut target = ZstdOutBuffer {
            dst: output.as_mut_ptr().cast(),
            size: output.len(),
            pos: 0,
        };
        let code = if self.encode {
            // ZSTD_e_continue = 0, ZSTD_e_end = 2.
            zstd_operation_result(unsafe {
                ZSTD_compressStream2(
                    self.ctx,
                    &mut target,
                    &mut source,
                    if finish { 2 } else { 0 },
                )
            })?
        } else {
            let result = unsafe { ZSTD_decompressStream(self.ctx, &mut target, &mut source) };
            if unsafe { ZSTD_isError(result) } != 0 {
                return Err(zstd_decompress_error_to_status(result));
            }
            result
        };
        Ok(Step {
            consumed: source.pos,
            produced: target.pos,
            complete: code == 0 && (!self.encode || finish),
        })
    }
}

struct Output<'a> {
    writer: &'a mut Writer,
    window: Vec<u8>,
    limit: i64,
    total: i64,
}

impl Output<'_> {
    /// Stop at the first frame boundary or when more input is needed. Drain a full last
    /// output grant before returning, since the next read itself can fail.
    fn feed(
        &mut self,
        codec: &mut impl Codec,
        input: &mut &[u8],
        finish: bool,
    ) -> Result<bool, i32> {
        let mut probe = [0u8; 1];
        loop {
            let remaining = usize::try_from(self.limit - self.total).unwrap_or(usize::MAX);
            let grant = remaining.min(self.window.len());
            let output = if grant == 0 {
                &mut probe[..]
            } else {
                &mut self.window[..grant]
            };
            let step = codec.step(input, output, finish)?;
            if step.produced > grant {
                return Err(AL_INVALID);
            }
            if step.produced != 0 {
                let count = i64::try_from(step.produced).map_err(|_| AL_INVALID)?;
                let status =
                    unsafe { align_rt_io_writer_write(self.writer, output.as_ptr(), count) };
                if status != 0 {
                    return Err(status);
                }
                self.total += count;
            }
            *input = input.get(step.consumed..).ok_or(AL_INVALID)?;
            if step.complete {
                return Ok(true);
            }
            if step.consumed == 0 && step.produced == 0 {
                return if input.is_empty() && !finish {
                    Ok(false)
                } else {
                    Err(AL_INVALID)
                };
            }
            if input.is_empty() && step.produced < output.len() && !finish {
                return Ok(false);
            }
        }
    }
}

#[derive(PartialEq, Eq)]
enum Frame {
    Start,
    Active,
    End,
}

fn pump(
    reader: &mut Reader,
    scratch: &mut Option<Buffer>,
    output: &mut Output<'_>,
    codec: &mut impl Codec,
    format: Format,
    encode: bool,
) -> Result<i64, i32> {
    let mut phase = Frame::Start;
    let mut prefix = [0u8; 4];
    let mut prefix_len = 0;
    loop {
        let mut input = io_copy_window(reader, scratch.as_mut()).map_err(|negative| {
            negative
                .checked_neg()
                .and_then(|n| i32::try_from(n).ok())
                .unwrap_or(AL_INVALID)
        })?;
        if input.is_empty() {
            if encode {
                if !output.feed(codec, &mut input, true)? {
                    return Err(AL_INVALID);
                }
            } else if phase != Frame::End {
                return Err(AL_INVALID);
            }
            return Ok(output.total);
        }
        if encode {
            output.feed(codec, &mut input, false)?;
            continue;
        }
        while !input.is_empty() {
            if phase == Frame::End {
                match format {
                    Format::Deflate(DeflateFormat::Gzip) => codec.next_member()?,
                    Format::Zstd => {}
                    _ => return Err(AL_INVALID),
                }
                phase = Frame::Start;
            }
            if phase == Frame::Start && format == Format::Zstd {
                let count = (prefix.len() - prefix_len).min(input.len());
                prefix[prefix_len..prefix_len + count].copy_from_slice(&input[..count]);
                prefix_len += count;
                input = &input[count..];
                if prefix_len != prefix.len() {
                    continue;
                }
                if !zstd_current_frame(&prefix) {
                    return Err(AL_INVALID);
                }
                let mut magic = &prefix[..];
                if output.feed(codec, &mut magic, false)? || !magic.is_empty() {
                    return Err(AL_INVALID);
                }
                prefix_len = 0;
                phase = Frame::Active;
                if input.is_empty() {
                    continue;
                }
            }
            phase = if output.feed(codec, &mut input, false)? {
                Frame::End
            } else {
                Frame::Active
            };
        }
    }
}

struct Call {
    reader: *mut Reader,
    writer: *mut Writer,
    number: i64,
    dict: *const u8,
    dict_len: i64,
}
struct Prepared<'a> {
    reader: &'a mut Reader,
    scratch: Option<Buffer>,
    output: Output<'a>,
    dictionary: &'a [u8],
}

impl Call {
    fn output_limit(&self, encode: bool) -> i64 {
        if encode {
            i64::MAX
        } else if self.number >= 0 {
            self.number
        } else {
            panic_abort("compress.decompress_to: max_bytes must be nonnegative")
        }
    }

    unsafe fn prepare(
        &mut self,
        limit: i64,
        max_dictionary: usize,
        normalize: impl FnOnce(&[u8]) -> Result<&[u8], i32>,
    ) -> Result<Prepared<'_>, i32> {
        let (Some(reader), Some(writer)) = (unsafe { self.reader.as_mut() }, unsafe {
            self.writer.as_mut()
        }) else {
            return Err(AL_INVALID);
        };
        let size = usize::try_from(self.dict_len).map_err(|_| AL_INVALID)?;
        if isize::try_from(size).is_err()
            || (size > 0 && self.dict.is_null())
            || size > max_dictionary
        {
            return Err(AL_INVALID);
        }
        let dictionary = if size == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.dict, size) }
        };
        let dictionary = normalize(dictionary)?;
        let scratch = io_copy_scratch(reader)?;
        let mut window = Vec::new();
        window
            .try_reserve_exact(GZIP_OUT_CHUNK)
            .map_err(|_| AL_INVALID)?;
        window.resize(GZIP_OUT_CHUNK, 0);
        let output = Output {
            writer,
            window,
            total: 0,
            limit,
        };
        Ok(Prepared {
            reader,
            scratch,
            output,
            dictionary,
        })
    }
    unsafe fn deflate(mut self, encode: bool, format: DeflateFormat) -> Result<i64, i32> {
        let limit = self.output_limit(encode);
        let level = if encode {
            deflate_level(format, self.number)
        } else {
            0
        };
        let width = if format == DeflateFormat::Gzip {
            0
        } else {
            zlib_input_chunk()
        };
        let mut io = unsafe { self.prepare(limit, width, |bytes| Ok(bytes)) }?;
        let mut slot = ZStream::zeroed();
        let mut codec = DeflateCodec::open(&mut slot, format, encode, level, io.dictionary)?;
        pump(
            io.reader,
            &mut io.scratch,
            &mut io.output,
            &mut codec,
            Format::Deflate(format),
            encode,
        )
    }
    unsafe fn zstd(mut self, encode: bool) -> Result<i64, i32> {
        let limit = self.output_limit(encode);
        let level = if encode { zstd_level(self.number) } else { 0 };
        let mut io = unsafe { self.prepare(limit, usize::MAX, zstd_dictionary) }?;
        let mut codec = ZstdCodec::open(encode, level, io.dictionary)?;
        pump(
            io.reader,
            &mut io.scratch,
            &mut io.output,
            &mut codec,
            Format::Zstd,
            encode,
        )
    }
}
macro_rules! stream_codec {
    ($name:ident, $method:ident, $encode:literal $(, $format:expr)?) => {
        /// Transcode through borrowed handles. Returns the accepted byte count or a negative I/O/codec status.
        ///
        /// # Safety
        /// Nonnull handles must point to live, distinct readers and writers. A nonempty dictionary
        /// must remain readable until this synchronous function returns.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            reader: *mut Reader,
            writer: *mut Writer,
            number: i64,
            dictionary: *const u8,
            dictionary_len: i64,
        ) -> i64 {
            unsafe {
                Call { reader, writer, number, dict: dictionary, dict_len: dictionary_len }.$method($encode $(, $format)?)
            }
            .unwrap_or_else(|status| -i64::from(status))
        }
    };
}

stream_codec!(
    align_rt_compress_gzip_compress_to,
    deflate,
    true,
    DeflateFormat::Gzip
);
stream_codec!(
    align_rt_compress_gzip_decompress_to,
    deflate,
    false,
    DeflateFormat::Gzip
);
stream_codec!(
    align_rt_compress_zlib_compress_to,
    deflate,
    true,
    DeflateFormat::Zlib
);
stream_codec!(
    align_rt_compress_zlib_decompress_to,
    deflate,
    false,
    DeflateFormat::Zlib
);
stream_codec!(
    align_rt_compress_deflate_raw_compress_to,
    deflate,
    true,
    DeflateFormat::Raw
);
stream_codec!(
    align_rt_compress_deflate_raw_decompress_to,
    deflate,
    false,
    DeflateFormat::Raw
);
stream_codec!(align_rt_compress_zstd_compress_to, zstd, true);
stream_codec!(align_rt_compress_zstd_decompress_to, zstd, false);
