//! Command-line surface: `cargo xtask itest <selector> [flags] [-- <cargo test args>]`.

use crate::{
    error::XtaskError,
    lifecycle::{self, BrokerMode},
    stack::{STACKS, Stack, StackName, stack_for_suite},
};
use clap::{ArgGroup, Args, Parser, Subcommand};
use std::path::{Path, PathBuf};

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
        }
    }
}

/// The runner's subcommands.
#[derive(Subcommand)]
enum Command {
    /// Run integration-test suites against a docker-compose stack.
    Itest(ItestArgs),
}

/// Arguments to `xtask itest`. Exactly one selector is required.
#[derive(Args)]
#[command(group(ArgGroup::new("selector").required(true).multiple(false).args(["stack", "suite", "all"])))]
struct ItestArgs {
    /// Bring up the named stack and run all of its suites.
    #[arg(long, value_name = "NAME")]
    stack: Option<StackName>,
    /// Run the named suites, resolving each to its owning stack.
    #[arg(long, value_name = "NAME", num_args = 1..)]
    suite: Vec<String>,
    /// Run every stack in sequence.
    #[arg(long)]
    all: bool,
    /// Leave the stack running after the suites finish.
    #[arg(long)]
    keep: bool,
    /// Base stack only: skip the broker profile and run two host brokers instead.
    #[arg(long)]
    local_broker: bool,
    /// Extra arguments forwarded to the test binary after `--test-threads`.
    #[arg(last = true, value_name = "CARGO_TEST_ARGS")]
    cargo_args: Vec<String>,
}

impl ItestArgs {
    /// Resolve the selector and drive each stack's lifecycle in turn.
    fn execute(self) -> Result<(), XtaskError> {
        let repository_root = repository_root();
        let plan = self.resolve()?;
        let broker_mode = if self.local_broker { BrokerMode::Local } else { BrokerMode::Containerized };
        for (stack, suites) in plan {
            lifecycle::run_stack(&repository_root, stack, &suites, self.keep, broker_mode, &self.cargo_args)?;
        }
        Ok(())
    }

    /// Resolve the selector into an ordered list of `(stack, suites)` runs.
    fn resolve(&self) -> Result<Vec<(&'static Stack, Vec<&'static str>)>, XtaskError> {
        let plan = if let Some(name) = self.stack {
            let stack = Stack::get(name);
            vec![(stack, stack.suites.to_vec())]
        } else if self.all {
            STACKS.iter().map(|stack| (stack, stack.suites.to_vec())).collect()
        } else {
            self.resolve_suites()?
        };
        self.check_local_broker(&plan)?;
        Ok(plan)
    }

    /// Resolve `--suite` names to a single owning stack, rejecting a cross-stack span.
    fn resolve_suites(&self) -> Result<Vec<(&'static Stack, Vec<&'static str>)>, XtaskError> {
        let mut owner: Option<&'static Stack> = None;
        let mut chosen: Vec<&'static str> = Vec::new();
        for name in &self.suite {
            let (stack, suite) = stack_for_suite(name).ok_or_else(|| XtaskError::UnknownSuite(name.clone()))?;
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
    fn check_local_broker(&self, plan: &[(&'static Stack, Vec<&'static str>)]) -> Result<(), XtaskError> {
        if self.local_broker && plan.iter().any(|(stack, _)| stack.name != StackName::Base) {
            return Err(XtaskError::LocalBrokerRequiresBase);
        }
        Ok(())
    }
}

/// The repository root: two directories above this crate's manifest.
fn repository_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(2).map_or_else(|| manifest.to_path_buf(), Path::to_path_buf)
}
