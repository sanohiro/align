//! ソース位置とファイル管理。全 IR ノードが [`Span`] を持ち、診断で元ソースを指す
//! (`docs/impl/01-pipeline.md` 横断クレート)。

/// ソースファイルの識別子。[`SourceMap`] が払い出す。
pub type FileId = u32;

/// ファイル内のバイトオフセット範囲 `[lo, hi)`。
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

    /// 2つの span を内包する最小の span。同一ファイル前提。
    pub fn merge(self, other: Span) -> Span {
        debug_assert_eq!(self.file, other.file);
        Span {
            file: self.file,
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
}

/// 1ファイルの名前と中身。
pub struct SourceFile {
    pub id: FileId,
    pub name: String,
    pub src: String,
}

impl SourceFile {
    /// バイトオフセットを 1 始まりの (行, 列) に変換する (診断表示用)。
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let off = (offset as usize).min(self.src.len());
        let mut line = 1u32;
        let mut col = 1u32;
        for &b in &self.src.as_bytes()[..off] {
            if b == b'\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }
}

/// 全ソースファイルを保持する。
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
}
