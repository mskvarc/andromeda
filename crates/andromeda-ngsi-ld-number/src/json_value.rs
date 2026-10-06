//! Exact-number handling for JSON read with `serde_json`'s arbitrary-precision reader.

pub mod json_pointer;
mod reader_private_members;
pub mod text_admission;
pub mod value_census;

/// The deepest container nesting `serde_json`'s reader accepts in one text:
/// its recursion budget of 128 is spent on entering each container, and
/// refused at zero. Every request body and every stored document is read with
/// that reader and that budget.
pub const READER_NESTING_LIMIT: usize = 127;
