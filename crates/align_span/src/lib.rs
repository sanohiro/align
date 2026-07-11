//! Source locations and file management. Every IR node carries a [`Span`] so
//! diagnostics can point back to the original source
//! (`docs/impl/01-pipeline.md`, cross-cutting crate).

/// Identifier of a source file. Handed out by [`SourceMap`].
pub type FileId = u32;

/// Byte-offset range `[lo, hi)` within a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub file: FileId,
    pub lo: u32,
    pub hi: u32,
}

impl Span {
    pub fn new(file: FileId, lo: u32, hi: u32) -> Span {
        Span { file, lo, hi }
    }

    /// Smallest span containing both spans. Assumes the same file.
    pub fn merge(self, other: Span) -> Span {
        debug_assert_eq!(self.file, other.file);
        Span {
            file: self.file,
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
}

/// Name and contents of a single file.
pub struct SourceFile {
    pub id: FileId,
    pub name: String,
    pub src: String,
}

impl SourceFile {
    /// Convert a byte offset to a 1-based (line, column) (for diagnostics).
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let off = (offset as usize).min(self.src.len());
        let mut line = 1u32;
        let mut col = 1u32;
        let mut byte_idx = 0usize;
        for c in self.src.chars() {
            if byte_idx >= off {
                break;
            }
            byte_idx += c.len_utf8();
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }
}

/// Holds all source files.
#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap { files: Vec::new() }
    }

    pub fn add_file(&mut self, name: impl Into<String>, src: impl Into<String>) -> FileId {
        let id = self.files.len() as FileId;
        self.files.push(SourceFile {
            id,
            name: name.into(),
            src: src.into(),
        });
        id
    }

    pub fn get(&self, id: FileId) -> &SourceFile {
        &self.files[id as usize]
    }

    /// All files, in `FileId` order (index == id). Used by MIR lowering to build a byte-offset →
    /// (line, col) table for debug locations (`docs/impl/09-explain-opt.md`, Slice 3b).
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_col_utf8() {
        // "あ" is 3 bytes (UTF-8).
        // Line 1: "あ\n" (length: 3 + 1 = 4 bytes)
        // Line 2: "い" (length: 3 bytes)
        let sf = SourceFile {
            id: 0,
            name: "test.rs".to_string(),
            src: "あ\nい".to_string(),
        };

        // Offset 0 (before "あ"): line 1, col 1
        assert_eq!(sf.line_col(0), (1, 1));
        // Offset 3 (after "あ", before "\n"): line 1, col 2
        assert_eq!(sf.line_col(3), (1, 2));
        // Offset 4 (after "\n", before "い"): line 2, col 1
        assert_eq!(sf.line_col(4), (2, 1));
        // Offset 7 (after "い"): line 2, col 2
        assert_eq!(sf.line_col(7), (2, 2));
    }
}
