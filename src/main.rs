use std::path::PathBuf;
use std::process::ExitCode;

use synchronizer::configuration::SynchronizerConfig;
use synchronizer::driver::SynchronizerRun;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let (Some(path), None) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: synchronizer <configuration.nota>");
        return ExitCode::from(2);
    };
    let config = match SynchronizerConfig::load(&PathBuf::from(path)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("synchronizer: {error}");
            return ExitCode::from(2);
        }
    };
    let report = match SynchronizerRun::new(config).execute() {
        Ok(report) => report,
        Err(error) => {
            eprintln!("synchronizer: {error}");
            return ExitCode::from(2);
        }
    };
    match report.to_nota_text() {
        Ok(text) => print!("{text}"),
        Err(error) => {
            eprintln!("synchronizer: report rendering: {error}");
            return ExitCode::from(2);
        }
    }
    if report.has_failures() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
