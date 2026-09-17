//! The resolved target identity: one place that decides the exact triple every artifact carries.
//!
//! LLVM's host default triple spells macOS as `arm64-apple-darwin<kernel>.0.0` — the *kernel*
//! version, which LLVM then rewrites through its Darwin → macOS renumbering table when it stamps
//! `LC_BUILD_VERSION`. On a host whose kernel major equals its macOS major that table lands one
//! major too high, so every `alignc` object claims a macOS it was not built for, the linker warns
//! once per object, and the kernel *patch* level (a non-input) is hashed into every cache key.
//!
//! The fix is not to patch the one wrong string: it is to make the deployment target an explicit,
//! single-sourced component of the target identity, exactly as `--target-cpu` already is. Every
//! Apple triple the compiler constructs is normalized to the platform's canonical LLVM OS spelling
//! with an explicit deployment version (`arm64-apple-macosx26.0`), resolved once by the precedence
//! in [`resolve_deployment_version`] and reused by the `TargetMachine`, every module triple, the
//! link, and the cache key. Non-Apple triples are left byte-identical.
//!
//! `docs/impl/65-open-issue-batch-plan.md` §"Target identity and inspection roots" owns the exact
//! rule; issue 1087 owns the evidence.

use std::sync::OnceLock;

use crate::CodegenError;

/// One Apple platform's normalization policy: how its OS component is spelled in a canonical LLVM
/// triple, which environment variable names its deployment target, which `cc` flag states it at the
/// link, and the documented floor used when nothing else resolves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplePlatform {
    /// The canonical LLVM OS spelling this platform normalizes to (never `darwin<N>`).
    pub canonical_os: &'static str,
    /// The platform's deployment-target environment variable — the one ambient input this contract
    /// names, matching the Apple toolchain convention `alignc` silently ignored before.
    pub env_var: &'static str,
    /// The `cc` flag prefix that states the same deployment target at the link, version appended.
    pub min_version_flag_prefix: &'static str,
    /// The documented floor, used only when neither an explicit value, the environment, nor a host
    /// query supplies one (a cross-compilation host has no product version to read).
    pub floor: &'static str,
    /// Whether a macOS host's product version is this platform's host version. Only the host
    /// platform may consult the host; an iOS triple on a macOS host does not inherit macOS's
    /// version.
    pub reads_host_version: bool,
}

/// macOS — the only Apple platform a host-triple build can reach today, and the one the
/// `darwin<kernel>` spelling mislabels.
const MACOS: ApplePlatform = ApplePlatform {
    canonical_os: "macosx",
    env_var: "MACOSX_DEPLOYMENT_TARGET",
    min_version_flag_prefix: "-mmacosx-version-min=",
    // Big Sur: the first macOS with an arm64 slice, and below every currently supported release.
    floor: "11.0",
    reads_host_version: true,
};

/// Every Apple OS component this normalizer claims to support, matched against the lowercased OS
/// field with its trailing version stripped. `darwin` maps to macOS: a bare Darwin triple from the
/// host default is a macOS build, and the kernel version it carries is never a deployment target.
const APPLE_PLATFORMS: &[(&str, ApplePlatform)] = &[
    ("darwin", MACOS),
    ("macos", MACOS),
    ("macosx", MACOS),
    (
        "ios",
        ApplePlatform {
            canonical_os: "ios",
            env_var: "IPHONEOS_DEPLOYMENT_TARGET",
            min_version_flag_prefix: "-miphoneos-version-min=",
            floor: "13.0",
            reads_host_version: false,
        },
    ),
    (
        "watchos",
        ApplePlatform {
            canonical_os: "watchos",
            env_var: "WATCHOS_DEPLOYMENT_TARGET",
            min_version_flag_prefix: "-mwatchos-version-min=",
            floor: "7.0",
            reads_host_version: false,
        },
    ),
    (
        "tvos",
        ApplePlatform {
            canonical_os: "tvos",
            env_var: "TVOS_DEPLOYMENT_TARGET",
            min_version_flag_prefix: "-mtvos-version-min=",
            floor: "13.0",
            reads_host_version: false,
        },
    ),
    // LLVM's canonical visionOS spelling is `xros`; `visionos` is the accepted alias, normalized to
    // it. Its version flag has no `-m<os>-version-min=` form, so it uses `-mtargetos=`.
    (
        "xros",
        ApplePlatform {
            canonical_os: "xros",
            env_var: "XROS_DEPLOYMENT_TARGET",
            min_version_flag_prefix: "-mtargetos=xros",
            floor: "1.0",
            reads_host_version: false,
        },
    ),
    (
        "visionos",
        ApplePlatform {
            canonical_os: "xros",
            env_var: "XROS_DEPLOYMENT_TARGET",
            min_version_flag_prefix: "-mtargetos=xros",
            floor: "1.0",
            reads_host_version: false,
        },
    ),
];

