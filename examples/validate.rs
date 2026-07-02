//! Config validator: decode a synchronizer configuration and report the
//! outcome, without running the tool. Use it to check a config edit before a
//! live run — it performs no git, nix, or network operation, only the
//! canonical NOTA decode.
//!
//! `cargo run --example validate -- <configuration.nota>`

use std::path::PathBuf;
use std::process::ExitCode;

use synchronizer::configuration::SynchronizerConfig;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let (Some(path), None) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: validate <configuration.nota>");
        return ExitCode::from(2);
    };
    match SynchronizerConfig::load(&PathBuf::from(path)) {
        Ok(config) => {
            println!("ok: {} components configured", config.components().len());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("invalid: {error}");
            ExitCode::from(1)
        }
    }
}
