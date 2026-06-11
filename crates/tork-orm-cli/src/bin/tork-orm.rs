//! The `tork-orm` command-line binary.

use std::process::ExitCode;

fn main() -> ExitCode {
    tork_orm_cli::run(std::env::args_os())
}