/// The OS field of `triple` (`<arch>-<vendor>-<os>[-<env>]`), as `(prefix, os_field, suffix)`.
///
/// A triple with fewer than three fields has no OS component at all and is returned unchanged by
/// every caller. The environment field (`-simulator`, `-gnu`, `-macabi`, …) is part of `suffix` and
/// is preserved byte-for-byte: normalization rewrites the OS spelling and version, nothing else.
fn split_os_field(triple: &str) -> Option<(&str, &str, &str)> {
    let first = triple.find('-')?;
    let second = triple[first + 1..].find('-')? + first + 1;
    let os_start = second + 1;
    let os_end = triple[os_start..]
        .find('-')
        .map(|i| i + os_start)
        .unwrap_or(triple.len());
    Some((&triple[..os_start], &triple[os_start..os_end], &triple[os_end..]))
}

/// The Apple platform named by an OS field, ignoring any trailing version digits
/// (`darwin27.0.0` → macOS, `ios17.0` → iOS). `None` for every non-Apple or unrecognized field.
fn platform_for_os_field(os_field: &str) -> Option<ApplePlatform> {
    let lower = os_field.to_ascii_lowercase();
    let name_end = lower
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(lower.len());
    let name = &lower[..name_end];
    APPLE_PLATFORMS
        .iter()
        .find(|(spelling, _)| *spelling == name)
        .map(|(_, platform)| *platform)
}

/// The Apple platform `triple` targets, or `None` when it is not an Apple triple this normalizer
/// recognizes (every non-Apple triple, and any `*-apple-*` triple whose OS component is outside
/// [`APPLE_PLATFORMS`], which is left byte-identical rather than guessed at).
pub fn apple_platform(triple: &str) -> Option<ApplePlatform> {
    let (_, os_field, _) = split_os_field(triple)?;
    if !triple.to_ascii_lowercase().contains("-apple-") {
        return None;
    }
    platform_for_os_field(os_field)
}

/// Canonicalize a deployment-target version to `major.minor`.
///
/// Accepts `N`, `N.M`, and `N.M.P` decimal components and returns `N.M` — the granularity every
/// deployment target is actually expressed in, and the granularity that keeps an OS *patch* update
/// (a non-input to compilation) from invalidating the whole cache. `source` names the origin so a
/// malformed value reports which input to fix. Empty, non-numeric, over-long, and NUL-bearing
/// values are hard errors, never a silent fallback to a different version.
pub fn canonical_version(value: &str, source: &str) -> Result<String, CodegenError> {
    let reject = |why: &str| {
        Err(CodegenError::Target(format!(
            "{source} value {value:?} is not a valid deployment target: {why} (expected `major`, \
             `major.minor`, or `major.minor.patch` decimal components)"
        )))
    };
    if value.is_empty() {
        return reject("it is empty");
    }
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() > 3 {
        return reject("it has more than three components");
    }
    let mut numbers = Vec::with_capacity(parts.len());
    for part in &parts {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return reject("every component must be one or more decimal digits");
        }
        match part.parse::<u32>() {
            Ok(n) => numbers.push(n),
            Err(_) => return reject("a component does not fit in 32 bits"),
        }
    }
    // `split` on a nonempty string always yields at least one part, and every part above was
    // accepted, so a major version exists — read it without indexing anyway, so a later change to
    // the loop above cannot turn this into a panic on user input.
    let Some(major) = numbers.first().copied() else {
        return reject("it has no major version");
    };
    if major == 0 {
        return reject("the major version must be at least 1");
    }
    Ok(format!("{major}.{}", numbers.get(1).copied().unwrap_or(0)))
}

