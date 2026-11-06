//! JSONX text format and BJSONX binary format (see `specification/`).
//! Zero dependencies. Library code contains no `unsafe`.
mod text;
mod compile;
mod schema;
mod binary;
pub use binary::*;
pub use schema::*;
pub use text::*;
pub use compile::{compile_file, compile_file_deps, compile_str, Loader};
pub mod rt;

use std::fmt;

/// Library error. `code` values are shared by every JSONX implementation.
#[derive(Debug, Clone)]
pub struct Error {
    pub code: String,
    pub message: String,
    pub pos: Option<usize>,
    pub line: Option<usize>,
    pub col: Option<usize>,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.col) {
            (Some(l), Some(c)) => write!(f, "{} at {}:{}: {}", self.code, l, c, self.message),
            _ => write!(f, "{}: {}", self.code, self.message),
        }
    }
}
impl std::error::Error for Error {}
pub type Res<T> = Result<T, Error>;
pub(crate) fn err<T>(code: &str, msg: impl Into<String>) -> Res<T> {
    Err(Error { code: code.into(), message: msg.into(), pos: None, line: None, col: None })
}
pub(crate) fn errp<T>(code: &str, msg: impl Into<String>, pos: usize) -> Res<T> {
    Err(Error { code: code.into(), message: msg.into(), pos: Some(pos), line: None, col: None })
}
pub(crate) fn line_col(src: &[char], pos: usize) -> (usize, usize) {
    let (mut line, mut ls, mut i) = (1, 0, 0);
    while i < pos && i < src.len() {
        match src[i] {
            '\n' => { line += 1; ls = i + 1; }
            '\r' => { if i + 1 < src.len() && src[i + 1] == '\n' { i += 1; } line += 1; ls = i + 1; }
            _ => {}
        }
        i += 1;
    }
    (line, pos.min(src.len()) - ls + 1)
}
