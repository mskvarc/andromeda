//! Requests whose answer the test does not check.
//!
//! An ETSI case sends its teardown requests whatever happened before, and checks none of their answers: the Hurl
//! port's `HTTP *`. A teardown answer is still printed by the exchange log, and consumed here so that no result is
//! dropped unread.

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
