//! Per-stack orchestration: pre-clean, bring up, run the suites, tear down. The
//! teardown is an RAII guard so a failing or panicking suite run still stops the
//! stack, exactly as the shell tier's `trap cleanup EXIT` did.

use crate::{
    compose::{self, ImageBuild},
    error::XtaskError,
    local_broker::{self, HostBroker},
    stack::{Stack, Suite},
    test_binaries::{TestManifest, run},
};
use std::{
    env,
    path::{Path, PathBuf},
    process::{Child, Command},
};

/// Callback host a containerized broker uses to reach the host test process.
const CONTAINER_CALLBACK_HOST: &str = "host.docker.internal";
/// Callback host a host-process broker uses to reach the host test process.
const LOCAL_CALLBACK_HOST: &str = "127.0.0.1";
/// Environment variable naming the broker-visible callback host.
const CALLBACK_HOST_ENV: &str = "ANDROMEDA_TEST_CALLBACK_HOST";
/// Environment variable naming a prebuilt-test manifest. When set, the suites are
/// run from their packaged binaries instead of recompiled through `cargo test`.
const TEST_MANIFEST_ENV: &str = "ANDROMEDA_XTASK_TEST_MANIFEST";

/// Whether the base tier's brokers run in containers or as host processes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrokerMode {
    /// Containerized brokers brought up by the compose `broker` profile.
    Containerized,
    /// Two host processes built and spawned by the runner.
    Local,
}

/// Tears the stack down when it leaves scope: kills any spawned host brokers, then
/// runs `docker compose down` unless the run asked to keep the stack.
struct StackTeardown<'root> {
    repository_root: &'root Path,
    compose_file: PathBuf,
    profile: Option<&'static str>,
    keep: bool,
    brokers: Vec<Child>,
}

impl Drop for StackTeardown<'_> {
    fn drop(&mut self) {
        for broker in &mut self.brokers {
            // Best-effort: a broker that already exited cannot be killed or waited on.
            let _killed = broker.kill();
            let _waited = broker.wait();
        }
        if !self.keep {
            compose::down(self.repository_root, &self.compose_file, self.profile);
        }
    }
}

/// Bring a stack up, run the requested subset of its suites, and tear it down.
///
/// # Errors
///
/// Propagates any preflight, certificate, compose, build, readiness, or test failure.
pub(crate) fn run_stack(
    repository_root: &Path,
    stack: &'static Stack,
    suites: &[&'static Suite],
    keep: bool,
    broker_mode: BrokerMode,
    image_build: ImageBuild,
    cargo_args: &[String],
) -> Result<(), XtaskError> {
    // With host brokers the compose `broker` profile is omitted, leaving only the
    // infrastructure services up; otherwise the stack's own profile decides.
    let effective_profile = match broker_mode {
        BrokerMode::Local => None,
        BrokerMode::Containerized => stack.profile,
    };
    let compose_file = repository_root.join(stack.compose_file);

    stack.material.provision(repository_root)?;
    if broker_mode == BrokerMode::Local {
        local_broker::preflight_ports()?;
    }

    compose::down(repository_root, &compose_file, effective_profile);
    let mut teardown = StackTeardown {
        repository_root,
        compose_file,
        profile: effective_profile,
        keep,
        brokers: Vec::new(),
    };
    compose::up(repository_root, &teardown.compose_file, effective_profile, image_build)?;

    let callback_host = match broker_mode {
        BrokerMode::Local => {
            local_broker::build(repository_root)?;
            for broker in HostBroker::ALL {
                teardown.brokers.push(local_broker::spawn_broker(repository_root, broker)?);
            }
            local_broker::await_ready()?;
            LOCAL_CALLBACK_HOST
        }
        BrokerMode::Containerized => CONTAINER_CALLBACK_HOST,
    };

    run_suites(repository_root, stack, suites, callback_host, cargo_args)
}

/// Run the given suites against the already-running stack, either from a prebuilt
/// manifest when CI sets [`TEST_MANIFEST_ENV`] or by compiling through `cargo test`.
fn run_suites(repository_root: &Path, stack: &Stack, suites: &[&Suite], callback_host: &str, cargo_args: &[String]) -> Result<(), XtaskError> {
    let mut env: Vec<(&str, String)> = stack.env.iter().map(|(key, value)| (*key, (*value).to_owned())).collect();
    env.push((CALLBACK_HOST_ENV, callback_host.to_owned()));

    match env::var(TEST_MANIFEST_ENV) {
        Ok(manifest_path) => run_prebuilt(&manifest_path, suites, &env, cargo_args, repository_root),
        Err(_) => run_via_cargo(repository_root, suites, &env, cargo_args),
    }
}

/// Run each suite from its packaged binary, listed in the manifest at `manifest_path`.
fn run_prebuilt(manifest_path: &str, suites: &[&Suite], env: &[(&str, String)], harness_args: &[String], working_directory: &Path) -> Result<(), XtaskError> {
    let manifest = TestManifest::load(Path::new(manifest_path))?;
    let mut all_passed = true;
    for suite in suites {
        let binary = manifest.resolve(suite.name)?;
        // Drive every selected suite to completion regardless of an earlier
        // failure, matching the `--no-fail-fast` the cargo path passes.
        let passed = run::run_suite(&binary, suite.test_threads, harness_args, env, working_directory)?;
        all_passed &= passed;
    }
    if all_passed { Ok(()) } else { Err(XtaskError::CargoTest) }
}

/// Compile and run the suites through `cargo test`, the path used for local development.
fn run_via_cargo(repository_root: &Path, suites: &[&Suite], env: &[(&str, String)], cargo_args: &[String]) -> Result<(), XtaskError> {
    let mut all_passed = true;
    for suite in suites {
        // One `cargo test` invocation per suite so each runs at the thread count
        // its isolation allows; a single shared invocation could only carry one
        // `--test-threads` for every suite at once.
        let mut args: Vec<String> = vec![
            "test".to_owned(),
            "--test".to_owned(),
            suite.name.to_owned(),
            "--locked".to_owned(),
            // One suite's failure must not hide the tests cargo would skip after it.
            "--no-fail-fast".to_owned(),
            "--".to_owned(),
            format!("--test-threads={}", suite.test_threads),
        ];
        args.extend(cargo_args.iter().cloned());

        let status = Command::new("cargo")
            .args(&args)
            .envs(env.iter().map(|(key, value)| (*key, value)))
            .current_dir(repository_root)
            .status()?;
        all_passed &= status.success();
    }
    if all_passed { Ok(()) } else { Err(XtaskError::CargoTest) }
}
