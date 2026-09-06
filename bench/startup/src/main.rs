//! S0A local startup-evidence observer.
//!
//! The public and persisted contracts are fixed by
//! `docs/impl/35-startup-observation-design.md`. This executable deliberately
//! has no convenience mode: the supported entry is `bench/startup/run.sh`,
//! which transfers an unlinked, digest-bound image on descriptor 9.

#![deny(unsafe_op_in_unsafe_fn)]

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const PROVENANCE_MAX: u64 = 64 * 1024;
const FIXTURE_MAX: u64 = 1024 * 1024;
const FILE_MAX: u64 = 64 * 1024 * 1024;
const WORK_TOTAL_MAX: u64 = 4 * 1024 * 1024 * 1024;
const WORK_AVAILABLE_MIN: u64 = 1024 * 1024 * 1024;
const WARMUPS: u32 = 2;
const MEASURED: u32 = 20;
const TIMED_EXECUTION_NS: u64 = 5_000_000_000;
const TIMED_CLEANUP_NS: u64 = 1_000_000_000;
const GLOBAL_NS: u64 = 900_000_000_000;

#[derive(Debug)]
struct AppError {
    usage: bool,
    message: String,
}

impl AppError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            usage: true,
            message: message.into(),
        }
    }

    fn operational(message: impl Into<String>) -> Self {
        Self {
            usage: false,
            message: message.into(),
        }
    }

    fn io(context: &str, error: io::Error) -> Self {
        Self::operational(format!("{context}: {error}"))
    }
}

type AppResult<T> = Result<T, AppError>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct PublicArgs {
    observer: PathBuf,
    observer_sha256: [u8; 32],
    observer_sha256_text: String,
    work_dir: PathBuf,
    provenance: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Arm {
    name: &'static str,
    source_revision: String,
    compiler: PathBuf,
    compiler_sha256: [u8; 32],
    runtime: PathBuf,
    runtime_sha256: [u8; 32],
    compiler_version: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Provenance {
    raw: Vec<u8>,
    digest: [u8; 32],
    fixture_revision: String,
    host_os: Vec<u8>,
    host_kernel: Vec<u8>,
    host_arch: Vec<u8>,
    cpu_identity: Vec<u8>,
    logical_cpu_count: u32,
    memory_bytes: u64,
    power_condition: Vec<u8>,
    load_condition: Vec<u8>,
    target_triple: Vec<u8>,
    libc_identity: Vec<u8>,
    build_path: Vec<PathBuf>,
    cc_path: PathBuf,
    cc_sha256: [u8; 32],
    cc_version: Vec<u8>,
    ld_path: PathBuf,
    ld_sha256: [u8; 32],
    ld_version: Vec<u8>,
    inspector_path: PathBuf,
    inspector_sha256: [u8; 32],
    inspector_version: Vec<u8>,
    arms: Vec<Arm>,
}

#[derive(Clone, Copy, Debug)]
struct Fixture {
    id: &'static str,
    bytes: &'static [u8],
    argv: &'static [&'static [u8]],
    stdout: &'static [u8],
    stderr: &'static [u8],
}

const FIXTURES: [Fixture; 10] = [
    Fixture {
        id: "empty-i32",
        bytes: include_bytes!("../fixtures/empty-i32.align"),
        argv: &[],
        stdout: b"",
        stderr: b"",
    },
    Fixture {
        id: "empty-result",
        bytes: include_bytes!("../fixtures/empty-result.align"),
        argv: &[],
        stdout: b"",
        stderr: b"",
    },
    Fixture {
        id: "argv",
        bytes: include_bytes!("../fixtures/argv.align"),
        argv: &[b"alpha", "β".as_bytes()],
        stdout: b"",
        stderr: b"",
    },
    Fixture {
        id: "primitive-output",
        bytes: include_bytes!("../fixtures/primitive-output.align"),
        argv: &[],
        stdout: b"42\n",
        stderr: b"",
    },
    Fixture {
        id: "arena-reset",
        bytes: include_bytes!("../fixtures/arena-reset.align"),
        argv: &[],
        stdout: b"",
        stderr: b"",
    },
    Fixture {
        id: "par-map-below-floor",
        bytes: include_bytes!("../fixtures/par-map-below-floor.align"),
        argv: &[],
        stdout: b"",
        stderr: b"",
    },
    Fixture {
        id: "par-map-first-pool",
        bytes: include_bytes!("../fixtures/par-map-first-pool.align"),
        argv: &[],
        stdout: b"",
        stderr: b"",
    },
    Fixture {
        id: "par-map-reused-pool",
        bytes: include_bytes!("../fixtures/par-map-reused-pool.align"),
        argv: &[],
        stdout: b"",
        stderr: b"",
    },
    Fixture {
        id: "task-group-single",
        bytes: include_bytes!("../fixtures/task-group-single.align"),
        argv: &[],
        stdout: b"",
        stderr: b"",
    },
    Fixture {
        id: "readonly-first-touch",
        bytes: include_bytes!("../fixtures/readonly-first-touch.align"),
        argv: &[b"probe"],
        stdout: b"",
        stderr: b"",
    },
];

fn main() -> ExitCode {
    match run(env::args_os().collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("align-startup-observer: {}", error.message);
            ExitCode::from(if error.usage { 2 } else { 1 })
        }
    }
}

fn run(arguments: Vec<OsString>) -> AppResult<()> {
    let Some(mode) = arguments.get(1).map(OsString::as_os_str) else {
        return Err(AppError::usage("missing internal invocation prefix"));
    };
    if mode.as_bytes() == b"--exec-tool-fd" || mode.as_bytes() == b"--exec-tool-path" {
        return exec_tool(&arguments[1..]);
    }
    if mode.as_bytes() == b"--self-probe" {
        return self_probe(&arguments);
    }
    if mode.as_bytes() != b"--adopt-fd" {
        return Err(AppError::usage("invalid internal invocation prefix"));
    }
    run_adopted(&arguments[1..])
}

fn run_adopted(arguments: &[OsString]) -> AppResult<()> {
    if arguments.len() < 4
        || arguments[0].as_bytes() != b"--adopt-fd"
        || arguments[1].as_bytes() != b"9"
        || arguments[2].as_bytes() != b"--expected-sha256"
    {
        return Err(AppError::operational("invalid descriptor-adoption prefix"));
    }
    let expected_text = arguments[3]
        .to_str()
        .ok_or_else(|| AppError::operational("observer digest is not ASCII"))?;
    let expected = parse_sha256(expected_text)
        .map_err(|_| AppError::operational("observer digest is not canonical SHA-256"))?;
    require_empty_environment()?;
    let mut observer = adopt_observer_fd(9)?;
    let observed = sha256_reader(&mut observer, FILE_MAX)?;
    if observed != expected {
        return Err(AppError::operational("observer descriptor digest mismatch"));
    }
    let public = parse_public_args(&arguments[4..])?;
    if public.observer_sha256 != expected {
        return Err(AppError::usage(
            "public and internal observer digests differ",
        ));
    }
    execute(public, observer)
}

fn require_empty_environment() -> AppResult<()> {
    if env::vars_os().next().is_some() {
        return Err(AppError::operational("wrapper environment is not empty"));
    }
    Ok(())
}

fn adopt_observer_fd(descriptor: RawFd) -> AppResult<File> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: `stat` points to writable storage and the wrapper promises fd 9.
    if unsafe { libc::fstat(descriptor, stat.as_mut_ptr()) } == -1 {
        return Err(AppError::io(
            "observer descriptor",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: successful fstat initialized the record.
    let stat = unsafe { stat.assume_init() };
    let file_type = stat.st_mode & libc::S_IFMT;
    let permissions = stat.st_mode & 0o7777;
    // SAFETY: geteuid has no preconditions.
    let euid = unsafe { libc::geteuid() };
    if file_type != libc::S_IFREG
        || permissions != 0o500
        || stat.st_uid != euid
        || stat.st_nlink != 0
        || stat.st_size < 0
        || u64::try_from(stat.st_size)
            .ok()
            .is_none_or(|size| size > FILE_MAX)
    {
        return Err(AppError::operational(
            "observer descriptor invariant failed",
        ));
    }
    // SAFETY: validation above established ownership transfer of the open fd.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn parse_public_args(arguments: &[OsString]) -> AppResult<PublicArgs> {
    if arguments.len() != 8 {
        return Err(AppError::usage("expected exactly four public options"));
    }
    let mut observer = None;
    let mut observer_sha256 = None;
    let mut work_dir = None;
    let mut provenance = None;
    for pair in arguments.chunks_exact(2) {
        let value = pair[1].clone();
        if value.as_bytes().is_empty() {
            return Err(AppError::usage("empty option value"));
        }
        match pair[0].as_bytes() {
            b"--observer" if observer.is_none() => observer = Some(absolute_path(value)?),
            b"--observer-sha256" if observer_sha256.is_none() => {
                let text = value
                    .to_str()
                    .ok_or_else(|| AppError::usage("observer digest is not ASCII"))?;
                observer_sha256 = Some((parse_sha256(text)?, text.to_owned()));
            }
            b"--work-dir" if work_dir.is_none() => work_dir = Some(absolute_path(value)?),
            b"--provenance" if provenance.is_none() => provenance = Some(absolute_path(value)?),
            b"--observer" | b"--observer-sha256" | b"--work-dir" | b"--provenance" => {
                return Err(AppError::usage("repeated public option"));
            }
            _ => return Err(AppError::usage("unknown public option")),
        }
    }
    let (observer_sha256, observer_sha256_text) =
        observer_sha256.ok_or_else(|| AppError::usage("missing --observer-sha256"))?;
    Ok(PublicArgs {
        observer: observer.ok_or_else(|| AppError::usage("missing --observer"))?,
        observer_sha256,
        observer_sha256_text,
        work_dir: work_dir.ok_or_else(|| AppError::usage("missing --work-dir"))?,
        provenance: provenance.ok_or_else(|| AppError::usage("missing --provenance"))?,
    })
}

fn absolute_path(value: OsString) -> AppResult<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_absolute() || path.as_os_str().as_bytes().contains(&0) {
        return Err(AppError::usage(
            "path option must be an absolute non-NUL path",
        ));
    }
    Ok(path)
}

fn parse_sha256(text: &str) -> AppResult<[u8; 32]> {
    if text.len() != 64
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AppError::usage(
            "digest must be 64 lowercase hexadecimal bytes",
        ));
    }
    let decoded = decode_hex(text.as_bytes(), false)?;
    let mut digest = [0; 32];
    digest.copy_from_slice(&decoded);
    Ok(digest)
}

fn execute(args: PublicArgs, observer: File) -> AppResult<()> {
    // Retain the loaded-image descriptor for the full observer lifetime.
    let observer = observer;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return Err(AppError::usage(
            "the startup observer refuses effective uid 0",
        ));
    }
    configure_observer_signals()?;
    let watchdog = Watchdog::new()?;
    let provenance = Provenance::read(&args.provenance)?;
    let repo = canonical_repo_root()?;
    let work = validate_work_root(&args.work_dir, &repo)?;
    validate_platform_provenance(&provenance)?;
    validate_tools(&provenance, &repo)?;
    let fixture_bundle = validate_fixtures(&repo)?;
    let execution_state = stable_execution_state()?;
    validate_inherited_file_limit()?;
    validate_tool_versions(&watchdog, &observer, &provenance, &repo, &execution_state)?;
    validate_launcher_probe(&watchdog, &observer, &provenance, &repo, &execution_state)?;
    create_owned_topology(&work.path, &provenance.raw)?;
    // Artifact production and the bounded child lifecycle are formed only
    // after the complete input boundary above has accepted.
    produce_evidence(
        &args,
        &provenance,
        &work.path,
        &work.filesystem_record,
        &execution_state,
        fixture_bundle,
        &observer,
        &watchdog,
    )
}

fn configure_observer_signals() -> AppResult<()> {
    let mut empty = MaybeUninit::<libc::sigset_t>::uninit();
    // SAFETY: empty is writable signal-set storage.
    if unsafe { libc::sigemptyset(empty.as_mut_ptr()) } == -1 {
        return Err(AppError::io("signal setup", io::Error::last_os_error()));
    }
    // SAFETY: successful sigemptyset initialized the set.
    let empty = unsafe { empty.assume_init() };
    for (signal, handler) in [
        (libc::SIGCHLD, libc::SIG_DFL),
        (libc::SIGPIPE, libc::SIG_IGN),
    ] {
        let action = libc::sigaction {
            sa_sigaction: handler,
            sa_mask: empty,
            sa_flags: 0,
            #[cfg(target_os = "linux")]
            sa_restorer: None,
        };
        // SAFETY: action is a fully initialized fixed disposition record.
        if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == -1 {
            return Err(AppError::io("signal setup", io::Error::last_os_error()));
        }
    }
    Ok(())
}

fn canonical_repo_root() -> AppResult<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::canonicalize(manifest.join("../.."))
        .map_err(|error| AppError::io("repository canonicalization", error))
}

struct WorkAdmission {
    path: PathBuf,
    filesystem_record: Vec<u8>,
}

fn validate_work_root(path: &Path, repo: &Path) -> AppResult<WorkAdmission> {
    let canonical =
        fs::canonicalize(path).map_err(|error| AppError::usage(format!("work root: {error}")))?;
    if canonical != path {
        return Err(AppError::usage(
            "work root is not a canonical nofollow path",
        ));
    }
    if canonical == repo || canonical.starts_with(repo) {
        return Err(AppError::usage("work root is contained by the repository"));
    }
    let metadata = fs::symlink_metadata(&canonical)
        .map_err(|error| AppError::usage(format!("work root: {error}")))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AppError::usage("work root is not a nofollow directory"));
    }
    let mut entries = fs::read_dir(&canonical)
        .map_err(|error| AppError::usage(format!("work root enumeration: {error}")))?;
    if entries
        .next()
        .transpose()
        .map_err(|error| AppError::usage(format!("work root enumeration: {error}")))?
        .is_some()
    {
        return Err(AppError::usage("work root is not empty"));
    }
    validate_work_filesystem(&canonical)?;
    let first = work_filesystem_record(&canonical)?;
    let second = work_filesystem_record(&canonical)?;
    if first != second {
        return Err(AppError::usage("work filesystem changed during admission"));
    }
    Ok(WorkAdmission {
        path: canonical,
        filesystem_record: first,
    })
}

#[allow(clippy::unnecessary_cast)] // statvfs field widths differ between Linux and macOS.
fn validate_work_filesystem(path: &Path) -> AppResult<()> {
    let c_path = cstring_path(path)?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: c_path is NUL-terminated and stat is writable.
    if unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) } == -1 {
        return Err(AppError::usage(format!(
            "work filesystem: {}",
            io::Error::last_os_error()
        )));
    }
    // SAFETY: successful statvfs initialized the record.
    let stat = unsafe { stat.assume_init() };
    let fragment = stat.f_frsize as u64;
    let blocks = stat.f_blocks as u64;
    let available = stat.f_bavail as u64;
    let total_bytes = fragment
        .checked_mul(blocks)
        .ok_or_else(|| AppError::usage("work filesystem capacity overflow"))?;
    let available_bytes = fragment
        .checked_mul(available)
        .ok_or_else(|| AppError::usage("work filesystem availability overflow"))?;
    if fragment == 0
        || blocks == 0
        || total_bytes > WORK_TOTAL_MAX
        || available_bytes < WORK_AVAILABLE_MIN
    {
        return Err(AppError::usage(
            "work filesystem capacity is outside the S0A bounds",
        ));
    }
    #[cfg(target_os = "linux")]
    let noexec = stat.f_flag & libc::ST_NOEXEC as libc::c_ulong != 0;
    #[cfg(target_os = "macos")]
    let noexec = stat.f_flag & libc::MNT_NOEXEC as libc::c_ulong != 0;
    if noexec {
        return Err(AppError::usage("work filesystem is mounted noexec"));
    }
    Ok(())
}

fn validate_platform_provenance(provenance: &Provenance) -> AppResult<()> {
    let expected_os: &[u8] = if cfg!(target_os = "linux") {
        b"linux"
    } else {
        b"macos"
    };
    let expected_arch: &[u8] = if cfg!(target_arch = "x86_64") {
        b"x86_64"
    } else {
        b"aarch64"
    };
    let expected_triple: &[u8] = match (cfg!(target_os = "linux"), cfg!(target_arch = "x86_64")) {
        (true, true) => b"x86_64-pc-linux-gnu",
        (true, false) => b"aarch64-unknown-linux-gnu",
        (false, false) => b"aarch64-apple-darwin",
        (false, true) => return Err(AppError::usage("macOS x86-64 is unsupported")),
    };
    if provenance.host_os != expected_os
        || provenance.host_arch != expected_arch
        || provenance.target_triple != expected_triple
    {
        return Err(AppError::usage(
            "provenance platform does not match the observer",
        ));
    }
    // SAFETY: sysconf is read-only process observation.
    let online = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if online <= 0 || u32::try_from(online).ok() != Some(provenance.logical_cpu_count) {
        return Err(AppError::usage("provenance logical CPU count mismatch"));
    }
    if current_memory_bytes()? != provenance.memory_bytes {
        return Err(AppError::usage("provenance memory size mismatch"));
    }
    if current_host_kernel()? != provenance.host_kernel {
        return Err(AppError::usage("provenance kernel identity mismatch"));
    }
    if current_cpu_identity()? != provenance.cpu_identity {
        return Err(AppError::usage("provenance CPU identity mismatch"));
    }
    if current_libc_identity()? != provenance.libc_identity {
        return Err(AppError::usage("provenance libc identity mismatch"));
    }
    Ok(())
}

fn current_host_kernel() -> AppResult<Vec<u8>> {
    let mut name = MaybeUninit::<libc::utsname>::zeroed();
    // SAFETY: name is writable storage for uname.
    if unsafe { libc::uname(name.as_mut_ptr()) } == -1 {
        return Err(AppError::usage("kernel identity unavailable"));
    }
    // SAFETY: successful uname initialized every fixed field.
    let name = unsafe { name.assume_init() };
    let sysname = bounded_c_char_array(&name.sysname, "uname sysname")?;
    let release = bounded_c_char_array(&name.release, "uname release")?;
    let version = bounded_c_char_array(&name.version, "uname version")?;
    let machine = bounded_c_char_array(&name.machine, "uname machine")?;
    encode_list_bytes(&[&sysname, &release, &version, &machine])
}

fn encode_list_bytes(elements: &[&[u8]]) -> AppResult<Vec<u8>> {
    let mut output = Vec::new();
    for element in elements {
        let length = u32::try_from(element.len())
            .map_err(|_| AppError::operational("list element overflow"))?;
        output.extend_from_slice(&length.to_be_bytes());
        output.extend_from_slice(element);
    }
    Ok(output)
}

#[cfg(target_arch = "x86_64")]
fn current_cpu_identity() -> AppResult<Vec<u8>> {
    use std::arch::x86_64::__cpuid_count;
    // SAFETY: CPUID is available on every admitted x86-64 host.
    let basic_max = __cpuid_count(0, 0).eax;
    // SAFETY: extended maximum query is universally defined when CPUID exists.
    let extended_max = __cpuid_count(0x8000_0000, 0).eax;
    let mut output = b"align-startup-cpu-v1\nplatform_arch\tlinux-x86_64\n".to_vec();
    for (leaf, subleaf) in [
        (0, 0),
        (1, 0),
        (7, 0),
        (0x8000_0000, 0),
        (0x8000_0001, 0),
        (0x8000_0002, 0),
        (0x8000_0003, 0),
        (0x8000_0004, 0),
    ] {
        let available = if leaf & 0x8000_0000 == 0 {
            leaf <= basic_max
        } else {
            leaf <= extended_max
        };
        if available {
            // SAFETY: the maximum-leaf query admitted this leaf/subleaf.
            let value = __cpuid_count(leaf, subleaf);
            output.extend_from_slice(
                format!(
                    "cpuid\t{leaf:08x}\t{subleaf:08x}\t{:08x}{:08x}{:08x}{:08x}\n",
                    value.eax, value.ebx, value.ecx, value.edx
                )
                .as_bytes(),
            );
        } else {
            output.extend_from_slice(
                format!("cpuid\t{leaf:08x}\t{subleaf:08x}\tunavailable\n").as_bytes(),
            );
        }
    }
    if output.len() > 64 * 1024 {
        return Err(AppError::usage("CPU identity exceeds 64 KiB"));
    }
    Ok(output)
}

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn current_cpu_identity() -> AppResult<Vec<u8>> {
    let online_bytes = read_bounded_virtual_nofollow(
        Path::new("/sys/devices/system/cpu/online"),
        4096,
        "online CPU list",
    )?;
    let online_text = online_bytes.strip_suffix(b"\n").unwrap_or(&online_bytes);
    let cpus = parse_cpu_list(online_text)?;
    // SAFETY: sysconf is a read-only process observation.
    let expected = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if expected <= 0 || cpus.len() != expected as usize {
        return Err(AppError::usage("online CPU identity count mismatch"));
    }
    // SAFETY: getauxval is a read-only process observation.
    let hwcap = unsafe { libc::getauxval(libc::AT_HWCAP) } as u64;
    // SAFETY: getauxval is a read-only process observation.
    let hwcap2 = unsafe { libc::getauxval(libc::AT_HWCAP2) } as u64;
    let mut output = format!("align-startup-cpu-v1\nplatform_arch\tlinux-aarch64\nauxv_hwcap\t{hwcap:016x}\nauxv_hwcap2\t{hwcap2:016x}\nonline_cpus\t{}\n", format_cpu_list(&cpus)).into_bytes();
    for cpu in cpus {
        let path = PathBuf::from(format!(
            "/sys/devices/system/cpu/cpu{cpu}/regs/identification/midr_el1"
        ));
        let bytes = read_bounded_virtual_nofollow(&path, 128, "MIDR identity")?;
        let text = std::str::from_utf8(bytes.strip_suffix(b"\n").unwrap_or(&bytes))
            .map_err(|_| AppError::usage("MIDR is not ASCII"))?;
        let digits = text.strip_prefix("0x").unwrap_or(text);
        if digits.is_empty()
            || digits.len() > 16
            || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(AppError::usage("MIDR is malformed"));
        }
        let value =
            u64::from_str_radix(digits, 16).map_err(|_| AppError::usage("MIDR overflow"))?;
        output.extend_from_slice(format!("midr\t{cpu}\t{value:016x}\n").as_bytes());
    }
    if output.len() > 64 * 1024 {
        return Err(AppError::usage("CPU identity exceeds 64 KiB"));
    }
    Ok(output)
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
fn current_cpu_identity() -> AppResult<Vec<u8>> {
    let model = macos_sysctl_bytes(c"hw.model", 4096)?;
    let cpu_type = macos_sysctl_i32(c"hw.cputype")?;
    let cpu_subtype = macos_sysctl_i32(c"hw.cpusubtype")?;
    let cpu_family = macos_sysctl_u32(c"hw.cpufamily")?;
    Ok(format!("align-startup-cpu-v1\nplatform_arch\tmacos-aarch64\nhw_model_hex\t{}\nhw_cputype\t{}\nhw_cpusubtype\t{}\nhw_cpufamily\t{}\n", encode_hex(&model), cpu_type, cpu_subtype, cpu_family).into_bytes())
}

#[cfg(target_os = "linux")]
fn current_libc_identity() -> AppResult<Vec<u8>> {
    unsafe extern "C" {
        fn gnu_get_libc_version() -> *const libc::c_char;
    }
    // SAFETY: glibc returns a process-lifetime NUL-terminated version string.
    let version = unsafe { CStr::from_ptr(gnu_get_libc_version()) }.to_bytes();
    let mut info = MaybeUninit::<libc::Dl_info>::zeroed();
    // SAFETY: info is writable and the function pointer belongs to the loaded libc image.
    if unsafe {
        libc::dladdr(
            gnu_get_libc_version as *const () as *const libc::c_void,
            info.as_mut_ptr(),
        )
    } == 0
    {
        return Err(AppError::usage("loaded glibc identity unavailable"));
    }
    // SAFETY: successful dladdr initialized the record.
    let info = unsafe { info.assume_init() };
    if info.dli_fname.is_null() {
        return Err(AppError::usage("loaded glibc path unavailable"));
    }
    // SAFETY: dladdr returns a process-lifetime NUL-terminated pathname.
    let raw_path = unsafe { CStr::from_ptr(info.dli_fname) }.to_bytes();
    let path = fs::canonicalize(Path::new(OsStr::from_bytes(raw_path)))
        .map_err(|error| AppError::usage(format!("loaded glibc path: {error}")))?;
    validate_immutable_chain(&path)?;
    let digest = digest_path(&path, FILE_MAX, "loaded glibc")?;
    Ok(format!("align-startup-libc-v1\nplatform\tlinux\nversion_hex\t{}\nimage_path_hex\t{}\nimage_sha256\t{}\nimage_uuid\tunavailable:linux\n", encode_hex(version), encode_hex(path.as_os_str().as_bytes()), encode_hex(&digest)).into_bytes())
}

#[cfg(target_os = "macos")]
fn current_libc_identity() -> AppResult<Vec<u8>> {
    let uuid = macos_libsystem_uuid()?;
    Ok(format!("align-startup-libc-v1\nplatform\tmacos\nversion_hex\t6c696253797374656d\nimage_path_hex\t2f7573722f6c69622f6c696253797374656d2e422e64796c6962\nimage_sha256\tunavailable:macos\nimage_uuid\t{}\n", encode_hex(&uuid)).into_bytes())
}

#[cfg(target_os = "linux")]
fn current_memory_bytes() -> AppResult<u64> {
    // SAFETY: sysconf is read-only process observation.
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    // SAFETY: sysconf is read-only process observation.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if pages <= 0 || page_size <= 0 {
        return Err(AppError::usage("physical memory is unavailable"));
    }
    u64::try_from(pages)
        .ok()
        .and_then(|value| value.checked_mul(u64::try_from(page_size).ok()?))
        .ok_or_else(|| AppError::usage("physical memory size overflow"))
}

#[cfg(target_os = "macos")]
fn current_memory_bytes() -> AppResult<u64> {
    let name = c"hw.memsize";
    let mut value = 0u64;
    let mut length = std::mem::size_of::<u64>();
    // SAFETY: name is fixed and the output storage/length are valid.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u64).cast(),
            &mut length,
            std::ptr::null_mut::<libc::c_void>(),
            0,
        )
    } == -1
        || length != std::mem::size_of::<u64>()
        || value == 0
    {
        return Err(AppError::usage("physical memory is unavailable"));
    }
    Ok(value)
}

fn validate_tools(provenance: &Provenance, repo: &Path) -> AppResult<()> {
    for (label, path, expected) in [
        ("C compiler", &provenance.cc_path, &provenance.cc_sha256),
        ("linker", &provenance.ld_path, &provenance.ld_sha256),
        (
            "object inspector",
            &provenance.inspector_path,
            &provenance.inspector_sha256,
        ),
    ] {
        let canonical =
            fs::canonicalize(path).map_err(|error| AppError::usage(format!("{label}: {error}")))?;
        if canonical != *path {
            return Err(AppError::usage(format!("{label} path is not canonical")));
        }
        validate_immutable_chain(path)?;
        validate_file_digest(label, path, expected, FILE_MAX)?;
        let bytes = read_bounded_nofollow(path, FILE_MAX, label)?;
        validate_native_image(&bytes, &provenance.target_triple, true)?;
    }
    for arm in &provenance.arms {
        validate_file_digest(
            "Align compiler",
            &arm.compiler,
            &arm.compiler_sha256,
            FILE_MAX,
        )?;
        let compiler = read_bounded_nofollow(&arm.compiler, FILE_MAX, "Align compiler")?;
        validate_native_image(&compiler, &provenance.target_triple, true)?;
        validate_file_digest("Align runtime", &arm.runtime, &arm.runtime_sha256, FILE_MAX)?;
        if arm.compiler.parent() != arm.runtime.parent()
            || arm.runtime.file_name() != Some(OsStr::new("libalign_runtime.a"))
        {
            return Err(AppError::usage(
                "arm runtime is not adjacent to its compiler",
            ));
        }
    }
    for path in &provenance.build_path {
        if !path.is_absolute() || !path.is_dir() || path.starts_with(repo) {
            return Err(AppError::usage(
                "build path contains a mutable or non-directory entry",
            ));
        }
        let canonical = fs::canonicalize(path)
            .map_err(|error| AppError::usage(format!("build path: {error}")))?;
        if canonical != *path {
            return Err(AppError::usage("build path entry is not canonical"));
        }
        validate_immutable_chain(path)?;
    }
    let cc = resolve_in_path(b"cc", &provenance.build_path)
        .map_err(|error| AppError::usage(format!("configured cc resolution: {error}")))?;
    if cc != provenance.cc_path {
        return Err(AppError::usage(
            "configured PATH resolves a different first cc",
        ));
    }
    Ok(())
}

fn validate_immutable_chain(path: &Path) -> AppResult<()> {
    // SAFETY: geteuid is a read-only process observation.
    let euid = unsafe { libc::geteuid() };
    let mut current = PathBuf::from("/");
    for component in path.components().skip(1) {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            AppError::usage(format!("immutable path {}: {error}", current.display()))
        })?;
        if metadata.file_type().is_symlink() || metadata.uid() == euid {
            return Err(AppError::usage(format!(
                "path is symlinked or observer-owned: {}",
                current.display()
            )));
        }
        let c_path = cstring_path(&current)?;
        // SAFETY: c_path is NUL-terminated; access performs no mutation.
        let writable = unsafe {
            libc::faccessat(
                libc::AT_FDCWD,
                c_path.as_ptr(),
                libc::W_OK,
                libc::AT_EACCESS | libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if writable == 0 {
            return Err(AppError::usage(format!(
                "path is writable by the observer: {}",
                current.display()
            )));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EACCES) && error.raw_os_error() != Some(libc::EROFS) {
            return Err(AppError::usage(format!(
                "path access could not be proved immutable: {}: {error}",
                current.display()
            )));
        }
    }
    Ok(())
}

fn validation_environment(provenance: &Provenance) -> Vec<OsString> {
    vec![
        OsString::from("ALIGNC_CACHE=off"),
        OsString::from("ALIGNC_LINKER=system"),
        OsString::from("LANG=C"),
        OsString::from("LC_ALL=C"),
        environment_entry(b"PATH", &join_path_bytes(&provenance.build_path)),
        OsString::from("TZ=UTC"),
    ]
}

fn join_path_bytes(paths: &[PathBuf]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if index != 0 {
            bytes.push(b':');
        }
        bytes.extend_from_slice(path.as_os_str().as_bytes());
    }
    bytes
}

fn environment_entry(key: &[u8], value: &[u8]) -> OsString {
    let mut bytes = Vec::with_capacity(key.len() + 1 + value.len());
    bytes.extend_from_slice(key);
    bytes.push(b'=');
    bytes.extend_from_slice(value);
    OsString::from_vec(bytes)
}

