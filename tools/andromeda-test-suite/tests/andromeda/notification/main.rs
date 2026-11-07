//! The andromeda tree, its `notification` area: one test per scenario of what this broker does beyond the NGSI-LD
//! specification or chooses where the specification leaves it open, each a sequence of requests and the requirements on
//! their answers, against the brokers of the andromeda stack.

mod delivery;
mod payload;
mod retry;
mod triggering;
