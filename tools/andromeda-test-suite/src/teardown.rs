//! Requests whose answer the test does not check.
//!
//! A teardown request is sent whatever happened before, and none of its answers is checked. A teardown answer is still
//! printed by the exchange log, and consumed here so that no result is dropped unread.

/// An answer the test accepts whatever it is.
pub trait AnyAnswer {
    /// Accepts the answer, success or failure.
    fn any_answer(self);
}

impl<T, E> AnyAnswer for Result<T, E> {
    fn any_answer(self) {
        match self {
            Ok(_) | Err(_) => {}
        }
    }
}