/// Resolve one Apple platform's deployment target from the stated precedence, with every ambient
/// input supplied by the caller so the rule itself is a pure function:
///
/// 1. `explicit` — the `--deployment-target` CLI value, the only way to get a non-host version;
/// 2. `env` — the platform's `*_DEPLOYMENT_TARGET` variable, the one ambient input this contract
///    names;
/// 3. `host` — the host product version, and only for the platform the host actually is;
/// 4. [`ApplePlatform::floor`] — the documented per-platform floor.
///
/// Every layer is canonicalized by [`canonical_version`], so a malformed value at any layer is a
/// hard error rather than a silent fall-through to the next one.
pub fn resolve_deployment_version(
    platform: &ApplePlatform,
    explicit: Option<&str>,
    env: Option<&str>,
    host: Option<&str>,
) -> Result<String, CodegenError> {
    if let Some(value) = explicit {
        return canonical_version(value, "--deployment-target");
    }
    if let Some(value) = env {
        return canonical_version(value, platform.env_var);
    }
    if platform.reads_host_version
        && let Some(value) = host
    {
        return canonical_version(value, "the host product version");
    }
    canonical_version(platform.floor, "the documented platform floor")
}

/// Rewrite `triple`'s OS component to `platform`'s canonical spelling plus `version`, leaving the
/// architecture, vendor, and environment fields byte-identical. A non-Apple (or unrecognized)
/// triple is returned unchanged.
pub fn normalize_apple_triple(triple: &str, platform: &ApplePlatform, version: &str) -> String {
    let Some((prefix, _, suffix)) = split_os_field(triple) else {
        return triple.to_string();
    };
    format!("{prefix}{}{version}{suffix}", platform.canonical_os)
}

/// The host's macOS product version (`sw_vers -productVersion`), read without spawning a process.
///
/// `kern.osproductversion` is the same value `sw_vers` prints and is the *product* version, never
/// the kernel version the default triple carries. `None` on any other host, and on a macOS host
/// where the query is unavailable — in which case resolution falls to the documented floor.
#[cfg(target_os = "macos")]
fn host_product_version() -> Option<String> {
    let name = c"kern.osproductversion";
    let mut len: usize = 0;
    // SAFETY: the two-call sysctl protocol. The sizing call passes a null output buffer with a live
    // `len` out-param; `name` is a NUL-terminated literal. No pointer is retained by the callee.
    let sized = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    // A sane product version is a handful of bytes; anything else is not one, and the cap keeps a
    // hostile/garbage length from requesting an unbounded allocation.
    if sized != 0 || len == 0 || len > 64 {
        return None;
    }
    let mut buf = vec![0u8; len];
    // SAFETY: `buf` is `len` writable bytes and `len` is the exact capacity the sizing call
    // reported; the kernel writes at most that many and updates `len` to what it wrote.
    let read = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast::<libc::c_void>(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 || len == 0 || len > buf.len() {
        return None;
    }
    buf.truncate(len);
    // The value is a NUL-terminated C string inside the buffer.
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    let text = String::from_utf8(buf[..end].to_vec()).ok()?;
    (!text.is_empty()).then_some(text)
}

#[cfg(not(target_os = "macos"))]
fn host_product_version() -> Option<String> {
    None
}

/// The `--deployment-target` value, installed once before any triple is resolved.
static EXPLICIT_DEPLOYMENT_TARGET: OnceLock<Option<String>> = OnceLock::new();
/// The resolved triple, computed once. Both [`crate::resolve_target_identity`] and
/// [`crate::create_target_machine`] read it, so they are byte-identical by construction rather than
/// by two matching code paths.
static RESOLVED_TRIPLE: OnceLock<Result<String, String>> = OnceLock::new();
/// The resolved Apple deployment target (`None` on a non-Apple host), computed with the triple.
static RESOLVED_DEPLOYMENT: OnceLock<Option<(ApplePlatform, String)>> = OnceLock::new();

/// Install the explicit `--deployment-target` value. The CLI calls this once, before any codegen,
/// cache key, or link.
///
/// Fails closed in three directions: a second call with a different value, a call made after the
/// triple was already resolved (which would let two artifacts in one process disagree), and a value
/// on a host whose triple is not an Apple triple — where a deployment target has no meaning and
/// silently ignoring it would be exactly the ambient guessing this contract removes.
pub fn set_deployment_target(version: &str) -> Result<(), CodegenError> {
    let canonical = canonical_version(version, "--deployment-target")?;
    let host = raw_default_triple();
    if apple_platform(&host).is_none() {
        return Err(CodegenError::Target(format!(
            "--deployment-target does not apply to target '{host}': it names an Apple platform \
             deployment target and this host is not an Apple target"
        )));
    }
    if RESOLVED_TRIPLE.get().is_some() {
        return Err(CodegenError::Target(
            "--deployment-target must be set before the target triple is resolved".to_string(),
        ));
    }
    match EXPLICIT_DEPLOYMENT_TARGET.set(Some(canonical.clone())) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Already installed: identical is a harmless repeat, different is a contradiction.
            let existing = EXPLICIT_DEPLOYMENT_TARGET.get().and_then(Option::as_deref);
            if existing == Some(canonical.as_str()) {
                Ok(())
            } else {
                Err(CodegenError::Target(format!(
                    "--deployment-target was already resolved as {:?} in this process",
                    existing.unwrap_or("<host default>")
                )))
            }
        }
    }
}

