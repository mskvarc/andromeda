//! The ngsi-ld tree, its `temporal` area, its `retrieve` part: one test per scenario of the broker's NGSI-LD behaviour,
//! each a sequence of requests and the requirements on their answers, in the shared default tenant unless the test
//! names a private one.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod n_tmp_ret_0001;
mod n_tmp_ret_0002;
mod n_tmp_ret_0003;
mod n_tmp_ret_0004;
mod n_tmp_ret_4002;
mod n_tmp_ret_4003;
mod n_tmp_ret_4004;
mod n_tmp_ret_4005;
mod n_tmp_ret_9001;
mod n_tmp_ret_9002;
mod n_tmp_ret_9003;
mod n_tmp_ret_9004;
mod n_tmp_ret_9005;
mod n_tmp_ret_9006;
mod n_tmp_ret_9007;
mod n_tmp_ret_9008;
mod n_tmp_ret_9010;
mod n_tmp_ret_9011;
mod n_tmp_ret_9012;
mod n_tmp_ret_9013;
mod n_tmp_ret_9014;
mod n_tmp_ret_9015;
mod n_tmp_ret_9018;
mod n_tmp_ret_9019;
mod n_tmp_ret_9020;
mod n_tmp_ret_9025;
mod n_tmp_ret_9028;
mod n_tmp_ret_9029;
mod n_tmp_ret_9030;
mod n_tmp_ret_9031;
mod n_tmp_ret_9032;
mod n_tmp_ret_9033;
mod n_tmp_ret_9034;
mod n_tmp_ret_9037;
mod n_tmp_ret_9040;
mod n_tmp_ret_9041;
mod n_tmp_ret_9042;
mod n_tmp_ret_9043;
mod n_tmp_ret_9044;
mod n_tmp_ret_9046;
mod n_tmp_ret_9050;
mod n_tmp_ret_9051;
mod n_tmp_ret_9052;
mod n_tmp_ret_9053;
mod n_tmp_ret_9055;
mod n_tmp_ret_9057;
mod n_tmp_ret_9058;
mod n_tmp_ret_9060;
mod n_tmp_ret_9061;
mod n_tmp_ret_9062;
mod n_tmp_ret_9071;
mod n_tmp_ret_9072;
mod n_tmp_ret_9074;
mod o_tmp_ret_4001;
mod o_tmp_ret_9009;
mod o_tmp_ret_9016;
mod o_tmp_ret_9017;
mod o_tmp_ret_9021;
mod o_tmp_ret_9022;
mod o_tmp_ret_9023;
mod o_tmp_ret_9024;
mod o_tmp_ret_9026;
mod o_tmp_ret_9027;
mod o_tmp_ret_9036;
mod o_tmp_ret_9038;
mod o_tmp_ret_9039;
mod o_tmp_ret_9047;
mod o_tmp_ret_9048;
mod o_tmp_ret_9049;
mod o_tmp_ret_9054;
mod o_tmp_ret_9056;
mod o_tmp_ret_9059;
mod o_tmp_ret_9066;
mod o_tmp_ret_9067;
mod o_tmp_ret_9068;
mod o_tmp_ret_9069;
mod o_tmp_ret_9073;
