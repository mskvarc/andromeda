//! Merging the per-pass robot reports into one unified `output.xml`, `log.html`,
//! and `report.html` with `rebot`, robotframework's report combiner. A run that
//! drives more than one topology pass writes each pass's artifacts into its own
//! subdirectory; this combines their `output.xml` files into a single report at the
//! top of the run's output directory. A single-pass run needs no merge -- its report
//! is already unified inside its one subdirectory.

use crate::{error::XtaskError, etsi::venv};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// The merged `output.xml` written beside the per-pass subdirectories.
const MERGED_OUTPUT: &str = "output.xml";
/// The merged `log.html` written beside the per-pass subdirectories.
const MERGED_LOG: &str = "log.html";
/// The merged `report.html` written beside the per-pass subdirectories.
const MERGED_REPORT: &str = "report.html";

/// Combine each pass's `output.xml` (the `inputs`) into one unified report written
/// directly into `output_dir`, so `output.xml`, `log.html`, and `report.html` sit
/// beside the per-pass subdirectories the inputs came from.
///
/// # Errors
///
/// Returns [`XtaskError::RebotSpawn`] when `rebot` cannot be spawned and
/// [`XtaskError::RebotFailed`] when it exits non-zero.
pub(crate) fn merge(output_dir: &Path, inputs: &[PathBuf]) -> Result<(), XtaskError> {
    let args = rebot_args(output_dir, inputs);
    let status = Command::new(venv::rebot_binary()).args(&args).status().map_err(XtaskError::RebotSpawn)?;
    if status.success() { Ok(()) } else { Err(XtaskError::RebotFailed) }
}

/// Assemble rebot's arguments: absolute `--output`/`--log`/`--report` targets inside
/// `output_dir`, then each pass's `output.xml`. The targets are absolute so rebot
/// writes them into the run's output directory regardless of its working directory,
/// and the combined report nests every input suite under one root.
fn rebot_args(output_dir: &Path, inputs: &[PathBuf]) -> Vec<String> {
    let mut args = vec![
        "--output".to_owned(),
        output_dir.join(MERGED_OUTPUT).display().to_string(),
        "--log".to_owned(),
        output_dir.join(MERGED_LOG).display().to_string(),
        "--report".to_owned(),
        output_dir.join(MERGED_REPORT).display().to_string(),
    ];
    args.extend(inputs.iter().map(|input| input.display().to_string()));
    args
}

#[cfg(test)]
mod tests {
    use crate::etsi::merge::rebot_args;
    use std::path::{Path, PathBuf};

    #[test]
    fn rebot_arguments_place_the_targets_before_the_inputs_in_order() {
        let inputs = [PathBuf::from("/out/single-broker/output.xml"), PathBuf::from("/out/distributed/output.xml")];
        let args = rebot_args(Path::new("/out"), &inputs);
        assert_eq!(
            args,
            vec![
                "--output".to_owned(),
                "/out/output.xml".to_owned(),
                "--log".to_owned(),
                "/out/log.html".to_owned(),
                "--report".to_owned(),
                "/out/report.html".to_owned(),
                "/out/single-broker/output.xml".to_owned(),
                "/out/distributed/output.xml".to_owned(),
            ]
        );
    }
}
