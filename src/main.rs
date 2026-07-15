use std::path::PathBuf;
use std::process::ExitCode;

use synchronizer::configuration::SynchronizerConfig;
use synchronizer::driver::{BaseSelection, SynchronizerRun};
use synchronizer::release_train::{ReleaseTrainIntent, ReleaseTrainRun};
use synchronizer::report::SynchronizerReport;

fn main() -> ExitCode {
    CommandLine::from_environment().execute()
}

struct CommandLine {
    arguments: Vec<std::ffi::OsString>,
}

impl CommandLine {
    fn from_environment() -> Self {
        Self {
            arguments: std::env::args_os().skip(1).collect(),
        }
    }

    fn execute(self) -> ExitCode {
        if let [command, configuration_path, intent_path] = self.arguments.as_slice()
            && command == "release-train"
        {
            return self.execute_release_train(configuration_path, intent_path);
        }
        let [configuration_path, mode_argument] = self.arguments.as_slice() else {
            eprintln!(
                "usage: synchronizer <configuration.nota> [mainline|staged-cascade]\n       synchronizer release-train <configuration.nota> <release-train.nota>"
            );
            return ExitCode::from(2);
        };
        let base_selection = match mode_argument.to_str() {
            Some("mainline") => BaseSelection::Mainline,
            Some("staged-cascade") => BaseSelection::StagedCascade,
            other => {
                eprintln!("synchronizer: unknown run mode {other:?} (mainline | staged-cascade)");
                return ExitCode::from(2);
            }
        };
        let config = match SynchronizerConfig::load(&PathBuf::from(configuration_path)) {
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
        self.render_report(report)
    }

    fn execute_release_train(
        &self,
        configuration_path: &std::ffi::OsString,
        intent_path: &std::ffi::OsString,
    ) -> ExitCode {
        let config = match SynchronizerConfig::load(&PathBuf::from(configuration_path)) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("synchronizer: {error}");
                return ExitCode::from(2);
            }
        };
        let intent_text = match std::fs::read_to_string(intent_path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("synchronizer: release-train intent unreadable: {error}");
                return ExitCode::from(2);
            }
        };
        let intent = match ReleaseTrainIntent::from_nota_text(&intent_text) {
            Ok(intent) => intent,
            Err(error) => {
                eprintln!("synchronizer: {error}");
                return ExitCode::from(2);
            }
        };
        let materialized = match ReleaseTrainRun::from_config(config, intent).execute() {
            Ok(materialized) => materialized,
            Err(error) => {
                eprintln!("synchronizer: {error}");
                return ExitCode::from(2);
            }
        };
        self.render_report(materialized.report().clone())
    }

    fn render_report(&self, report: SynchronizerReport) -> ExitCode {
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
}