fn validate_tool_versions(
    watchdog: &Watchdog,
    observer: &File,
    provenance: &Provenance,
    repo: &Path,
    execution_state: &[u8],
) -> AppResult<()> {
    let environment = validation_environment(provenance);
    for arm in &provenance.arms {
        let result = run_tool(
            watchdog,
            observer,
            &arm.compiler,
            &arm.compiler_sha256,
            &[OsString::from("--version")],
            &environment,
            repo,
            4_000_000_000,
            1_000_000_000,
            64 * 1024,
            false,
            false,
            execution_state,
        )?;
        require_probe(
            &format!("{} compiler version", arm.name),
            &result,
            &arm.compiler_version,
            b"",
        )?;
    }
    let cc_ld = run_tool(
        watchdog,
        observer,
        &provenance.cc_path,
        &provenance.cc_sha256,
        &[OsString::from("-print-prog-name=ld")],
        &environment,
        repo,
        4_000_000_000,
        1_000_000_000,
        64 * 1024,
        true,
        false,
        execution_state,
    )?;
    require_probe("C compiler linker resolution", &cc_ld, &cc_ld.stdout, b"")?;
    let emitted = cc_ld
        .stdout
        .strip_suffix(b"\n")
        .filter(|bytes| !bytes.is_empty() && !bytes.contains(&b'\n') && !bytes.contains(&b'\r'))
        .ok_or_else(|| {
            AppError::usage("C compiler linker resolution is not one LF-terminated token")
        })?;
    let emitted_path = PathBuf::from(OsString::from_vec(emitted.to_vec()));
    let resolved = if emitted_path.is_absolute() {
        fs::canonicalize(&emitted_path)
    } else if !emitted.contains(&b'/') {
        resolve_in_path(emitted, &provenance.build_path)
    } else {
        return Err(AppError::usage(
            "C compiler returned an invalid linker token",
        ));
    }
    .map_err(|error| AppError::usage(format!("C compiler linker resolution: {error}")))?;
    let declared_ld = fs::canonicalize(&provenance.ld_path)
        .map_err(|error| AppError::usage(format!("declared linker: {error}")))?;
    if resolved != declared_ld {
        return Err(AppError::usage("C compiler resolved a different linker"));
    }
    for (label, path, arguments, expected_stdout, expected_stderr) in [
        (
            "C compiler version",
            provenance.cc_path.as_path(),
            vec![OsString::from("--version")],
            provenance.cc_version.as_slice(),
            b"".as_slice(),
        ),
        (
            "object inspector version",
            provenance.inspector_path.as_path(),
            vec![OsString::from("--version")],
            provenance.inspector_version.as_slice(),
            b"".as_slice(),
        ),
    ] {
        let result = run_tool(
            watchdog,
            observer,
            path,
            if label == "C compiler version" {
                &provenance.cc_sha256
            } else {
                &provenance.inspector_sha256
            },
            &arguments,
            &environment,
            repo,
            4_000_000_000,
            1_000_000_000,
            64 * 1024,
            true,
            false,
            execution_state,
        )?;
        require_probe(label, &result, expected_stdout, expected_stderr)?;
    }
    #[cfg(target_os = "linux")]
    let (ld_args, ld_stdout, ld_stderr) = (
        vec![OsString::from("--version")],
        provenance.ld_version.as_slice(),
        b"".as_slice(),
    );
    #[cfg(target_os = "macos")]
    let (ld_args, ld_stdout, ld_stderr) = (
        vec![OsString::from("-v")],
        b"".as_slice(),
        provenance.ld_version.as_slice(),
    );
    let result = run_tool(
        watchdog,
        observer,
        &provenance.ld_path,
        &provenance.ld_sha256,
        &ld_args,
        &environment,
        repo,
        4_000_000_000,
        1_000_000_000,
        64 * 1024,
        true,
        false,
        execution_state,
    )?;
    require_probe("linker version", &result, ld_stdout, ld_stderr)
}

fn validate_launcher_probe(
    watchdog: &Watchdog,
    observer: &File,
    provenance: &Provenance,
    repo: &Path,
    execution_state: &[u8],
) -> AppResult<()> {
    if execution_state_record()? != execution_state {
        return Err(AppError::operational(
            "execution-state changed before launcher self-probe",
        ));
    }
    let argv = vec![
        OsString::from("/dev/fd/3"),
        OsString::from("--exec-tool-fd"),
        OsString::from("4"),
        OsString::from("--argv0"),
        OsString::from("align-startup-observer"),
        OsString::from("--self-probe"),
    ];
    let result = run_child(
        watchdog,
        observer,
        &ChildSpec {
            executable: Path::new("/dev/fd/3"),
            argv: &argv,
            environment: &validation_environment(provenance),
            cwd: repo,
            capture: CaptureKind::Infrastructure { cap: 64 * 1024 },
            execution_ns: 4_000_000_000,
            cleanup_ns: 1_000_000_000,
            tool: Some(ToolLaunch {
                descriptor: observer.as_raw_fd(),
            }),
            timed_capture: None,
        },
    )?;
    require_probe("launcher self-probe", &result, b"", b"")?;
    if execution_state_record()? != execution_state {
        return Err(AppError::operational(
            "execution-state changed after launcher self-probe",
        ));
    }
    Ok(())
}

fn resolve_in_path(name: &[u8], paths: &[PathBuf]) -> io::Result<PathBuf> {
    for directory in paths {
        let candidate = directory.join(OsStr::from_bytes(name));
        if candidate.is_file() {
            return fs::canonicalize(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "tool not found in configured PATH",
    ))
}

fn require_probe(label: &str, result: &ChildResult, stdout: &[u8], stderr: &[u8]) -> AppResult<()> {
    if result.timed_out
        || result.stdout_overflow
        || result.stderr_overflow
        || result.exit != ChildExit::Code(0)
        || result.stdout != stdout
        || result.stderr != stderr
    {
        return Err(AppError::usage(format!("{label} mismatch")));
    }
    Ok(())
}

fn validate_file_digest(
    label: &str,
    path: &Path,
    expected: &[u8; 32],
    limit: u64,
) -> AppResult<()> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|error| AppError::usage(format!("{label}: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| AppError::usage(format!("{label}: {error}")))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(AppError::usage(format!(
            "{label} is not a bounded regular file"
        )));
    }
    let observed = sha256_reader(&mut file, limit)?;
    if &observed != expected {
        return Err(AppError::usage(format!("{label} digest mismatch")));
    }
    Ok(())
}

fn validate_fixtures(repo: &Path) -> AppResult<[u8; 32]> {
    let mut bundle = Sha256::new();
    for fixture in FIXTURES {
        let path = repo
            .join("bench/startup/fixtures")
            .join(format!("{}.align", fixture.id));
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let mut file = options
            .open(&path)
            .map_err(|error| AppError::usage(format!("fixture {}: {error}", fixture.id)))?;
        let metadata = file
            .metadata()
            .map_err(|error| AppError::usage(format!("fixture {}: {error}", fixture.id)))?;
        if !metadata.is_file() || metadata.len() > FIXTURE_MAX {
            return Err(AppError::usage(format!(
                "fixture {} is not a bounded regular file",
                fixture.id
            )));
        }
        let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
        file.read_to_end(&mut bytes)
            .map_err(|error| AppError::usage(format!("fixture {}: {error}", fixture.id)))?;
        if bytes != fixture.bytes {
            return Err(AppError::usage(format!(
                "fixture {} differs from the compiled inventory",
                fixture.id
            )));
        }
        bundle.update(
            u32::try_from(fixture.id.len())
                .map_err(|_| AppError::operational("fixture identifier overflow"))?
                .to_be_bytes(),
        );
        bundle.update(fixture.id.as_bytes());
        bundle.update(
            u64::try_from(bytes.len())
                .map_err(|_| AppError::operational("fixture size overflow"))?
                .to_be_bytes(),
        );
        bundle.update(&bytes);
    }
    Ok(bundle.finalize().into())
}

fn create_owned_topology(work: &Path, provenance: &[u8]) -> AppResult<()> {
    for relative in [
        "build",
        "cache",
        "logs",
        "observer",
        "build/tmp",
        "build/input",
        "build/tool",
        "observer/cwd",
    ] {
        let path = work.join(relative);
        fs::create_dir(&path).map_err(|error| AppError::io("work topology", error))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .map_err(|error| AppError::io("work topology mode", error))?;
    }
    write_exclusive(&work.join("provenance.tsv"), provenance, 0o600)?;
    for fixture in FIXTURES {
        write_exclusive(
            &work
                .join("build/input")
                .join(format!("{}.align", fixture.id)),
            fixture.bytes,
            0o600,
        )?;
    }
    Ok(())
}

fn write_exclusive(path: &Path, bytes: &[u8], mode: u32) -> AppResult<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| AppError::io("exclusive output", error))?;
    set_file_mode(&file, mode, "exclusive output mode")?;
    file.write_all(bytes)
        .map_err(|error| AppError::io("exclusive output write", error))?;
    file.flush()
        .map_err(|error| AppError::io("exclusive output flush", error))?;
    Ok(())
}

fn set_file_mode(file: &File, mode: u32, label: &str) -> AppResult<()> {
    // SAFETY: file retains the descriptor and mode contains only permission bits.
    if unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) } == -1 {
        return Err(AppError::io(label, io::Error::last_os_error()));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CaptureKind {
    Infrastructure { cap: usize },
    Timed,
}

#[derive(Debug)]
struct ChildSpec<'a> {
    executable: &'a Path,
    argv: &'a [OsString],
    environment: &'a [OsString],
    cwd: &'a Path,
    capture: CaptureKind,
    execution_ns: u64,
    cleanup_ns: u64,
    tool: Option<ToolLaunch>,
    timed_capture: Option<&'a TimedCapture<'a>>,
}

#[derive(Debug)]
struct TimedCapture<'a> {
    expected_stdout: &'a [u8],
    expected_stderr: &'a [u8],
    stdout_log: &'a Path,
    stderr_log: &'a Path,
    stdout_relative: &'a [u8],
    stderr_relative: &'a [u8],
}

#[derive(Default)]
struct FormationFiles {
    stdout_read: Option<File>,
    stdout_write: Option<File>,
    stdout_source: Option<File>,
    stderr_read: Option<File>,
    stderr_write: Option<File>,
    stderr_source: Option<File>,
    stdin: Option<File>,
    stdin_source: Option<File>,
    observer_source: Option<File>,
    tool_source: Option<File>,
    ready_read: Option<File>,
    ready_write: Option<File>,
}

impl FormationFiles {
    fn rollback(mut self, failure: AppError) -> AppError {
        let mut first_close = None;
        for (operation, file) in [
            ("close-stdout", self.stdout_read.take()),
            ("close-stdout", self.stdout_write.take()),
            ("close-stdout", self.stdout_source.take()),
            ("close-stderr", self.stderr_read.take()),
            ("close-stderr", self.stderr_write.take()),
            ("close-stderr", self.stderr_source.take()),
            ("devnull", self.stdin.take()),
            ("spawn-actions", self.stdin_source.take()),
            ("spawn-actions", self.observer_source.take()),
            ("spawn-actions", self.tool_source.take()),
            ("spawn-actions", self.ready_read.take()),
            ("spawn-actions", self.ready_write.take()),
        ] {
            if let Some(file) = file
                && let Err(error) = close_owned(file)
            {
                first_close.get_or_insert((operation, error));
            }
        }
        if let Some((operation, error)) = first_close {
            AppError::operational(format!("formation-cleanup-error:{operation}:{error}"))
        } else {
            failure
        }
    }

    fn spawn_rollback(self, spawn_code: libc::c_int, wall: Option<u64>) -> AppError {
        let failure = AppError::operational(format!(
            "spawn-error:{spawn_code}:wall_ns={}",
            optional_u64(wall)
        ));
        let rollback = self.rollback(failure);
        if let Some(rest) = rollback.message.strip_prefix("formation-cleanup-error:") {
            AppError::operational(format!(
                "spawn-cleanup-error:{rest}:wall_ns={}",
                optional_u64(wall)
            ))
        } else {
            rollback
        }
    }
}

