//! The ngsi-ld tree, its `temporal` area, its `delete` part: one test per scenario of the broker's NGSI-LD behaviour,
//! each a sequence of requests and the requirements on their answers, in the shared default tenant unless the test
//! names a private one.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod n_tmp_del_0001;
mod o_tmp_del_2700;
mod o_tmp_del_9001;
mod o_tmp_del_9002;
mod o_tmp_del_9003;
mod o_tmp_del_9004;
mod o_tmp_del_9005;
mod o_tmp_del_9006;
