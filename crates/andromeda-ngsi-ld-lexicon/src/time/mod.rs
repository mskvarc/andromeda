//! Every conversion between text and an NGSI-LD time value, and the resolution the broker holds one at.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.3 defines three time value forms,
//! `DateTime`, `Date` and `Time`, and pins each completely: every component
//! present, the literal `Z` and no other zone expression, a fraction of at most
//! six digits, a comma accepted for the decimal point in requests. The broker
//! holds every one of them to those rules wherever it reads one, a request
//! body, a query parameter, a header, a stored document, a peer's answer, and
//! writes each back in one spelling. Everything that turns text into a time
//! value, a time value into text, or a finer instant into a microsecond one
//! lives here, so no two paths can apply different rules.
//!
//! * [`lexical`] reads the three forms character by character;
//! * [`date_time`], [`date`] and [`time_of_day`] build the value types on it;
//! * [`value`] decides which form a literal is written in;
//! * [`rendering`] spells a held value back;
//! * [`resolution`] brings a broker-produced value to the microsecond;
//! * [`instant_serde`] carries the rules into serde models;
//! * [`written_date_time`] and [`system_instant`] are the instants an
//!   attribute instance carries, as a client wrote them and as the broker
//!   stamps them.

pub mod date;
pub mod date_time;
pub mod instant_serde;
pub mod lexical;
pub mod rendering;
pub mod resolution;
pub mod system_instant;
pub mod time_of_day;
pub mod value;
pub mod written_date_time;
