//! Per-stack orchestration: pre-clean, bring up, run the suites, tear down. The
//! teardown is an RAII guard so a failing or panicking suite run still stops the
//! stack, exactly as the shell tier's `trap cleanup EXIT` did.

use crate::{
    certs,
    compose::{self, ImageBuild},
    error::XtaskError,
    local_broker::{self, BROKER_PORT, ForwardedHeaders, TRUSTED_PORT},
    stack::{Stack, StackName},
};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
};

/// Callback host a containerized broker uses to reach the host test process.
const CONTAINER_CALLBACK_HOST: &str = "host.docker.internal";
/// Callback host a host-process broker uses to reach the host test process.
const LOCAL_CALLBACK_HOST: &str = "127.0.0.1";
/// Environment variable naming the broker-visible callback host.
const CALLBACK_HOST_ENV: &str = "ANDROMEDA_TEST_CALLBACK_HOST";

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
    suites: &[&'static str],
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

    if stack.name == StackName::Http3 {
        certs::ensure(repository_root)?;
    }
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
            teardown
                .brokers
                .push(local_broker::spawn_broker(repository_root, BROKER_PORT, ForwardedHeaders::Untrusted)?);
            teardown
                .brokers
                .push(local_broker::spawn_broker(repository_root, TRUSTED_PORT, ForwardedHeaders::Trusted)?);
            local_broker::await_ready()?;
            LOCAL_CALLBACK_HOST
        }
        BrokerMode::Containerized => CONTAINER_CALLBACK_HOST,
    };

    run_suites(repository_root, stack, suites, callback_host, cargo_args)
}

/// Run the given suites against the already-running stack.
fn run_suites(repository_root: &Path, stack: &Stack, suites: &[&str], callback_host: &str, cargo_args: &[String]) -> Result<(), XtaskError> {
    let mut args: Vec<String> = vec!["test".to_owned()];
    for suite in suites {
        args.push("--test".to_owned());
        args.push((*suite).to_owned());
    }
    args.push("--locked".to_owned());
    // Drive every selected suite to completion: one suite's failure must not
    // hide the verdict of the suites cargo would otherwise skip after it.
    args.push("--no-fail-fast".to_owned());
    args.push("--".to_owned());
    args.push(format!("--test-threads={}", stack.test_threads));
    args.extend(cargo_args.iter().cloned());

    let mut env: Vec<(&str, String)> = stack.env.iter().map(|(key, value)| (*key, (*value).to_owned())).collect();
    env.push((CALLBACK_HOST_ENV, callback_host.to_owned()));

    let status = Command::new("cargo").args(&args).envs(env).current_dir(repository_root).status()?;
    if status.success() { Ok(()) } else { Err(XtaskError::CargoTest) }
}
