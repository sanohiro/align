//! M15 S3b — parallel codegen, `--cache-stats`, `cache clear`, runtime-archive digest, default-ON.
//!
//! These drive the real `alignc` binary (the parallel build path + CLI surface). Every assertion is
//! on bytes / stderr shape / exit codes — never elapsed time. Each test controls the cache via an
//! explicit `ALIGNC_CACHE` (a per-test temp dir or `off`), except the default-ON smoke gate, which
//! deliberately leaves `ALIGNC_CACHE` unset and pins the XDG root to a temp dir.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NONCE: AtomicU64 = AtomicU64::new(0);
fn nonce() -> u64 {
    NONCE.fetch_add(1, Ordering::Relaxed)
}

fn backend() -> bool {
    align_driver::backend_available()
}

fn cc_available() -> bool {
    Command::new("cc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A ≥3-unit DAG program (main → b → c) written to a fresh temp dir. `main` prints 11.
struct Proj {
    dir: PathBuf,
}

impl Proj {
    fn new(tag: &str) -> Proj {
        let dir = std::env::temp_dir().join(format!("align-s3b-{}-{tag}-{}", std::process::id(), nonce()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("c.align"), "module c\npub fn cval() -> i64 = 1\n").unwrap();
        std::fs::write(dir.join("b.align"), "module b\nimport c\npub fn bval() -> i64 = c.cval() + 10\n").unwrap();
        std::fs::write(dir.join("main.align"), "import b\nfn main() {\n  print(b.bval())\n}\n").unwrap();
        Proj { dir }
    }
    /// `alignc <args>` in the project dir, with `ALIGNC_CACHE` set to `cache` (`"off"` or a path).
    fn alignc(&self, cache: &str, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_alignc"))
            .args(args)
            .current_dir(&self.dir)
            .env("ALIGNC_CACHE", cache)
            .output()
            .expect("spawn alignc")
    }
    fn exe_bytes(&self) -> Vec<u8> {
        std::fs::read(self.dir.join("main")).expect("read built exe")
    }
    fn cache_dir(&self) -> PathBuf {
        self.dir.join("cache")
    }
}

impl Drop for Proj {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ---- P1: parallel build byte-identical to -j 1 --------------------------------------------------

#[test]
fn parallel_build_is_byte_identical_to_serial() {
    if !backend() || !cc_available() {
        return;
    }
    // Cache OFF ⇒ every unit is produced by codegen; -j 1 is serial, -j 4 runs the 3 units across
    // workers. Each object is an independent single-threaded codegen, so the linked exe must match.
    let p1 = Proj::new("serial");
    assert!(p1.alignc("off", &["build", "main.align", "-j", "1"]).status.success());
    let serial = p1.exe_bytes();

    let p4 = Proj::new("parallel");
    assert!(p4.alignc("off", &["build", "main.align", "-j", "4"]).status.success());
    let parallel = p4.exe_bytes();

    assert_eq!(serial, parallel, "parallel (-j 4) build must be byte-identical to serial (-j 1)");
}

// ---- P2: ≥3-unit DAG parallel build twice → byte-identical --------------------------------------

#[test]
fn parallel_dag_build_is_deterministic() {
    if !backend() || !cc_available() {
        return;
    }
    let p = Proj::new("determinism");
    assert!(p.alignc("off", &["build", "main.align", "-j", "4"]).status.success());
    let first = p.exe_bytes();
    assert!(p.alignc("off", &["build", "main.align", "-j", "4"]).status.success());
    let second = p.exe_bytes();
    assert_eq!(first, second, "a ≥3-unit DAG parallel build must be byte-identical across runs");
}

// ---- P3: -j / ALIGNC_JOBS parsing + precedence --------------------------------------------------

#[test]
fn jobs_flag_and_env_are_honored() {
    if !backend() || !cc_available() {
        return;
    }
    let p = Proj::new("jobs");
    let run = |cache: &str, args: &[&str], jobs_env: Option<&str>| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_alignc"));
        cmd.args(args).current_dir(&p.dir).env("ALIGNC_CACHE", cache);
        match jobs_env {
            Some(v) => cmd.env("ALIGNC_JOBS", v),
            None => cmd.env_remove("ALIGNC_JOBS"),
        };
        cmd.output().expect("spawn alignc")
    };

    // The flag wins over the env: a BAD `ALIGNC_JOBS` is never even consulted when `-j` is given.
    assert!(run("off", &["build", "main.align", "-j", "3"], Some("not-a-number")).status.success(), "-j wins; a bad ALIGNC_JOBS is ignored");
    // No flag + a valid env → success (env used).
    assert!(run("off", &["build", "main.align"], Some("2")).status.success(), "a valid ALIGNC_JOBS is honored");
    // No flag + a BAD env → hard error naming ALIGNC_JOBS.
    let bad_env = run("off", &["build", "main.align"], Some("xyz"));
    assert!(!bad_env.status.success(), "a bad ALIGNC_JOBS with no -j must fail");
    assert!(String::from_utf8_lossy(&bad_env.stderr).contains("ALIGNC_JOBS"), "the error must name ALIGNC_JOBS");
    // A zero / non-numeric `-j` value is a hard error.
    assert!(!run("off", &["build", "main.align", "-j", "0"], None).status.success(), "-j 0 is rejected");
    assert!(!run("off", &["build", "main.align", "-j", "abc"], None).status.success(), "-j abc is rejected");
    // `-j` on a non-build verb is rejected.
    let wrong_verb = run("off", &["check", "main.align", "-j", "2"], None);
    assert!(!wrong_verb.status.success(), "-j on `check` is rejected");
    assert!(String::from_utf8_lossy(&wrong_verb.stderr).contains("-j/--jobs is only valid"));
}

// ---- P4: --cache-stats output shape -------------------------------------------------------------

#[test]
fn cache_stats_reports_hits_and_misses() {
    if !backend() || !cc_available() {
        return;
    }
    let p = Proj::new("stats");
    let cache = p.cache_dir();
    let cache = cache.to_str().unwrap();

    // Silent by default (no --cache-stats): no `cache:` lines on stderr.
    let quiet = p.alignc(cache, &["build", "main.align"]);
    assert!(quiet.status.success());
    assert!(!String::from_utf8_lossy(&quiet.stderr).contains("cache:"), "no --cache-stats ⇒ silent");

    // Fresh cache dir for the stats run so the first build is all-miss.
    let p2 = Proj::new("stats2");
    let cache2 = p2.cache_dir();
    let cache2 = cache2.to_str().unwrap();
    let cold = p2.alignc(cache2, &["build", "main.align", "--cache-stats"]);
    assert!(cold.status.success());
    let cold_err = String::from_utf8_lossy(&cold.stderr);
    assert!(cold_err.contains("miss"), "cold --cache-stats reports misses:\n{cold_err}");
    assert!(cold_err.contains("3 unit(s):") && cold_err.contains("0 hit, 3 miss"), "cold summary line:\n{cold_err}");

    let hot = p2.alignc(cache2, &["build", "main.align", "--cache-stats"]);
    assert!(hot.status.success());
    let hot_err = String::from_utf8_lossy(&hot.stderr);
    assert!(hot_err.contains("main hit") && hot_err.contains("b hit") && hot_err.contains("c hit"), "hot per-unit hit lines:\n{hot_err}");
    assert!(hot_err.contains("3 unit(s):") && hot_err.contains("3 hit, 0 miss"), "hot summary line:\n{hot_err}");

    // --cache-stats with the cache disabled reports "disabled".
    let off = p2.alignc("off", &["build", "main.align", "--cache-stats"]);
    assert!(off.status.success());
    assert!(String::from_utf8_lossy(&off.stderr).contains("cache: disabled"), "off + --cache-stats ⇒ disabled note");
}

// ---- P5: cache clear removes the root, next build all-miss then all-hit -------------------------

#[test]
fn cache_clear_resets_the_cache() {
    if !backend() || !cc_available() {
        return;
    }
    let p = Proj::new("clear");
    let cache = p.cache_dir();
    let cache = cache.to_str().unwrap();

    // Populate.
    assert!(p.alignc(cache, &["build", "main.align"]).status.success());
    assert!(p.cache_dir().join("actions").join("codegen").exists(), "the build populated the action cache");

    // Clear.
    let cleared = Command::new(env!("CARGO_BIN_EXE_alignc"))
        .args(["cache", "clear"])
        .env("ALIGNC_CACHE", cache)
        .output()
        .expect("spawn alignc cache clear");
    assert!(cleared.status.success(), "cache clear succeeds");
    assert!(!p.cache_dir().join("actions").exists(), "cache clear removed the actions subtree");

    // Next build: all-miss (fresh), then all-hit.
    let after = p.alignc(cache, &["build", "main.align", "--cache-stats"]);
    assert!(after.status.success());
    assert!(String::from_utf8_lossy(&after.stderr).contains("0 hit, 3 miss"), "post-clear build is all-miss");
    let again = p.alignc(cache, &["build", "main.align", "--cache-stats"]);
    assert!(String::from_utf8_lossy(&again.stderr).contains("3 hit, 0 miss"), "the repopulated build is all-hit");

    // `cache clear` is safe on an absent root.
    let empty = std::env::temp_dir().join(format!("align-s3b-empty-{}-{}", std::process::id(), nonce()));
    let safe = Command::new(env!("CARGO_BIN_EXE_alignc"))
        .args(["cache", "clear"])
        .env("ALIGNC_CACHE", &empty)
        .output()
        .expect("spawn");
    assert!(safe.status.success(), "cache clear on an absent root is safe");
}

// ---- P6: runtime-archive digest -----------------------------------------------------------------

#[test]
fn runtime_archive_digest_is_content_addressed() {
    if !backend() {
        return;
    }
    // The archive-bytes digest (doc-10 §6.3 future link-key input) is available and stable across
    // calls (the on-disk `.a` is unchanged): a `touch`-but-identical archive yields the same digest.
    let d1 = align_driver::runtime_archive_digest().expect("runtime archive digest");
    let d2 = align_driver::runtime_archive_digest().expect("runtime archive digest");
    assert_eq!(d1, d2, "the same archive bytes hash to the same digest (touched-but-identical passes)");
    // Byte-changed content hashes differently (the teeth): a distinct byte buffer ⇒ distinct digest.
    assert_ne!(
        align_driver::Hash128::of(b"an archive"),
        align_driver::Hash128::of(b"a different archive"),
        "changed bytes ⇒ changed digest"
    );
}

// ---- P7: default-ON smoke (no ALIGNC_CACHE) -----------------------------------------------------

#[test]
fn default_on_second_build_all_hit_and_byte_identical() {
    if !backend() || !cc_available() {
        return;
    }
    let p = Proj::new("defaulton");
    // Pin the XDG cache root to a temp dir and leave ALIGNC_CACHE UNSET → default-ON uses it.
    let xdg = p.dir.join("xdg");
    let run = |stats: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_alignc"));
        cmd.arg("build").arg("main.align");
        if stats {
            cmd.arg("--cache-stats");
        }
        cmd.current_dir(&p.dir)
            .env_remove("ALIGNC_CACHE") // unset ⇒ default-ON
            .env("XDG_CACHE_HOME", &xdg)
            .output()
            .expect("spawn alignc")
    };

    let cold = run(true);
    assert!(cold.status.success(), "default-ON cold build: {}", String::from_utf8_lossy(&cold.stderr));
    assert!(String::from_utf8_lossy(&cold.stderr).contains("miss"), "cold build misses");
    let cold_exe = p.exe_bytes();
    // The default-ON build created the cache under the pinned XDG root.
    assert!(xdg.join("alignc").exists(), "default-ON populated the XDG cache root");

    let hot = run(true);
    assert!(hot.status.success());
    assert!(String::from_utf8_lossy(&hot.stderr).contains("3 hit, 0 miss"), "default-ON second build is all-hit");
    assert_eq!(cold_exe, p.exe_bytes(), "cache-hit executable is byte-identical to the cold build");
}

// ---- SV: identical cross-process producers publish one complete immutable result ----------------

fn tree_contains_stage(path: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(path) else { return false };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with(".cache-stage-") {
            return true;
        }
        if path.is_dir() && tree_contains_stage(&path) {
            return true;
        }
    }
    false
}

#[test]
fn concurrent_identical_builds_share_complete_actions() {
    if !backend() || !cc_available() {
        return;
    }
    const N: usize = 4;
    let projects: Vec<Proj> = (0..N).map(|i| Proj::new(&format!("same-key-{i}"))).collect();
    let shared = projects[0].dir.join("shared-cache");

    let children: Vec<std::process::Child> = projects
        .iter()
        .map(|p| {
            Command::new(env!("CARGO_BIN_EXE_alignc"))
                .args(["build", "main.align", "-j", "2"])
                .current_dir(&p.dir)
                .env("ALIGNC_CACHE", &shared)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("spawn concurrent alignc")
        })
        .collect();

    for (i, child) in children.into_iter().enumerate() {
        let out = child.wait_with_output().expect("wait concurrent alignc");
        assert!(out.status.success(), "producer {i} failed: {}", String::from_utf8_lossy(&out.stderr));
        let run = Command::new(projects[i].dir.join("main")).output().expect("run concurrent output");
        assert!(run.status.success());
        assert_eq!(run.stdout, b"11\n", "producer {i} published/linked the wrong bytes");
    }

    let action_dir = shared.join("actions").join("codegen");
    let actions = std::fs::read_dir(&action_dir)
        .expect("action dir")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .count();
    assert_eq!(actions, 3, "four identical 3-unit builds converge on exactly three immutable actions");
    assert!(!tree_contains_stage(&shared), "successful racing producers leave no private staging files");

    // Every action produced under the race is readable: a fresh process must see an all-hit build.
    let hot = projects[0].alignc(shared.to_str().unwrap(), &["build", "main.align", "--cache-stats"]);
    assert!(hot.status.success(), "post-race hit failed: {}", String::from_utf8_lossy(&hot.stderr));
    assert!(String::from_utf8_lossy(&hot.stderr).contains("3 hit, 0 miss"));
}

#[test]
fn concurrent_different_same_basename_builds_keep_distinct_actions() {
    if !backend() || !cc_available() {
        return;
    }
    let a = Proj::new("different-key-a");
    let b = Proj::new("different-key-b");
    std::fs::write(b.dir.join("c.align"), "module c\npub fn cval() -> i64 = 2\n").unwrap();
    let shared = a.dir.join("shared-cache");

    let spawn = |p: &Proj| {
        Command::new(env!("CARGO_BIN_EXE_alignc"))
            .args(["build", "main.align", "-j", "2"])
            .current_dir(&p.dir)
            .env("ALIGNC_CACHE", &shared)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn concurrent alignc")
    };
    let achild = spawn(&a);
    let bchild = spawn(&b);
    let (ao, bo) = (achild.wait_with_output().unwrap(), bchild.wait_with_output().unwrap());
    assert!(ao.status.success(), "first build failed: {}", String::from_utf8_lossy(&ao.stderr));
    assert!(bo.status.success(), "second build failed: {}", String::from_utf8_lossy(&bo.stderr));
    assert_eq!(Command::new(a.dir.join("main")).output().unwrap().stdout, b"11\n");
    assert_eq!(Command::new(b.dir.join("main")).output().unwrap().stdout, b"12\n");

    // b/main are byte-identical shared actions; c has two implementation keys. A racing slot-pointer
    // update may name either c key, but both full actions remain immutable and directly hittable.
    let actions = std::fs::read_dir(shared.join("actions").join("codegen"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .count();
    assert_eq!(actions, 4, "two same-basename DAGs retain both distinct c actions plus shared b/main");
    for p in [&a, &b] {
        let hot = p.alignc(shared.to_str().unwrap(), &["build", "main.align", "--cache-stats"]);
        assert!(hot.status.success());
        assert!(String::from_utf8_lossy(&hot.stderr).contains("3 hit, 0 miss"));
    }
}
