//! Command-line surface: `cargo xtask itest <selector> [flags] [-- <cargo test args>]`.

use crate::{
    compose::ImageBuild,
    error::XtaskError,
    etsi::EtsiArgs,
    lifecycle::{self, BrokerMode},
    sql_format::{self, FormatMode},
    stack::{OwnedSuite, STACKS, Stack, StackName, Suite, stack_for_suite},
    test_binaries::build,
};
use clap::{ArgGroup, Args, Parser, Subcommand};
use std::{
    env,
    path::{Path, PathBuf},
};

/// Environment variable that, set to `1`, forces `--no-build` without the flag.
/// CI sets it once for the whole `xtask itest` invocation.
const NO_BUILD_ENV: &str = "ANDROMEDA_XTASK_NO_BUILD";

/// The `xtask` developer command.
#[derive(Parser)]
#[command(name = "xtask", about = "Andromeda integration-test orchestration")]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Command,
}

impl Cli {
    /// Dispatch the parsed command.
    ///
    /// # Errors
    ///
    /// Propagates the selected command's failure.
    pub(crate) fn execute(self) -> Result<(), XtaskError> {
        match self.command {
            Command::Itest(arguments) => arguments.execute(),
            Command::Etsi(arguments) => arguments.execute(),
            Command::BuildTests(arguments) => arguments.execute(),
            Command::FormatSql(arguments) => arguments.execute(),
        }
    }
}

/// The runner's subcommands.
#[derive(Subcommand)]
enum Command {
    /// Run integration-test suites against a docker-compose stack.
    Itest(ItestArgs),
    /// Fetch, patch, and run the official ETSI NGSI-LD Robot Framework suite.
    Etsi(EtsiArgs),
    /// Compile every integration-test suite once and package the binaries for CI fan-out.
    BuildTests(BuildTestsArgs),
    /// Reformat the SQL migrations with `pg_format`, or check them under `--check`.
    FormatSql(FormatSqlArgs),
}

/// Arguments to `xtask format-sql`.
#[derive(Args)]
struct FormatSqlArgs {
    /// Report migrations that are not canonically formatted and fail instead of rewriting them.
    #[arg(long)]
    check: bool,
}

impl FormatSqlArgs {
    /// Reformat the migrations in place, or verify them without writing.
    fn execute(self) -> Result<(), XtaskError> {
        let mode = if self.check { FormatMode::Check } else { FormatMode::Write };
        sql_format::run(mode)
    }
}

/// Arguments to `xtask build-tests`.
#[derive(Args)]
struct BuildTestsArgs {
    /// Directory to write the suite binaries and their manifest into.
    #[arg(long, value_name = "DIR")]
    out: PathBuf,
}

impl BuildTestsArgs {
    /// Compile and package the integration-test suites into the output directory.
    fn execute(self) -> Result<(), XtaskError> {
        build::build(&self.out)
    }
}

/// Arguments to `xtask itest`. Exactly one selector is required.
#[derive(Args)]
#[command(group(ArgGroup::new("selector").required(true).multiple(false).args(["stack", "suite", "all"])))]
struct ItestArgs {
    /// Bring up the named stack and run all of its suites.
    #[arg(long, value_name = "NAME")]
    stack: Option<StackName>,
    /// Run the named suites, resolving each to its owning stack.
    #[arg(long, value_name = "NAME", num_args = 1.., value_parser = stack_for_suite)]
    suite: Vec<OwnedSuite>,
    /// Run every stack in sequence.
    #[arg(long)]
    all: bool,
    /// Toggles that shape how the selected stacks run.
    #[command(flatten)]
    options: RunOptions,
    /// Extra arguments forwarded to the test binary after `--test-threads`.
    #[arg(last = true, value_name = "CARGO_TEST_ARGS")]
    cargo_args: Vec<String>,
}

/// Execution toggles common to every selector, kept in their own group so no
/// single arg struct carries more boolean flags than clippy tolerates.
#[derive(Args)]
struct RunOptions {
    /// Leave the stack running after the suites finish.
    #[arg(long)]
    keep: bool,
    /// Base stack only: skip the broker profile and run two host brokers instead.
    #[arg(long)]
    local_broker: bool,
    /// Reuse images already tagged into the compose `image:` names instead of
    /// rebuilding them; also enabled by `ANDROMEDA_XTASK_NO_BUILD=1`.
    #[arg(long)]
    no_build: bool,
}

impl ItestArgs {
    /// Resolve the selector and drive each stack's lifecycle in turn.
    fn execute(self) -> Result<(), XtaskError> {
        let repository_root = repository_root();
        let plan = self.resolve()?;
        let broker_mode = if self.options.local_broker {
            BrokerMode::Local
        } else {
            BrokerMode::Containerized
        };
        let image_build = if self.options.no_build || no_build_from_env() {
            ImageBuild::Reuse
        } else {
            ImageBuild::Build
        };
        for (stack, suites) in plan {
            lifecycle::run_stack(&repository_root, stack, &suites, self.options.keep, broker_mode, image_build, &self.cargo_args)?;
        }
        Ok(())
    }

    /// Resolve the selector into an ordered list of `(stack, suites)` runs.
    fn resolve(&self) -> Result<Vec<(&'static Stack, Vec<&'static Suite>)>, XtaskError> {
        let plan = if let Some(name) = self.stack {
            let stack = Stack::get(name);
            vec![(stack, stack.suites.iter().collect())]
        } else if self.all {
            STACKS.iter().map(|stack| (stack, stack.suites.iter().collect())).collect()
        } else {
            self.resolve_suites()?
        };
        self.check_local_broker(&plan)?;
        Ok(plan)
    }

    /// Group the `--suite` selections under their single owning stack, rejecting a
    /// cross-stack span.
    fn resolve_suites(&self) -> Result<Vec<(&'static Stack, Vec<&'static Suite>)>, XtaskError> {
        let mut owner: Option<&'static Stack> = None;
        let mut chosen: Vec<&'static Suite> = Vec::new();
        for &OwnedSuite { stack, suite } in &self.suite {
            match owner {
                None => owner = Some(stack),
                Some(existing) => {
                    if existing.name != stack.name {
                        return Err(XtaskError::SuitesSpanMultipleStacks);
                    }
                }
            }
            chosen.push(suite);
        }
        let owner = owner.ok_or(XtaskError::NoSuitesSelected)?;
        Ok(vec![(owner, chosen)])
    }

    /// Reject `--local-broker` unless every selected stack is the base tier.
    fn check_local_broker(&self, plan: &[(&'static Stack, Vec<&'static Suite>)]) -> Result<(), XtaskError> {
        if self.options.local_broker && plan.iter().any(|(stack, _)| stack.name != StackName::Base) {
            return Err(XtaskError::LocalBrokerRequiresBase);
        }
        Ok(())
    }
}

/// Whether the environment forces image reuse: `ANDROMEDA_XTASK_NO_BUILD=1`.
fn no_build_from_env() -> bool {
    env::var(NO_BUILD_ENV).is_ok_and(|value| value == "1")
}

/// The repository root: two directories above this crate's manifest.
pub(crate) fn repository_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(2).map_or_else(|| manifest.to_path_buf(), Path::to_path_buf)
}
