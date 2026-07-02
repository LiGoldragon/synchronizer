use std::process::ExitCode;

fn main() -> ExitCode {
    todo!(
        "one argument: the NOTA configuration file path; \
         SynchronizerConfig::load, SynchronizerRun::new with the production \
         boundaries, execute, print the NOTA report to stdout; exit nonzero \
         when the report carries failures or the run was fatal"
    )
}
