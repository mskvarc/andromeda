//! The ngsi-ld tree, its `temporal` area, its `modify_instance` part: one test per scenario of the broker's NGSI-LD
//! behaviour, each a sequence of requests and the requirements on their answers, in the shared default tenant unless
//! the test names a private one.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod n_tmp_mod_0001;
mod n_tmp_mod_0003;
mod n_tmp_mod_0004;
mod n_tmp_mod_0005;
mod n_tmp_mod_0007;
mod n_tmp_mod_0008;
mod n_tmp_mod_0009;
mod n_tmp_mod_0010;
mod n_tmp_mod_0011;
mod n_tmp_mod_2701;
mod n_tmp_mod_7601;
mod n_tmp_mod_7602;
mod n_tmp_mod_9001;
mod n_tmp_mod_9002;
mod n_tmp_mod_9003;
mod n_tmp_mod_9004;
mod o_tmp_mod_0002;
mod o_tmp_mod_0006;
mod o_tmp_mod_2700;
mod o_tmp_mod_9005;
