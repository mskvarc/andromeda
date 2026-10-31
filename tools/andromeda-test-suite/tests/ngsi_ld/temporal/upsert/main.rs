//! The ngsi-ld tree, its `temporal` area, its `upsert` part: one test per scenario of the broker's NGSI-LD behaviour,
//! each a sequence of requests and the requirements on their answers, in the shared default tenant unless the test
//! names a private one.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod n_tmp_ups_0001;
mod n_tmp_ups_0002;
mod n_tmp_ups_0003;
mod n_tmp_ups_0007;
mod n_tmp_ups_0008;
mod n_tmp_ups_0009;
mod n_tmp_ups_0010;
mod n_tmp_ups_0011;
mod n_tmp_ups_0012;
mod n_tmp_ups_0013;
mod n_tmp_ups_0014;
mod n_tmp_ups_0015;
mod n_tmp_ups_0016;
mod n_tmp_ups_0017;
mod n_tmp_ups_0018;
mod n_tmp_ups_0019;
mod n_tmp_ups_0020;
mod n_tmp_ups_2001;
mod n_tmp_ups_2301;
mod n_tmp_ups_2302;
mod n_tmp_ups_2303;
mod n_tmp_ups_2304;
mod n_tmp_ups_2305;
mod n_tmp_ups_2306;
mod n_tmp_ups_2307;
mod n_tmp_ups_2308;
mod n_tmp_ups_2310;
mod n_tmp_ups_2700;
mod n_tmp_ups_2701;
mod n_tmp_ups_2702;
mod n_tmp_ups_2703;
mod n_tmp_ups_2705;
mod n_tmp_ups_2706;
mod n_tmp_ups_2707;
mod n_tmp_ups_2708;
mod n_tmp_ups_2709;
mod n_tmp_ups_2710;
mod n_tmp_ups_2711;
mod n_tmp_ups_9001;
mod n_tmp_ups_9002;
mod n_tmp_ups_9003;
mod n_tmp_ups_9004;
mod n_tmp_ups_9005;
mod n_tmp_ups_9006;
mod n_tmp_ups_9007;
mod n_tmp_ups_9008;
mod n_tmp_ups_9009;
mod n_tmp_ups_9010;
mod n_tmp_ups_9014;
mod n_tmp_ups_9015;
mod n_tmp_ups_9016;
mod n_tmp_ups_9017;
mod n_tmp_ups_9018;
mod n_tmp_ups_9019;
mod n_tmp_ups_9020;
mod n_tmp_ups_9023;
mod n_tmp_ups_9026;
mod n_tmp_ups_9027;
mod n_tmp_ups_9029;
mod n_tmp_ups_9094;
mod o_tmp_ups_0005;
mod o_tmp_ups_0006;
mod o_tmp_ups_2704;
mod o_tmp_ups_9011;
mod o_tmp_ups_9012;
mod o_tmp_ups_9013;
mod o_tmp_ups_9021;
mod o_tmp_ups_9022;
mod o_tmp_ups_9024;
mod o_tmp_ups_9025;
mod o_tmp_ups_9028;
mod o_tmp_ups_9030;