#[derive(Debug)]
struct ToolLaunch {
    descriptor: RawFd,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UsageMetrics {
    user_ns: Option<u64>,
    system_ns: Option<u64>,
    minor_faults: Option<u64>,
    major_faults: Option<u64>,
    voluntary_switches: Option<u64>,
    involuntary_switches: Option<u64>,
    peak_rss_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ChildExit {
    Code(u32),
    Signal(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChildResult {
    wall_ns: Option<u64>,
    usage: UsageMetrics,
    exit: ChildExit,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_len: Option<u32>,
    stderr_len: Option<u32>,
    stdout_complete: bool,
    stderr_complete: bool,
    stdout_overflow: bool,
    stderr_overflow: bool,
    timed_out: bool,
    cleanup_issue: Option<LifecycleIssue>,
    stdout_path: Option<Vec<u8>>,
    stderr_path: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LifecycleIssue {
    operation: &'static str,
    os_error: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug)]
struct ActiveGeneration {
    generation: u64,
    pid: libc::pid_t,
    direct_live: bool,
    group_safe: bool,
    execution_deadline: u64,
    terminal_deadline: u64,
    terminal_observed: bool,
    timeout_won: bool,
}

#[derive(Debug)]
struct WatchdogState {
    generation: u64,
    reserved: Option<u64>,
    active: Option<ActiveGeneration>,
    shutdown: bool,
    global_execution_deadline: u64,
    global_terminal_deadline: u64,
}

#[derive(Debug)]
struct Watchdog {
    shared: Arc<(Mutex<WatchdogState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

fn nonpoisoning_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Watchdog {
    fn new() -> AppResult<Self> {
        let start = monotonic_ns().map_err(|error| AppError::io("global clock", error))?;
        let global_execution_deadline = start
            .checked_add(GLOBAL_NS - 5_000_000_000)
            .ok_or_else(|| AppError::operational("global execution deadline overflow"))?;
        let global_terminal_deadline = start
            .checked_add(GLOBAL_NS)
            .ok_or_else(|| AppError::operational("global terminal deadline overflow"))?;
        let shared = Arc::new((
            Mutex::new(WatchdogState {
                generation: 0,
                reserved: None,
                active: None,
                shutdown: false,
                global_execution_deadline,
                global_terminal_deadline,
            }),
            Condvar::new(),
        ));
        let worker = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("align-startup-watchdog".to_owned())
            .spawn(move || watchdog_loop(&worker))
            .map_err(|error| AppError::io("watchdog creation", error))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    fn reserve(&self) -> AppResult<u64> {
        let (mutex, _) = &*self.shared;
        let mut state = nonpoisoning_lock(mutex);
        if state.reserved.is_some() || state.active.is_some() {
            return Err(AppError::operational(
                "watchdog already owns a child generation",
            ));
        }
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| AppError::operational("watchdog generation exhausted"))?;
        state.reserved = Some(state.generation);
        Ok(state.generation)
    }

    fn cancel_reservation(&self, generation: u64) -> AppResult<()> {
        let (mutex, condition) = &*self.shared;
        let mut state = nonpoisoning_lock(mutex);
        if state.reserved != Some(generation) || state.active.is_some() {
            return Err(AppError::operational("watchdog reservation ownership lost"));
        }
        state.reserved = None;
        condition.notify_all();
        Ok(())
    }

    fn global_deadlines(&self) -> (u64, u64) {
        let (mutex, _) = &*self.shared;
        let state = nonpoisoning_lock(mutex);
        (
            state.global_execution_deadline,
            state.global_terminal_deadline,
        )
    }

    fn publish(&self, active: ActiveGeneration) {
        let (mutex, condition) = &*self.shared;
        let mut state = nonpoisoning_lock(mutex);
        if state.reserved != Some(active.generation) || state.active.is_some() {
            drop(state);
            hard_abort(b"align-startup-observer: watchdog publication invariant\n");
        }
        state.reserved = None;
        state.active = Some(active);
        condition.notify_all();
    }

    fn mark_terminal(&self, generation: u64) -> AppResult<bool> {
        let (mutex, condition) = &*self.shared;
        let mut state = nonpoisoning_lock(mutex);
        let active = state
            .active
            .as_mut()
            .filter(|active| active.generation == generation)
            .ok_or_else(|| AppError::operational("watchdog generation ownership lost"))?;
        active.terminal_observed = true;
        let timeout = active.timeout_won;
        condition.notify_all();
        Ok(timeout)
    }

    fn suppress_group(&self, generation: u64) {
        let (mutex, condition) = &*self.shared;
        let mut state = nonpoisoning_lock(mutex);
        if let Some(active) = state
            .active
            .as_mut()
            .filter(|active| active.generation == generation)
        {
            active.group_safe = false;
            condition.notify_all();
        }
    }

    fn mark_reaped(&self, generation: u64, keep_group: bool) -> AppResult<()> {
        let (mutex, condition) = &*self.shared;
        let mut state = nonpoisoning_lock(mutex);
        let active = state
            .active
            .as_mut()
            .filter(|active| active.generation == generation)
            .ok_or_else(|| AppError::operational("watchdog generation ownership lost"))?;
        active.direct_live = false;
        active.group_safe = keep_group;
        condition.notify_all();
        Ok(())
    }

    fn release(&self, generation: u64) -> AppResult<()> {
        let (mutex, condition) = &*self.shared;
        let mut state = nonpoisoning_lock(mutex);
        if state.active.as_ref().map(|active| active.generation) != Some(generation) {
            return Err(AppError::operational("watchdog generation ownership lost"));
        }
        state.active = None;
        condition.notify_all();
        Ok(())
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        let (mutex, condition) = &*self.shared;
        {
            let mut state = nonpoisoning_lock(mutex);
            state.shutdown = true;
            if let Some(active) = state.active.as_mut() {
                kill_owned_child(active);
            }
            condition.notify_all();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn watchdog_loop(shared: &Arc<(Mutex<WatchdogState>, Condvar)>) {
    let (mutex, condition) = &**shared;
    let mut state = nonpoisoning_lock(mutex);
    loop {
        if state.shutdown {
            return;
        }
        let Some(active) = state.active else {
            let now = monotonic_ns_unchecked();
            if now >= state.global_terminal_deadline {
                hard_abort(b"align-startup-observer: global terminal deadline\n");
            }
            let wait_ns = state
                .global_terminal_deadline
                .saturating_sub(now)
                .min(50_000_000);
            state = condition
                .wait_timeout(state, Duration::from_nanos(wait_ns.max(1)))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
            continue;
        };
        let now = monotonic_ns_unchecked();
        if now >= active.terminal_deadline {
            let mut doomed = active;
            kill_owned_child(&mut doomed);
            hard_abort(b"align-startup-observer: terminal child-cleanup deadline\n");
        }
        if now >= active.execution_deadline && !active.terminal_observed {
            if let Some(current) = state.active.as_mut() {
                current.timeout_won = true;
                kill_owned_child(current);
            }
            state = condition
                .wait_timeout(state, Duration::from_millis(1))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
            continue;
        }
        let next = if active.terminal_observed {
            active.terminal_deadline
        } else {
            active.execution_deadline.min(active.terminal_deadline)
        };
        let wait_ns = next.saturating_sub(now).min(50_000_000);
        state = condition
            .wait_timeout(state, Duration::from_nanos(wait_ns.max(1)))
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0;
    }
}

fn hard_abort(message: &'static [u8]) -> ! {
    // SAFETY: static bytes remain readable and `_exit` terminates without
    // invoking allocation or destructors in the deadline-failure path.
    unsafe {
        libc::write(2, message.as_ptr().cast(), message.len());
        libc::_exit(1);
    }
}

fn kill_owned_child(active: &mut ActiveGeneration) {
    if active.group_safe {
        let _ = signal_retry(-active.pid, libc::SIGKILL);
    }
    if active.direct_live {
        let _ = signal_retry(active.pid, libc::SIGKILL);
    }
}

fn signal_retry(target: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
    loop {
        // SAFETY: the caller retains ownership proof for the target.
        if unsafe { libc::kill(target, signal) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(error);
    }
}

struct FileActions(libc::posix_spawn_file_actions_t);

impl FileActions {
    fn new() -> io::Result<Self> {
        let mut value = MaybeUninit::uninit();
        // SAFETY: value is writable storage for the POSIX object.
        let code = unsafe { libc::posix_spawn_file_actions_init(value.as_mut_ptr()) };
        posix_code(code)?;
        // SAFETY: successful init initialized the object.
        Ok(Self(unsafe { value.assume_init() }))
    }

    fn dup2(&mut self, source: RawFd, target: RawFd) -> io::Result<()> {
        // SAFETY: self is initialized and both descriptors are integer slots.
        posix_code(unsafe { libc::posix_spawn_file_actions_adddup2(&mut self.0, source, target) })
    }

    fn chdir(&mut self, path: &CString) -> io::Result<()> {
        // SAFETY: self is initialized and path remains live during formation.
        posix_code(unsafe { spawn_addchdir(&mut self.0, path.as_ptr()) })
    }

    #[cfg(target_os = "linux")]
    fn close_from(&mut self, descriptor: RawFd) -> io::Result<()> {
        // SAFETY: self is initialized.
        posix_code(unsafe {
            libc::posix_spawn_file_actions_addclosefrom_np(&mut self.0, descriptor)
        })
    }
}

impl Drop for FileActions {
    fn drop(&mut self) {
        // SAFETY: self owns one initialized POSIX object.
        let _ = unsafe { libc::posix_spawn_file_actions_destroy(&mut self.0) };
    }
}

#[cfg(target_os = "linux")]
unsafe fn spawn_addchdir(
    actions: *mut libc::posix_spawn_file_actions_t,
    path: *const libc::c_char,
) -> libc::c_int {
    // SAFETY: forwarded contract is upheld by FileActions::chdir.
    unsafe { libc::posix_spawn_file_actions_addchdir_np(actions, path) }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    #[link_name = "posix_spawn_file_actions_addchdir_np"]
    fn macos_spawn_addchdir(
        actions: *mut libc::posix_spawn_file_actions_t,
        path: *const libc::c_char,
    ) -> libc::c_int;
}

#[cfg(target_os = "macos")]
unsafe fn spawn_addchdir(
    actions: *mut libc::posix_spawn_file_actions_t,
    path: *const libc::c_char,
) -> libc::c_int {
    // SAFETY: forwarded contract is upheld by FileActions::chdir.
    unsafe { macos_spawn_addchdir(actions, path) }
}

struct SpawnAttributes(libc::posix_spawnattr_t);

impl SpawnAttributes {
    fn child_group() -> io::Result<Self> {
        let mut value = MaybeUninit::uninit();
        // SAFETY: value is writable storage for the POSIX object.
        posix_code(unsafe { libc::posix_spawnattr_init(value.as_mut_ptr()) })?;
        // SAFETY: successful init initialized the object.
        let mut value = Self(unsafe { value.assume_init() });
        // SAFETY: value is initialized; pgroup zero requests pid-as-pgid.
        posix_code(unsafe { libc::posix_spawnattr_setpgroup(&mut value.0, 0) })?;
        let mut mask = MaybeUninit::<libc::sigset_t>::uninit();
        let mut defaults = MaybeUninit::<libc::sigset_t>::uninit();
        // SAFETY: both records are writable.
        if unsafe { libc::sigemptyset(mask.as_mut_ptr()) } == -1
            || unsafe { libc::sigfillset(defaults.as_mut_ptr()) } == -1
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful initializers made both sets valid.
        let mask = unsafe { mask.assume_init() };
        // SAFETY: successful initializers made both sets valid.
        let mut defaults = unsafe { defaults.assume_init() };
        // POSIX does not permit changing these two dispositions.
        // SAFETY: defaults is initialized.
        unsafe {
            libc::sigdelset(&mut defaults, libc::SIGKILL);
            libc::sigdelset(&mut defaults, libc::SIGSTOP);
        }
        // SAFETY: pointers remain valid for these copying setters.
        posix_code(unsafe { libc::posix_spawnattr_setsigmask(&mut value.0, &mask) })?;
        // SAFETY: pointers remain valid for these copying setters.
        posix_code(unsafe { libc::posix_spawnattr_setsigdefault(&mut value.0, &defaults) })?;
        #[cfg(target_os = "linux")]
        let flags = libc::POSIX_SPAWN_SETPGROUP
            | libc::POSIX_SPAWN_SETSIGMASK
            | libc::POSIX_SPAWN_SETSIGDEF;
        #[cfg(target_os = "macos")]
        let flags = libc::POSIX_SPAWN_SETPGROUP
            | libc::POSIX_SPAWN_SETSIGMASK
            | libc::POSIX_SPAWN_SETSIGDEF
            | libc::POSIX_SPAWN_CLOEXEC_DEFAULT;
        // SAFETY: value is initialized and flags contain admitted platform bits.
        posix_code(unsafe {
            libc::posix_spawnattr_setflags(&mut value.0, flags as libc::c_short)
        })?;
        Ok(value)
    }
}

impl Drop for SpawnAttributes {
    fn drop(&mut self) {
        // SAFETY: self owns one initialized POSIX object.
        let _ = unsafe { libc::posix_spawnattr_destroy(&mut self.0) };
    }
}

fn posix_code(code: libc::c_int) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

fn capture_pipe() -> io::Result<(File, File)> {
    let mut descriptors = [-1; 2];
    // SAFETY: descriptors names writable storage for two fds.
    if unsafe { libc::pipe(descriptors.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful pipe returned two newly owned descriptors.
    let read = unsafe { File::from_raw_fd(descriptors[0]) };
    // SAFETY: successful pipe returned two newly owned descriptors.
    let write = unsafe { File::from_raw_fd(descriptors[1]) };
    set_cloexec(read.as_raw_fd()).and_then(|()| set_cloexec(write.as_raw_fd()))?;
    Ok((read, write))
}

fn readiness_pipe() -> io::Result<(File, File)> {
    capture_pipe()
}

fn set_cloexec(descriptor: RawFd) -> io::Result<()> {
    // SAFETY: descriptor is borrowed for a flag query.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: descriptor remains valid and flags preserve existing bits.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_nonblocking(descriptor: RawFd) -> io::Result<()> {
    // SAFETY: descriptor is borrowed for a flag query.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: descriptor remains valid and flags preserve existing bits.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn duplicate_above(descriptor: RawFd, minimum: RawFd) -> io::Result<File> {
    // SAFETY: descriptor is borrowed and fcntl returns a distinct owned slot.
    let duplicate = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, minimum) };
    if duplicate == -1 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: successful fcntl returned a fresh descriptor.
        Ok(unsafe { File::from_raw_fd(duplicate) })
    }
}

fn close_owned(file: File) -> io::Result<()> {
    let descriptor = file.into_raw_fd();
    // SAFETY: ownership was removed before this exactly-once close.
    if unsafe { libc::close(descriptor) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn monotonic_ns() -> io::Result<u64> {
    let mut time = MaybeUninit::<libc::timespec>::zeroed();
    // SAFETY: time is writable and CLOCK_MONOTONIC is supported by admitted hosts.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, time.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful clock_gettime initialized time.
    let time = unsafe { time.assume_init() };
    if time.tv_sec < 0 || time.tv_nsec < 0 || time.tv_nsec >= 1_000_000_000 {
        return Err(io::Error::other("noncanonical monotonic clock value"));
    }
    u64::try_from(time.tv_sec)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000_000_000))
        .and_then(|base| base.checked_add(time.tv_nsec as u64))
        .ok_or_else(|| io::Error::other("monotonic clock overflow"))
}

fn monotonic_ns_unchecked() -> u64 {
    monotonic_ns().unwrap_or(u64::MAX)
}

#[allow(clippy::too_many_arguments)]
fn run_tool(
    watchdog: &Watchdog,
    observer: &File,
    tool_path: &Path,
    expected_digest: &[u8; 32],
    tool_arguments: &[OsString],
    environment: &[OsString],
    cwd: &Path,
    execution_ns: u64,
    cleanup_ns: u64,
    cap: usize,
    immutable_path: bool,
    pathname_exec: bool,
    expected_execution_state: &[u8],
) -> AppResult<ChildResult> {
    if execution_state_record()? != expected_execution_state {
        return Err(AppError::operational(
            "execution-state changed before infrastructure child",
        ));
    }
    if immutable_path {
        validate_immutable_chain(tool_path)?;
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let tool = options
        .open(tool_path)
        .map_err(|error| AppError::io("tool open", error))?;
    verify_open_regular(tool.as_raw_fd())?;
    let mut tool_hash = tool
        .try_clone()
        .map_err(|error| AppError::io("tool hash", error))?;
    if sha256_reader(&mut tool_hash, FILE_MAX)? != *expected_digest {
        return Err(AppError::operational(
            "retained tool descriptor digest mismatch",
        ));
    }
    let mut argv = vec![OsString::from("/dev/fd/3")];
    if pathname_exec {
        argv.extend([
            OsString::from("--exec-tool-path"),
            tool_path.as_os_str().to_owned(),
            OsString::from("--tool-fd"),
            OsString::from("4"),
        ]);
    } else {
        argv.extend([
            OsString::from("--exec-tool-fd"),
            OsString::from("4"),
            OsString::from("--argv0"),
            tool_path.as_os_str().to_owned(),
        ]);
    }
    argv.extend_from_slice(tool_arguments);
    let result = run_child(
        watchdog,
        observer,
        &ChildSpec {
            executable: Path::new("/dev/fd/3"),
            argv: &argv,
            environment,
            cwd,
            capture: CaptureKind::Infrastructure { cap },
            execution_ns,
            cleanup_ns,
            tool: Some(ToolLaunch {
                descriptor: tool.as_raw_fd(),
            }),
            timed_capture: None,
        },
    )?;
    if immutable_path {
        validate_immutable_chain(tool_path)?;
        validate_file_digest("tool after execution", tool_path, expected_digest, FILE_MAX)?;
    }
    if execution_state_record()? != expected_execution_state {
        return Err(AppError::operational(
            "execution-state changed after infrastructure child",
        ));
    }
    Ok(result)
}

fn run_child(watchdog: &Watchdog, observer: &File, spec: &ChildSpec<'_>) -> AppResult<ChildResult> {
    let mut files = FormationFiles::default();
    let (stdout_read, stdout_write) =
        capture_pipe().map_err(|error| AppError::io("pipe-stdout", error))?;
    files.stdout_read = Some(stdout_read);
    files.stdout_write = Some(stdout_write);
    let (stderr_read, stderr_write) = match capture_pipe() {
        Ok(pipe) => pipe,
        Err(error) => return Err(files.rollback(AppError::io("pipe-stderr", error))),
    };
    files.stderr_read = Some(stderr_read);
    files.stderr_write = Some(stderr_write);
    let stdin = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC)
        .open("/dev/null")
    {
        Ok(stdin) => stdin,
        Err(error) => return Err(files.rollback(AppError::io("devnull", error))),
    };
    files.stdin = Some(stdin);
    let minimum = if spec.tool.is_some() { 6 } else { 3 };
    files.stdin_source = match duplicate_above(
        files.stdin.as_ref().expect("formed stdin").as_raw_fd(),
        minimum,
    ) {
        Ok(source) => Some(source),
        Err(error) => return Err(files.rollback(AppError::io("spawn-actions", error))),
    };
    files.stdout_source = match duplicate_above(
        files
            .stdout_write
            .as_ref()
            .expect("formed stdout")
            .as_raw_fd(),
        minimum,
    ) {
        Ok(source) => Some(source),
        Err(error) => return Err(files.rollback(AppError::io("spawn-actions", error))),
    };
    files.stderr_source = match duplicate_above(
        files
            .stderr_write
            .as_ref()
            .expect("formed stderr")
            .as_raw_fd(),
        minimum,
    ) {
        Ok(source) => Some(source),
        Err(error) => return Err(files.rollback(AppError::io("spawn-actions", error))),
    };
    if let Some(tool) = &spec.tool {
        let (ready_read, ready_write) = match readiness_pipe() {
            Ok(pipe) => pipe,
            Err(error) => return Err(files.rollback(AppError::io("spawn-actions", error))),
        };
        files.ready_read = Some(ready_read);
        files.ready_write = Some(ready_write);
        files.observer_source = match duplicate_above(observer.as_raw_fd(), 6) {
            Ok(source) => Some(source),
            Err(error) => return Err(files.rollback(AppError::io("spawn-actions", error))),
        };
        files.tool_source = match duplicate_above(tool.descriptor, 6) {
            Ok(source) => Some(source),
            Err(error) => return Err(files.rollback(AppError::io("spawn-actions", error))),
        };
    }
    let cwd = match cstring_path(spec.cwd) {
        Ok(cwd) => cwd,
        Err(error) => return Err(files.rollback(error)),
    };
    let executable = match cstring_path(spec.executable) {
        Ok(executable) => executable,
        Err(error) => return Err(files.rollback(error)),
    };
    let argv = match cstring_vector(spec.argv) {
        Ok(argv) => argv,
        Err(error) => return Err(files.rollback(error)),
    };
    let environment = match cstring_vector(spec.environment) {
        Ok(environment) => environment,
        Err(error) => return Err(files.rollback(error)),
    };
    let mut argv_pointers: Vec<*mut libc::c_char> =
        argv.iter().map(|item| item.as_ptr().cast_mut()).collect();
    argv_pointers.push(std::ptr::null_mut());
    let mut environment_pointers: Vec<*mut libc::c_char> = environment
        .iter()
        .map(|item| item.as_ptr().cast_mut())
        .collect();
    environment_pointers.push(std::ptr::null_mut());
    let mut actions = match FileActions::new() {
        Ok(actions) => actions,
        Err(error) => return Err(files.rollback(AppError::io("spawn-actions", error))),
    };
    if let Err(error) = actions.chdir(&cwd) {
        return Err(files.rollback(AppError::io("spawn-actions", error)));
    }
    if let Err(error) = actions.dup2(
        files
            .stdin_source
            .as_ref()
            .expect("formed stdin source")
            .as_raw_fd(),
        0,
    ) {
        return Err(files.rollback(AppError::io("spawn-actions", error)));
    }
    if let Err(error) = actions.dup2(
        files
            .stdout_source
            .as_ref()
            .expect("formed stdout source")
            .as_raw_fd(),
        1,
    ) {
        return Err(files.rollback(AppError::io("spawn-actions", error)));
    }
    if let Err(error) = actions.dup2(
        files
            .stderr_source
            .as_ref()
            .expect("formed stderr source")
            .as_raw_fd(),
        2,
    ) {
        return Err(files.rollback(AppError::io("spawn-actions", error)));
    }
    if let (Some(observer_source), Some(tool_source), Some(ready_read)) = (
        &files.observer_source,
        &files.tool_source,
        &files.ready_read,
    ) {
        if let Err(error) = actions.dup2(observer_source.as_raw_fd(), 3) {
            return Err(files.rollback(AppError::io("spawn-actions", error)));
        }
        if let Err(error) = actions.dup2(tool_source.as_raw_fd(), 4) {
            return Err(files.rollback(AppError::io("spawn-actions", error)));
        }
        if let Err(error) = actions.dup2(ready_read.as_raw_fd(), 5) {
            return Err(files.rollback(AppError::io("spawn-actions", error)));
        }
    }
    #[cfg(target_os = "linux")]
    if let Err(error) = actions.close_from(if spec.tool.is_some() { 6 } else { 3 }) {
        return Err(files.rollback(AppError::io("spawn-actions", error)));
    }
    let attributes = match SpawnAttributes::child_group() {
        Ok(attributes) => attributes,
        Err(error) => return Err(files.rollback(AppError::io("spawn-attributes", error))),
    };
    let generation = match watchdog.reserve() {
        Ok(generation) => generation,
        Err(error) => return Err(files.rollback(error)),
    };
    let start = match monotonic_ns() {
        Ok(start) => start,
        Err(error) => {
            watchdog.cancel_reservation(generation)?;
            return Err(files.rollback(AppError::io("clock-start", error)));
        }
    };
    let (global_execution, global_terminal) = watchdog.global_deadlines();
    if start >= global_execution {
        watchdog.cancel_reservation(generation)?;
        return Err(files.rollback(AppError::operational("global execution cutoff reached")));
    }
    let Some(phase_execution_deadline) = start.checked_add(spec.execution_ns) else {
        watchdog.cancel_reservation(generation)?;
        return Err(files.rollback(AppError::operational("execution deadline overflow")));
    };
    let Some(phase_terminal_deadline) = phase_execution_deadline.checked_add(spec.cleanup_ns)
    else {
        watchdog.cancel_reservation(generation)?;
        return Err(files.rollback(AppError::operational("terminal deadline overflow")));
    };
    let execution_deadline = phase_execution_deadline.min(global_execution);
    let terminal_deadline = phase_terminal_deadline.min(global_terminal);
    let mut pid = 0;
    // SAFETY: POSIX objects and NUL-terminated vectors remain live across the call.
    let spawn_code = unsafe {
        libc::posix_spawn(
            &mut pid,
            executable.as_ptr(),
            &actions.0,
            &attributes.0,
            argv_pointers.as_mut_ptr(),
            environment_pointers.as_mut_ptr(),
        )
    };
    if spawn_code != 0 {
        watchdog.cancel_reservation(generation)?;
        let end = monotonic_ns().ok();
        let wall = end.map(|end| end.saturating_sub(start));
        return Err(files.spawn_rollback(spawn_code, wall));
    }
    // No fallible or allocating operation occurs between successful spawn and
    // publication of the direct pid plus pgroup=0 ownership.
    watchdog.publish(ActiveGeneration {
        generation,
        pid,
        direct_live: true,
        group_safe: true,
        execution_deadline,
        terminal_deadline,
        terminal_observed: false,
        timeout_won: false,
    });
    drop(actions);
    drop(attributes);
    let mut stdout_read = files.stdout_read.take().expect("formed stdout read");
    let stdout_write = files.stdout_write.take().expect("formed stdout write");
    let stdout_source = files.stdout_source.take().expect("formed stdout source");
    let mut stderr_read = files.stderr_read.take().expect("formed stderr read");
    let stderr_write = files.stderr_write.take().expect("formed stderr write");
    let stderr_source = files.stderr_source.take().expect("formed stderr source");
    let stdin = files.stdin.take().expect("formed stdin");
    let stdin_source = files.stdin_source.take().expect("formed stdin source");
    let mut observer_source = files.observer_source.take();
    let mut tool_source = files.tool_source.take();
    let mut readiness = match (files.ready_read.take(), files.ready_write.take()) {
        (Some(read), Some(write)) => Some((read, write)),
        (None, None) => None,
        _ => hard_abort(b"align-startup-observer: partial readiness ownership\n"),
    };
    let infrastructure = matches!(spec.capture, CaptureKind::Infrastructure { .. });
    let mut cleanup_issue = None;
    retain_io_issue(
        &mut cleanup_issue,
        "spawn-actions",
        close_owned(stdin_source),
    );
    retain_io_issue(
        &mut cleanup_issue,
        "close-stdout",
        close_owned(stdout_source),
    );
    retain_io_issue(
        &mut cleanup_issue,
        "close-stderr",
        close_owned(stderr_source),
    );
    retain_io_issue(&mut cleanup_issue, "devnull", close_owned(stdin));
    retain_io_issue(
        &mut cleanup_issue,
        "close-stdout",
        close_owned(stdout_write),
    );
    retain_io_issue(
        &mut cleanup_issue,
        "close-stderr",
        close_owned(stderr_write),
    );
    if let Some(source) = observer_source.take() {
        retain_io_issue(&mut cleanup_issue, "spawn-actions", close_owned(source));
    }
    if let Some(source) = tool_source.take() {
        retain_io_issue(&mut cleanup_issue, "spawn-actions", close_owned(source));
    }
    let ready_write = if let Some((ready_read, ready_write)) = readiness.take() {
        retain_io_issue(&mut cleanup_issue, "spawn-actions", close_owned(ready_read));
        Some(ready_write)
    } else {
        None
    };
    let group_safe = match verify_child_group(pid) {
        Ok(()) => true,
        Err(error) => {
            watchdog.suppress_group(generation);
            retain_first_issue(
                &mut cleanup_issue,
                Some(LifecycleIssue::io("group-check", error)),
            );
            false
        }
    };
    if let Some(ready_write) = ready_write {
        if group_safe && cleanup_issue.is_none() {
            retain_io_issue(&mut cleanup_issue, "group-check", write_ready(ready_write));
        } else {
            retain_io_issue(&mut cleanup_issue, "group-check", close_owned(ready_write));
        }
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let CaptureKind::Infrastructure { cap } = spec.capture {
        retain_io_issue(
            &mut cleanup_issue,
            "read-stdout",
            set_nonblocking(stdout_read.as_raw_fd()),
        );
        retain_io_issue(
            &mut cleanup_issue,
            "read-stderr",
            set_nonblocking(stderr_read.as_raw_fd()),
        );
        if cleanup_issue.is_none()
            && let Err(error) = wait_and_pump(
                pid,
                &mut stdout_read,
                &mut stderr_read,
                &mut stdout,
                &mut stderr,
                cap,
                terminal_deadline,
            )
        {
            retain_first_issue(
                &mut cleanup_issue,
                Some(LifecycleIssue::io("waitid", error)),
            );
        }
    } else if cleanup_issue.is_none()
        && let Err(error) = waitid_terminal(pid)
    {
        retain_first_issue(
            &mut cleanup_issue,
            Some(LifecycleIssue::io("waitid", error)),
        );
    }
    let timed_out = watchdog.mark_terminal(generation).unwrap_or_else(|_| {
        hard_abort(b"align-startup-observer: watchdog terminal transition failed\n")
    });
    if cleanup_issue.is_some() {
        if group_safe {
            retain_io_issue(
                &mut cleanup_issue,
                "group-kill",
                signal_retry(-pid, libc::SIGKILL),
            );
        }
        retain_io_issue(
            &mut cleanup_issue,
            "direct-kill",
            signal_retry(pid, libc::SIGKILL),
        );
    } else if infrastructure {
        retain_io_issue(
            &mut cleanup_issue,
            "group-kill",
            signal_retry(-pid, libc::SIGKILL),
        );
    }
    let (status, usage) = wait4_reap(pid)
        .unwrap_or_else(|_| hard_abort(b"align-startup-observer: direct-child reap failed\n"));
    watchdog
        .mark_reaped(generation, infrastructure && group_safe)
        .unwrap_or_else(|_| {
            hard_abort(b"align-startup-observer: watchdog reap transition failed\n")
        });
    let end = match monotonic_ns() {
        Ok(end) => Some(end),
        Err(error) => {
            retain_first_issue(
                &mut cleanup_issue,
                Some(LifecycleIssue::io("clock-end", error)),
            );
            None
        }
    };
    let (
        stdout_len,
        stderr_len,
        stdout_complete,
        stderr_complete,
        stdout_overflow,
        stderr_overflow,
    );
    let mut stdout_path = None;
    let mut stderr_path = None;
    if infrastructure {
        if group_safe && let Err(error) = confirm_group_absence(pid, terminal_deadline) {
            retain_first_issue(
                &mut cleanup_issue,
                Some(LifecycleIssue {
                    operation: "group-absence",
                    os_error: Some(error.message.into_bytes()),
                }),
            );
        }
        watchdog.suppress_group(generation);
        let stdout_drain_complete =
            match drain_to_eof(&mut stdout_read, &mut stdout, capture_cap(spec.capture)) {
                Ok(()) => true,
                Err(error) => {
                    retain_first_issue(
                        &mut cleanup_issue,
                        Some(LifecycleIssue {
                            operation: "read-stdout",
                            os_error: Some(error.message.into_bytes()),
                        }),
                    );
                    false
                }
            };
        let stderr_drain_complete =
            match drain_to_eof(&mut stderr_read, &mut stderr, capture_cap(spec.capture)) {
                Ok(()) => true,
                Err(error) => {
                    retain_first_issue(
                        &mut cleanup_issue,
                        Some(LifecycleIssue {
                            operation: "read-stderr",
                            os_error: Some(error.message.into_bytes()),
                        }),
                    );
                    false
                }
            };
        stdout_len = u32::try_from(stdout.len()).ok();
        stderr_len = u32::try_from(stderr.len()).ok();
        stdout_complete = stdout_drain_complete && stdout_len.is_some();
        stderr_complete = stderr_drain_complete && stderr_len.is_some();
        stdout_overflow = stdout.len() > capture_cap(spec.capture);
        stderr_overflow = stderr.len() > capture_cap(spec.capture);
    } else {
        let stdout_snapshot = snapshot_length(&stdout_read, "stdout");
        let stderr_snapshot = snapshot_length(&stderr_read, "stderr");
        let stdout_capture = read_snapshot_quota(&mut stdout_read, stdout_snapshot, "stdout");
        let stderr_capture = read_snapshot_quota(&mut stderr_read, stderr_snapshot, "stderr");
        stdout = stdout_capture.bytes;
        stdout_len = stdout_capture.length;
        stdout_complete = stdout_capture.complete;
        stdout_overflow = stdout_capture.overflow;
        retain_first_issue(&mut cleanup_issue, stdout_capture.issue);
        stderr = stderr_capture.bytes;
        stderr_len = stderr_capture.length;
        stderr_complete = stderr_capture.complete;
        stderr_overflow = stderr_capture.overflow;
        retain_first_issue(&mut cleanup_issue, stderr_capture.issue);
        if let Some(capture) = spec.timed_capture {
            stdout_path = persist_stream_capture(
                &stdout,
                stdout_complete,
                stdout_overflow,
                capture.expected_stdout,
                capture.stdout_log,
                capture.stdout_relative,
                "persist-stdout",
                &mut cleanup_issue,
            );
            stderr_path = persist_stream_capture(
                &stderr,
                stderr_complete,
                stderr_overflow,
                capture.expected_stderr,
                capture.stderr_log,
                capture.stderr_relative,
                "persist-stderr",
                &mut cleanup_issue,
            );
        }
    }
    retain_io_issue(&mut cleanup_issue, "close-stdout", close_owned(stdout_read));
    retain_io_issue(&mut cleanup_issue, "close-stderr", close_owned(stderr_read));
    watchdog
        .release(generation)
        .unwrap_or_else(|_| hard_abort(b"align-startup-observer: watchdog release failed\n"));
    if infrastructure && let Some(issue) = cleanup_issue {
        return Err(issue.into_app_error());
    }
    Ok(ChildResult {
        wall_ns: end.map(|end| end.saturating_sub(start)),
        usage: usage_metrics(&usage),
        exit: decode_wait_status(status).unwrap_or_else(|_| {
            hard_abort(b"align-startup-observer: wait4 returned nonterminal status\n")
        }),
        stdout,
        stderr,
        stdout_len,
        stderr_len,
        stdout_complete,
        stderr_complete,
        stdout_overflow,
        stderr_overflow,
        timed_out,
        cleanup_issue,
        stdout_path,
        stderr_path,
    })
}

fn capture_cap(kind: CaptureKind) -> usize {
    match kind {
        CaptureKind::Infrastructure { cap } => cap,
        CaptureKind::Timed => 4096,
    }
}

fn verify_child_group(pid: libc::pid_t) -> io::Result<()> {
    loop {
        // SAFETY: pid is an unreaped direct child.
        let group = unsafe { libc::getpgid(pid) };
        if group == pid {
            return Ok(());
        }
        if group >= 0 {
            return Err(io::Error::other("child process-group mismatch"));
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if error.raw_os_error() == Some(libc::ESRCH) && child_terminal_without_reap(pid)? {
            return Ok(());
        }
        return Err(error);
    }
}

fn child_terminal_without_reap(pid: libc::pid_t) -> io::Result<bool> {
    let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
    loop {
        // SAFETY: info is writable and WNOWAIT retains child ownership.
        if unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } == 0
        {
            // SAFETY: successful waitid initialized info.
            let info = unsafe { info.assume_init() };
            // SAFETY: successful SIGCHLD info exposes si_pid.
            return Ok(unsafe { info.si_pid() } != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn waitid_terminal(pid: libc::pid_t) -> io::Result<()> {
    let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
    loop {
        // SAFETY: info is writable and WNOWAIT retains child ownership.
        if unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOWAIT,
            )
        } == 0
        {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn wait_and_pump(
    pid: libc::pid_t,
    stdout: &mut File,
    stderr: &mut File,
    stdout_bytes: &mut Vec<u8>,
    stderr_bytes: &mut Vec<u8>,
    cap: usize,
    terminal_deadline: u64,
) -> io::Result<()> {
    loop {
        pump_nonblocking(stdout, stdout_bytes, cap)?;
        pump_nonblocking(stderr, stderr_bytes, cap)?;
        if stdout_bytes.len() > cap || stderr_bytes.len() > cap {
            let _ = signal_retry(-pid, libc::SIGKILL);
            let _ = signal_retry(pid, libc::SIGKILL);
        }
        if child_terminal_without_reap(pid)? {
            return Ok(());
        }
        if monotonic_ns()? >= terminal_deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "child terminal deadline",
            ));
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn pump_nonblocking(file: &mut File, bytes: &mut Vec<u8>, cap: usize) -> io::Result<()> {
    let mut buffer = [0u8; 4096];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                let retain = count.min(cap.saturating_add(1).saturating_sub(bytes.len()));
                bytes.extend_from_slice(&buffer[..retain]);
                if bytes.len() > cap {
                    return Ok(());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}

fn drain_to_eof(file: &mut File, bytes: &mut Vec<u8>, cap: usize) -> AppResult<()> {
    loop {
        let before = bytes.len();
        pump_nonblocking(file, bytes, cap)
            .map_err(|error| AppError::io("infrastructure capture", error))?;
        if bytes.len() == before {
            return Ok(());
        }
    }
}

#[derive(Debug)]
struct StreamCapture {
    bytes: Vec<u8>,
    length: Option<u32>,
    complete: bool,
    overflow: bool,
    issue: Option<LifecycleIssue>,
}

impl LifecycleIssue {
    fn io(operation: &'static str, error: io::Error) -> Self {
        Self {
            operation,
            os_error: Some(error.to_string().into_bytes()),
        }
    }

    fn into_app_error(self) -> AppError {
        match self.os_error {
            Some(error) => AppError::operational(format!(
                "{}: {}",
                self.operation,
                String::from_utf8_lossy(&error)
            )),
            None => AppError::operational(self.operation),
        }
    }
}

fn retain_first_issue(slot: &mut Option<LifecycleIssue>, issue: Option<LifecycleIssue>) {
    if let Some(issue) = issue
        && slot.as_ref().is_none_or(|current| {
            lifecycle_issue_rank(issue.operation) < lifecycle_issue_rank(current.operation)
        })
    {
        *slot = Some(issue);
    }
}

fn retain_io_issue(
    slot: &mut Option<LifecycleIssue>,
    operation: &'static str,
    result: io::Result<()>,
) {
    if let Err(error) = result {
        retain_first_issue(slot, Some(LifecycleIssue::io(operation, error)));
    }
}

#[allow(clippy::too_many_arguments)]
fn persist_stream_capture(
    bytes: &[u8],
    complete: bool,
    overflow: bool,
    expected: &[u8],
    path: &Path,
    relative: &[u8],
    operation: &'static str,
    cleanup_issue: &mut Option<LifecycleIssue>,
) -> Option<Vec<u8>> {
    if bytes.is_empty() || (complete && !overflow && bytes == expected) {
        return None;
    }
    match write_exclusive(path, bytes, 0o600) {
        Ok(()) => Some(relative.to_vec()),
        Err(error) => {
            retain_first_issue(
                cleanup_issue,
                Some(LifecycleIssue {
                    operation,
                    os_error: Some(error.message.into_bytes()),
                }),
            );
            None
        }
    }
}

fn lifecycle_issue_rank(operation: &str) -> usize {
    match operation {
        "execution-state" => 0,
        "cwd-check" => 1,
        "clock-start" => 2,
        "pipe-stdout" => 3,
        "pipe-stderr" => 4,
        "devnull" => 5,
        "spawn-actions" => 6,
        "spawn-attributes" => 7,
        "spawn" => 8,
        "group-check" => 9,
        "group-kill" => 10,
        "direct-kill" => 11,
        "waitid" => 12,
        "group-absence" => 13,
        "clock-end" => 14,
        "fionread-stdout" => 15,
        "fionread-stderr" => 16,
        "read-stdout" => 17,
        "read-stderr" => 18,
        "persist-stdout" => 19,
        "persist-stderr" => 20,
        "close-stdout" => 21,
        "close-stderr" => 22,
        _ => usize::MAX,
    }
}

fn snapshot_length(file: &File, stream: &'static str) -> Result<u32, LifecycleIssue> {
    let mut queued: libc::c_int = 0;
    loop {
        // SAFETY: queued is writable and file retains the descriptor.
        if unsafe { libc::ioctl(file.as_raw_fd(), libc::FIONREAD, &mut queued) } == 0 {
            break;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(LifecycleIssue::io(
                if stream == "stdout" {
                    "fionread-stdout"
                } else {
                    "fionread-stderr"
                },
                error,
            ));
        }
    }
    if queued < 0 {
        return Err(LifecycleIssue {
            operation: if stream == "stdout" {
                "fionread-stdout"
            } else {
                "fionread-stderr"
            },
            os_error: None,
        });
    }
    let length = usize::try_from(queued).map_err(|_| LifecycleIssue {
        operation: if stream == "stdout" {
            "fionread-stdout"
        } else {
            "fionread-stderr"
        },
        os_error: None,
    })?;
    u32::try_from(length).map_err(|_| LifecycleIssue {
        operation: if stream == "stdout" {
            "fionread-stdout"
        } else {
            "fionread-stderr"
        },
        os_error: None,
    })
}

fn read_snapshot_quota(
    file: &mut File,
    snapshot: Result<u32, LifecycleIssue>,
    stream: &'static str,
) -> StreamCapture {
    let length_u32 = match snapshot {
        Ok(length) => length,
        Err(issue) => {
            return StreamCapture {
                bytes: Vec::new(),
                length: None,
                complete: false,
                overflow: false,
                issue: Some(issue),
            };
        }
    };
    let length = length_u32 as usize;
    let mut output = Vec::with_capacity(length.min(4096));
    let mut buffer = [0u8; 4096];
    let mut offset = 0;
    while offset < length {
        let requested = (length - offset).min(buffer.len());
        match file.read(&mut buffer[..requested]) {
            Ok(0) => {
                return StreamCapture {
                    bytes: output,
                    length: Some(length_u32),
                    complete: false,
                    overflow: length > 4096,
                    issue: Some(LifecycleIssue {
                        operation: if stream == "stdout" {
                            "read-stdout"
                        } else {
                            "read-stderr"
                        },
                        os_error: None,
                    }),
                };
            }
            Ok(count) => {
                let retain = count.min(4096usize.saturating_sub(output.len()));
                output.extend_from_slice(&buffer[..retain]);
                offset += count;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return StreamCapture {
                    bytes: output,
                    length: Some(length_u32),
                    complete: false,
                    overflow: length > 4096,
                    issue: Some(LifecycleIssue::io(
                        if stream == "stdout" {
                            "read-stdout"
                        } else {
                            "read-stderr"
                        },
                        error,
                    )),
                };
            }
        }
    }
    StreamCapture {
        bytes: output,
        length: Some(length_u32),
        complete: true,
        overflow: length > 4096,
        issue: None,
    }
}

fn write_ready(file: File) -> io::Result<()> {
    let descriptor = file.into_raw_fd();
    let byte = b'G';
    loop {
        // SAFETY: descriptor is owned and byte is readable.
        if unsafe { libc::write(descriptor, (&byte as *const u8).cast(), 1) } == 1 {
            // SAFETY: ownership was removed before this exactly-once close.
            return if unsafe { libc::close(descriptor) } == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            };
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            // SAFETY: best-effort exactly-once close after ownership extraction.
            let _ = unsafe { libc::close(descriptor) };
            return Err(error);
        }
    }
}

fn wait4_reap(pid: libc::pid_t) -> io::Result<(libc::c_int, libc::rusage)> {
    let mut status = 0;
    let mut usage = MaybeUninit::<libc::rusage>::zeroed();
    loop {
        // SAFETY: pid is retained, status/usage are writable, and success transfers reap ownership.
        let result = unsafe { libc::wait4(pid, &mut status, 0, usage.as_mut_ptr()) };
        if result == pid {
            // SAFETY: successful wait4 initialized usage.
            return Ok((status, unsafe { usage.assume_init() }));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn confirm_group_absence(pid: libc::pid_t, deadline: u64) -> AppResult<()> {
    loop {
        // SAFETY: signal zero probes the previously pinned group without delivery.
        if unsafe { libc::kill(-pid, 0) } == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(AppError::io("group-absence", error));
        }
        if monotonic_ns().map_err(|error| AppError::io("group-absence", error))? >= deadline {
            return Err(AppError::operational("group-absence: terminal deadline"));
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn decode_wait_status(status: libc::c_int) -> AppResult<ChildExit> {
    if libc::WIFEXITED(status) {
        Ok(ChildExit::Code(libc::WEXITSTATUS(status) as u32))
    } else if libc::WIFSIGNALED(status) {
        Ok(ChildExit::Signal(libc::WTERMSIG(status) as u32))
    } else {
        Err(AppError::operational("wait4 returned a nonterminal status"))
    }
}

fn usage_metrics(usage: &libc::rusage) -> UsageMetrics {
    UsageMetrics {
        user_ns: timeval_ns(usage.ru_utime),
        system_ns: timeval_ns(usage.ru_stime),
        minor_faults: nonnegative_i64(usage.ru_minflt),
        major_faults: nonnegative_i64(usage.ru_majflt),
        voluntary_switches: nonnegative_i64(usage.ru_nvcsw),
        involuntary_switches: nonnegative_i64(usage.ru_nivcsw),
        peak_rss_bytes: nonnegative_i64(usage.ru_maxrss).and_then(|value| {
            if cfg!(target_os = "linux") {
                value.checked_mul(1024)
            } else {
                Some(value)
            }
        }),
    }
}

fn timeval_ns(time: libc::timeval) -> Option<u64> {
    if time.tv_sec < 0 || time.tv_usec < 0 || time.tv_usec >= 1_000_000 {
        return None;
    }
    u64::try_from(time.tv_sec)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(time.tv_usec).ok()?.checked_mul(1000)?)
}

fn nonnegative_i64<T>(value: T) -> Option<u64>
where
    i64: From<T>,
{
    u64::try_from(i64::from(value)).ok()
}

#[derive(Debug)]
struct SealedArm {
    arm: Arm,
    compiler: PathBuf,
    runtime: PathBuf,
    compiler_bytes: Vec<u8>,
}

#[derive(Debug)]
struct Artifact {
    fixture: Fixture,
    arm_index: usize,
    executable: PathBuf,
    executable_file: File,
    executable_sha256: [u8; 32],
    executable_size: u64,
    key: CodegenKey,
    inspection: Vec<u8>,
    inspection_sha256: [u8; 32],
    dependencies: Vec<Vec<u8>>,
    loader_closure: Vec<u8>,
    build_argv: Vec<OsString>,
    build_environment: Vec<OsString>,
}

#[allow(clippy::too_many_arguments)]
fn produce_evidence(
    args: &PublicArgs,
    provenance: &Provenance,
    work: &Path,
    work_filesystem: &[u8],
    execution_state: &[u8],
    fixture_bundle: [u8; 32],
    observer: &File,
    watchdog: &Watchdog,
) -> AppResult<()> {
    let sealed_arms = seal_arm_tools(provenance, work)?;
    let mut artifacts = Vec::with_capacity(FIXTURES.len() * sealed_arms.len());
    let mut llvm_identity = None;
    let mut llvm_version: Option<String> = None;
    let mut rt_lto_by_arm = vec![None; sealed_arms.len()];
    for fixture in FIXTURES {
        for (arm_index, arm) in sealed_arms.iter().enumerate() {
            let artifact = build_artifact(
                watchdog,
                observer,
                provenance,
                work,
                fixture,
                arm_index,
                arm,
                execution_state,
            )?;
            require_same(
                &mut llvm_identity,
                artifact.key.llvm_build_id,
                "LLVM build identity",
            )?;
            require_same(
                &mut llvm_version,
                artifact.key.llvm_version.clone(),
                "LLVM version",
            )?;
            require_same(
                &mut rt_lto_by_arm[arm_index],
                artifact.key.rt_lto_digest.expect("validated"),
                "runtime LTO identity",
            )?;
            artifacts.push(artifact);
        }
    }
    revalidate_all_artifacts(
        watchdog,
        observer,
        provenance,
        work,
        execution_state,
        &sealed_arms,
        &artifacts,
    )?;
    let mut results = ResultWriter::create(&work.join("results.tsv"))?;
    results.row(&schema_row())?;
    results.row(&run_row(
        &provenance.digest,
        &fixture_bundle,
        &args.observer_sha256,
        provenance.arms.len(),
        work_filesystem,
        execution_state,
        work,
    )?)?;
    for artifact in &artifacts {
        results.row(&artifact_row(artifact, provenance, &sealed_arms)?)?;
    }
    measure_all(
        watchdog,
        observer,
        provenance,
        work,
        execution_state,
        &artifacts,
        &mut results,
    )
}

fn require_same<T: Eq + Clone>(slot: &mut Option<T>, value: T, label: &str) -> AppResult<()> {
    match slot {
        Some(existing) if existing != &value => {
            Err(AppError::operational(format!("{label} mismatch")))
        }
        Some(_) => Ok(()),
        None => {
            *slot = Some(value);
            Ok(())
        }
    }
}

fn seal_arm_tools(provenance: &Provenance, work: &Path) -> AppResult<Vec<SealedArm>> {
    let mut output = Vec::with_capacity(provenance.arms.len());
    for arm in &provenance.arms {
        let directory = work.join("build/tool").join(arm.name);
        create_owned_directory(&directory)?;
        let compiler = directory.join("alignc");
        let runtime = directory.join("libalign_runtime.a");
        let compiler_bytes = read_bounded_nofollow(&arm.compiler, FILE_MAX, "Align compiler")?;
        let runtime_bytes = read_bounded_nofollow(&arm.runtime, FILE_MAX, "Align runtime")?;
        if Sha256::digest(&compiler_bytes).as_slice() != arm.compiler_sha256
            || Sha256::digest(&runtime_bytes).as_slice() != arm.runtime_sha256
        {
            return Err(AppError::usage(format!(
                "{} arm changed during sealing",
                arm.name
            )));
        }
        validate_native_image(&compiler_bytes, &provenance.target_triple, true)?;
        write_exclusive(&compiler, &compiler_bytes, 0o700)?;
        write_exclusive(&runtime, &runtime_bytes, 0o600)?;
        validate_file_digest("sealed compiler", &compiler, &arm.compiler_sha256, FILE_MAX)?;
        validate_file_digest("sealed runtime", &runtime, &arm.runtime_sha256, FILE_MAX)?;
        output.push(SealedArm {
            arm: arm.clone(),
            compiler,
            runtime,
            compiler_bytes,
        });
    }
    Ok(output)
}

fn create_owned_directory(path: &Path) -> AppResult<()> {
    fs::create_dir(path).map_err(|error| AppError::io("owned directory", error))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| AppError::io("owned directory mode", error))
}

fn read_bounded_nofollow(path: &Path, limit: u64, label: &str) -> AppResult<Vec<u8>> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let mut file = options
        .open(path)
        .map_err(|error| AppError::io(label, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| AppError::io(label, error))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(AppError::operational(format!(
            "{label} is not a bounded regular file"
        )));
    }
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| AppError::operational(format!("{label} size overflow")))?;
    let mut bytes = Vec::with_capacity(capacity);
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io(label, error))?;
    let after = file
        .metadata()
        .map_err(|error| AppError::io(label, error))?;
    if bytes.len() as u64 != metadata.len()
        || bytes.len() as u64 > limit
        || metadata.dev() != after.dev()
        || metadata.ino() != after.ino()
        || metadata.mode() != after.mode()
        || metadata.len() != after.len()
    {
        return Err(AppError::operational(format!(
            "{label} changed size while read"
        )));
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn read_bounded_virtual_nofollow(path: &Path, limit: usize, label: &str) -> AppResult<Vec<u8>> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options
        .open(path)
        .map_err(|error| AppError::io(label, error))?;
    if !file
        .metadata()
        .map_err(|error| AppError::io(label, error))?
        .is_file()
    {
        return Err(AppError::operational(format!(
            "{label} is not a regular kernel file"
        )));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io(label, error))?;
    if bytes.len() > limit {
        return Err(AppError::operational(format!("{label} exceeds its bound")));
    }
    Ok(bytes)
}

fn validate_native_image(bytes: &[u8], target: &[u8], executable: bool) -> AppResult<()> {
    if target.ends_with(b"linux-gnu") {
        if bytes.len() < 20 || &bytes[..4] != b"\x7fELF" || bytes[4] != 2 || bytes[5] != 1 {
            return Err(AppError::operational(
                "file is not native 64-bit little-endian ELF",
            ));
        }
        let kind = u16::from_le_bytes([bytes[16], bytes[17]]);
        let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
        let expected_machine = if target == b"x86_64-pc-linux-gnu" {
            62
        } else {
            183
        };
        if machine != expected_machine || (executable && kind != 2 && kind != 3) {
            return Err(AppError::operational("ELF target or image kind mismatch"));
        }
    } else {
        if bytes.len() < 16 || &bytes[..4] != b"\xcf\xfa\xed\xfe" {
            return Err(AppError::operational(
                "file is not thin little-endian 64-bit Mach-O",
            ));
        }
        let cpu = u32::from_le_bytes(bytes[4..8].try_into().expect("four-byte slice"));
        let subtype = u32::from_le_bytes(bytes[8..12].try_into().expect("four-byte slice"));
        let kind = u32::from_le_bytes(bytes[12..16].try_into().expect("four-byte slice"));
        if cpu != 0x0100_000c || subtype != 0 || (executable && kind != 2) {
            return Err(AppError::operational(
                "Mach-O target or image kind mismatch",
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_artifact(
    watchdog: &Watchdog,
    observer: &File,
    provenance: &Provenance,
    work: &Path,
    fixture: Fixture,
    arm_index: usize,
    arm: &SealedArm,
    execution_state: &[u8],
) -> AppResult<Artifact> {
    let cache = work.join("cache").join(fixture.id).join(arm.arm.name);
    let fixture_cache_parent = cache.parent().expect("fixed cache topology");
    if !fixture_cache_parent.exists() {
        create_owned_directory(fixture_cache_parent)?;
    }
    create_owned_directory(&cache)?;
    let cache_root = open_directory_nofollow(&cache, "cache root")?;
    let cache_identity = directory_identity(cache_root.as_raw_fd(), "cache root")?;
    if cache_identity.mode & 0o7777 != 0o700 {
        return Err(AppError::operational("cache root mode is not 0700"));
    }
    require_exact_directories(cache_root.as_raw_fd(), &[])?;
    let build_parent = work.join("build").join(fixture.id);
    if !build_parent.exists() {
        create_owned_directory(&build_parent)?;
    }
    let build = build_parent.join(arm.arm.name);
    create_owned_directory(&build)?;
    let log_parent = work.join("logs").join(fixture.id);
    if !log_parent.exists() {
        create_owned_directory(&log_parent)?;
    }
    let arm_log = log_parent.join(arm.arm.name);
    create_owned_directory(&arm_log)?;
    let source = work
        .join("build/input")
        .join(format!("{}.align", fixture.id));
    let build_argv = vec![
        arm.compiler.as_os_str().to_owned(),
        OsString::from("build"),
        source.as_os_str().to_owned(),
        OsString::from("--profile"),
        OsString::from("release"),
        OsString::from("--target-cpu"),
        OsString::from("baseline"),
        OsString::from("--rt-lto"),
    ];
    let build_environment = vec![
        environment_entry(b"ALIGNC_CACHE", cache.as_os_str().as_bytes()),
        OsString::from("ALIGNC_LINKER=system"),
        OsString::from("LANG=C"),
        OsString::from("LC_ALL=C"),
        environment_entry(b"PATH", &join_path_bytes(&provenance.build_path)),
        environment_entry(b"TMPDIR", work.join("build/tmp").as_os_str().as_bytes()),
        OsString::from("TZ=UTC"),
    ];
    let result = run_tool(
        watchdog,
        observer,
        &arm.compiler,
        &arm.arm.compiler_sha256,
        &build_argv[1..],
        &build_environment,
        &build,
        55_000_000_000,
        5_000_000_000,
        1024 * 1024,
        false,
        cfg!(target_os = "macos"),
        execution_state,
    )?;
    persist_phase_logs(work, fixture.id, arm.arm.name, "build", &result)?;
    if result.timed_out
        || result.stdout_overflow
        || result.stderr_overflow
        || result.exit != ChildExit::Code(0)
    {
        return Err(AppError::operational(format!(
            "build failed for {}/{}",
            fixture.id, arm.arm.name
        )));
    }
    require_empty_directory(&work.join("build/tmp"), "build temporary directory")?;
    let executable = build.join(fixture.id);
    let mut executable_file = open_bounded_regular(&executable, FILE_MAX, "artifact executable")?;
    let executable_size = executable_file
        .metadata()
        .map_err(|error| AppError::io("artifact metadata", error))?
        .len();
    let mut header = [0u8; 20];
    executable_file
        .read_exact(&mut header)
        .map_err(|error| AppError::io("artifact header", error))?;
    validate_native_image(&header, &provenance.target_triple, true)?;
    let executable_sha256 = sha256_reader(&mut executable_file, FILE_MAX)?;
    let key = validate_cache(&cache_root, provenance, &arm.arm, &arm.compiler_bytes)?;
    if directory_identity(cache_root.as_raw_fd(), "cache root")? != cache_identity {
        return Err(AppError::operational(
            "cache root identity changed across build",
        ));
    }
    let inspector_arguments = vec![
        OsString::from("--sections"),
        OsString::from("--needed-libs"),
        OsString::from(format!("./{}", fixture.id)),
    ];
    let child_environment = timed_environment();
    let inspection_result = run_tool(
        watchdog,
        observer,
        &provenance.inspector_path,
        &provenance.inspector_sha256,
        &inspector_arguments,
        &child_environment,
        &build,
        8_000_000_000,
        2_000_000_000,
        8 * 1024 * 1024,
        true,
        false,
        execution_state,
    )?;
    persist_phase_logs(
        work,
        fixture.id,
        arm.arm.name,
        "inspection",
        &inspection_result,
    )?;
    if inspection_result.timed_out
        || inspection_result.stdout_overflow
        || inspection_result.stderr_overflow
        || inspection_result.exit != ChildExit::Code(0)
        || !inspection_result.stderr.is_empty()
    {
        return Err(AppError::operational(format!(
            "inspection failed for {}/{}",
            fixture.id, arm.arm.name
        )));
    }
    let dependencies = parse_needed_libraries(&inspection_result.stdout)?;
    let inspection_sha256 = Sha256::digest(&inspection_result.stdout).into();
    let loader_closure = loader_closure(
        watchdog,
        observer,
        &executable,
        &mut executable_file,
        fixture.id,
        arm.arm.name,
        work,
        &child_environment,
        execution_state,
        true,
        &dependencies,
    )?;
    require_only_named_file(&build, fixture.id)?;
    Ok(Artifact {
        fixture,
        arm_index,
        executable,
        executable_file,
        executable_sha256,
        executable_size,
        key,
        inspection: inspection_result.stdout,
        inspection_sha256,
        dependencies,
        loader_closure,
        build_argv,
        build_environment,
    })
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn loader_closure(
    watchdog: &Watchdog,
    observer: &File,
    executable: &Path,
    executable_file: &mut File,
    fixture: &str,
    arm: &str,
    work: &Path,
    environment: &[OsString],
    execution_state: &[u8],
    persist_logs: bool,
    expected_dependencies: &[Vec<u8>],
) -> AppResult<Vec<u8>> {
    if fs::symlink_metadata("/etc/ld.so.preload").is_ok() {
        return Err(AppError::operational("/etc/ld.so.preload is present"));
    }
    let interpreter = elf_interpreter(executable_file)?;
    if !interpreter.is_absolute() {
        return Err(AppError::operational("ELF interpreter is not absolute"));
    }
    let interpreter =
        fs::canonicalize(interpreter).map_err(|error| AppError::io("ELF interpreter", error))?;
    validate_immutable_chain(&interpreter)?;
    let loader_digest = digest_path(&interpreter, FILE_MAX, "ELF interpreter")?;
    let expected_operand = format!("./{fixture}");
    let result = run_tool(
        watchdog,
        observer,
        &interpreter,
        &loader_digest,
        &[OsString::from("--list"), OsString::from(expected_operand)],
        environment,
        executable.parent().expect("artifact has parent"),
        8_000_000_000,
        2_000_000_000,
        1024 * 1024,
        true,
        false,
        execution_state,
    )?;
    if persist_logs {
        let log_dir = work.join("logs").join(fixture).join(arm);
        write_exclusive(&log_dir.join("loader.stdout"), &result.stdout, 0o600)?;
        write_exclusive(&log_dir.join("loader.stderr"), &result.stderr, 0o600)?;
    }
    if result.timed_out
        || result.stdout_overflow
        || result.stderr_overflow
        || result.exit != ChildExit::Code(0)
        || !result.stderr.is_empty()
    {
        return Err(AppError::operational("Linux loader resolution failed"));
    }
    let parsed = parse_linux_loader(&result.stdout, &interpreter)?;
    for expected in expected_dependencies {
        if parsed
            .entries
            .iter()
            .filter(|(soname, _)| soname == expected)
            .count()
            != 1
        {
            return Err(AppError::operational(
                "inspector dependency is absent or duplicated in loader output",
            ));
        }
    }
    let mut closure = Vec::new();
    closure.extend_from_slice(b"align-startup-loader-closure-v1\nplatform\tlinux\n");
    closure.extend_from_slice(
        format!(
            "loader_path_hex\t{}\n",
            encode_hex(interpreter.as_os_str().as_bytes())
        )
        .as_bytes(),
    );
    closure
        .extend_from_slice(format!("loader_sha256\t{}\n", encode_hex(&loader_digest)).as_bytes());
    closure.extend_from_slice(format!("vdso_name_hex\t{}\n", encode_hex(&parsed.vdso)).as_bytes());
    closure.extend_from_slice(
        format!(
            "vdso_build_id_hex\t{}\n",
            encode_hex(&linux_vdso_build_id()?)
        )
        .as_bytes(),
    );
    closure.extend_from_slice(format!("entry_count\t{}\n", parsed.entries.len()).as_bytes());
    for (ordinal, (soname, path)) in parsed.entries.iter().enumerate() {
        let canonical =
            fs::canonicalize(path).map_err(|error| AppError::io("loader dependency", error))?;
        validate_immutable_chain(&canonical)?;
        let digest = digest_path(&canonical, FILE_MAX, "loader dependency")?;
        closure.extend_from_slice(
            format!(
                "entry\t{}\t{}\t{}\t{}\n",
                ordinal,
                encode_hex(soname),
                encode_hex(canonical.as_os_str().as_bytes()),
                encode_hex(&digest)
            )
            .as_bytes(),
        );
    }
    if closure.len() > 1024 * 1024 {
        return Err(AppError::operational("loader closure exceeds 1 MiB"));
    }
    let _ = executable;
    Ok(closure)
}

#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
fn loader_closure(
    _watchdog: &Watchdog,
    _observer: &File,
    _executable: &Path,
    executable_file: &mut File,
    _fixture: &str,
    _arm: &str,
    _work: &Path,
    _environment: &[OsString],
    _execution_state: &[u8],
    _persist_logs: bool,
    expected_dependencies: &[Vec<u8>],
) -> AppResult<Vec<u8>> {
    let (dylinker, dependencies) = macho_load_commands(executable_file)?;
    let expected = [b"/usr/lib/libSystem.B.dylib".to_vec()];
    if dylinker != b"/usr/lib/dyld" || dependencies != expected || expected_dependencies != expected
    {
        return Err(AppError::operational("Mach-O loader closure mismatch"));
    }
    let cache_uuid = macos_shared_cache_uuid()?;
    let libc_uuid = macos_libsystem_uuid()?;
    let mut closure = Vec::new();
    closure.extend_from_slice(b"align-startup-loader-closure-v1\nplatform\tmacos\n");
    closure.extend_from_slice(b"loader_path_hex\t2f7573722f6c69622f64796c64\n");
    closure.extend_from_slice(
        format!("loader_shared_cache_uuid\t{}\n", encode_hex(&cache_uuid)).as_bytes(),
    );
    closure.extend_from_slice(
        b"vdso_name_hex\tunavailable:macos\nvdso_build_id_hex\tunavailable:macos\nentry_count\t1\n",
    );
    closure.extend_from_slice(
        format!(
            "entry\t0\t2f7573722f6c69622f6c696253797374656d2e422e64796c6962\t2f7573722f6c69622f6c696253797374656d2e422e64796c6962\t{}\n",
            encode_hex(&libc_uuid)
        )
        .as_bytes(),
    );
    Ok(closure)
}

#[cfg(target_os = "macos")]
fn macho_load_commands(file: &mut File) -> AppResult<(Vec<u8>, Vec<Vec<u8>>)> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| AppError::io("Mach-O header", error))?;
    let mut header = [0u8; 32];
    file.read_exact(&mut header)
        .map_err(|error| AppError::io("Mach-O header", error))?;
    validate_native_image(&header, b"aarch64-apple-darwin", true)?;
    let commands = u32::from_le_bytes(header[16..20].try_into().expect("four-byte slice"));
    let command_bytes =
        u32::from_le_bytes(header[20..24].try_into().expect("four-byte slice")) as usize;
    if commands == 0 || commands > 4096 || command_bytes > 1024 * 1024 {
        return Err(AppError::operational(
            "Mach-O load-command table is malformed",
        ));
    }
    let mut bytes = vec![0; command_bytes];
    file.read_exact(&mut bytes)
        .map_err(|error| AppError::io("Mach-O load commands", error))?;
    let mut cursor = 0usize;
    let mut dylinker = None;
    let mut dependencies = Vec::new();
    for _ in 0..commands {
        let command = bytes
            .get(cursor..cursor + 8)
            .ok_or_else(|| AppError::operational("truncated Mach-O load command"))?;
        let kind = u32::from_le_bytes(command[..4].try_into().expect("four-byte slice"));
        let size = u32::from_le_bytes(command[4..8].try_into().expect("four-byte slice")) as usize;
        if size < 8 || !size.is_multiple_of(8) {
            return Err(AppError::operational("invalid Mach-O load-command size"));
        }
        let command = bytes
            .get(
                cursor
                    ..cursor
                        .checked_add(size)
                        .ok_or_else(|| AppError::operational("Mach-O command overflow"))?,
            )
            .ok_or_else(|| AppError::operational("truncated Mach-O load command"))?;
        if matches!(kind, 0x20 | 0x8000_0018 | 0x8000_001f | 0x8000_0023) {
            return Err(AppError::operational(
                "unsupported Mach-O dependency command",
            ));
        }
        if kind == 0xe || kind == 0xc {
            if command.len() < 12 {
                return Err(AppError::operational("short Mach-O path command"));
            }
            let offset =
                u32::from_le_bytes(command[8..12].try_into().expect("four-byte slice")) as usize;
            let tail = command
                .get(offset..)
                .ok_or_else(|| AppError::operational("invalid Mach-O path offset"))?;
            let nul = tail
                .iter()
                .position(|byte| *byte == 0)
                .ok_or_else(|| AppError::operational("unterminated Mach-O path"))?;
            let path = tail[..nul].to_vec();
            if path.is_empty() || path[0] != b'/' {
                return Err(AppError::operational("non-absolute Mach-O loader path"));
            }
            if kind == 0xe {
                if dylinker.replace(path).is_some() {
                    return Err(AppError::operational("multiple Mach-O dylinker commands"));
                }
            } else {
                dependencies.push(path);
            }
        }
        cursor += size;
    }
    if cursor != bytes.len() {
        return Err(AppError::operational("Mach-O load-command size mismatch"));
    }
    Ok((
        dylinker.ok_or_else(|| AppError::operational("missing Mach-O dylinker"))?,
        dependencies,
    ))
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn _dyld_get_shared_cache_uuid(uuid: *mut u8) -> bool;
    fn _dyld_shared_cache_contains_path(path: *const libc::c_char) -> bool;
    fn _dyld_image_count() -> u32;
    fn _dyld_get_image_name(index: u32) -> *const libc::c_char;
    fn _dyld_get_image_header(index: u32) -> *const u8;
}

#[cfg(target_os = "macos")]
fn macos_shared_cache_uuid() -> AppResult<[u8; 16]> {
    let mut uuid = [0u8; 16];
    // SAFETY: uuid is writable and the dyld API copies exactly one UUID.
    if !unsafe { _dyld_get_shared_cache_uuid(uuid.as_mut_ptr()) } || uuid == [0; 16] {
        return Err(AppError::operational("dyld shared-cache UUID unavailable"));
    }
    let path = c"/usr/lib/libSystem.B.dylib";
    // SAFETY: path is a fixed NUL-terminated absolute path.
    if !unsafe { _dyld_shared_cache_contains_path(path.as_ptr()) } {
        return Err(AppError::operational(
            "libSystem is absent from the dyld shared cache",
        ));
    }
    Ok(uuid)
}

#[cfg(target_os = "macos")]
fn macos_libsystem_uuid() -> AppResult<[u8; 16]> {
    let mut found = None;
    // SAFETY: dyld owns a stable table for the process lifetime.
    let count = unsafe { _dyld_image_count() };
    if count > 4096 {
        return Err(AppError::operational("dyld image table exceeds its bound"));
    }
    for index in 0..count {
        // SAFETY: index is below the snapshot count.
        let name = unsafe { _dyld_get_image_name(index) };
        // SAFETY: index is below the snapshot count.
        let header = unsafe { _dyld_get_image_header(index) };
        if name.is_null() || header.is_null() {
            return Err(AppError::operational(
                "dyld image table contains a null entry",
            ));
        }
        // SAFETY: dyld returns a process-lifetime NUL-terminated pathname.
        let name = unsafe { CStr::from_ptr(name) }.to_bytes();
        if name != b"/usr/lib/libSystem.B.dylib" {
            continue;
        }
        // SAFETY: dyld returns a readable native Mach-O header and command table.
        let fixed = unsafe { std::slice::from_raw_parts(header, 32) };
        if &fixed[..4] != b"\xcf\xfa\xed\xfe" {
            return Err(AppError::operational(
                "loaded libSystem is not native Mach-O",
            ));
        }
        let commands = u32::from_le_bytes(fixed[16..20].try_into().expect("four-byte slice"));
        let command_bytes =
            u32::from_le_bytes(fixed[20..24].try_into().expect("four-byte slice")) as usize;
        if commands == 0 || commands > 4096 || command_bytes > 1024 * 1024 {
            return Err(AppError::operational(
                "loaded libSystem commands are malformed",
            ));
        }
        // SAFETY: the validated load-command byte count follows the fixed header in this mapping.
        let bytes = unsafe { std::slice::from_raw_parts(header.add(32), command_bytes) };
        let mut cursor = 0usize;
        let mut uuid = None;
        let mut install_name = None;
        for _ in 0..commands {
            let command = bytes
                .get(cursor..cursor + 8)
                .ok_or_else(|| AppError::operational("truncated loaded Mach-O command"))?;
            let kind = u32::from_le_bytes(command[..4].try_into().expect("four-byte slice"));
            let size =
                u32::from_le_bytes(command[4..8].try_into().expect("four-byte slice")) as usize;
            if size < 8 || !size.is_multiple_of(8) {
                return Err(AppError::operational("invalid loaded Mach-O command size"));
            }
            let command = bytes
                .get(
                    cursor
                        ..cursor.checked_add(size).ok_or_else(|| {
                            AppError::operational("loaded Mach-O command overflow")
                        })?,
                )
                .ok_or_else(|| AppError::operational("truncated loaded Mach-O command"))?;
            if kind == 0x1b
                && (command.len() != 24
                    || uuid
                        .replace(command[8..24].try_into().expect("sixteen-byte slice"))
                        .is_some())
            {
                return Err(AppError::operational(
                    "invalid loaded libSystem UUID command",
                ));
            }
            if kind == 0xd {
                if command.len() < 12 {
                    return Err(AppError::operational(
                        "short loaded Mach-O identity command",
                    ));
                }
                let offset = u32::from_le_bytes(command[8..12].try_into().expect("four-byte slice"))
                    as usize;
                let tail = command.get(offset..).ok_or_else(|| {
                    AppError::operational("invalid loaded Mach-O identity offset")
                })?;
                let nul = tail
                    .iter()
                    .position(|byte| *byte == 0)
                    .ok_or_else(|| AppError::operational("unterminated loaded Mach-O identity"))?;
                if install_name.replace(tail[..nul].to_vec()).is_some() {
                    return Err(AppError::operational(
                        "duplicate loaded Mach-O identity command",
                    ));
                }
            }
            cursor += size;
        }
        if cursor != bytes.len()
            || install_name.as_deref() != Some(b"/usr/lib/libSystem.B.dylib".as_slice())
        {
            return Err(AppError::operational(
                "loaded libSystem identity is malformed",
            ));
        }
        let uuid = uuid.ok_or_else(|| AppError::operational("loaded libSystem UUID absent"))?;
        if uuid == [0; 16] || found.replace(uuid).is_some() {
            return Err(AppError::operational(
                "invalid or duplicate loaded libSystem image",
            ));
        }
    }
    found.ok_or_else(|| AppError::operational("loaded libSystem image absent"))
}

#[cfg(target_os = "linux")]
fn digest_path(path: &Path, limit: u64, label: &str) -> AppResult<[u8; 32]> {
    let mut file = open_bounded_regular(path, limit, label)?;
    sha256_reader(&mut file, limit)
}

#[cfg(target_os = "linux")]
fn elf_interpreter(file: &mut File) -> AppResult<PathBuf> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| AppError::io("ELF header", error))?;
    let mut header = [0u8; 64];
    file.read_exact(&mut header)
        .map_err(|error| AppError::io("ELF header", error))?;
    validate_native_image(
        &header,
        if cfg!(target_arch = "x86_64") {
            b"x86_64-pc-linux-gnu"
        } else {
            b"aarch64-unknown-linux-gnu"
        },
        true,
    )?;
    let phoff = u64::from_le_bytes(header[32..40].try_into().expect("eight-byte slice"));
    let phentsize = u16::from_le_bytes(header[54..56].try_into().expect("two-byte slice")) as u64;
    let phnum = u16::from_le_bytes(header[56..58].try_into().expect("two-byte slice")) as u64;
    if phentsize != 56 || phnum == 0 || phnum > 1024 {
        return Err(AppError::operational(
            "ELF program-header table is malformed",
        ));
    }
    let mut found = None;
    for index in 0..phnum {
        let offset = phoff
            .checked_add(
                index
                    .checked_mul(phentsize)
                    .ok_or_else(|| AppError::operational("ELF program-header overflow"))?,
            )
            .ok_or_else(|| AppError::operational("ELF program-header overflow"))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| AppError::io("ELF program header", error))?;
        let mut program = [0u8; 56];
        file.read_exact(&mut program)
            .map_err(|error| AppError::io("ELF program header", error))?;
        if u32::from_le_bytes(program[..4].try_into().expect("four-byte slice")) == 3 {
            if found.is_some() {
                return Err(AppError::operational("ELF has multiple PT_INTERP records"));
            }
            let offset = u64::from_le_bytes(program[8..16].try_into().expect("eight-byte slice"));
            let size = u64::from_le_bytes(program[32..40].try_into().expect("eight-byte slice"));
            if !(2..=4096).contains(&size) {
                return Err(AppError::operational("ELF PT_INTERP size is invalid"));
            }
            let mut bytes = vec![0; size as usize];
            file.seek(SeekFrom::Start(offset))
                .map_err(|error| AppError::io("ELF interpreter", error))?;
            file.read_exact(&mut bytes)
                .map_err(|error| AppError::io("ELF interpreter", error))?;
            if bytes.pop() != Some(0) || bytes.contains(&0) || bytes.first() != Some(&b'/') {
                return Err(AppError::operational("ELF PT_INTERP is malformed"));
            }
            found = Some(PathBuf::from(OsString::from_vec(bytes)));
        }
    }
    found.ok_or_else(|| AppError::operational("ELF has no PT_INTERP"))
}

#[cfg(target_os = "linux")]
struct LinuxLoaderOutput {
    vdso: Vec<u8>,
    entries: Vec<(Vec<u8>, PathBuf)>,
}

#[cfg(target_os = "linux")]
fn parse_linux_loader(bytes: &[u8], interpreter: &Path) -> AppResult<LinuxLoaderOutput> {
    if bytes.contains(&0) || !bytes.is_ascii() || !bytes.ends_with(b"\n") {
        return Err(AppError::operational("loader output framing is malformed"));
    }
    let mut vdso = None;
    let mut loader_self = false;
    let mut entries = Vec::new();
    let mut sonames = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for line in bytes[..bytes.len() - 1].split(|byte| *byte == b'\n') {
        let indented = line.starts_with(b"\t");
        let row = line.strip_prefix(b"\t").unwrap_or(line);
        if row.starts_with(b"linux-vdso.so.1 ") {
            let (name, address) = rsplit_once_byte(row, b' ')
                .ok_or_else(|| AppError::operational("malformed vDSO row"))?;
            validate_loader_address(address)?;
            if vdso.replace(name.to_vec()).is_some() || name != b"linux-vdso.so.1" {
                return Err(AppError::operational("invalid or duplicate vDSO row"));
            }
            continue;
        }
        if !indented {
            return Err(AppError::operational("unindented loader dependency row"));
        }
        let (body, address) = rsplit_once_byte(row, b' ')
            .ok_or_else(|| AppError::operational("malformed loader row"))?;
        validate_loader_address(address)?;
        if let Some((soname, path)) = split_once_bytes(body, b" => ") {
            if soname.first() == Some(&b'/') {
                let canonical = fs::canonicalize(Path::new(OsStr::from_bytes(path)))
                    .map_err(|error| AppError::io("loader-self canonicalization", error))?;
                if loader_self || canonical != interpreter {
                    return Err(AppError::operational("invalid loader-self row"));
                }
                loader_self = true;
                continue;
            }
            if soname.is_empty()
                || soname
                    .iter()
                    .any(|byte| byte.is_ascii_whitespace() || *byte == b'/')
                || path.first() != Some(&b'/')
                || path.iter().any(u8::is_ascii_whitespace)
                || !sonames.insert(soname.to_vec())
                || !paths.insert(path.to_vec())
            {
                return Err(AppError::operational(
                    "invalid or duplicate loader dependency",
                ));
            }
            entries.push((
                soname.to_vec(),
                PathBuf::from(OsString::from_vec(path.to_vec())),
            ));
        } else {
            if loader_self || body != interpreter.as_os_str().as_bytes() {
                return Err(AppError::operational("invalid loader-self row"));
            }
            loader_self = true;
        }
    }
    Ok(LinuxLoaderOutput {
        vdso: vdso.ok_or_else(|| AppError::operational("missing vDSO row"))?,
        entries: if loader_self {
            entries
        } else {
            return Err(AppError::operational("missing loader-self row"));
        },
    })
}

#[cfg(target_os = "linux")]
fn split_once_bytes<'a>(bytes: &'a [u8], separator: &[u8]) -> Option<(&'a [u8], &'a [u8])> {
    bytes
        .windows(separator.len())
        .position(|window| window == separator)
        .map(|index| (&bytes[..index], &bytes[index + separator.len()..]))
}

#[cfg(target_os = "linux")]
fn rsplit_once_byte(bytes: &[u8], separator: u8) -> Option<(&[u8], &[u8])> {
    bytes
        .iter()
        .rposition(|byte| *byte == separator)
        .map(|index| (&bytes[..index], &bytes[index + 1..]))
}

#[cfg(target_os = "linux")]
fn validate_loader_address(bytes: &[u8]) -> AppResult<()> {
    if bytes.len() < 4
        || !bytes.starts_with(b"(0x")
        || !bytes.ends_with(b")")
        || !bytes[3..bytes.len() - 1]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(AppError::operational("noncanonical loader address"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_vdso_build_id() -> AppResult<Vec<u8>> {
    // SAFETY: AT_SYSINFO_EHDR is a kernel-owned readable in-process ELF image.
    let base = unsafe { libc::getauxval(libc::AT_SYSINFO_EHDR) } as usize;
    if base == 0 {
        return Err(AppError::operational("vDSO image is unavailable"));
    }
    // SAFETY: the kernel-provided ELF mapping contains the fixed 64-byte header.
    let header = unsafe { std::slice::from_raw_parts(base as *const u8, 64) };
    validate_native_image(
        header,
        if cfg!(target_arch = "x86_64") {
            b"x86_64-pc-linux-gnu"
        } else {
            b"aarch64-unknown-linux-gnu"
        },
        false,
    )?;
    let phoff = u64::from_le_bytes(header[32..40].try_into().expect("eight-byte slice")) as usize;
    let phentsize = u16::from_le_bytes(header[54..56].try_into().expect("two-byte slice")) as usize;
    let phnum = u16::from_le_bytes(header[56..58].try_into().expect("two-byte slice")) as usize;
    if phentsize != 56 || phnum == 0 || phnum > 128 {
        return Err(AppError::operational("vDSO program headers are malformed"));
    }
    let mut found = None;
    for index in 0..phnum {
        let address = base
            .checked_add(phoff)
            .and_then(|value| value.checked_add(index.checked_mul(phentsize)?))
            .ok_or_else(|| AppError::operational("vDSO address overflow"))?;
        // SAFETY: validated kernel ELF table bounds identify a fixed program header.
        let program = unsafe { std::slice::from_raw_parts(address as *const u8, 56) };
        if u32::from_le_bytes(program[..4].try_into().expect("four-byte slice")) != 4 {
            continue;
        }
        let offset =
            u64::from_le_bytes(program[16..24].try_into().expect("eight-byte slice")) as usize;
        let size =
            u64::from_le_bytes(program[40..48].try_into().expect("eight-byte slice")) as usize;
        if size == 0 || size > 64 * 1024 {
            return Err(AppError::operational("vDSO note segment is malformed"));
        }
        let note_address = base
            .checked_add(offset)
            .ok_or_else(|| AppError::operational("vDSO note address overflow"))?;
        // SAFETY: PT_NOTE names a kernel-owned segment in the validated vDSO mapping.
        let notes = unsafe { std::slice::from_raw_parts(note_address as *const u8, size) };
        let mut cursor = 0usize;
        while cursor < notes.len() {
            let header = notes
                .get(cursor..cursor + 12)
                .ok_or_else(|| AppError::operational("truncated vDSO note"))?;
            let name_size =
                u32::from_ne_bytes(header[..4].try_into().expect("four-byte slice")) as usize;
            let desc_size =
                u32::from_ne_bytes(header[4..8].try_into().expect("four-byte slice")) as usize;
            let kind = u32::from_ne_bytes(header[8..12].try_into().expect("four-byte slice"));
            cursor += 12;
            let name = notes
                .get(
                    cursor
                        ..cursor
                            .checked_add(name_size)
                            .ok_or_else(|| AppError::operational("vDSO note overflow"))?,
                )
                .ok_or_else(|| AppError::operational("truncated vDSO note name"))?;
            cursor = align4(cursor + name_size)?;
            let description = notes
                .get(
                    cursor
                        ..cursor
                            .checked_add(desc_size)
                            .ok_or_else(|| AppError::operational("vDSO note overflow"))?,
                )
                .ok_or_else(|| AppError::operational("truncated vDSO note value"))?;
            cursor = align4(cursor + desc_size)?;
            if kind == 3
                && name == b"GNU\0"
                && (description.is_empty() || found.replace(description.to_vec()).is_some())
            {
                return Err(AppError::operational("invalid or duplicate vDSO build id"));
            }
        }
    }
    found.ok_or_else(|| AppError::operational("vDSO GNU build id is absent"))
}

#[cfg(target_os = "linux")]
fn align4(value: usize) -> AppResult<usize> {
    value
        .checked_add(3)
        .map(|value| value & !3)
        .ok_or_else(|| AppError::operational("alignment overflow"))
}

fn timed_environment() -> Vec<OsString> {
    vec![
        OsString::from("LANG=C"),
        OsString::from("LC_ALL=C"),
        OsString::from("TZ=UTC"),
    ]
}

fn open_bounded_regular(path: &Path, limit: u64, label: &str) -> AppResult<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options
        .open(path)
        .map_err(|error| AppError::io(label, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| AppError::io(label, error))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(AppError::operational(format!(
            "{label} is not a bounded regular file"
        )));
    }
    Ok(file)
}

fn persist_phase_logs(
    work: &Path,
    fixture: &str,
    arm: &str,
    phase: &str,
    result: &ChildResult,
) -> AppResult<()> {
    let directory = work.join("logs").join(fixture).join(arm);
    write_exclusive(
        &directory.join(format!("{phase}.stdout")),
        &result.stdout,
        0o600,
    )?;
    write_exclusive(
        &directory.join(format!("{phase}.stderr")),
        &result.stderr,
        0o600,
    )
}

fn require_empty_directory(path: &Path, label: &str) -> AppResult<()> {
    let mut entries = fs::read_dir(path).map_err(|error| AppError::io(label, error))?;
    if entries
        .next()
        .transpose()
        .map_err(|error| AppError::io(label, error))?
        .is_some()
    {
        return Err(AppError::operational(format!("{label} is not empty")));
    }
    Ok(())
}

fn require_only_named_file(directory: &Path, name: &str) -> AppResult<()> {
    let entries: Vec<_> = fs::read_dir(directory)
        .map_err(|error| AppError::io("artifact directory", error))?
        .collect::<Result<_, _>>()
        .map_err(|error| AppError::io("artifact directory", error))?;
    if entries.len() != 1
        || entries[0].file_name().as_bytes() != name.as_bytes()
        || !entries[0]
            .file_type()
            .map_err(|error| AppError::io("artifact directory", error))?
            .is_file()
    {
        return Err(AppError::operational(
            "artifact directory contains an unexpected entry",
        ));
    }
    Ok(())
}

fn parse_needed_libraries(report: &[u8]) -> AppResult<Vec<Vec<u8>>> {
    if report.contains(&0) || report.contains(&b'\r') || !report.ends_with(b"\n") {
        return Err(AppError::operational(
            "inspection report framing is malformed",
        ));
    }
    let mut found = None;
    let lines: Vec<&[u8]> = report[..report.len() - 1]
        .split(|byte| *byte == b'\n')
        .collect();
    let mut index = 0;
    while index < lines.len() {
        if lines[index] == b"NeededLibraries [" {
            if found.is_some() {
                return Err(AppError::operational("repeated NeededLibraries block"));
            }
            index += 1;
            let mut libraries = Vec::new();
            while index < lines.len() && lines[index] != b"]" {
                let value = lines[index]
                    .strip_prefix(b"  ")
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| AppError::operational("malformed NeededLibraries entry"))?;
                libraries.push(value.to_vec());
                index += 1;
            }
            if index == lines.len() {
                return Err(AppError::operational("unterminated NeededLibraries block"));
            }
            found = Some(libraries);
        }
        index += 1;
    }
    found.ok_or_else(|| AppError::operational("missing NeededLibraries block"))
}

struct ResultWriter {
    file: File,
}

impl ResultWriter {
    fn create(path: &Path) -> AppResult<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(path)
            .map_err(|error| AppError::io("result creation", error))?;
        set_file_mode(&file, 0o600, "result mode")?;
        Ok(Self { file })
    }

    fn row(&mut self, row: &str) -> AppResult<()> {
        if !row.ends_with('\n') || row[..row.len() - 1].contains('\n') || row.contains('\r') {
            return Err(AppError::operational(
                "attempted to write a noncanonical result row",
            ));
        }
        self.file
            .write_all(row.as_bytes())
            .map_err(|error| AppError::io("result write", error))?;
        self.file
            .flush()
            .map_err(|error| AppError::io("result flush", error))
    }
}

fn encode_os_list(values: &[OsString]) -> AppResult<String> {
    let borrowed: Vec<&[u8]> = values.iter().map(|value| value.as_bytes()).collect();
    encode_list(&borrowed)
}

fn artifact_row(
    artifact: &Artifact,
    _provenance: &Provenance,
    arms: &[SealedArm],
) -> AppResult<String> {
    let arm = &arms[artifact.arm_index];
    let fixture_sha: [u8; 32] = Sha256::digest(artifact.fixture.bytes).into();
    let child_argv: Vec<OsString> = std::iter::once(artifact.executable.as_os_str().to_owned())
        .chain(
            artifact
                .fixture
                .argv
                .iter()
                .map(|value| OsString::from_vec(value.to_vec())),
        )
        .collect();
    let dependencies: Vec<&[u8]> = artifact.dependencies.iter().map(Vec::as_slice).collect();
    let full = Hash128::of(&artifact.key.wire).to_hex();
    let slot = slot_digest(&artifact.key)?.to_hex();
    Ok(format!(
        "artifact\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t\t{}\t{}\t{}\t{}\n",
        artifact.fixture.id,
        arm.arm.name,
        encode_hex(&fixture_sha),
        encode_hex(&arm.arm.compiler_sha256),
        encode_hex(&arm.arm.runtime_sha256),
        artifact.key.llvm_build_id.to_hex(),
        encode_hex(&artifact.key.wire),
        full,
        slot,
        encode_hex(&artifact.executable_sha256),
        artifact.executable_size,
        encode_hex(&artifact.inspection_sha256),
        encode_hex(&artifact.loader_closure),
        encode_list(&dependencies)?,
        encode_os_list(&artifact.build_argv)?,
        encode_os_list(&artifact.build_environment)?,
        encode_os_list(&child_argv)?,
    ))
}

fn revalidate_artifacts(artifacts: &[Artifact]) -> AppResult<()> {
    for artifact in artifacts {
        let retained = artifact
            .executable_file
            .metadata()
            .map_err(|error| AppError::io("artifact revalidation", error))?;
        let path = fs::symlink_metadata(&artifact.executable)
            .map_err(|error| AppError::io("artifact revalidation", error))?;
        if !path.is_file()
            || retained.dev() != path.dev()
            || retained.ino() != path.ino()
            || retained.mode() != path.mode()
            || retained.len() != path.len()
            || retained.len() != artifact.executable_size
        {
            return Err(AppError::operational(
                "artifact identity changed before timing",
            ));
        }
        let mut duplicate = artifact
            .executable_file
            .try_clone()
            .map_err(|error| AppError::io("artifact revalidation", error))?;
        if sha256_reader(&mut duplicate, FILE_MAX)? != artifact.executable_sha256 {
            return Err(AppError::operational(
                "artifact bytes changed before timing",
            ));
        }
    }
    Ok(())
}

fn revalidate_all_artifacts(
    watchdog: &Watchdog,
    observer: &File,
    provenance: &Provenance,
    work: &Path,
    execution_state: &[u8],
    arms: &[SealedArm],
    artifacts: &[Artifact],
) -> AppResult<()> {
    for (label, path, digest) in [
        ("C compiler", &provenance.cc_path, &provenance.cc_sha256),
        ("linker", &provenance.ld_path, &provenance.ld_sha256),
        (
            "object inspector",
            &provenance.inspector_path,
            &provenance.inspector_sha256,
        ),
    ] {
        validate_file_digest(label, path, digest, FILE_MAX)?;
    }
    for arm in arms {
        validate_file_digest(
            "sealed compiler",
            &arm.compiler,
            &arm.arm.compiler_sha256,
            FILE_MAX,
        )?;
        validate_file_digest(
            "sealed runtime",
            &arm.runtime,
            &arm.arm.runtime_sha256,
            FILE_MAX,
        )?;
    }
    revalidate_artifacts(artifacts)?;
    let environment = timed_environment();
    for artifact in artifacts {
        let build = artifact
            .executable
            .parent()
            .expect("artifact has a build directory");
        let inspection = run_tool(
            watchdog,
            observer,
            &provenance.inspector_path,
            &provenance.inspector_sha256,
            &[
                OsString::from("--sections"),
                OsString::from("--needed-libs"),
                OsString::from(format!("./{}", artifact.fixture.id)),
            ],
            &environment,
            build,
            8_000_000_000,
            2_000_000_000,
            8 * 1024 * 1024,
            true,
            false,
            execution_state,
        )?;
        if inspection.exit != ChildExit::Code(0)
            || inspection.stdout != artifact.inspection
            || !inspection.stderr.is_empty()
            || parse_needed_libraries(&inspection.stdout)? != artifact.dependencies
        {
            return Err(AppError::operational(
                "artifact inspection changed before timing",
            ));
        }
        let mut file = artifact
            .executable_file
            .try_clone()
            .map_err(|error| AppError::io("artifact closure revalidation", error))?;
        let closure = loader_closure(
            watchdog,
            observer,
            &artifact.executable,
            &mut file,
            artifact.fixture.id,
            arms[artifact.arm_index].arm.name,
            work,
            &environment,
            execution_state,
            false,
            &artifact.dependencies,
        )?;
        if closure != artifact.loader_closure {
            return Err(AppError::operational(
                "loader closure changed before timing",
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(clippy::unnecessary_cast)] // libc exposes platform-specific stat field widths.
fn work_filesystem_record(path: &Path) -> AppResult<Vec<u8>> {
    let metadata =
        fs::metadata(path).map_err(|error| AppError::io("work filesystem metadata", error))?;
    let mountinfo =
        read_bounded_virtual_nofollow(Path::new("/proc/self/mountinfo"), 1024 * 1024, "mountinfo")?;
    let canonical = path.as_os_str().as_bytes();
    let mut selected = None;
    let mut same_device_count = 0usize;
    for line in mountinfo
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let separator = line
            .windows(3)
            .position(|window| window == b" - ")
            .ok_or_else(|| AppError::usage("malformed mountinfo row"))?;
        let left: Vec<&[u8]> = line[..separator].split(|byte| *byte == b' ').collect();
        let right: Vec<&[u8]> = line[separator + 3..].split(|byte| *byte == b' ').collect();
        if left.len() < 6 || right.len() < 3 {
            return Err(AppError::usage("malformed mountinfo field count"));
        }
        let mount_id = parse_u64(left[0])?;
        let (major, minor) = parse_device(left[2])?;
        let root = decode_mountinfo_path(left[3])?;
        let mountpoint = decode_mountinfo_path(left[4])?;
        let fs_type = decode_mountinfo_path(right[0])?;
        let source = decode_mountinfo_path(right[1])?;
        if major == libc::major(metadata.dev()) as u32
            && minor == libc::minor(metadata.dev()) as u32
        {
            same_device_count += 1;
        }
        if mountpoint == canonical
            && selected
                .replace((mount_id, major, minor, root, fs_type, source))
                .is_some()
        {
            return Err(AppError::usage("work root has multiple mount records"));
        }
    }
    let (mount_id, major, minor, root, fs_type, source) =
        selected.ok_or_else(|| AppError::usage("work root is not an exact mount point"))?;
    if root != b"/" || same_device_count != 1 {
        return Err(AppError::usage(
            "work filesystem is a bind subtree or shared alias",
        ));
    }
    let c_path = cstring_path(path)?;
    let mut vfs = MaybeUninit::<libc::statvfs>::zeroed();
    let mut fsstat = MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: c_path is valid and both output records are writable.
    if unsafe { libc::statvfs(c_path.as_ptr(), vfs.as_mut_ptr()) } == -1
        || unsafe { libc::statfs(c_path.as_ptr(), fsstat.as_mut_ptr()) } == -1
    {
        return Err(AppError::io("work filesystem", io::Error::last_os_error()));
    }
    // SAFETY: successful calls initialized both records.
    let vfs = unsafe { vfs.assume_init() };
    // SAFETY: successful calls initialized both records.
    let fsstat = unsafe { fsstat.assume_init() };
    let fragment = vfs.f_frsize as u64;
    let blocks = vfs.f_blocks as u64;
    let available = vfs.f_bavail as u64;
    let total = fragment
        .checked_mul(blocks)
        .ok_or_else(|| AppError::usage("work filesystem capacity overflow"))?;
    let available_bytes = fragment
        .checked_mul(available)
        .ok_or_else(|| AppError::usage("work filesystem availability overflow"))?;
    let fsid_words: [i32; 2] = unsafe { std::mem::transmute_copy(&fsstat.f_fsid) };
    Ok(format!(
        "align-startup-work-filesystem-v1\nplatform\tlinux\nmount_id\t{mount_id}\nmount_device_major\t{major}\nmount_device_minor\t{minor}\nmount_root_hex\t{}\nfsid_word_0\t{}\nfsid_word_1\t{}\nfilesystem_type_hex\t{}\nmount_source_hex\t{}\nmount_flags\t{}\nfragment_size\t{fragment}\nblock_count\t{blocks}\ntotal_bytes\t{total}\navailable_blocks\t{available}\navailable_bytes\t{available_bytes}\n",
        encode_hex(&root), fsid_words[0], fsid_words[1], encode_hex(&fs_type), encode_hex(&source), vfs.f_flag as u64
    ).into_bytes())
}

#[cfg(target_os = "macos")]
#[allow(clippy::unnecessary_cast)] // libc field widths vary across supported SDKs.
fn work_filesystem_record(path: &Path) -> AppResult<Vec<u8>> {
    let first = macos_mount_snapshot()?;
    let canonical = path.as_os_str().as_bytes();
    let matching: Vec<&MacMount> = first
        .iter()
        .filter(|entry| entry.mountpoint == canonical)
        .collect();
    if matching.len() != 1 {
        return Err(AppError::usage(
            "work root is not one exact macOS mount point",
        ));
    }
    let selected = matching[0];
    if first
        .iter()
        .filter(|entry| entry.fsid == selected.fsid)
        .count()
        != 1
    {
        return Err(AppError::usage("work filesystem has a shared mount alias"));
    }
    let c_path = cstring_path(path)?;
    let mut vfs = MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: c_path is valid and vfs is writable.
    if unsafe { libc::statvfs(c_path.as_ptr(), vfs.as_mut_ptr()) } == -1 {
        return Err(AppError::io("work filesystem", io::Error::last_os_error()));
    }
    // SAFETY: successful statvfs initialized vfs.
    let vfs = unsafe { vfs.assume_init() };
    let fragment = vfs.f_frsize as u64;
    let blocks = vfs.f_blocks as u64;
    let available = vfs.f_bavail as u64;
    let total = fragment
        .checked_mul(blocks)
        .ok_or_else(|| AppError::usage("work filesystem capacity overflow"))?;
    let available_bytes = fragment
        .checked_mul(available)
        .ok_or_else(|| AppError::usage("work filesystem availability overflow"))?;
    Ok(format!(
        "align-startup-work-filesystem-v1\nplatform\tmacos\nmount_id\tunavailable:macos\nmount_device_major\tunavailable:macos\nmount_device_minor\tunavailable:macos\nmount_root_hex\tunavailable:macos\nfsid_word_0\t{}\nfsid_word_1\t{}\nfilesystem_type_hex\t{}\nmount_source_hex\t{}\nmount_flags\t{}\nfragment_size\t{}\nblock_count\t{}\ntotal_bytes\t{}\navailable_blocks\t{}\navailable_bytes\t{}\n",
        selected.fsid[0], selected.fsid[1], encode_hex(&selected.fs_type), encode_hex(&selected.source),
        selected.flags, fragment, blocks, total, available, available_bytes
    ).into_bytes())
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct MacMount {
    fsid: [i32; 2],
    fs_type: Vec<u8>,
    source: Vec<u8>,
    mountpoint: Vec<u8>,
    flags: u64,
}

#[cfg(target_os = "macos")]
fn macos_mount_snapshot() -> AppResult<Vec<MacMount>> {
    // SAFETY: null-buffer getfsstat returns the required entry count.
    let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    if !(0..=1024).contains(&count) {
        return Err(AppError::usage(
            "macOS mount count is unavailable or exceeds 1024",
        ));
    }
    let count = count as usize;
    let mut entries: Vec<MaybeUninit<libc::statfs>> = Vec::with_capacity(count);
    // SAFETY: getfsstat initializes at most `count` records in allocated storage.
    let filled = unsafe {
        libc::getfsstat(
            entries.as_mut_ptr().cast(),
            (count * std::mem::size_of::<libc::statfs>()) as libc::c_int,
            libc::MNT_NOWAIT,
        )
    };
    if filled < 0 || filled as usize != count {
        return Err(AppError::usage(
            "macOS mount snapshot was truncated or changed",
        ));
    }
    // SAFETY: the successful exact fill initialized all `count` records.
    unsafe { entries.set_len(count) };
    let mut output = Vec::with_capacity(count);
    for entry in entries {
        // SAFETY: exact fill initialized this record.
        let entry = unsafe { entry.assume_init() };
        let fsid: [i32; 2] = unsafe { std::mem::transmute_copy(&entry.f_fsid) };
        output.push(MacMount {
            fsid,
            fs_type: bounded_c_char_array(&entry.f_fstypename, "filesystem type")?,
            source: bounded_c_char_array(&entry.f_mntfromname, "mount source")?,
            mountpoint: bounded_c_char_array(&entry.f_mntonname, "mount point")?,
            flags: entry.f_flags as u64,
        });
    }
    Ok(output)
}

fn bounded_c_char_array<const N: usize>(
    bytes: &[libc::c_char; N],
    label: &str,
) -> AppResult<Vec<u8>> {
    let raw: Vec<u8> = bytes.iter().map(|byte| *byte as u8).collect();
    let end = raw
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| AppError::usage(format!("unterminated {label}")))?;
    if end == 0 {
        return Err(AppError::usage(format!("empty {label}")));
    }
    Ok(raw[..end].to_vec())
}

#[cfg(target_os = "macos")]
fn macos_sysctl_bytes(name: &CStr, limit: usize) -> AppResult<Vec<u8>> {
    let mut length = 0usize;
    // SAFETY: name is NUL-terminated and length is writable.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } == -1
        || length == 0
        || length > limit
    {
        return Err(AppError::usage("macOS sysctl byte value unavailable"));
    }
    let mut bytes = vec![0u8; length];
    // SAFETY: bytes has the queried writable capacity.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } == -1
        || length != bytes.len()
    {
        return Err(AppError::usage("macOS sysctl byte value changed"));
    }
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(AppError::usage("macOS sysctl byte value malformed"));
    }
    Ok(bytes)
}

#[cfg(target_os = "macos")]
fn macos_sysctl_i32(name: &CStr) -> AppResult<i32> {
    let mut value = 0i32;
    let mut length = std::mem::size_of::<i32>();
    // SAFETY: value and length are writable fixed-size storage.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut i32).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } == -1
        || length != std::mem::size_of::<i32>()
    {
        return Err(AppError::usage("macOS i32 sysctl unavailable"));
    }
    Ok(value)
}

#[cfg(target_os = "macos")]
fn macos_sysctl_u32(name: &CStr) -> AppResult<u32> {
    let mut value = 0u32;
    let mut length = std::mem::size_of::<u32>();
    // SAFETY: value and length are writable fixed-size storage.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u32).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } == -1
        || length != std::mem::size_of::<u32>()
    {
        return Err(AppError::usage("macOS u32 sysctl unavailable"));
    }
    Ok(value)
}

#[cfg(target_os = "linux")]
fn parse_device(bytes: &[u8]) -> AppResult<(u32, u32)> {
    let (major, minor) =
        split_once_bytes(bytes, b":").ok_or_else(|| AppError::usage("malformed mount device"))?;
    Ok((
        u32::try_from(parse_u64(major)?).map_err(|_| AppError::usage("mount major overflow"))?,
        u32::try_from(parse_u64(minor)?).map_err(|_| AppError::usage("mount minor overflow"))?,
    ))
}

#[cfg(target_os = "linux")]
fn decode_mountinfo_path(bytes: &[u8]) -> AppResult<Vec<u8>> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'\\' {
            output.push(bytes[cursor]);
            cursor += 1;
            continue;
        }
        let escape = bytes
            .get(cursor + 1..cursor + 4)
            .ok_or_else(|| AppError::usage("truncated mountinfo escape"))?;
        let value = match escape {
            b"040" => b' ',
            b"011" => b'\t',
            b"012" => b'\n',
            b"134" => b'\\',
            _ => return Err(AppError::usage("unknown mountinfo escape")),
        };
        output.push(value);
        cursor += 4;
    }
    Ok(output)
}

fn stable_execution_state() -> AppResult<Vec<u8>> {
    let first = execution_state_record()?;
    let second = execution_state_record()?;
    if first != second {
        return Err(AppError::usage("inherited execution state is unstable"));
    }
    Ok(first)
}

#[cfg(target_os = "linux")]
fn execution_state_record() -> AppResult<Vec<u8>> {
    // SAFETY: read-only process observations.
    let online = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if online <= 0 {
        return Err(AppError::usage("online CPU count unavailable"));
    }
    let available = thread::available_parallelism()
        .map_err(|error| AppError::usage(format!("available parallelism: {error}")))?
        .get();
    let affinity = linux_affinity()?;
    require_default_numa_policy()?;
    let nice = current_nice()?;
    // SAFETY: read-only scheduler queries for the current process.
    let scheduler = unsafe { libc::sched_getscheduler(0) };
    let mut parameter = MaybeUninit::<libc::sched_param>::zeroed();
    // SAFETY: parameter is writable.
    if scheduler == -1 || unsafe { libc::sched_getparam(0, parameter.as_mut_ptr()) } == -1 {
        return Err(AppError::usage("scheduler state unavailable"));
    }
    // SAFETY: successful sched_getparam initialized the record.
    let priority = unsafe { parameter.assume_init() }.sched_priority;
    let status =
        read_bounded_virtual_nofollow(Path::new("/proc/self/status"), 64 * 1024, "process status")?;
    let cgroup =
        read_bounded_virtual_nofollow(Path::new("/proc/self/cgroup"), 64 * 1024, "process cgroup")?;
    let caps = proc_status_fields(&status)?;
    for key in ["CapInh", "CapPrm", "CapEff", "CapAmb"] {
        if caps
            .get(key)
            .is_none_or(|value| *value != "0000000000000000")
        {
            return Err(AppError::usage("observer has mutable Linux capabilities"));
        }
    }
    let cpus = caps
        .get("Cpus_allowed_list")
        .ok_or_else(|| AppError::usage("Cpus_allowed_list absent"))?;
    if parse_cpu_list(cpus.as_bytes())? != parse_cpu_list(affinity.as_bytes())? {
        return Err(AppError::usage("affinity and Cpus_allowed_list differ"));
    }
    let mems = caps
        .get("Mems_allowed_list")
        .ok_or_else(|| AppError::usage("Mems_allowed_list absent"))?;
    let mut output = format!(
        "align-startup-execution-state-v1\nonline_cpu_count\t{}\navailable_parallelism\t{}\naffinity\t{}\nnuma_policy\tdefault\nnuma_nodes\tnone\nnice\t{}\nscheduler_policy\t{}\nscheduler_priority\t{}\nqos_class\tunavailable:linux\nqos_relative_priority\tunavailable:linux\nlinux_cap_inheritable\t{}\nlinux_cap_permitted\t{}\nlinux_cap_effective\t{}\nlinux_cap_bounding\t{}\nlinux_cap_ambient\t{}\nlinux_no_new_privs\t{}\n",
        online, available, affinity, nice, scheduler, priority,
        caps["CapInh"], caps["CapPrm"], caps["CapEff"], caps["CapBnd"], caps["CapAmb"], caps["NoNewPrivs"]
    ).into_bytes();
    append_rlimits(&mut output)?;
    let constraints = linux_cgroup_constraints(&cgroup)?;
    output.extend_from_slice(format!("proc_cgroup\t{}\ncpus_allowed_list\t{}\nmems_allowed_list\t{}\ncgroup_constraints\t{}\n", encode_hex(&cgroup), encode_hex(cpus.as_bytes()), encode_hex(mems.as_bytes()), constraints).as_bytes());
    if output.len() > 64 * 1024 {
        return Err(AppError::usage("execution-state record exceeds 64 KiB"));
    }
    Ok(output)
}

#[cfg(target_os = "macos")]
fn execution_state_record() -> AppResult<Vec<u8>> {
    // SAFETY: read-only process observation.
    let online = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if online <= 0 {
        return Err(AppError::usage("online CPU count unavailable"));
    }
    let available = thread::available_parallelism()
        .map_err(|error| AppError::usage(format!("available parallelism: {error}")))?
        .get();
    let nice = current_nice()?;
    let mut relative_priority = 0;
    // SAFETY: pthread_self returns the current live thread and priority is writable.
    let qos = unsafe { pthread_get_qos_class_np(libc::pthread_self(), &mut relative_priority) };
    let qos = match qos {
        0x21 => "user-interactive",
        0x19 => "user-initiated",
        0x15 => "default",
        0x11 => "utility",
        0x09 => "background",
        0x00 => "unspecified",
        _ => return Err(AppError::usage("unsupported macOS QoS class")),
    };
    let mut output = format!(
        "align-startup-execution-state-v1\nonline_cpu_count\t{}\navailable_parallelism\t{}\naffinity\tunavailable:macos\nnuma_policy\tunavailable:macos\nnuma_nodes\tunavailable:macos\nnice\t{}\nscheduler_policy\tunavailable:macos\nscheduler_priority\tunavailable:macos\nqos_class\t{}\nqos_relative_priority\t{}\nlinux_cap_inheritable\tunavailable:macos\nlinux_cap_permitted\tunavailable:macos\nlinux_cap_effective\tunavailable:macos\nlinux_cap_bounding\tunavailable:macos\nlinux_cap_ambient\tunavailable:macos\nlinux_no_new_privs\tunavailable:macos\n",
        online, available, nice, qos, relative_priority
    ).into_bytes();
    append_rlimits(&mut output)?;
    output.extend_from_slice(b"proc_cgroup\tunavailable:macos\ncpus_allowed_list\tunavailable:macos\nmems_allowed_list\tunavailable:macos\ncgroup_constraints\tunavailable:macos\n");
    Ok(output)
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn pthread_get_qos_class_np(
        thread: libc::pthread_t,
        relative_priority: *mut libc::c_int,
    ) -> u32;
}

#[cfg(target_os = "linux")]
fn linux_affinity() -> AppResult<String> {
    let mut set = MaybeUninit::<libc::cpu_set_t>::zeroed();
    // SAFETY: set is writable and size matches the platform ABI type.
    if unsafe {
        libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), set.as_mut_ptr())
    } == -1
    {
        return Err(AppError::usage("CPU affinity unavailable"));
    }
    // SAFETY: successful call initialized the set.
    let set = unsafe { set.assume_init() };
    // SAFETY: set is a fully initialized plain byte record.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            (&set as *const libc::cpu_set_t).cast::<u8>(),
            std::mem::size_of::<libc::cpu_set_t>(),
        )
    };
    let cpus: Vec<u32> = bytes
        .iter()
        .enumerate()
        .flat_map(|(byte_index, byte)| {
            (0..8).filter_map(move |bit| {
                ((byte & (1 << bit)) != 0).then_some((byte_index * 8 + bit) as u32)
            })
        })
        .collect();
    if cpus.is_empty() {
        return Err(AppError::usage("CPU affinity is empty"));
    }
    Ok(format_cpu_list(&cpus))
}

#[cfg(target_os = "linux")]
fn require_default_numa_policy() -> AppResult<()> {
    let mut mode = -1;
    let mut nodes = [0 as libc::c_ulong; 16];
    // SAFETY: mode and nodemask are writable; no address or policy-changing flag is supplied.
    if unsafe {
        libc::syscall(
            libc::SYS_get_mempolicy,
            &mut mode,
            nodes.as_mut_ptr(),
            (nodes.len() * libc::c_ulong::BITS as usize) as libc::c_ulong,
            std::ptr::null_mut::<libc::c_void>(),
            0 as libc::c_ulong,
        )
    } == -1
    {
        return Err(AppError::usage("NUMA policy unavailable"));
    }
    if mode != 0 || nodes.iter().any(|word| *word != 0) {
        return Err(AppError::usage("inherited NUMA policy is not default"));
    }
    Ok(())
}

fn current_nice() -> AppResult<i32> {
    #[cfg(target_os = "linux")]
    // SAFETY: errno location is thread-local writable storage.
    unsafe {
        *libc::__errno_location() = 0;
    }
    #[cfg(target_os = "macos")]
    // SAFETY: errno location is thread-local writable storage.
    unsafe {
        *libc::__error() = 0;
    }
    // SAFETY: getpriority is a read-only query for the current process.
    let value = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
    #[cfg(target_os = "linux")]
    // SAFETY: errno location is readable thread-local storage.
    let errno = unsafe { *libc::__errno_location() };
    #[cfg(target_os = "macos")]
    // SAFETY: errno location is readable thread-local storage.
    let errno = unsafe { *libc::__error() };
    if value == -1 && errno != 0 {
        return Err(AppError::usage("nice value unavailable"));
    }
    Ok(value)
}

#[cfg(target_os = "linux")]
fn proc_status_fields(bytes: &[u8]) -> AppResult<std::collections::BTreeMap<&'static str, String>> {
    let mut output = std::collections::BTreeMap::new();
    for (source, key) in [
        ("CapInh", "CapInh"),
        ("CapPrm", "CapPrm"),
        ("CapEff", "CapEff"),
        ("CapBnd", "CapBnd"),
        ("CapAmb", "CapAmb"),
        ("NoNewPrivs", "NoNewPrivs"),
        ("Cpus_allowed_list", "Cpus_allowed_list"),
        ("Mems_allowed_list", "Mems_allowed_list"),
    ] {
        let prefix = format!("{source}:\t");
        let value = bytes
            .split(|byte| *byte == b'\n')
            .find_map(|line| line.strip_prefix(prefix.as_bytes()))
            .ok_or_else(|| AppError::usage(format!("missing /proc status field {source}")))?;
        let value = std::str::from_utf8(value)
            .map_err(|_| AppError::usage("non-ASCII /proc status field"))?;
        output.insert(key, value.to_owned());
    }
    for cap in ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"] {
        if output[cap].len() != 16
            || !output[cap]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(AppError::usage("noncanonical capability mask"));
        }
    }
    if output["NoNewPrivs"] != "0" && output["NoNewPrivs"] != "1" {
        return Err(AppError::usage("noncanonical NoNewPrivs value"));
    }
    Ok(output)
}

#[cfg(target_os = "linux")]
fn linux_cgroup_constraints(proc_cgroup: &[u8]) -> AppResult<String> {
    if !proc_cgroup.ends_with(b"\n") || proc_cgroup[..proc_cgroup.len() - 1].contains(&b'\n') {
        return Err(AppError::usage(
            "process cgroup record is not one LF-terminated row",
        ));
    }
    let path = proc_cgroup[..proc_cgroup.len() - 1]
        .strip_prefix(b"0::")
        .ok_or_else(|| AppError::usage("legacy or hybrid cgroup hierarchy"))?;
    validate_cgroup_path(path)?;
    let mount = Path::new("/sys/fs/cgroup");
    validate_unified_cgroup_mount()?;
    let root = open_directory_nofollow(mount, "cgroup mount")?;
    let mut filesystem = MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: root is retained and filesystem is writable.
    if unsafe { libc::fstatfs(root.as_raw_fd(), filesystem.as_mut_ptr()) } == -1 {
        return Err(AppError::io(
            "cgroup mount statfs",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: successful fstatfs initialized the record.
    let filesystem = unsafe { filesystem.assume_init() };
    if filesystem.f_type as u64 != 0x6367_7270 {
        return Err(AppError::usage("cgroup v2 mount is unavailable"));
    }
    let relative = &path[1..];
    let components: Vec<&[u8]> = if relative.is_empty() {
        Vec::new()
    } else {
        relative.split(|byte| *byte == b'/').collect()
    };
    let mut directories = vec![(Vec::new(), root)];
    for (index, component) in components.iter().enumerate() {
        let parent = &directories.last().expect("root retained").1;
        let directory = open_directory_at(parent.as_raw_fd(), component, "cgroup component")?;
        directories.push((join_components(&components[..=index]), directory));
    }
    let mut records = Vec::new();
    for (relative_bytes, directory) in directories.iter().rev() {
        let before = directory_identity(directory.as_raw_fd(), "cgroup directory")?;
        let mut present_names = BTreeSet::new();
        let descriptor_path = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
        for entry in fs::read_dir(&descriptor_path)
            .map_err(|error| AppError::usage(format!("cgroup enumeration: {error}")))?
        {
            let entry =
                entry.map_err(|error| AppError::usage(format!("cgroup enumeration: {error}")))?;
            let name = entry.file_name();
            let bytes = name.as_bytes();
            if cgroup_namespace(bytes) {
                if entry_is_symlink_at(directory.as_raw_fd(), bytes)? {
                    return Err(AppError::usage("cgroup namespace contains a symlink"));
                }
                present_names.insert(bytes.to_vec());
            }
        }
        let mut keys: Vec<Vec<u8>> = CGROUP_KEYS
            .iter()
            .map(|key| key.as_bytes().to_vec())
            .collect();
        let mut huge_sizes = BTreeSet::new();
        for name in &present_names {
            if let Some(size) = hugepage_constraint_size(name)? {
                huge_sizes.insert(size);
            }
        }
        for size in huge_sizes {
            keys.push([b"hugetlb.".as_slice(), &size, b".max"].concat());
            keys.push([b"hugetlb.".as_slice(), &size, b".rsvd.max"].concat());
        }
        let admitted: BTreeSet<Vec<u8>> = keys.iter().cloned().collect();
        for name in &present_names {
            if !admitted.contains(name) && !is_volatile_cgroup_name(name) {
                return Err(AppError::usage(format!(
                    "unknown cgroup constraint {}",
                    String::from_utf8_lossy(name)
                )));
            }
        }
        for key in keys {
            let mut record = format!(
                "{}\t{}\t",
                encode_hex(relative_bytes),
                String::from_utf8_lossy(&key)
            )
            .into_bytes();
            if present_names.contains(&key) {
                let value = read_bounded_virtual_at(
                    directory.as_raw_fd(),
                    &key,
                    4096,
                    "cgroup constraint",
                )?;
                if value.contains(&0)
                    || value.contains(&b'\r')
                    || (!value.is_empty() && !value.ends_with(b"\n"))
                {
                    return Err(AppError::usage("cgroup constraint framing is malformed"));
                }
                record.extend_from_slice(encode_hex(&value).as_bytes());
            } else {
                record.extend_from_slice(b"absent");
            }
            records.push(record);
        }
        let after = directory_identity(directory.as_raw_fd(), "cgroup directory")?;
        if before != after {
            return Err(AppError::usage("cgroup directory identity changed"));
        }
    }
    let refs: Vec<&[u8]> = records.iter().map(Vec::as_slice).collect();
    encode_list(&refs)
}

#[cfg(target_os = "linux")]
fn validate_unified_cgroup_mount() -> AppResult<()> {
    let mountinfo =
        read_bounded_virtual_nofollow(Path::new("/proc/self/mountinfo"), 1024 * 1024, "mountinfo")?;
    let mut found = false;
    for line in mountinfo
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let separator = line
            .windows(3)
            .position(|window| window == b" - ")
            .ok_or_else(|| AppError::usage("malformed mountinfo row"))?;
        let left: Vec<&[u8]> = line[..separator].split(|byte| *byte == b' ').collect();
        let right: Vec<&[u8]> = line[separator + 3..].split(|byte| *byte == b' ').collect();
        if left.len() < 6 || right.len() < 3 {
            return Err(AppError::usage("malformed mountinfo field count"));
        }
        if right[0] != b"cgroup" && right[0] != b"cgroup2" {
            continue;
        }
        let root = decode_mountinfo_path(left[3])?;
        let mountpoint = decode_mountinfo_path(left[4])?;
        if right[0] != b"cgroup2" || root != b"/" || mountpoint != b"/sys/fs/cgroup" || found {
            return Err(AppError::usage(
                "legacy, hybrid, or aliased cgroup hierarchy",
            ));
        }
        found = true;
    }
    if !found {
        return Err(AppError::usage("cgroup v2 mount is unavailable"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    device: libc::dev_t,
    inode: libc::ino_t,
    mode: libc::mode_t,
}

fn directory_identity(descriptor: RawFd, label: &str) -> AppResult<DirectoryIdentity> {
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: descriptor is retained and stat is writable.
    if unsafe { libc::fstat(descriptor, stat.as_mut_ptr()) } == -1 {
        return Err(AppError::io(label, io::Error::last_os_error()));
    }
    // SAFETY: successful fstat initialized the record.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Err(AppError::usage(format!("{label} is not a directory")));
    }
    Ok(DirectoryIdentity {
        device: stat.st_dev,
        inode: stat.st_ino,
        mode: stat.st_mode,
    })
}

fn open_directory_nofollow(path: &Path, label: &str) -> AppResult<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY);
    let file = options
        .open(path)
        .map_err(|error| AppError::io(label, error))?;
    directory_identity(file.as_raw_fd(), label)?;
    Ok(file)
}

fn open_directory_at(parent: RawFd, name: &[u8], label: &str) -> AppResult<File> {
    let name =
        CString::new(name).map_err(|_| AppError::usage("directory component contains NUL"))?;
    // SAFETY: parent is retained and name is a NUL-terminated single component.
    let descriptor = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY,
        )
    };
    if descriptor == -1 {
        return Err(AppError::io(label, io::Error::last_os_error()));
    }
    // SAFETY: successful openat returned one owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    directory_identity(file.as_raw_fd(), label)?;
    Ok(file)
}

#[derive(Debug)]
struct DirectoryEntry {
    name: Vec<u8>,
    mode: libc::mode_t,
}

fn directory_entries(directory: RawFd, label: &str) -> AppResult<Vec<DirectoryEntry>> {
    // A duplicate would share the directory stream offset. Opening `.` from
    // the retained descriptor creates an independent enumeration cursor while
    // preserving the selected directory identity.
    // SAFETY: directory is retained and the fixed name is NUL-terminated.
    let duplicate = unsafe {
        libc::openat(
            directory,
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY,
        )
    };
    if duplicate == -1 {
        return Err(AppError::io(label, io::Error::last_os_error()));
    }
    // SAFETY: duplicate is a live owned directory descriptor transferred to DIR.
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        let error = io::Error::last_os_error();
        // SAFETY: fdopendir failed and retained no ownership of duplicate.
        let _ = unsafe { libc::close(duplicate) };
        return Err(AppError::io(label, error));
    }
    let mut entries = Vec::new();
    let result = loop {
        set_errno(0);
        // SAFETY: stream remains live and is used by this thread only.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let errno = get_errno();
            break if errno == 0 {
                Ok(entries)
            } else {
                Err(AppError::io(label, io::Error::from_raw_os_error(errno)))
            };
        }
        // SAFETY: readdir returned a live dirent with a NUL-terminated d_name.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        let name_c = match CString::new(name) {
            Ok(name) => name,
            Err(_) => break Err(AppError::operational(format!("{label} name contains NUL"))),
        };
        let mut stat = MaybeUninit::<libc::stat>::zeroed();
        // SAFETY: directory is retained, name is a single NUL-terminated entry,
        // and stat points to writable storage.
        if unsafe {
            libc::fstatat(
                directory,
                name_c.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == -1
        {
            break Err(AppError::io(label, io::Error::last_os_error()));
        }
        // SAFETY: successful fstatat initialized stat.
        entries.push(DirectoryEntry {
            name: name.to_vec(),
            mode: unsafe { stat.assume_init() }.st_mode,
        });
    };
    // SAFETY: stream is live and closed exactly once; closedir closes duplicate.
    if unsafe { libc::closedir(stream) } == -1 {
        return Err(AppError::io(label, io::Error::last_os_error()));
    }
    result
}

fn set_errno(value: libc::c_int) {
    #[cfg(target_os = "linux")]
    // SAFETY: errno location is thread-local writable storage.
    unsafe {
        *libc::__errno_location() = value;
    }
    #[cfg(target_os = "macos")]
    // SAFETY: errno location is thread-local writable storage.
    unsafe {
        *libc::__error() = value;
    }
}

fn get_errno() -> libc::c_int {
    #[cfg(target_os = "linux")]
    // SAFETY: errno location is readable thread-local storage.
    unsafe {
        *libc::__errno_location()
    }
    #[cfg(target_os = "macos")]
    // SAFETY: errno location is readable thread-local storage.
    unsafe {
        *libc::__error()
    }
}

fn open_bounded_regular_at(parent: RawFd, name: &[u8], limit: u64, label: &str) -> AppResult<File> {
    let name = CString::new(name).map_err(|_| AppError::usage("file name contains NUL"))?;
    // SAFETY: parent is retained and name is one NUL-terminated component.
    let descriptor = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if descriptor == -1 {
        return Err(AppError::io(label, io::Error::last_os_error()));
    }
    // SAFETY: successful openat returned one owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file
        .metadata()
        .map_err(|error| AppError::io(label, error))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(AppError::operational(format!(
            "{label} is not a bounded regular file"
        )));
    }
    Ok(file)
}

#[cfg(target_os = "linux")]
fn entry_is_symlink_at(parent: RawFd, name: &[u8]) -> AppResult<bool> {
    let name = CString::new(name).map_err(|_| AppError::usage("cgroup entry contains NUL"))?;
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: parent is retained, name is NUL-terminated, and stat is writable.
    if unsafe {
        libc::fstatat(
            parent,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == -1
    {
        return Err(AppError::io("cgroup entry", io::Error::last_os_error()));
    }
    // SAFETY: successful fstatat initialized the record.
    Ok(unsafe { stat.assume_init() }.st_mode & libc::S_IFMT == libc::S_IFLNK)
}

#[cfg(target_os = "linux")]
fn read_bounded_virtual_at(
    parent: RawFd,
    name: &[u8],
    limit: usize,
    label: &str,
) -> AppResult<Vec<u8>> {
    let name = CString::new(name).map_err(|_| AppError::usage("cgroup entry contains NUL"))?;
    // SAFETY: parent is retained and name is a NUL-terminated single component.
    let descriptor = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if descriptor == -1 {
        return Err(AppError::io(label, io::Error::last_os_error()));
    }
    // SAFETY: successful openat returned one owned descriptor.
    let mut file = unsafe { File::from_raw_fd(descriptor) };
    let before = file
        .metadata()
        .map_err(|error| AppError::io(label, error))?;
    if !before.is_file() {
        return Err(AppError::usage(format!(
            "{label} is not a regular kernel file"
        )));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io(label, error))?;
    if bytes.len() > limit {
        return Err(AppError::usage(format!("{label} exceeds its bound")));
    }
    let after = file
        .metadata()
        .map_err(|error| AppError::io(label, error))?;
    if before.dev() != after.dev() || before.ino() != after.ino() || before.mode() != after.mode() {
        return Err(AppError::usage(format!("{label} identity changed")));
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
const CGROUP_KEYS: &[&str] = &[
    "cgroup.type",
    "cgroup.controllers",
    "cgroup.subtree_control",
    "cgroup.max.depth",
    "cgroup.max.descendants",
    "cgroup.freeze",
    "cpu.idle",
    "cpu.max",
    "cpu.max.burst",
    "cpu.weight",
    "cpu.weight.nice",
    "cpu.uclamp.min",
    "cpu.uclamp.max",
    "cpuset.cpus",
    "cpuset.cpus.effective",
    "cpuset.cpus.exclusive",
    "cpuset.cpus.exclusive.effective",
    "cpuset.cpus.isolated",
    "cpuset.cpus.partition",
    "cpuset.mems",
    "cpuset.mems.effective",
    "io.latency",
    "io.max",
    "io.weight",
    "io.prio.class",
    "io.cost.model",
    "io.cost.qos",
    "memory.min",
    "memory.low",
    "memory.high",
    "memory.max",
    "memory.oom.group",
    "memory.swap.high",
    "memory.swap.max",
    "memory.zswap.max",
    "memory.zswap.writeback",
    "pids.max",
    "rdma.max",
    "misc.max",
    "dmem.min",
    "dmem.low",
    "dmem.max",
];

#[cfg(target_os = "linux")]
fn validate_cgroup_path(path: &[u8]) -> AppResult<()> {
    if path.first() != Some(&b'/')
        || (path.len() > 1 && path.ends_with(b"/"))
        || path.contains(&0)
        || path.contains(&b'\t')
        || path.contains(&b'\r')
        || path.contains(&b'\n')
    {
        return Err(AppError::usage("invalid cgroup path"));
    }
    if path.len() > 1
        && path[1..]
            .split(|byte| *byte == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b"..")
    {
        return Err(AppError::usage("invalid cgroup path component"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn join_components(components: &[&[u8]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            bytes.push(b'/');
        }
        bytes.extend_from_slice(component);
    }
    bytes
}

#[cfg(target_os = "linux")]
fn cgroup_namespace(name: &[u8]) -> bool {
    [
        b"cgroup.".as_slice(),
        b"cpu.",
        b"cpuset.",
        b"io.",
        b"memory.",
        b"pids.",
        b"rdma.",
        b"hugetlb.",
        b"misc.",
        b"dmem.",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

#[cfg(target_os = "linux")]
fn hugepage_constraint_size(name: &[u8]) -> AppResult<Option<Vec<u8>>> {
    let Some(tail) = name.strip_prefix(b"hugetlb.") else {
        return Ok(None);
    };
    let size = if let Some(size) = tail.strip_suffix(b".rsvd.max") {
        size
    } else if let Some(size) = tail.strip_suffix(b".max") {
        size
    } else {
        return Ok(None);
    };
    if size.is_empty() || !size.iter().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(AppError::usage("malformed hugepage size"));
    }
    Ok(Some(size.to_vec()))
}

#[cfg(target_os = "linux")]
fn is_volatile_cgroup_name(name: &[u8]) -> bool {
    const EXACT: &[&[u8]] = &[
        b"cgroup.procs",
        b"cgroup.threads",
        b"cgroup.kill",
        b"memory.reclaim",
    ];
    const SUFFIXES: &[&[u8]] = &[
        b".current",
        b".events",
        b".events.local",
        b".peak",
        b".pressure",
        b".stat",
        b".stat.local",
        b".numa_stat",
    ];
    EXACT.contains(&name) || SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

#[cfg(target_os = "linux")]
fn parse_cpu_list(bytes: &[u8]) -> AppResult<Vec<u32>> {
    if bytes.is_empty() {
        return Err(AppError::usage("empty CPU list"));
    }
    let mut cpus = Vec::new();
    for part in bytes.split(|byte| *byte == b',') {
        if let Some((start, end)) = split_once_bytes(part, b"-") {
            let start = u32::try_from(parse_u64(start)?)
                .map_err(|_| AppError::usage("CPU index overflow"))?;
            let end = u32::try_from(parse_u64(end)?)
                .map_err(|_| AppError::usage("CPU index overflow"))?;
            if start >= end {
                return Err(AppError::usage("noncanonical CPU range"));
            }
            cpus.extend(start..=end);
        } else {
            cpus.push(
                u32::try_from(parse_u64(part)?)
                    .map_err(|_| AppError::usage("CPU index overflow"))?,
            );
        }
    }
    if cpus.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(AppError::usage("CPU list is not strictly increasing"));
    }
    Ok(cpus)
}

#[cfg(target_os = "linux")]
fn format_cpu_list(cpus: &[u32]) -> String {
    cpus.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn append_rlimits(output: &mut Vec<u8>) -> AppResult<()> {
    for (name, resource) in [
        ("cpu", libc::RLIMIT_CPU),
        ("fsize", libc::RLIMIT_FSIZE),
        ("data", libc::RLIMIT_DATA),
        ("stack", libc::RLIMIT_STACK),
        ("core", libc::RLIMIT_CORE),
        ("rss", libc::RLIMIT_RSS),
        ("memlock", libc::RLIMIT_MEMLOCK),
        ("nproc", libc::RLIMIT_NPROC),
        ("nofile", libc::RLIMIT_NOFILE),
        ("as", libc::RLIMIT_AS),
    ] {
        let mut limit = MaybeUninit::<libc::rlimit>::zeroed();
        // SAFETY: limit is writable for the named fixed resource.
        if unsafe { libc::getrlimit(resource, limit.as_mut_ptr()) } == -1 {
            return Err(AppError::usage(format!("rlimit {name} unavailable")));
        }
        // SAFETY: successful getrlimit initialized the record.
        let limit = unsafe { limit.assume_init() };
        output.extend_from_slice(
            format!(
                "rlimit\t{name}\t{}\t{}\n",
                format_limit(limit.rlim_cur),
                format_limit(limit.rlim_max)
            )
            .as_bytes(),
        );
    }
    Ok(())
}

fn format_limit(value: libc::rlim_t) -> String {
    if value == libc::RLIM_INFINITY {
        "infinity".to_owned()
    } else {
        value.to_string()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SampleOutcome {
    Ok,
    FormationError,
    SpawnError,
    CleanupError,
    Timeout,
    Signal,
    Exit,
    OutputOverflow,
    MetricError,
    StdoutMismatch,
    StderrMismatch,
}

impl SampleOutcome {
    fn token(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::FormationError => "formation-error",
            Self::SpawnError => "spawn-error",
            Self::CleanupError => "cleanup-error",
            Self::Timeout => "timeout",
            Self::Signal => "signal",
            Self::Exit => "exit",
            Self::OutputOverflow => "output-overflow",
            Self::MetricError => "metric-error",
            Self::StdoutMismatch => "stdout-mismatch",
            Self::StderrMismatch => "stderr-mismatch",
        }
    }
}

#[derive(Clone, Debug)]
struct SuccessfulMetrics {
    wall_ns: u64,
    user_ns: u64,
    system_ns: u64,
    minor_faults: u64,
    major_faults: u64,
    voluntary_switches: u64,
    involuntary_switches: u64,
    peak_rss_bytes: u64,
}

#[allow(clippy::too_many_arguments)]
fn measure_all(
    watchdog: &Watchdog,
    observer: &File,
    provenance: &Provenance,
    work: &Path,
    expected_execution_state: &[u8],
    artifacts: &[Artifact],
    results: &mut ResultWriter,
) -> AppResult<()> {
    let cwd = work.join("observer/cwd");
    env::set_current_dir(&cwd).map_err(|error| AppError::io("observer cwd", error))?;
    let environment = timed_environment();
    let arm_count = provenance.arms.len();
    let fixture_schedule = schedule(arm_count)?;
    let mut summaries: Vec<(Fixture, usize, bool, Vec<SuccessfulMetrics>)> = Vec::new();
    let mut failed_fixtures = Vec::new();
    let mut completed = 0u32;
    for (fixture_index, fixture) in FIXTURES.iter().copied().enumerate() {
        let fixture_artifacts =
            &artifacts[fixture_index * arm_count..(fixture_index + 1) * arm_count];
        let mut arm_metrics = vec![Vec::new(); arm_count];
        let mut fixture_failed = false;
        for scheduled in &fixture_schedule {
            if require_empty_directory(&cwd, "timed child cwd").is_err() {
                results.row(&unstarted_sample_row(
                    fixture,
                    *scheduled,
                    &provenance.arms,
                    SampleOutcome::FormationError,
                    b"cwd-check",
                )?)?;
                fixture_failed = true;
                break;
            }
            let state_matches =
                execution_state_record().is_ok_and(|state| state == expected_execution_state);
            if !state_matches {
                results.row(&unstarted_sample_row(
                    fixture,
                    *scheduled,
                    &provenance.arms,
                    SampleOutcome::FormationError,
                    b"execution-state",
                )?)?;
                fixture_failed = true;
                break;
            }
            let artifact = &fixture_artifacts[scheduled.arm];
            if let Err(error) = revalidate_artifacts(std::slice::from_ref(artifact)) {
                results.row(&unstarted_sample_row_with_details(
                    fixture,
                    *scheduled,
                    &provenance.arms,
                    SampleOutcome::FormationError,
                    None,
                    "spawn-actions",
                    Some(error.message.as_bytes()),
                )?)?;
                fixture_failed = true;
                break;
            }
            let argv: Vec<OsString> = std::iter::once(artifact.executable.as_os_str().to_owned())
                .chain(
                    fixture
                        .argv
                        .iter()
                        .map(|value| OsString::from_vec(value.to_vec())),
                )
                .collect();
            let stdout_relative = format!(
                "logs/{}/{}-{}-{}-stdout.bin",
                fixture.id,
                scheduled.phase.token(),
                scheduled.sequence,
                provenance.arms[scheduled.arm].name
            );
            let stderr_relative = format!(
                "logs/{}/{}-{}-{}-stderr.bin",
                fixture.id,
                scheduled.phase.token(),
                scheduled.sequence,
                provenance.arms[scheduled.arm].name
            );
            let stdout_log = work.join(&stdout_relative);
            let stderr_log = work.join(&stderr_relative);
            let timed_capture = TimedCapture {
                expected_stdout: fixture.stdout,
                expected_stderr: fixture.stderr,
                stdout_log: &stdout_log,
                stderr_log: &stderr_log,
                stdout_relative: stdout_relative.as_bytes(),
                stderr_relative: stderr_relative.as_bytes(),
            };
            let child = run_child(
                watchdog,
                observer,
                &ChildSpec {
                    executable: &artifact.executable,
                    argv: &argv,
                    environment: &environment,
                    cwd: &cwd,
                    capture: CaptureKind::Timed,
                    execution_ns: TIMED_EXECUTION_NS,
                    cleanup_ns: TIMED_CLEANUP_NS,
                    tool: None,
                    timed_capture: Some(&timed_capture),
                },
            );
            match child {
                Ok(child) => {
                    let (row, outcome, metrics) =
                        completed_sample_row(fixture, *scheduled, &provenance.arms, &child)?;
                    results.row(&row)?;
                    if scheduled.phase == Phase::Measure && outcome == SampleOutcome::Ok {
                        arm_metrics[scheduled.arm].push(metrics.expect("ok has complete metrics"));
                    }
                    if outcome != SampleOutcome::Ok {
                        fixture_failed = true;
                        break;
                    }
                }
                Err(error) => {
                    let failure = classify_child_failure(&error);
                    results.row(&unstarted_sample_row_with_details(
                        fixture,
                        *scheduled,
                        &provenance.arms,
                        failure.outcome,
                        failure.wall_ns,
                        failure.operation,
                        failure.os_error.as_deref(),
                    )?)?;
                    fixture_failed = true;
                    break;
                }
            }
        }
        if fixture_failed {
            failed_fixtures.push(fixture.id.as_bytes());
        } else {
            completed += 1;
        }
        for (arm, metrics) in arm_metrics.into_iter().enumerate() {
            summaries.push((
                fixture,
                arm,
                !fixture_failed && metrics.len() == MEASURED as usize,
                metrics,
            ));
        }
    }
    for (fixture, arm, complete, metrics) in &mut summaries {
        results.row(&summary_row(
            *fixture,
            &provenance.arms[*arm],
            *complete,
            metrics,
        )?)?;
    }
    let failed: Vec<&[u8]> = failed_fixtures.to_vec();
    results.row(&format!(
        "end\t{}\t{}\t{}\n",
        if failed.is_empty() { "ok" } else { "failed" },
        completed,
        encode_list(&failed)?
    ))?;
    if failed.is_empty() {
        Ok(())
    } else {
        Err(AppError::operational("one or more startup fixtures failed"))
    }
}

fn unstarted_sample_row(
    fixture: Fixture,
    sample: ScheduledSample,
    arms: &[Arm],
    outcome: SampleOutcome,
    detail: &[u8],
) -> AppResult<String> {
    let operation = std::str::from_utf8(detail).unwrap_or("formation-error");
    unstarted_sample_row_with_details(fixture, sample, arms, outcome, None, operation, None)
}

struct ChildFailureRecord {
    outcome: SampleOutcome,
    wall_ns: Option<u64>,
    operation: &'static str,
    os_error: Option<Vec<u8>>,
}

fn classify_child_failure(error: &AppError) -> ChildFailureRecord {
    if let Some(rest) = error.message.strip_prefix("formation-cleanup-error:") {
        let (operation, os_error) = rest.split_once(':').unwrap_or(("spawn-actions", rest));
        return ChildFailureRecord {
            outcome: SampleOutcome::CleanupError,
            wall_ns: None,
            operation: stable_formation_operation(operation),
            os_error: Some(os_error.as_bytes().to_vec()),
        };
    }
    if let Some(rest) = error.message.strip_prefix("spawn-cleanup-error:") {
        let (body, wall) = rest.rsplit_once(":wall_ns=").unwrap_or((rest, "-"));
        let (operation, os_error) = body.split_once(':').unwrap_or(("close-stdout", body));
        return ChildFailureRecord {
            outcome: SampleOutcome::CleanupError,
            wall_ns: wall.parse().ok(),
            operation: stable_formation_operation(operation),
            os_error: Some(os_error.as_bytes().to_vec()),
        };
    }
    if let Some(rest) = error.message.strip_prefix("spawn-error:") {
        let (code, wall) = rest.split_once(":wall_ns=").unwrap_or((rest, "-"));
        return ChildFailureRecord {
            outcome: SampleOutcome::SpawnError,
            wall_ns: wall.parse().ok(),
            operation: "spawn",
            os_error: Some(code.as_bytes().to_vec()),
        };
    }
    let prefix = error.message.split(':').next().unwrap_or("");
    let operation = match prefix {
        "pipe-stdout" => "pipe-stdout",
        "pipe-stderr" => "pipe-stderr",
        "devnull" => "devnull",
        "spawn-actions" => "spawn-actions",
        "spawn-attributes" => "spawn-attributes",
        "clock-start"
        | "global execution cutoff reached"
        | "execution deadline overflow"
        | "terminal deadline overflow" => "clock-start",
        "close-stdout" => "close-stdout",
        "close-stderr" => "close-stderr",
        "group-check" => "group-check",
        "group-kill" => "group-kill",
        "direct-kill" => "direct-kill",
        "waitid" => "waitid",
        _ => "spawn-actions",
    };
    let formation = matches!(
        operation,
        "pipe-stdout"
            | "pipe-stderr"
            | "devnull"
            | "spawn-actions"
            | "spawn-attributes"
            | "clock-start"
    );
    ChildFailureRecord {
        outcome: if formation {
            SampleOutcome::FormationError
        } else {
            SampleOutcome::CleanupError
        },
        wall_ns: None,
        operation,
        os_error: Some(error.message.as_bytes().to_vec()),
    }
}

fn stable_formation_operation(operation: &str) -> &'static str {
    match operation {
        "close-stdout" => "close-stdout",
        "close-stderr" => "close-stderr",
        "devnull" => "devnull",
        _ => "spawn-actions",
    }
}

#[allow(clippy::too_many_arguments)]
fn unstarted_sample_row_with_details(
    fixture: Fixture,
    sample: ScheduledSample,
    arms: &[Arm],
    outcome: SampleOutcome,
    wall_ns: Option<u64>,
    operation: &str,
    os_error: Option<&[u8]>,
) -> AppResult<String> {
    let operation = [b"operation\t".as_slice(), operation.as_bytes()].concat();
    let mut details = vec![operation];
    if let Some(error) = os_error {
        details.push([b"os-error\t".as_slice(), error].concat());
    }
    let detail_refs: Vec<&[u8]> = details.iter().map(Vec::as_slice).collect();
    Ok(format!(
        "sample\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t-\t-\t-\t-\t-\t-\t-\tnone\t-\t-\t-\t-\t-\t{}\n",
        fixture.id,
        sample.phase.token(),
        sample.sequence,
        arms[sample.arm].name,
        sample.arm_index,
        sample.position,
        outcome.token(),
        optional_u64(wall_ns),
        encode_list(&detail_refs)?,
    ))
}

fn completed_sample_row(
    fixture: Fixture,
    sample: ScheduledSample,
    arms: &[Arm],
    child: &ChildResult,
) -> AppResult<(String, SampleOutcome, Option<SuccessfulMetrics>)> {
    let metrics = child
        .wall_ns
        .zip(child.usage.user_ns)
        .zip(child.usage.system_ns)
        .zip(child.usage.minor_faults)
        .zip(child.usage.major_faults)
        .zip(child.usage.voluntary_switches)
        .zip(child.usage.involuntary_switches)
        .zip(child.usage.peak_rss_bytes)
        .map(
            |(
                (
                    (
                        ((((wall_ns, user_ns), system_ns), minor_faults), major_faults),
                        voluntary_switches,
                    ),
                    involuntary_switches,
                ),
                peak_rss_bytes,
            )| SuccessfulMetrics {
                wall_ns,
                user_ns,
                system_ns,
                minor_faults,
                major_faults,
                voluntary_switches,
                involuntary_switches,
                peak_rss_bytes,
            },
        );
    let stdout_matches = child.stdout_complete.then_some(
        !child.stdout_overflow
            && child.stdout_len == u32::try_from(fixture.stdout.len()).ok()
            && child.stdout == fixture.stdout,
    );
    let stderr_matches = child.stderr_complete.then_some(
        !child.stderr_overflow
            && child.stderr_len == u32::try_from(fixture.stderr.len()).ok()
            && child.stderr == fixture.stderr,
    );
    let cleanup_issue = child.cleanup_issue.clone();
    let mut details = Vec::new();
    if let Some(path) = &child.stdout_path {
        details.push([b"stdout-path\t".as_slice(), path].concat());
    }
    if let Some(path) = &child.stderr_path {
        details.push([b"stderr-path\t".as_slice(), path].concat());
    }
    let outcome = if cleanup_issue.is_some() {
        SampleOutcome::CleanupError
    } else if child.timed_out {
        SampleOutcome::Timeout
    } else if matches!(child.exit, ChildExit::Signal(_)) {
        SampleOutcome::Signal
    } else if child.exit != ChildExit::Code(0) {
        SampleOutcome::Exit
    } else if child.stdout_overflow || child.stderr_overflow {
        SampleOutcome::OutputOverflow
    } else if metrics.is_none() {
        SampleOutcome::MetricError
    } else if stdout_matches == Some(false) {
        SampleOutcome::StdoutMismatch
    } else if stderr_matches == Some(false) {
        SampleOutcome::StderrMismatch
    } else {
        SampleOutcome::Ok
    };
    if let Some(issue) = cleanup_issue {
        details.insert(
            0,
            [b"operation\t".as_slice(), issue.operation.as_bytes()].concat(),
        );
        if let Some(error) = issue.os_error {
            details.insert(1, [b"os-error\t".as_slice(), &error].concat());
        }
    } else if outcome != SampleOutcome::Ok {
        details.push(format!("operation\t{}", outcome.token()).into_bytes());
    }
    let detail_refs: Vec<&[u8]> = details.iter().map(Vec::as_slice).collect();
    let (exit_kind, exit_value) = match child.exit {
        ChildExit::Code(value) => ("code", value.to_string()),
        ChildExit::Signal(value) => ("signal", value.to_string()),
    };
    let row = format!(
        "sample\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        fixture.id,
        sample.phase.token(),
        sample.sequence,
        arms[sample.arm].name,
        sample.arm_index,
        sample.position,
        outcome.token(),
        optional_u64(child.wall_ns),
        optional_u64(child.usage.user_ns),
        optional_u64(child.usage.system_ns),
        optional_u64(child.usage.minor_faults),
        optional_u64(child.usage.major_faults),
        optional_u64(child.usage.voluntary_switches),
        optional_u64(child.usage.involuntary_switches),
        optional_u64(child.usage.peak_rss_bytes),
        exit_kind,
        exit_value,
        optional_match(stdout_matches),
        optional_match(stderr_matches),
        optional_u32(child.stdout_len),
        optional_u32(child.stderr_len),
        encode_list(&detail_refs)?,
    );
    Ok((row, outcome, metrics))
}

fn optional_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

fn optional_u32(value: Option<u32>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

fn optional_match(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "-",
    }
}

fn summary_row(
    fixture: Fixture,
    arm: &Arm,
    complete: bool,
    metrics: &mut [SuccessfulMetrics],
) -> AppResult<String> {
    let count = metrics.len();
    if !complete {
        return Ok(format!(
            "summary\t{}\t{}\tfailed\t{}\t-\t-\t-\t-\t-\t-\t-\t-\tsample-failed\n",
            fixture.id, arm.name, count
        ));
    }
    macro_rules! med {
        ($field:ident) => {{
            let mut values: Vec<u64> = metrics.iter().map(|sample| sample.$field).collect();
            median(&mut values)?
        }};
    }
    Ok(format!(
        "summary\t{}\t{}\tcomplete\t20\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\tnone\n",
        fixture.id,
        arm.name,
        med!(wall_ns),
        med!(user_ns),
        med!(system_ns),
        med!(minor_faults),
        med!(major_faults),
        med!(voluntary_switches),
        med!(involuntary_switches),
        med!(peak_rss_bytes)
    ))
}

fn validate_cache(
    cache: &File,
    provenance: &Provenance,
    arm: &Arm,
    compiler_bytes: &[u8],
) -> AppResult<CodegenKey> {
    let root_before = directory_identity(cache.as_raw_fd(), "cache root")?;
    require_exact_directories(cache.as_raw_fd(), &[b"actions", b"cas", b"index"])?;
    let actions = open_directory_at(cache.as_raw_fd(), b"actions", "cache actions")?;
    let index = open_directory_at(cache.as_raw_fd(), b"index", "cache index")?;
    let cas = open_directory_at(cache.as_raw_fd(), b"cas", "cache CAS")?;
    let actions_before = directory_identity(actions.as_raw_fd(), "cache actions")?;
    let index_before = directory_identity(index.as_raw_fd(), "cache index")?;
    let cas_before = directory_identity(cas.as_raw_fd(), "cache CAS")?;
    require_exact_directories(actions.as_raw_fd(), &[b"codegen", b"unit"])?;
    require_exact_directories(index.as_raw_fd(), &[b"codegen", b"unit"])?;
    let action_unit = open_directory_at(actions.as_raw_fd(), b"unit", "cache action unit")?;
    let index_unit = open_directory_at(index.as_raw_fd(), b"unit", "cache index unit")?;
    let action_codegen =
        open_directory_at(actions.as_raw_fd(), b"codegen", "cache action codegen")?;
    let index_codegen = open_directory_at(index.as_raw_fd(), b"codegen", "cache index codegen")?;
    let (_unit_action_name, unit_action) = one_regular_file_at(action_unit.as_raw_fd(), FILE_MAX)?;
    let (_unit_index_name, unit_index) = one_regular_file_at(index_unit.as_raw_fd(), FILE_MAX)?;
    if unit_action != unit_index {
        return Err(AppError::operational(
            "unit action and slot manifests differ",
        ));
    }
    let (action_name, action_bytes) = one_regular_file_at(action_codegen.as_raw_fd(), FILE_MAX)?;
    let (slot_name, slot_bytes) = one_regular_file_at(index_codegen.as_raw_fd(), FILE_MAX)?;
    if action_bytes != slot_bytes {
        return Err(AppError::operational(
            "codegen action and slot manifests differ",
        ));
    }
    let key = decode_codegen_manifest(&action_bytes)?;
    validate_codegen_key(&key, provenance, arm, compiler_bytes)?;
    let action_expected = Hash128::of(&key.wire).to_hex();
    let slot_expected = slot_digest(&key)?.to_hex();
    if action_name.as_bytes() != action_expected.as_bytes()
        || slot_name.as_bytes() != slot_expected.as_bytes()
    {
        return Err(AppError::operational(
            "codegen cache filename does not authenticate its key",
        ));
    }
    let hash = key.blob_digest.to_hex();
    require_exact_directories(cas.as_raw_fd(), &[&hash.as_bytes()[..2]])?;
    let shard = open_directory_at(cas.as_raw_fd(), &hash.as_bytes()[..2], "cache CAS shard")?;
    require_exact_regular_files(shard.as_raw_fd(), &[hash.as_bytes()])?;
    let mut object =
        open_bounded_regular_at(shard.as_raw_fd(), hash.as_bytes(), FILE_MAX, "cache object")?;
    let observed = hash128_stream(&mut object)?;
    if observed != key.blob_digest {
        return Err(AppError::operational("cache object digest mismatch"));
    }
    if directory_identity(actions.as_raw_fd(), "cache actions")? != actions_before
        || directory_identity(index.as_raw_fd(), "cache index")? != index_before
        || directory_identity(cas.as_raw_fd(), "cache CAS")? != cas_before
        || directory_identity(cache.as_raw_fd(), "cache root")? != root_before
    {
        return Err(AppError::operational(
            "cache directory identity changed during validation",
        ));
    }
    Ok(key)
}

fn require_exact_directories(directory: RawFd, expected: &[&[u8]]) -> AppResult<()> {
    require_exact_entries(directory, expected, libc::S_IFDIR)
}

fn require_exact_regular_files(directory: RawFd, expected: &[&[u8]]) -> AppResult<()> {
    require_exact_entries(directory, expected, libc::S_IFREG)
}

fn require_exact_entries(
    directory: RawFd,
    expected: &[&[u8]],
    expected_kind: libc::mode_t,
) -> AppResult<()> {
    let mut entries = directory_entries(directory, "cache enumeration")?;
    if entries
        .iter()
        .any(|entry| entry.mode & libc::S_IFMT != expected_kind)
    {
        return Err(AppError::operational(
            "cache contains an entry with the wrong kind",
        ));
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let observed: Vec<&[u8]> = entries.iter().map(|entry| entry.name.as_slice()).collect();
    let mut expected = expected.to_vec();
    expected.sort();
    if observed != expected {
        return Err(AppError::operational("cache tree topology mismatch"));
    }
    Ok(())
}

fn one_regular_file_at(directory: RawFd, limit: u64) -> AppResult<(OsString, Vec<u8>)> {
    let entries = directory_entries(directory, "cache enumeration")?;
    if entries.len() != 1 {
        return Err(AppError::operational(
            "cache directory does not contain exactly one entry",
        ));
    }
    let entry = &entries[0];
    if entry.mode & libc::S_IFMT != libc::S_IFREG {
        return Err(AppError::operational(
            "cache manifest is not a regular file",
        ));
    }
    if entry.name.len() != 32
        || !entry
            .name
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(AppError::operational(
            "cache filename is not a canonical Hash128",
        ));
    }
    let mut file = open_bounded_regular_at(directory, &entry.name, limit, "cache manifest")?;
    let before = file
        .metadata()
        .map_err(|error| AppError::io("cache manifest", error))?;
    let mut bytes = Vec::with_capacity(before.len() as usize);
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io("cache manifest", error))?;
    let after = file
        .metadata()
        .map_err(|error| AppError::io("cache manifest", error))?;
    if bytes.len() as u64 != before.len()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.mode() != after.mode()
        || before.len() != after.len()
    {
        return Err(AppError::operational(
            "cache manifest changed during validation",
        ));
    }
    Ok((OsString::from_vec(entry.name.clone()), bytes))
}

fn hash128_stream(file: &mut File) -> AppResult<Hash128> {
    let before = file
        .metadata()
        .map_err(|error| AppError::io("cache object metadata", error))?;
    let length = usize::try_from(before.len())
        .map_err(|_| AppError::operational("cache object size overflow"))?;
    let mut low = align_hash::WyHashStream::for_len(0x9E37_79B9_7F4A_7C15, length);
    let mut high = align_hash::WyHashStream::for_len(0xC2B2_AE3D_27D4_EB4F, length);
    file.seek(SeekFrom::Start(0))
        .map_err(|error| AppError::io("cache object seek", error))?;
    let mut buffer = [0u8; 64 * 1024];
    let mut consumed = 0usize;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| AppError::io("cache object read", error))?;
        if count == 0 {
            break;
        }
        consumed = consumed
            .checked_add(count)
            .ok_or_else(|| AppError::operational("cache object size overflow"))?;
        if !low.update(&buffer[..count]) || !high.update(&buffer[..count]) {
            return Err(AppError::operational("cache object grew during hashing"));
        }
    }
    let after = file
        .metadata()
        .map_err(|error| AppError::io("cache object metadata", error))?;
    if consumed != length
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.mode() != after.mode()
        || before.len() != after.len()
    {
        return Err(AppError::operational("cache object changed during hashing"));
    }
    Ok(Hash128 {
        lo: low
            .finish()
            .ok_or_else(|| AppError::operational("cache object length mismatch"))?,
        hi: high
            .finish()
            .ok_or_else(|| AppError::operational("cache object length mismatch"))?,
    })
}

impl Provenance {
    fn read(path: &Path) -> AppResult<Self> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let mut file = options
            .open(path)
            .map_err(|error| AppError::usage(format!("provenance: {error}")))?;
        let metadata = file
            .metadata()
            .map_err(|error| AppError::usage(format!("provenance: {error}")))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > PROVENANCE_MAX {
            return Err(AppError::usage(
                "provenance is not a bounded nonempty regular file",
            ));
        }
        let mut raw = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
        Read::by_ref(&mut file)
            .take(PROVENANCE_MAX + 1)
            .read_to_end(&mut raw)
            .map_err(|error| AppError::usage(format!("provenance: {error}")))?;
        let after = file
            .metadata()
            .map_err(|error| AppError::usage(format!("provenance: {error}")))?;
        if raw.len() as u64 != metadata.len()
            || metadata.dev() != after.dev()
            || metadata.ino() != after.ino()
            || metadata.mode() != after.mode()
            || metadata.len() != after.len()
        {
            return Err(AppError::usage("provenance changed while read"));
        }
        if raw.starts_with(&[0xef, 0xbb, 0xbf]) || raw.contains(&b'\r') || !raw.ends_with(b"\n") {
            return Err(AppError::usage("provenance framing is not canonical"));
        }
        let digest = Sha256::digest(&raw).into();
        let lines: Vec<&[u8]> = raw[..raw.len() - 1].split(|byte| *byte == b'\n').collect();
        let mut cursor = ProvenanceCursor::new(lines);
        cursor.header(b"align-startup-provenance-v1")?;
        let fixture_revision = cursor.hex40("fixture_revision")?;
        let host_os = cursor.hex("host_os_hex", true)?;
        let host_kernel = cursor.hex("host_kernel_hex", true)?;
        let host_arch = cursor.hex("host_arch_hex", true)?;
        let cpu_identity = cursor.hex("cpu_identity_hex", true)?;
        let logical_cpu_count = cursor.u32_nonzero("logical_cpu_count")?;
        let memory_bytes = cursor.u64_nonzero("memory_bytes")?;
        let power_condition = cursor.annotation("power_condition_hex")?;
        let load_condition = cursor.annotation("load_condition_hex")?;
        let target_triple = cursor.hex("target_triple_hex", false)?;
        cursor.literal("target_cpu", b"baseline")?;
        cursor.literal("profile", b"release")?;
        cursor.literal("linker", b"system")?;
        let libc_identity = cursor.hex("libc_identity_hex", false)?;
        cursor.literal("libc_link", b"dynamic")?;
        cursor.literal("strip_policy", b"unstripped")?;
        cursor.literal("runtime_lto", b"on")?;
        let build_path = cursor.path_list("build_path_list_hex")?;
        let cc_path = cursor.path("cc_path_hex")?;
        let cc_sha256 = cursor.sha256("cc_sha256")?;
        let cc_version = cursor.hex("cc_version_hex", true)?;
        let ld_path = cursor.path("ld_path_hex")?;
        let ld_sha256 = cursor.sha256("ld_sha256")?;
        let ld_version = cursor.hex("ld_version_hex", true)?;
        let inspector_path = cursor.path("object_inspector_path_hex")?;
        let inspector_sha256 = cursor.sha256("object_inspector_sha256")?;
        let inspector_version = cursor.hex("object_inspector_version_hex", true)?;
        let arm_count = cursor.enum_u32("arm_count", &[1, 2])?;
        let mut arms = Vec::with_capacity(arm_count as usize);
        arms.push(cursor.arm("baseline")?);
        if arm_count == 2 {
            arms.push(cursor.arm("candidate")?);
        }
        cursor.end()?;
        Ok(Self {
            raw,
            digest,
            fixture_revision,
            host_os,
            host_kernel,
            host_arch,
            cpu_identity,
            logical_cpu_count,
            memory_bytes,
            power_condition,
            load_condition,
            target_triple,
            libc_identity,
            build_path,
            cc_path,
            cc_sha256,
            cc_version,
            ld_path,
            ld_sha256,
            ld_version,
            inspector_path,
            inspector_sha256,
            inspector_version,
            arms,
        })
    }
}

struct ProvenanceCursor<'a> {
    lines: Vec<&'a [u8]>,
    index: usize,
}

impl<'a> ProvenanceCursor<'a> {
    fn new(lines: Vec<&'a [u8]>) -> Self {
        Self { lines, index: 0 }
    }

    fn take(&mut self) -> AppResult<&'a [u8]> {
        let line = self
            .lines
            .get(self.index)
            .copied()
            .ok_or_else(|| AppError::usage("truncated provenance"))?;
        self.index += 1;
        if line.contains(&0) {
            return Err(AppError::usage("provenance contains NUL"));
        }
        Ok(line)
    }

    fn fields(&mut self, key: &str, count: usize) -> AppResult<Vec<&'a [u8]>> {
        let line = self.take()?;
        let fields: Vec<&[u8]> = line.split(|byte| *byte == b'\t').collect();
        if fields.len() != count || fields[0] != key.as_bytes() {
            return Err(AppError::usage(format!("expected provenance row {key}")));
        }
        Ok(fields)
    }

    fn header(&mut self, expected: &[u8]) -> AppResult<()> {
        if self.take()? != expected {
            return Err(AppError::usage("invalid provenance header"));
        }
        Ok(())
    }

    fn literal(&mut self, key: &str, value: &[u8]) -> AppResult<()> {
        let fields = self.fields(key, 2)?;
        if fields[1] != value {
            return Err(AppError::usage(format!(
                "invalid provenance value for {key}"
            )));
        }
        Ok(())
    }

    fn hex(&mut self, key: &str, allow_empty: bool) -> AppResult<Vec<u8>> {
        let fields = self.fields(key, 2)?;
        decode_hex(fields[1], allow_empty)
    }

    fn sha256(&mut self, key: &str) -> AppResult<[u8; 32]> {
        let fields = self.fields(key, 2)?;
        let text = std::str::from_utf8(fields[1])
            .map_err(|_| AppError::usage(format!("invalid {key}")))?;
        parse_sha256(text)
    }

    fn hex40(&mut self, key: &str) -> AppResult<String> {
        let fields = self.fields(key, 2)?;
        if fields[1].len() != 40
            || !fields[1]
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return Err(AppError::usage(format!("invalid {key}")));
        }
        Ok(String::from_utf8(fields[1].to_vec()).expect("ASCII validated"))
    }

    fn u32_nonzero(&mut self, key: &str) -> AppResult<u32> {
        let fields = self.fields(key, 2)?;
        let value = parse_u64(fields[1])?;
        u32::try_from(value)
            .ok()
            .filter(|value| *value != 0)
            .ok_or_else(|| AppError::usage(format!("invalid {key}")))
    }

    fn u64_nonzero(&mut self, key: &str) -> AppResult<u64> {
        let fields = self.fields(key, 2)?;
        parse_u64(fields[1])?
            .checked_sub(1)
            .map(|value| value + 1)
            .ok_or_else(|| AppError::usage(format!("invalid {key}")))
    }

    fn enum_u32(&mut self, key: &str, admitted: &[u32]) -> AppResult<u32> {
        let fields = self.fields(key, 2)?;
        let value = u32::try_from(parse_u64(fields[1])?)
            .map_err(|_| AppError::usage(format!("invalid {key}")))?;
        admitted
            .contains(&value)
            .then_some(value)
            .ok_or_else(|| AppError::usage(format!("invalid {key}")))
    }

    fn annotation(&mut self, key: &str) -> AppResult<Vec<u8>> {
        let value = self.hex(key, false)?;
        let text =
            std::str::from_utf8(&value).map_err(|_| AppError::usage(format!("invalid {key}")))?;
        if value.len() > 256
            || value.iter().any(|byte| byte.is_ascii_control())
            || !(text
                .strip_prefix("observed:")
                .is_some_and(|tail| !tail.is_empty())
                || text
                    .strip_prefix("unavailable:")
                    .is_some_and(|tail| !tail.is_empty()))
        {
            return Err(AppError::usage(format!("invalid {key}")));
        }
        Ok(value)
    }

    fn path(&mut self, key: &str) -> AppResult<PathBuf> {
        let bytes = self.hex(key, false)?;
        decoded_absolute_path(bytes, key)
    }

    fn path_list(&mut self, key: &str) -> AppResult<Vec<PathBuf>> {
        let encoded = self.fields(key, 2)?;
        let bytes = decode_hex(encoded[1], false)?;
        let elements = decode_list(&bytes)?;
        if elements.is_empty() {
            return Err(AppError::usage(format!("empty {key}")));
        }
        let mut seen = BTreeSet::new();
        let mut paths = Vec::with_capacity(elements.len());
        for element in elements {
            if element.contains(&b':') {
                return Err(AppError::usage(format!("invalid {key}")));
            }
            let path = decoded_absolute_path(element, key)?;
            if !seen.insert(path.clone()) {
                return Err(AppError::usage(format!("duplicate {key} element")));
            }
            paths.push(path);
        }
        Ok(paths)
    }

    fn arm(&mut self, expected_name: &'static str) -> AppResult<Arm> {
        let fields = self.fields("arm", 8)?;
        if fields[1] != expected_name.as_bytes() {
            return Err(AppError::usage(format!("expected {expected_name} arm")));
        }
        let source_revision = parse_hex40_bytes(fields[2], "arm source revision")?;
        let compiler = decoded_absolute_path(decode_hex(fields[3], false)?, "compiler path")?;
        let compiler_sha256 = parse_sha256_bytes(fields[4], "compiler digest")?;
        let runtime = decoded_absolute_path(decode_hex(fields[5], false)?, "runtime path")?;
        let runtime_sha256 = parse_sha256_bytes(fields[6], "runtime digest")?;
        let compiler_version = decode_hex(fields[7], false)?;
        if compiler_version.contains(&0)
            || !compiler_version.ends_with(b"\n")
            || compiler_version[..compiler_version.len() - 1].contains(&b'\n')
        {
            return Err(AppError::usage(
                "compiler version is not one LF-terminated line",
            ));
        }
        Ok(Arm {
            name: expected_name,
            source_revision,
            compiler,
            compiler_sha256,
            runtime,
            runtime_sha256,
            compiler_version,
        })
    }

    fn end(&self) -> AppResult<()> {
        if self.index != self.lines.len() {
            return Err(AppError::usage("extra provenance rows"));
        }
        Ok(())
    }
}

fn parse_hex40_bytes(bytes: &[u8], label: &str) -> AppResult<String> {
    if bytes.len() != 40
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(AppError::usage(format!("invalid {label}")));
    }
    Ok(String::from_utf8(bytes.to_vec()).expect("ASCII validated"))
}

fn parse_sha256_bytes(bytes: &[u8], label: &str) -> AppResult<[u8; 32]> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| AppError::usage(format!("invalid {label}")))?;
    parse_sha256(text).map_err(|_| AppError::usage(format!("invalid {label}")))
}

fn decoded_absolute_path(bytes: Vec<u8>, label: &str) -> AppResult<PathBuf> {
    if bytes.contains(&0) || bytes.first() != Some(&b'/') {
        return Err(AppError::usage(format!(
            "{label} is not an absolute non-NUL path"
        )));
    }
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

fn parse_u64(bytes: &[u8]) -> AppResult<u64> {
    if bytes.is_empty()
        || (bytes.len() > 1 && bytes[0] == b'0')
        || !bytes.iter().all(u8::is_ascii_digit)
    {
        return Err(AppError::usage("noncanonical unsigned integer"));
    }
    let text = std::str::from_utf8(bytes).expect("ASCII digits");
    text.parse()
        .map_err(|_| AppError::usage("unsigned integer overflow"))
}

fn decode_hex(bytes: &[u8], allow_empty: bool) -> AppResult<Vec<u8>> {
    if (!allow_empty && bytes.is_empty())
        || !bytes.len().is_multiple_of(2)
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(AppError::usage("noncanonical hexadecimal field"));
    }
    let mut output = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = hex_nibble(pair[0]);
        let low = hex_nibble(pair[1]);
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => unreachable!("hex grammar checked before decoding"),
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("String writes cannot fail");
    }
    output
}

fn encode_list(elements: &[&[u8]]) -> AppResult<String> {
    let mut bytes = Vec::new();
    for element in elements {
        let length = u32::try_from(element.len())
            .map_err(|_| AppError::operational("list element overflow"))?;
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(element);
    }
    Ok(encode_hex(&bytes))
}

fn decode_list(bytes: &[u8]) -> AppResult<Vec<Vec<u8>>> {
    let mut cursor = 0usize;
    let mut output = Vec::new();
    while cursor < bytes.len() {
        let header = bytes
            .get(cursor..cursor + 4)
            .ok_or_else(|| AppError::usage("truncated list length"))?;
        let length = u32::from_be_bytes(header.try_into().expect("four-byte slice")) as usize;
        cursor += 4;
        let value = bytes
            .get(
                cursor
                    ..cursor
                        .checked_add(length)
                        .ok_or_else(|| AppError::usage("list length overflow"))?,
            )
            .ok_or_else(|| AppError::usage("truncated list element"))?;
        if value.contains(&0) {
            return Err(AppError::usage("list element contains NUL"));
        }
        output.push(value.to_vec());
        cursor += length;
    }
    Ok(output)
}

fn sha256_reader(file: &mut File, limit: u64) -> AppResult<[u8; 32]> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| AppError::io("hash seek", error))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| AppError::io("hash read", error))?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| AppError::operational("hash size overflow"))?;
        if total > limit {
            return Err(AppError::operational("hashed file exceeds its bound"));
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Hash128 {
    lo: u64,
    hi: u64,
}

impl Hash128 {
    fn of(bytes: &[u8]) -> Self {
        Self {
            lo: align_hash::wyhash(bytes, 0x9E37_79B9_7F4A_7C15),
            hi: align_hash::wyhash(bytes, 0xC2B2_AE3D_27D4_EB4F),
        }
    }

    fn to_hex(self) -> String {
        format!("{:016x}{:016x}", self.lo, self.hi)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CodegenKey {
    wire: Vec<u8>,
    cache_format_version: u32,
    compiler_build_id: Hash128,
    frontend_schema: u32,
    located: bool,
    impl_hash: Hash128,
    dep_interface_hashes: Vec<(String, Hash128)>,
    exports: Vec<String>,
    target_triple: String,
    object_format: u8,
    resolved_cpu: String,
    resolved_features: String,
    profile_name: String,
    pipeline: String,
    codegen_opt: String,
    reloc_model: String,
    code_model: String,
    llvm_version: String,
    llvm_build_id: Hash128,
    rt_lto: bool,
    rt_lto_digest: Option<Hash128>,
    pgo_tag: u8,
    unit: String,
    blob_digest: Hash128,
}

struct CacheReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> CacheReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> AppResult<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| AppError::operational("cache manifest length overflow"))?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| AppError::operational("truncated cache manifest"))?;
        self.position = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> AppResult<u8> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> AppResult<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(AppError::operational("invalid cache manifest bool tag")),
        }
    }

    fn u32(&mut self) -> AppResult<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> AppResult<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn hash128(&mut self) -> AppResult<Hash128> {
        Ok(Hash128 {
            lo: self.u64()?,
            hi: self.u64()?,
        })
    }

    fn string(&mut self) -> AppResult<String> {
        let count = self.u32()? as usize;
        if count > 4096 {
            return Err(AppError::operational(
                "cache manifest string exceeds key bound",
            ));
        }
        let bytes = self.take(count)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| AppError::operational("cache manifest string is not UTF-8"))
    }

    fn sequence<T>(
        &mut self,
        mut decode: impl FnMut(&mut Self) -> AppResult<T>,
    ) -> AppResult<Vec<T>> {
        let count = self.u32()? as usize;
        if count > 1024 {
            return Err(AppError::operational(
                "cache manifest sequence exceeds key bound",
            ));
        }
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(decode(self)?);
        }
        Ok(values)
    }

    fn optional_hash128(&mut self) -> AppResult<Option<Hash128>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.hash128()?)),
            _ => Err(AppError::operational("invalid cache manifest option tag")),
        }
    }
}

