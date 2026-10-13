// SPDX-License-Identifier: Apache-2.0

use crate::Cli;
use clap::CommandFactory;
use std::io::Write;
use std::path::Path;

pub(super) fn completions(
    shell: clap_complete::Shell,
    stdout: &mut impl Write,
) -> anyhow::Result<i32> {
    // Generate to memory because clap_complete's writer interface cannot return I/O errors.
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut completion_command(), "claude-rs", &mut script);
    stdout.write_all(&script)?;
    Ok(0)
}

/// The command line as completions offer it. `clap_complete` lists hidden subcommands and
/// flags (`man`, `update-worker`, `--bridge-script`) like any other, so they are left out
/// of the command it is given; help and manual pages already skip them.
fn completion_command() -> clap::Command {
    let full = Cli::command();
    clap::Command::new("claude-rs")
        .version(env!("CARGO_PKG_VERSION"))
        .args(full.get_arguments().filter(|arg| !arg.is_hide_set()).cloned())
        .subcommands(full.get_subcommands().filter(|command| !command.is_hide_set()).cloned())
}

pub(super) fn man(out_dir: &Path) -> anyhow::Result<i32> {
    std::fs::create_dir_all(out_dir)?;
    clap_mangen::generate_to(Cli::command(), out_dir)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::completions;

    #[test]
    fn completions_leave_out_hidden_commands_and_flags() {
        use clap::ValueEnum as _;
        for shell in clap_complete::Shell::value_variants() {
            let mut script = Vec::new();
            assert_eq!(completions(*shell, &mut script).expect("completions"), 0);
            let script = String::from_utf8(script).expect("UTF-8 script");
            // `out_dir` is the hidden `man` command's only argument.
            for hidden in ["update-worker", "bridge-script", "out_dir"] {
                assert!(!script.to_lowercase().contains(hidden), "{shell}: offers {hidden}");
            }
            for visible in ["doctor", "resume", "config", "model", "log-filter"] {
                assert!(script.contains(visible), "{shell}: lacks {visible}");
            }
        }
    }
}
