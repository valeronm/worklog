//! Converts a store of the `worklog` crate to the store `worklog-next` keeps.
//!
//! Outside the tests, `old` alone reaches the crate that wrote the store being converted.

use std::process::ExitCode;

mod cli;
mod decisions;
#[cfg(test)]
mod fixture;
mod key;
mod old;
mod plan;
mod question;
mod report;
mod text;
mod write;

#[must_use]
pub fn main() -> ExitCode {
    let cwd = std::env::current_dir().ok();
    let code = cli::main_in(
        std::env::args_os(),
        cwd.as_deref(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
