use agents_manifest::{cli::Cli, error::Error};
use clap::Parser;
use std::{io, process::ExitCode};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::BrokenPipe => {
            ExitCode::SUCCESS
        }
        Err(error) => {
            let code = error.code();
            let _ = cli.write_error(&error);
            ExitCode::from(code)
        }
    }
}