fn decode_codegen_manifest(bytes: &[u8]) -> AppResult<CodegenKey> {
    let mut reader = CacheReader::new(bytes);
    if reader.u32()? != 5 {
        return Err(AppError::operational("unknown codegen manifest version"));
    }
    let wire_start = reader.position;
    let cache_format_version = reader.u32()?;
    let compiler_build_id = reader.hash128()?;
    let frontend_schema = reader.u32()?;
    let located = reader.bool()?;
    let impl_hash = reader.hash128()?;
    let dep_interface_hashes =
        reader.sequence(|reader| Ok((reader.string()?, reader.hash128()?)))?;
    let exports = reader.sequence(CacheReader::string)?;
    let target_triple = reader.string()?;
    let object_format = reader.u8()?;
    let resolved_cpu = reader.string()?;
    let resolved_features = reader.string()?;
    let profile_name = reader.string()?;
    let pipeline = reader.string()?;
    let codegen_opt = reader.string()?;
    let reloc_model = reader.string()?;
    let code_model = reader.string()?;
    let llvm_version = reader.string()?;
    let llvm_build_id = reader.hash128()?;
    let rt_lto = reader.bool()?;
    let rt_lto_digest = reader.optional_hash128()?;
    let pgo_tag = reader.u8()?;
    if pgo_tag == 2 {
        let _ = reader.hash128()?;
    } else if pgo_tag > 2 {
        return Err(AppError::operational("unknown cache manifest PGO tag"));
    }
    let unit = reader.string()?;
    let wire_end = reader.position;
    let blob_digest = reader.hash128()?;
    if reader.position != bytes.len() {
        return Err(AppError::operational("trailing cache manifest bytes"));
    }
    let wire = bytes[wire_start..wire_end].to_vec();
    if wire.is_empty() || wire.len() > 4096 {
        return Err(AppError::operational(
            "codegen key exceeds its evidence bound",
        ));
    }
    Ok(CodegenKey {
        wire,
        cache_format_version,
        compiler_build_id,
        frontend_schema,
        located,
        impl_hash,
        dep_interface_hashes,
        exports,
        target_triple,
        object_format,
        resolved_cpu,
        resolved_features,
        profile_name,
        pipeline,
        codegen_opt,
        reloc_model,
        code_model,
        llvm_version,
        llvm_build_id,
        rt_lto,
        rt_lto_digest,
        pgo_tag,
        unit,
        blob_digest,
    })
}

