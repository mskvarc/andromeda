//! Records the toolchain and target the broker is compiled with, for its
//! version output and its component report.
//!
//! Nothing here reads the repository's history or the clock, so the same
//! sources compiled with the same toolchain record the same build.

use rustc_version::Channel;
use std::{
    env::{self, VarError},
    io,
    process::{Command, ExitStatus},
    string::FromUtf8Error,
};
use thiserror::Error;

/// Failure while recording the build toolchain.
#[derive(Debug, Error)]
enum BuildToolchainError {
    /// The compiler did not report its version.
    #[error("the compiler did not report its version: {0}")]
    Compiler(#[from] rustc_version::Error),
    /// A variable cargo sets for every build script is unset or not Unicode.
    #[error("cargo did not set `{name}` for the build script: {source}")]
    Variable {
        /// The variable's name.
        name: &'static str,
        /// Why it could not be read.
        source: VarError,
    },
    /// Cargo could not be started.
    #[error("cargo could not be started to report its version: {0}")]
    CargoStart(#[from] io::Error),
    /// Cargo exited without reporting its version.
    #[error("cargo exited with {0} instead of reporting its version")]
    CargoExit(ExitStatus),
    /// Cargo reported its version in something other than UTF-8.
    #[error("cargo reported its version in something other than UTF-8: {0}")]
    CargoOutput(#[from] FromUtf8Error),
}

/// Hand the toolchain and target to the crate as compile-time environment
/// variables.
fn main() -> Result<(), BuildToolchainError> {
    println!("cargo:rerun-if-changed=build.rs");
    let compiler = rustc_version::version_meta()?;
    emit("ANDROMEDA_RUST_VERSION", &compiler.short_version_string);
    emit("ANDROMEDA_RUST_CHANNEL", channel_name(compiler.channel));
    emit("ANDROMEDA_CARGO_VERSION", &cargo_version()?);
    emit("ANDROMEDA_BUILD_OPERATING_SYSTEM", &format!("{}-{}", env::consts::OS, env::consts::ARCH));
    emit("ANDROMEDA_BUILD_TARGET", &variable("TARGET")?);
    Ok(())
}

/// Set `name` to `value` in the environment the crate compiles in.
fn emit(name: &str, value: &str) {
    println!("cargo:rustc-env={name}={value}");
}

/// The release channel as rustup spells it.
const fn channel_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Dev => "dev",
        Channel::Nightly => "nightly",
        Channel::Beta => "beta",
        Channel::Stable => "stable",
    }
}

/// The version line of the cargo running this build, as `cargo --version`
/// prints it.
fn cargo_version() -> Result<String, BuildToolchainError> {
    let output = Command::new(variable("CARGO")?).arg("--version").output()?;
    if !output.status.success() {
        return Err(BuildToolchainError::CargoExit(output.status));
    }
    let mut version = String::from_utf8(output.stdout)?;
    version.truncate(version.trim_end().len());
    Ok(version)
}

/// The value cargo set for `name`.
fn variable(name: &'static str) -> Result<String, BuildToolchainError> {
    env::var(name).map_err(|source| BuildToolchainError::Variable { name, source })
}
