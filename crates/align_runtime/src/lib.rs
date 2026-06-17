//! ABI sketch of the minimal runtime (`docs/impl/06-runtime-std.md`).
//!
//! No GC. Holds only "the minimum the language requires" such as arena / parallelism
//! / panic / mutable buffers. Lifetimes and free points are already settled by the
//! compiler (MIR); the runtime allocates/frees exactly as told.
//!
//! Since codegen does not yet emit objects in M0, this stays a template to pin down
//! the ABI shape (`#[no_mangle]` exposure is enabled once codegen is wired up).

/// Immediate abort called on arithmetic traps / invariant violations (`draft.md` §5).
/// Normally not called since overflow defaults to wrap.
pub fn panic_abort(msg: &str) -> ! {
    eprintln!("align: panic: {msg}");
    std::process::abort();
}

/// Sketch of a bump allocator. Unused in M0 (arena is M3).
pub struct Arena {
    _private: (),
}

impl Arena {
    pub fn begin() -> Arena {
        Arena { _private: () }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_begin_smoke() {
        let _a = Arena::begin();
    }
}