fn validate_codegen_key(
    key: &CodegenKey,
    provenance: &Provenance,
    arm: &Arm,
    compiler_bytes: &[u8],
) -> AppResult<()> {
    let target = std::str::from_utf8(&provenance.target_triple)
        .map_err(|_| AppError::operational("target triple is not UTF-8"))?;
    let expected_format = if cfg!(target_os = "linux") { 0 } else { 1 };
    let expected_cpu = if cfg!(target_arch = "x86_64") {
        "x86-64-v2"
    } else {
        "generic"
    };
    if key.cache_format_version != 5
        || key.compiler_build_id != Hash128::of(compiler_bytes)
        || key.frontend_schema != 9
        || key.located
        || !key.dep_interface_hashes.is_empty()
        || !key.exports.is_empty()
        || key.target_triple != target
        || key.object_format != expected_format
        || key.resolved_cpu != expected_cpu
        || !key.resolved_features.is_empty()
        || key.profile_name != "release"
        || key.pipeline != "default<O2>"
        || key.codegen_opt != "default"
        || key.reloc_model != "PIC"
        || key.code_model != "Default"
        || !canonical_llvm_version(&key.llvm_version)
        || !key.rt_lto
        || key.rt_lto_digest.is_none()
        || key.pgo_tag != 0
        || key.unit != "main"
    {
        return Err(AppError::operational(format!(
            "invalid codegen key for {}",
            arm.name
        )));
    }
    Ok(())
}