/// LLVM's unmodified host default triple — the *input* to normalization, never an artifact's triple.
fn raw_default_triple() -> String {
    inkwell::targets::TargetMachine::get_default_triple()
        .as_str()
        .to_string_lossy()
        .into_owned()
}

fn resolve_once() -> &'static Result<String, String> {
    RESOLVED_TRIPLE.get_or_init(|| {
        let raw = raw_default_triple();
        let Some(platform) = apple_platform(&raw) else {
            let _ = RESOLVED_DEPLOYMENT.set(None);
            return Ok(raw);
        };
        let explicit = EXPLICIT_DEPLOYMENT_TARGET
            .get()
            .and_then(Option::as_deref)
            .map(str::to_string);
        let env = std::env::var(platform.env_var).ok().filter(|v| !v.is_empty());
        let host = host_product_version();
        match resolve_deployment_version(&platform, explicit.as_deref(), env.as_deref(), host.as_deref()) {
            Ok(version) => {
                let normalized = normalize_apple_triple(&raw, &platform, &version);
                let _ = RESOLVED_DEPLOYMENT.set(Some((platform, version)));
                Ok(normalized)
            }
            // Keep the resolver's own text: this is an input-validation failure, and a caller that
            // re-wraps it as `CodegenError::Target` must not stack a second `target/output failed:`.
            Err(CodegenError::Target(message)) => Err(message),
            Err(other) => Err(other.to_string()),
        }
    })
}

/// The exact triple every artifact this process produces carries: LLVM's host default triple with
/// an Apple OS component normalized to `<platform><deployment>`, and every non-Apple triple
/// unchanged. Resolved once per process, so the `TargetMachine`, the module triples copied from it,
/// the link, and the cache key can never disagree.
pub fn resolved_triple() -> Result<String, CodegenError> {
    resolve_once().clone().map_err(CodegenError::Target)
}

/// The resolved Apple deployment target as `(platform, version)`, or `None` on a non-Apple target.
/// The link states this same value, so a direct `ld` or an `ALIGNC_LINKER` override cannot stamp an
/// image differently from the objects that went into it.
pub fn resolved_deployment() -> Result<Option<(ApplePlatform, String)>, CodegenError> {
    resolved_triple()?;
    Ok(RESOLVED_DEPLOYMENT.get().cloned().flatten())
}

