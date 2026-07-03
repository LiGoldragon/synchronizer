use std::path::PathBuf;
use std::process::ExitCode;

use synchronizer::configuration::SynchronizerConfig;
use synchronizer::driver::{BaseSelection, SynchronizerRun};

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let (Some(path), mode_argument, None) = (arguments.next(), arguments.next(), arguments.next())
    else {
        eprintln!("usage: synchronizer <configuration.nota> [staged-cascade]");
        return ExitCode::from(2);
    };
    let base_selection = match mode_argument.as_deref().and_then(|token| token.to_str()) {
        None | Some("mainline") => BaseSelection::Mainline,
        // Coordinated cross-branch verify over an already-staged set: read
        // components at their staging tip where it exists and cascade
        // consumers onto it (see BaseSelection).
        Some("staged-cascade") => BaseSelection::StagedCascade,
        Some(other) => {
            eprintln!("synchronizer: unknown run mode {other:?} (mainline | staged-cascade)");
            return ExitCode::from(2);
        }
    };
    let config = match SynchronizerConfig::load(&PathBuf::from(path)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("synchronizer: {error}");
            return ExitCode::from(2);
        }
    };
    let report = match SynchronizerRun::new(config)
        .with_base_selection(base_selection)
        .execute()
    {
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
