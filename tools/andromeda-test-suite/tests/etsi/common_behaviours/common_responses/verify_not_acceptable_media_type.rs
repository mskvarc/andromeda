//! A GET whose `Accept` field names neither `application/json` nor `application/ld+json`, or names
//! `application/geo+json` for an operation that does not answer it, is refused with 406 (clause 6.3.4).

mod e049_01;
mod e049_02;