/// The `cc` flag that states the resolved deployment target at the link (`None` off Apple).
pub fn apple_min_version_flag() -> Result<Option<String>, CodegenError> {
    Ok(resolved_deployment()?
        .map(|(platform, version)| format!("{}{version}", platform.min_version_flag_prefix)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Apple spelling normalizes to its canonical OS plus an explicit version, the
    /// environment field survives byte-for-byte, and no output contains `darwin<N>`.
    #[test]
    fn apple_triples_normalize_and_non_apple_triples_stay_byte_identical() {
        let cases = [
            ("arm64-apple-darwin27.0.0", "arm64-apple-macosx26.0"),
            ("x86_64-apple-darwin23.6.0", "x86_64-apple-macosx26.0"),
            ("arm64-apple-macosx14.0", "arm64-apple-macosx26.0"),
            ("arm64-apple-macos14", "arm64-apple-macosx26.0"),
            ("arm64-apple-ios17.0", "arm64-apple-ios26.0"),
            ("arm64-apple-ios17.0-simulator", "arm64-apple-ios26.0-simulator"),
            ("arm64-apple-watchos10.0", "arm64-apple-watchos26.0"),
            ("arm64-apple-tvos17.0-simulator", "arm64-apple-tvos26.0-simulator"),
            ("arm64-apple-xros1.0", "arm64-apple-xros26.0"),
            ("arm64-apple-visionos1.0", "arm64-apple-xros26.0"),
        ];
        for (input, expected) in cases {
            let platform = apple_platform(input).unwrap_or_else(|| panic!("{input} is Apple"));
            let out = normalize_apple_triple(input, &platform, "26.0");
            assert_eq!(out, expected, "{input}");
            assert!(!out.contains("darwin"), "no constructed triple may spell darwin: {out}");
        }
        for other in [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-musl",
            "x86_64-pc-windows-msvc",
            "wasm32-unknown-unknown",
            "arm64-apple-bridgeos1.0",
            "arm64",
        ] {
            assert_eq!(apple_platform(other), None, "{other} must not normalize");
        }
    }

    /// The precedence chain is exactly explicit > environment > host > floor, and only the host's
    /// own platform consults the host version.
    #[test]
    fn deployment_precedence_is_explicit_then_env_then_host_then_floor() {
        let mac = apple_platform("arm64-apple-darwin27.0.0").expect("macOS");
        let ios = apple_platform("arm64-apple-ios17.0").expect("iOS");
        let all = resolve_deployment_version(&mac, Some("15.4"), Some("14.0"), Some("27.0"));
        assert_eq!(all.unwrap(), "15.4");
        let env = resolve_deployment_version(&mac, None, Some("14.0"), Some("27.0"));
        assert_eq!(env.unwrap(), "14.0");
        let host = resolve_deployment_version(&mac, None, None, Some("27.0"));
        assert_eq!(host.unwrap(), "27.0");
        let floor = resolve_deployment_version(&mac, None, None, None);
        assert_eq!(floor.unwrap(), mac.floor);
        // A non-host platform never inherits the macOS host version.
        let cross = resolve_deployment_version(&ios, None, None, Some("27.0"));
        assert_eq!(cross.unwrap(), ios.floor);
        // A malformed value is a hard error at its own layer, not a fall-through to the next.
        for bad in ["", "0.1", "1.2.3.4", "14.x", "14..0", "-1", "99999999999"] {
            let error = resolve_deployment_version(&mac, Some(bad), Some("14.0"), Some("27.0"))
                .expect_err("malformed explicit value must fail closed");
            assert!(error.to_string().contains("--deployment-target"), "{bad}: {error}");
        }
        let error = resolve_deployment_version(&mac, None, Some("nope"), Some("27.0"))
            .expect_err("a malformed environment value must fail closed");
        assert!(error.to_string().contains(mac.env_var), "{error}");
    }

    /// Canonicalization is `major.minor`: an OS *patch* level is not a compilation input and must
    /// not reach the triple (and therefore the cache key).
    #[test]
    fn versions_canonicalize_to_major_minor() {
        for (input, expected) in [
            ("26", "26.0"),
            ("26.0", "26.0"),
            ("15.3.1", "15.3"),
            ("10.13.6", "10.13"),
        ] {
            assert_eq!(canonical_version(input, "test").unwrap(), expected, "{input}");
        }
    }
}
