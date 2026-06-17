//! 最小ランタイムの ABI スケッチ (`docs/impl/06-runtime-std.md`)。
//!
//! GC なし。arena / 並列 / panic / 可変バッファ など「言語が要求する最小」だけを持つ。
//! 寿命・解放点はコンパイラ (MIR) が確定済みで、ランタイムは与えられた通り確保/解放する。
//!
//! M0 では codegen がまだ object を出さないため、ここは ABI の形を固定するための
//! 雛形に留める (`#[no_mangle]` 公開は codegen 結線時に有効化する)。

/// 算術トラップ・不変条件違反で呼ぶ即時アボート (`draft.md` §5)。
/// overflow は既定 wrap なので通常は呼ばない。
pub fn panic_abort(msg: &str) -> ! {
    eprintln!("align: panic: {msg}");
    std::process::abort();
}

/// bump allocator のスケッチ。M0 未使用 (arena は M3)。
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
