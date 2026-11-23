//! Where the broker keeps the files whose directory the configuration leaves
//! out: its runtime files, the local socket among them, and its log files.
//!
//! Two conventions place them. A user session, which a set
//! `$XDG_RUNTIME_DIR` marks, follows the XDG Base Directory Specification:
//! runtime files under that directory, logs under the state directory. A Linux
//! system service, a container among them, follows the Filesystem Hierarchy
//! Standard 3.0: runtime files under `/run`, logs under `/var/log`. macOS and
//! Windows lay out no system service that way, so without a session runtime
//! directory they keep runtime files in the per-user temporary directory and
//! logs in the state directory.

use etcetera::{BaseStrategy, choose_base_strategy};
use std::{env, path::PathBuf};

/// The directory under each base directory that holds the broker's files.
const SUBDIRECTORY: &str = "andromeda";

/// The log directory of a session with no home directory to resolve a state
/// directory under, relative to the working directory.
const HOMELESS_LOG_DIRECTORY: &str = "andromeda-logs";

/// The directory the broker keeps its runtime files in when the
/// configuration names none.
#[must_use]
pub fn default_runtime_directory() -> PathBuf {
    Layout::detect().runtime_directory()
}

/// The directory the log file sink writes to when the configuration names
/// none.
#[must_use]
pub fn default_log_directory() -> PathBuf {
    Layout::detect().log_directory(user_state_directory())
}

/// The XDG state directory, or the data directory on a platform without one;
/// `None` when no home directory can be determined.
fn user_state_directory() -> Option<PathBuf> {
    choose_base_strategy()
        .ok()
        .map(|strategy| strategy.state_dir().unwrap_or_else(|| strategy.data_dir()))
}

/// Whether the platform lays out system services by the Filesystem Hierarchy
/// Standard.
#[derive(Clone, Copy)]
enum ServiceConvention {
    /// Linux: `/run` and `/var/log` hold a service's runtime files and logs.
    Fhs,
    /// Every other platform: a process keeps its files per user.
    PerUser,
}

/// The convention of the platform the broker was built for.
const SERVICE_CONVENTION: ServiceConvention = if cfg!(target_os = "linux") {
    ServiceConvention::Fhs
} else {
    ServiceConvention::PerUser
};

/// The convention placing the broker's files in the running environment.
#[derive(Debug, PartialEq, Eq)]
enum Layout {
    /// A user session, with the runtime directory `$XDG_RUNTIME_DIR` names.
    Session {
        /// The session's runtime directory.
        runtime: PathBuf,
    },
    /// A Linux system service outside any session.
    FhsService,
    /// A process outside any session on a platform with no system layout.
    PerUser,
}

impl Layout {
    /// The layout of the running environment.
    fn detect() -> Layout {
        Layout::of(SERVICE_CONVENTION, choose_base_strategy().ok().and_then(|strategy| strategy.runtime_dir()))
    }

    /// The layout under `convention`, in a session whose runtime directory
    /// is `session_runtime`, or outside any session when it is `None`.
    fn of(convention: ServiceConvention, session_runtime: Option<PathBuf>) -> Layout {
        match (session_runtime, convention) {
            (Some(runtime), ServiceConvention::Fhs | ServiceConvention::PerUser) => Layout::Session { runtime },
            (None, ServiceConvention::Fhs) => Layout::FhsService,
            (None, ServiceConvention::PerUser) => Layout::PerUser,
        }
    }

    /// The directory runtime files go in.
    fn runtime_directory(self) -> PathBuf {
        match self {
            Layout::Session { runtime } => runtime.join(SUBDIRECTORY),
            Layout::FhsService => PathBuf::from("/run").join(SUBDIRECTORY),
            Layout::PerUser => env::temp_dir().join(SUBDIRECTORY),
        }
    }

    /// The directory log files go in, `user_state` being the state directory
    /// of the user the broker runs as.
    fn log_directory(self, user_state: Option<PathBuf>) -> PathBuf {
        match self {
            Layout::FhsService => PathBuf::from("/var/log").join(SUBDIRECTORY),
            Layout::Session { .. } | Layout::PerUser => user_state.map_or_else(|| PathBuf::from(HOMELESS_LOG_DIRECTORY), |state| state.join(SUBDIRECTORY)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::platform_directories::{Layout, ServiceConvention};
    use pretty_assertions::assert_eq;
    use std::{env, path::PathBuf};

    #[test]
    fn a_session_runtime_directory_marks_a_session_under_every_convention() {
        for convention in [ServiceConvention::Fhs, ServiceConvention::PerUser] {
            assert_eq!(
                Layout::of(convention, Some(PathBuf::from("/run/user/1000"))),
                Layout::Session {
                    runtime: PathBuf::from("/run/user/1000")
                }
            );
        }
    }

    #[test]
    fn outside_a_session_linux_takes_the_service_layout_and_other_platforms_the_per_user_one() {
        assert_eq!(Layout::of(ServiceConvention::Fhs, None), Layout::FhsService);
        assert_eq!(Layout::of(ServiceConvention::PerUser, None), Layout::PerUser);
    }

    #[test]
    fn a_session_keeps_runtime_files_and_logs_in_its_xdg_directories() {
        let session = || Layout::Session {
            runtime: PathBuf::from("/run/user/1000"),
        };
        assert_eq!(session().runtime_directory(), PathBuf::from("/run/user/1000/andromeda"));
        assert_eq!(
            session().log_directory(Some(PathBuf::from("/home/operator/.local/state"))),
            PathBuf::from("/home/operator/.local/state/andromeda")
        );
    }

    #[test]
    fn a_linux_service_keeps_runtime_files_under_run_and_logs_under_var_log() {
        assert_eq!(Layout::FhsService.runtime_directory(), PathBuf::from("/run/andromeda"));
        assert_eq!(
            Layout::FhsService.log_directory(Some(PathBuf::from("/var/lib/andromeda/.local/state"))),
            PathBuf::from("/var/log/andromeda")
        );
    }

    #[test]
    fn a_per_user_process_keeps_runtime_files_in_the_temporary_directory_and_logs_in_its_state_directory() {
        assert_eq!(Layout::PerUser.runtime_directory(), env::temp_dir().join("andromeda"));
        assert_eq!(
            Layout::PerUser.log_directory(Some(PathBuf::from("/Users/operator/.local/state"))),
            PathBuf::from("/Users/operator/.local/state/andromeda")
        );
    }

    #[test]
    fn without_a_home_directory_logs_go_under_the_working_directory() {
        assert_eq!(Layout::PerUser.log_directory(None), PathBuf::from("andromeda-logs"));
    }
}
