pub mod style;

mod execution;
mod formatters;
mod helpers;
mod inspect;
mod render;

use crate::cli::OutputFormat;
use crate::types::{DaemonResponse, daemon_response};
use anyhow::Result;
use std::fmt;

use formatters::formatter_for;
use render::{RenderOutput, render_with};

/// Print a daemon response in the selected format.
pub fn print_response(response: &DaemonResponse, format: OutputFormat) {
    let rendered = render_response(response, format);
    if !rendered.stdout.is_empty() {
        print!("{}", rendered.stdout);
    }
    if !rendered.stderr.is_empty() {
        eprint!("{}", rendered.stderr);
    }
}

#[derive(Debug)]
pub struct PrintedDaemonError;

impl fmt::Display for PrintedDaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("daemon returned an error response")
    }
}

impl std::error::Error for PrintedDaemonError {}

/// Print a daemon response and return an error if it was a daemon-level failure.
pub fn print_response_checked(response: &DaemonResponse, format: OutputFormat) -> Result<()> {
    print_response(response, format);
    if matches!(response.kind_ref()?, daemon_response::Kind::Error(_)) {
        return Err(PrintedDaemonError.into());
    }
    Ok(())
}

pub(crate) fn render_response(response: &DaemonResponse, format: OutputFormat) -> RenderOutput {
    let fmt = formatter_for(format);
    render_with(response, fmt)
}
