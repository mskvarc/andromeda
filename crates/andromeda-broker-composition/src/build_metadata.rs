//! The broker's own build, as its version output and its component report
//! name it.
//!
//! Every value is fixed by the sources and the toolchain they pin, so two
//! compiles of the same sources report the same build whatever commit or
//! moment they ran at.

/// The release, as the workspace manifest names it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What `andromeda --version` prints after the program name: the release,
/// then the compiler's and cargo's version lines and the target triple, each
/// on a line of its own.
pub const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\n",
    env!("ANDROMEDA_RUST_VERSION"),
    "\n",
    env!("ANDROMEDA_CARGO_VERSION"),
    "\ntarget ",
    env!("ANDROMEDA_BUILD_TARGET"),
);

/// The compiler's version line, as `rustc --version` prints it.
pub(crate) const RUST_VERSION: &str = env!("ANDROMEDA_RUST_VERSION");

/// The compiler's release channel: `stable`, `beta`, `nightly` or `dev`.
pub(crate) const RUST_CHANNEL: &str = env!("ANDROMEDA_RUST_CHANNEL");

/// Cargo's version line, as `cargo --version` prints it.
pub(crate) const CARGO_VERSION: &str = env!("ANDROMEDA_CARGO_VERSION");

/// The operating system and architecture of the machine that compiled the
/// broker, as `os-arch`.
pub(crate) const BUILD_OPERATING_SYSTEM: &str = env!("ANDROMEDA_BUILD_OPERATING_SYSTEM");

/// The target triple the broker is compiled for.
pub(crate) const BUILD_TARGET: &str = env!("ANDROMEDA_BUILD_TARGET");
