#![allow(clippy::expect_used, clippy::unwrap_used)]
mod context_information;
mod harness;

use libtest_mimic::Arguments;

fn main() {
    let mut args = Arguments::from_args();

    // All tests share a single database — force serial execution to avoid deadlocks.
    if args.test_threads.is_none() {
        args.test_threads = Some(1);
    }

    let mut trials = Vec::new();

    context_information::tests(&mut trials);

    libtest_mimic::run(&args, trials).exit();
}