fn canonical_llvm_version(version: &str) -> bool {
    let fields: Vec<&str> = version.split('.').collect();
    fields.len() == 3
        && fields.iter().all(|field| {
            !field.is_empty()
                && field.bytes().all(|byte| byte.is_ascii_digit())
                && (field.len() == 1 || !field.starts_with('0'))
                && field.parse::<u64>().is_ok()
        })
}

fn slot_digest(key: &CodegenKey) -> AppResult<Hash128> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&key.cache_format_version.to_le_bytes());
    bytes.extend_from_slice(&key.compiler_build_id.lo.to_le_bytes());
    bytes.extend_from_slice(&key.compiler_build_id.hi.to_le_bytes());
    let length =
        u32::try_from(key.unit.len()).map_err(|_| AppError::operational("unit name overflow"))?;
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(key.unit.as_bytes());
    Ok(Hash128::of(&bytes))
}

fn cstring_path(path: &Path) -> AppResult<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| AppError::usage("path contains NUL"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Warmup,
    Measure,
}

impl Phase {
    fn token(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Measure => "measure",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScheduledSample {
    phase: Phase,
    sequence: u32,
    arm: usize,
    arm_index: u32,
    position: u32,
}

fn schedule(arm_count: usize) -> AppResult<Vec<ScheduledSample>> {
    if arm_count != 1 && arm_count != 2 {
        return Err(AppError::operational("invalid arm count"));
    }
    let mut output = Vec::with_capacity(if arm_count == 1 { 22 } else { 44 });
    let mut sequence = 0u32;
    for (phase, count) in [(Phase::Warmup, WARMUPS), (Phase::Measure, MEASURED)] {
        for block in 0..count {
            if arm_count == 1 {
                output.push(ScheduledSample {
                    phase,
                    sequence,
                    arm: 0,
                    arm_index: block,
                    position: 0,
                });
                sequence += 1;
            } else {
                let order = if block % 2 == 0 { [0, 1] } else { [1, 0] };
                for (position, arm) in order.into_iter().enumerate() {
                    output.push(ScheduledSample {
                        phase,
                        sequence,
                        arm,
                        arm_index: block,
                        position: position as u32,
                    });
                    sequence += 1;
                }
            }
        }
    }
    Ok(output)
}

fn median(values: &mut [u64]) -> AppResult<u64> {
    if values.len() != MEASURED as usize {
        return Err(AppError::operational(
            "median requires exactly twenty samples",
        ));
    }
    values.sort_unstable();
    Ok(values[9] + (values[10] - values[9]) / 2)
}

fn schema_row() -> String {
    "schema\t1\n".to_owned()
}

fn run_row(
    provenance: &[u8; 32],
    fixtures: &[u8; 32],
    observer: &[u8; 32],
    arm_count: usize,
    work_filesystem: &[u8],
    execution_state: &[u8],
    work_dir: &Path,
) -> AppResult<String> {
    let child_environment = encode_list(&[b"LANG=C", b"LC_ALL=C", b"TZ=UTC"])?;
    Ok(format!(
        "run\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        encode_hex(provenance),
        encode_hex(fixtures),
        encode_hex(observer),
        arm_count,
        WARMUPS,
        MEASURED,
        TIMED_EXECUTION_NS,
        TIMED_CLEANUP_NS,
        GLOBAL_NS,
        encode_hex(work_filesystem),
        encode_hex(execution_state),
        if arm_count == 1 { "single" } else { "abba" },
        child_environment,
        encode_hex(work_dir.as_os_str().as_bytes()),
    ))
}

fn self_probe(arguments: &[OsString]) -> AppResult<()> {
    if arguments.len() != 2 || arguments[1].as_bytes() != b"--self-probe" {
        return Err(AppError::operational("malformed launcher self-probe"));
    }
    // SAFETY: process and process-group queries have no preconditions.
    if unsafe { libc::getpid() } != unsafe { libc::getpgrp() } {
        return Err(AppError::operational(
            "launcher self-probe is not its process-group leader",
        ));
    }
    for descriptor in [3, 4, 5] {
        // SAFETY: the query does not take ownership of the numeric descriptor.
        if unsafe { libc::fcntl(descriptor, libc::F_GETFD) } != -1
            || io::Error::last_os_error().raw_os_error() != Some(libc::EBADF)
        {
            return Err(AppError::operational(
                "launcher transfer descriptor survived exec",
            ));
        }
    }
    let mut file_size = MaybeUninit::<libc::rlimit>::zeroed();
    let mut core = MaybeUninit::<libc::rlimit>::zeroed();
    // SAFETY: both records are writable for fixed resource queries.
    if unsafe { libc::getrlimit(libc::RLIMIT_FSIZE, file_size.as_mut_ptr()) } == -1
        || unsafe { libc::getrlimit(libc::RLIMIT_CORE, core.as_mut_ptr()) } == -1
    {
        return Err(AppError::io(
            "launcher self-probe resource limit",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: successful getrlimit initialized both records.
    let file_size = unsafe { file_size.assume_init() };
    // SAFETY: successful getrlimit initialized both records.
    let core = unsafe { core.assume_init() };
    if file_size.rlim_cur != FILE_MAX as libc::rlim_t
        || file_size.rlim_max != FILE_MAX as libc::rlim_t
        || core.rlim_cur != 0
        || core.rlim_max != 0
    {
        return Err(AppError::operational(
            "launcher self-probe resource limits differ",
        ));
    }
    let environment: Vec<(OsString, OsString)> = env::vars_os().collect();
    if environment.len() != 6
        || environment[0] != (OsString::from("ALIGNC_CACHE"), OsString::from("off"))
        || environment[1] != (OsString::from("ALIGNC_LINKER"), OsString::from("system"))
        || environment[2] != (OsString::from("LANG"), OsString::from("C"))
        || environment[3] != (OsString::from("LC_ALL"), OsString::from("C"))
        || environment[4].0 != OsStr::new("PATH")
        || environment[4].1.as_bytes().is_empty()
        || environment[5] != (OsString::from("TZ"), OsString::from("UTC"))
    {
        return Err(AppError::operational(
            "launcher self-probe environment differs",
        ));
    }
    Ok(())
}

fn exec_tool(arguments: &[OsString]) -> AppResult<()> {
    let (path, tool_fd, argv_values, pathname_mode) = if arguments
        .first()
        .is_some_and(|arg| arg.as_bytes() == b"--exec-tool-fd")
    {
        if arguments.len() < 5
            || arguments[1].as_bytes() != b"4"
            || arguments[2].as_bytes() != b"--argv0"
        {
            return Err(AppError::operational("malformed descriptor tool launch"));
        }
        (
            PathBuf::from("/dev/fd/4"),
            4,
            arguments[3..].to_vec(),
            false,
        )
    } else {
        if arguments.len() < 6
            || arguments[1].as_bytes().first() != Some(&b'/')
            || arguments[2].as_bytes() != b"--tool-fd"
            || arguments[3].as_bytes() != b"4"
        {
            return Err(AppError::operational("malformed pathname tool launch"));
        }
        let mut values = vec![arguments[1].clone()];
        values.extend_from_slice(&arguments[4..]);
        (PathBuf::from(&arguments[1]), 4, values, true)
    };
    if argv_values.is_empty() {
        return Err(AppError::operational("missing tool argv0"));
    }
    verify_open_regular(tool_fd)?;
    if pathname_mode {
        verify_private_compiler_path(tool_fd, &path)?;
    }
    set_child_limits()?;
    let mut ready = [0u8; 1];
    let read = loop {
        // SAFETY: fd 5 is the launcher's fixed readiness descriptor and `ready` is writable.
        let read = unsafe { libc::read(5, ready.as_mut_ptr().cast(), 1) };
        if read != -1 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            break read;
        }
    };
    if read != 1 || ready[0] != b'G' {
        return Err(AppError::operational("invalid launcher readiness byte"));
    }
    // The parent must close immediately after the sole readiness byte.
    let extra = loop {
        // SAFETY: fd 5 remains open and the one-byte buffer is writable.
        let read = unsafe { libc::read(5, ready.as_mut_ptr().cast(), 1) };
        if read != -1 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            break read;
        }
    };
    if extra != 0 {
        return Err(AppError::operational("extra launcher readiness byte"));
    }
    for descriptor in [3, 4] {
        // SAFETY: fcntl mutates only the named owned descriptor flag.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
        if flags == -1
            || unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1
        {
            return Err(AppError::io(
                "launcher close-on-exec",
                io::Error::last_os_error(),
            ));
        }
    }
    // SAFETY: fd 5 is owned by the launcher and is nulled by this one close.
    if unsafe { libc::close(5) } == -1 {
        return Err(AppError::io(
            "launcher readiness close",
            io::Error::last_os_error(),
        ));
    }
    let executable = cstring_path(&path)?;
    let argv = cstring_vector(&argv_values)?;
    let mut pointers: Vec<*const libc::c_char> = argv.iter().map(|arg| arg.as_ptr()).collect();
    pointers.push(std::ptr::null());
    let environment = current_environment_cstrings()?;
    let mut environment_pointers: Vec<*const libc::c_char> =
        environment.iter().map(|item| item.as_ptr()).collect();
    environment_pointers.push(std::ptr::null());
    // SAFETY: all vectors are NUL-terminated and remain live across execve.
    unsafe {
        libc::execve(
            executable.as_ptr(),
            pointers.as_ptr(),
            environment_pointers.as_ptr(),
        )
    };
    Err(AppError::io("launcher exec", io::Error::last_os_error()))
}

#[allow(clippy::unnecessary_cast)] // libc stat field widths differ between Linux and macOS.
fn verify_private_compiler_path(descriptor: RawFd, path: &Path) -> AppResult<()> {
    if path.file_name() != Some(OsStr::new("alignc")) {
        return Err(AppError::operational(
            "private compiler pathname is not the fixed arm path",
        ));
    }
    let arm = path
        .parent()
        .ok_or_else(|| AppError::operational("private compiler has no arm directory"))?;
    if arm.file_name() != Some(OsStr::new("baseline"))
        && arm.file_name() != Some(OsStr::new("candidate"))
    {
        return Err(AppError::operational("private compiler arm is invalid"));
    }
    let tool = arm
        .parent()
        .filter(|path| path.file_name() == Some(OsStr::new("tool")))
        .ok_or_else(|| AppError::operational("private compiler tool directory is invalid"))?;
    let build = tool
        .parent()
        .filter(|path| path.file_name() == Some(OsStr::new("build")))
        .ok_or_else(|| AppError::operational("private compiler build directory is invalid"))?;
    // SAFETY: geteuid is a read-only process observation.
    let euid = unsafe { libc::geteuid() };
    for directory in [arm, tool, build] {
        let metadata = fs::symlink_metadata(directory)
            .map_err(|error| AppError::io("private compiler ancestor", error))?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != euid
            || metadata.mode() & 0o7777 != 0o700
        {
            return Err(AppError::operational(
                "private compiler ancestor invariant failed",
            ));
        }
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|error| AppError::io("private compiler path", error))?;
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: stat is writable and descriptor remains retained.
    if unsafe { libc::fstat(descriptor, stat.as_mut_ptr()) } == -1 {
        return Err(AppError::io(
            "private compiler descriptor",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: successful fstat initialized stat.
    let stat = unsafe { stat.assume_init() };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.dev() != stat.st_dev as u64
        || metadata.ino() != stat.st_ino as u64
        || metadata.mode() != stat.st_mode as u32
        || metadata.uid() != euid
        || metadata.mode() & 0o7777 != 0o700
        || metadata.nlink() != 1
    {
        return Err(AppError::operational(
            "private compiler descriptor/path identity mismatch",
        ));
    }
    Ok(())
}

fn verify_open_regular(descriptor: RawFd) -> AppResult<()> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: stat is writable and descriptor is borrowed.
    if unsafe { libc::fstat(descriptor, stat.as_mut_ptr()) } == -1 {
        return Err(AppError::io("tool descriptor", io::Error::last_os_error()));
    }
    // SAFETY: successful fstat initialized stat.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG
        || stat.st_size < 0
        || stat.st_size as u64 > FILE_MAX
    {
        return Err(AppError::operational(
            "tool descriptor is not a bounded regular file",
        ));
    }
    Ok(())
}

fn validate_inherited_file_limit() -> AppResult<()> {
    let mut inherited = MaybeUninit::<libc::rlimit>::zeroed();
    // SAFETY: inherited is writable storage for the fixed resource query.
    if unsafe { libc::getrlimit(libc::RLIMIT_FSIZE, inherited.as_mut_ptr()) } == -1 {
        return Err(AppError::io(
            "inherited file-size limit",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: successful getrlimit initialized the record.
    let inherited = unsafe { inherited.assume_init() };
    if inherited.rlim_max != libc::RLIM_INFINITY && inherited.rlim_max < FILE_MAX as libc::rlim_t {
        return Err(AppError::usage(
            "inherited hard file-size limit is below 64 MiB",
        ));
    }
    Ok(())
}

fn set_child_limits() -> AppResult<()> {
    for (resource, value) in [(libc::RLIMIT_FSIZE, FILE_MAX), (libc::RLIMIT_CORE, 0)] {
        let limit = libc::rlimit {
            rlim_cur: value as libc::rlim_t,
            rlim_max: value as libc::rlim_t,
        };
        // SAFETY: limit is a valid fixed record for the named resource.
        if unsafe { libc::setrlimit(resource, &limit) } == -1 {
            return Err(AppError::io(
                "launcher resource limit",
                io::Error::last_os_error(),
            ));
        }
    }
    Ok(())
}

fn cstring_vector(values: &[OsString]) -> AppResult<Vec<CString>> {
    values
        .iter()
        .map(|value| {
            CString::new(value.as_bytes())
                .map_err(|_| AppError::operational("tool argument contains NUL"))
        })
        .collect()
}

fn current_environment_cstrings() -> AppResult<Vec<CString>> {
    env::vars_os()
        .map(|(key, value)| {
            let mut bytes = key.into_vec();
            bytes.push(b'=');
            bytes.extend_from_slice(&value.into_vec());
            CString::new(bytes).map_err(|_| AppError::operational("tool environment contains NUL"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory(label: &str) -> PathBuf {
        let id = TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            env::temp_dir().join(format!("align-startup-{label}-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }

    fn push_cache_string(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn cache_manifest_golden() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(&5u32.to_le_bytes());
        for value in [1u64, 2] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.push(0);
        for value in [3u64, 4] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        push_cache_string(&mut bytes, "x86_64-pc-linux-gnu");
        bytes.push(0);
        for value in [
            "x86-64-v2",
            "",
            "release",
            "default<O2>",
            "default",
            "PIC",
            "Default",
            "22.0.0",
        ] {
            push_cache_string(&mut bytes, value);
        }
        for value in [5u64, 6] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(1);
        bytes.push(1);
        for value in [7u64, 8] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0);
        push_cache_string(&mut bytes, "main");
        for value in [9u64, 10] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn list_codec_golden_vectors() {
        assert_eq!(encode_list(&[]).unwrap(), "");
        assert_eq!(
            encode_list(&[b"", b"a", &[0xff]]).unwrap(),
            "00000000000000016100000001ff"
        );
        assert_eq!(decode_list(&[]).unwrap(), Vec::<Vec<u8>>::new());
        assert_eq!(
            decode_list(&[0, 0, 0, 1, b'a']).unwrap(),
            vec![b"a".to_vec()]
        );
        assert!(decode_list(&[0, 0, 0]).is_err());
        assert!(decode_list(&[0, 0, 0, 2, b'a']).is_err());
    }

    #[test]
    fn canonical_scalars_reject_alias_spellings() {
        assert_eq!(parse_u64(b"0").unwrap(), 0);
        assert_eq!(parse_u64(b"18446744073709551615").unwrap(), u64::MAX);
        for bad in [b"".as_slice(), b"00", b"+1", b"-0", b"18446744073709551616"] {
            assert!(
                parse_u64(bad).is_err(),
                "accepted {:?}",
                String::from_utf8_lossy(bad)
            );
        }
        assert!(decode_hex(b"", true).is_ok());
        for bad in [b"0".as_slice(), b"AA", b"gg"] {
            assert!(decode_hex(bad, true).is_err());
        }
    }

    #[test]
    fn cache_v5_codec_golden_and_malformed_inputs() {
        let bytes = cache_manifest_golden();
        assert_eq!(
            encode_hex(&Sha256::digest(&bytes)),
            "fb5159b6223210a4c44f855adafefb06cba8edb70a641ba0fa34972ab4a00b48"
        );
        let key = decode_codegen_manifest(&bytes).unwrap();
        assert_eq!(key.cache_format_version, 5);
        assert_eq!(key.compiler_build_id, Hash128 { lo: 1, hi: 2 });
        assert_eq!(key.frontend_schema, 9);
        assert!(!key.located);
        assert_eq!(key.impl_hash, Hash128 { lo: 3, hi: 4 });
        assert!(key.dep_interface_hashes.is_empty());
        assert!(key.exports.is_empty());
        assert_eq!(key.target_triple, "x86_64-pc-linux-gnu");
        assert_eq!(key.llvm_build_id, Hash128 { lo: 5, hi: 6 });
        assert_eq!(key.rt_lto_digest, Some(Hash128 { lo: 7, hi: 8 }));
        assert_eq!(key.blob_digest, Hash128 { lo: 9, hi: 10 });
        assert_eq!(
            Hash128::of(&key.wire),
            Hash128::of(&bytes[4..bytes.len() - 16])
        );

        for length in 0..bytes.len() {
            assert!(
                decode_codegen_manifest(&bytes[..length]).is_err(),
                "length {length}"
            );
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_codegen_manifest(&trailing).is_err());
        let mut unknown_version = bytes.clone();
        unknown_version[..4].copy_from_slice(&6u32.to_le_bytes());
        assert!(decode_codegen_manifest(&unknown_version).is_err());
        let mut oversized_sequence = bytes.clone();
        // manifest + cache version + compiler hash + schema + bool + impl hash
        let dependency_count = 4 + 4 + 16 + 4 + 1 + 16;
        oversized_sequence[dependency_count..dependency_count + 4]
            .copy_from_slice(&1025u32.to_le_bytes());
        assert!(decode_codegen_manifest(&oversized_sequence).is_err());
    }

    #[test]
    fn single_arm_schedule_is_exact() {
        let schedule = schedule(1).unwrap();
        assert_eq!(schedule.len(), 22);
        for (ordinal, sample) in schedule.iter().enumerate() {
            assert_eq!(sample.sequence, ordinal as u32);
            assert_eq!(sample.arm, 0);
            assert_eq!(sample.position, 0);
            assert_eq!(
                sample.arm_index,
                if ordinal < 2 { ordinal } else { ordinal - 2 } as u32
            );
        }
    }

    #[test]
    fn two_arm_schedule_counterbalances_every_block() {
        let schedule = schedule(2).unwrap();
        assert_eq!(schedule.len(), 44);
        for (block, pair) in schedule.chunks_exact(2).enumerate() {
            let expected = if (block % 2) == 0 { [0, 1] } else { [1, 0] };
            assert_eq!([pair[0].arm, pair[1].arm], expected);
            assert_eq!([pair[0].position, pair[1].position], [0, 1]);
            assert_eq!(pair[0].arm_index, pair[1].arm_index);
        }
    }

    #[test]
    fn median_uses_overflow_safe_lower_half_rounding() {
        let mut values = [0u64; 20];
        for (index, value) in values.iter_mut().enumerate() {
            *value = if index < 10 { u64::MAX - 1 } else { u64::MAX };
        }
        assert_eq!(median(&mut values).unwrap(), u64::MAX - 1);
    }

    #[test]
    fn result_prefix_golden_vector() {
        let digest = [0xabu8; 32];
        let row = run_row(
            &digest,
            &digest,
            &digest,
            1,
            b"fs\n",
            b"state\n",
            Path::new("/w"),
        )
        .unwrap();
        assert_eq!(schema_row(), "schema\t1\n");
        assert!(row.starts_with(&format!(
            "run\t{}\t{}\t{}\t1\t2\t20\t5000000000\t1000000000\t900000000000\t",
            "ab".repeat(32),
            "ab".repeat(32),
            "ab".repeat(32)
        )));
        assert!(row.ends_with(
            "\tsingle\t000000064c414e473d43000000084c435f414c4c3d4300000006545a3d555443\t2f77\n"
        ));
        assert_eq!(row.trim_end_matches('\n').split('\t').count(), 15);
    }

    #[test]
    fn result_row_field_counts_are_exact() {
        let arm = Arm {
            name: "baseline",
            source_revision: "0".repeat(40),
            compiler: PathBuf::from("/compiler"),
            compiler_sha256: [1; 32],
            runtime: PathBuf::from("/runtime"),
            runtime_sha256: [2; 32],
            compiler_version: b"alignc\n".to_vec(),
        };
        let scheduled = ScheduledSample {
            phase: Phase::Warmup,
            sequence: 0,
            arm: 0,
            arm_index: 0,
            position: 0,
        };
        let sample = unstarted_sample_row(
            FIXTURES[0],
            scheduled,
            std::slice::from_ref(&arm),
            SampleOutcome::FormationError,
            b"clock-start",
        )
        .unwrap();
        let summary = summary_row(FIXTURES[0], &arm, false, &mut []).unwrap();
        for (row, expected) in [
            (schema_row(), 2),
            (sample, 23),
            (summary, 14),
            ("end\tfailed\t0\t\n".to_owned(), 4),
        ] {
            assert_eq!(row.trim_end_matches('\n').split('\t').count(), expected);
        }
    }

    #[test]
    fn completed_row_preserves_independent_stream_state_and_cleanup_precedence() {
        let arm = Arm {
            name: "baseline",
            source_revision: "0".repeat(40),
            compiler: PathBuf::from("/compiler"),
            compiler_sha256: [1; 32],
            runtime: PathBuf::from("/runtime"),
            runtime_sha256: [2; 32],
            compiler_version: b"alignc\n".to_vec(),
        };
        let scheduled = ScheduledSample {
            phase: Phase::Measure,
            sequence: 2,
            arm: 0,
            arm_index: 0,
            position: 0,
        };
        let child = ChildResult {
            wall_ns: Some(1),
            usage: UsageMetrics {
                user_ns: Some(2),
                system_ns: Some(3),
                minor_faults: Some(4),
                major_faults: Some(5),
                voluntary_switches: Some(6),
                involuntary_switches: Some(7),
                peak_rss_bytes: Some(8),
            },
            exit: ChildExit::Code(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_len: None,
            stderr_len: Some(0),
            stdout_complete: false,
            stderr_complete: true,
            stdout_overflow: false,
            stderr_overflow: false,
            timed_out: true,
            cleanup_issue: Some(LifecycleIssue {
                operation: "fionread-stdout",
                os_error: Some(b"synthetic".to_vec()),
            }),
            stdout_path: None,
            stderr_path: None,
        };
        let (row, outcome, metrics) =
            completed_sample_row(FIXTURES[0], scheduled, std::slice::from_ref(&arm), &child)
                .unwrap();
        assert_eq!(outcome, SampleOutcome::CleanupError);
        assert!(metrics.is_some());
        let fields: Vec<&str> = row.trim_end_matches('\n').split('\t').collect();
        assert_eq!(fields.len(), 23);
        assert_eq!(fields[7], "cleanup-error");
        assert_eq!(&fields[8..16], &["1", "2", "3", "4", "5", "6", "7", "8"]);
        assert_eq!(&fields[16..22], &["code", "0", "-", "yes", "-", "0"]);
        let details = decode_list(&decode_hex(fields[22].as_bytes(), true).unwrap()).unwrap();
        assert_eq!(
            details,
            vec![
                b"operation\tfionread-stdout".to_vec(),
                b"os-error\tsynthetic".to_vec()
            ]
        );
    }

    #[test]
    fn spawn_failure_retains_the_parent_wall_interval() {
        let watchdog = Watchdog::new().unwrap();
        let observer = File::open(env::current_exe().unwrap()).unwrap();
        let executable = Path::new("/definitely-missing-align-startup-child");
        let argv = [executable.as_os_str().to_owned()];
        let environment = timed_environment();
        let error = run_child(
            &watchdog,
            &observer,
            &ChildSpec {
                executable,
                argv: &argv,
                environment: &environment,
                cwd: Path::new("/"),
                capture: CaptureKind::Timed,
                execution_ns: 2_000_000_000,
                cleanup_ns: 1_000_000_000,
                tool: None,
                timed_capture: None,
            },
        )
        .unwrap_err();
        let failure = classify_child_failure(&error);
        assert_eq!(failure.outcome, SampleOutcome::SpawnError);
        assert_eq!(failure.operation, "spawn");
        assert!(failure.wall_ns.is_some());
    }

    #[test]
    fn timed_capture_persists_a_mismatch_before_return() {
        let directory = temporary_directory("capture");
        let stdout_log = directory.join("stdout.bin");
        let stderr_log = directory.join("stderr.bin");
        let capture = TimedCapture {
            expected_stdout: b"expected",
            expected_stderr: b"",
            stdout_log: &stdout_log,
            stderr_log: &stderr_log,
            stdout_relative: b"logs/stdout.bin",
            stderr_relative: b"logs/stderr.bin",
        };
        let watchdog = Watchdog::new().unwrap();
        let observer = File::open(env::current_exe().unwrap()).unwrap();
        let executable = Path::new("/usr/bin/printf");
        if !executable.exists() {
            fs::remove_dir(&directory).unwrap();
            return;
        }
        let argv = [OsString::from("/usr/bin/printf"), OsString::from("actual")];
        let environment = timed_environment();
        let result = run_child(
            &watchdog,
            &observer,
            &ChildSpec {
                executable,
                argv: &argv,
                environment: &environment,
                cwd: Path::new("/"),
                capture: CaptureKind::Timed,
                execution_ns: 2_000_000_000,
                cleanup_ns: 1_000_000_000,
                tool: None,
                timed_capture: Some(&capture),
            },
        )
        .unwrap();
        assert_eq!(
            result.stdout_path.as_deref(),
            Some(b"logs/stdout.bin".as_slice())
        );
        assert_eq!(fs::read(&stdout_log).unwrap(), b"actual");
        assert_eq!(fs::metadata(&stdout_log).unwrap().mode() & 0o7777, 0o600);
        assert!(!stderr_log.exists());
        fs::remove_file(stdout_log).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn watchdog_rejects_overlap_and_generation_exhaustion() {
        let watchdog = Watchdog::new().unwrap();
        let generation = watchdog.reserve().unwrap();
        assert!(watchdog.reserve().is_err());
        watchdog.cancel_reservation(generation).unwrap();
        {
            let (mutex, _) = &*watchdog.shared;
            nonpoisoning_lock(mutex).generation = u64::MAX;
        }
        assert!(watchdog.reserve().is_err());
    }

    #[test]
    fn retained_directory_enumeration_does_not_follow_a_replacement_path() {
        let directory = temporary_directory("retained-root");
        let root = directory.join("cache");
        let moved = directory.join("cache-retained");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let retained = open_directory_nofollow(&root, "test cache").unwrap();
        require_exact_directories(retained.as_raw_fd(), &[]).unwrap();
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("replacement"), b"foreign").unwrap();
        fs::create_dir(moved.join("actions")).unwrap();
        require_exact_directories(retained.as_raw_fd(), &[b"actions"]).unwrap();
        assert!(require_exact_regular_files(retained.as_raw_fd(), &[b"replacement"]).is_err());
        fs::remove_file(root.join("replacement")).unwrap();
        fs::remove_dir(root).unwrap();
        fs::remove_dir(moved.join("actions")).unwrap();
        fs::remove_dir(moved).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn fixture_bundle_is_structural_and_ordered() {
        let mut digest = Sha256::new();
        for fixture in FIXTURES {
            digest.update((fixture.id.len() as u32).to_be_bytes());
            digest.update(fixture.id.as_bytes());
            digest.update((fixture.bytes.len() as u64).to_be_bytes());
            digest.update(fixture.bytes);
        }
        let first: [u8; 32] = digest.finalize().into();
        assert_ne!(first, [0; 32]);
        assert_eq!(FIXTURES[2].argv, &[b"alpha".as_slice(), "β".as_bytes()]);
        assert_eq!(FIXTURES[3].stdout, b"42\n");
    }

    #[test]
    fn real_child_is_reaped_with_direct_usage_and_passive_capture() {
        let watchdog = Watchdog::new().unwrap();
        let observer = File::open(env::current_exe().unwrap()).unwrap();
        let executable = Path::new("/usr/bin/printf");
        if !executable.exists() {
            return;
        }
        let argv = [OsString::from("/usr/bin/printf"), OsString::from("ok")];
        let environment = [
            OsString::from("LANG=C"),
            OsString::from("LC_ALL=C"),
            OsString::from("TZ=UTC"),
        ];
        let result = run_child(
            &watchdog,
            &observer,
            &ChildSpec {
                executable,
                argv: &argv,
                environment: &environment,
                cwd: Path::new("/"),
                capture: CaptureKind::Timed,
                execution_ns: 2_000_000_000,
                cleanup_ns: 1_000_000_000,
                tool: None,
                timed_capture: None,
            },
        )
        .unwrap();
        assert_eq!(result.exit, ChildExit::Code(0));
        assert_eq!(result.stdout, b"ok");
        assert_eq!(result.stderr, b"");
        assert!(!result.timed_out);
        assert!(result.wall_ns.is_some());
        assert!(result.usage.user_ns.is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_execution_state_is_canonical_and_stable() {
        let first = execution_state_record().unwrap();
        let second = execution_state_record().unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with(b"align-startup-execution-state-v1\n"));
        assert!(first.ends_with(b"\n"));
        assert!(first.len() <= 64 * 1024);
    }

    #[test]
    fn machine_identity_producers_are_bounded_and_framed() {
        for record in [
            current_host_kernel().unwrap(),
            current_cpu_identity().unwrap(),
            current_libc_identity().unwrap(),
        ] {
            assert!(!record.is_empty());
            assert!(record.len() <= 64 * 1024);
        }
    }

    #[test]
    fn watchdog_kills_and_reaps_a_timed_out_direct_child() {
        let executable = Path::new("/usr/bin/sleep");
        if !executable.exists() {
            return;
        }
        let watchdog = Watchdog::new().unwrap();
        let observer = File::open(env::current_exe().unwrap()).unwrap();
        let argv = [OsString::from("/usr/bin/sleep"), OsString::from("5")];
        let environment = timed_environment();
        let result = run_child(
            &watchdog,
            &observer,
            &ChildSpec {
                executable,
                argv: &argv,
                environment: &environment,
                cwd: Path::new("/"),
                capture: CaptureKind::Timed,
                execution_ns: 20_000_000,
                cleanup_ns: 1_000_000_000,
                tool: None,
                timed_capture: None,
            },
        )
        .unwrap();
        assert!(result.timed_out);
        assert_eq!(result.exit, ChildExit::Signal(libc::SIGKILL as u32));
    }
}
