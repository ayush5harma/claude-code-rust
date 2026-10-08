// SPDX-License-Identifier: Apache-2.0
// A native fake `claude` for `auth login` and `agents`. Never uses real
// credentials or sessions.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let profile = PathBuf::from(std::env::var_os("CLAUDE_CONFIG_DIR").expect("isolated profile"));
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["auth", "login"] => auth_login(&profile),
        ["agents", "--json"] => agents_json(&profile),
        ["agents"] => agent_view(&profile, &args),
        other => panic!("unexpected fake claude argv: {other:?}"),
    }
}

fn auth_login(profile: &Path) -> io::Result<()> {
    println!("AUTH_CHILD_READY");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    std::fs::write(profile.join("auth-input"), input)?;
    println!("AUTH_INPUT_RECORDED");
    io::stdout().flush()?;
    while !profile.join("auth-release").exists() {
        std::thread::sleep(Duration::from_millis(20));
    }
    if std::env::var("FAKE_AUTH_MODE").as_deref() == Ok("failure") {
        std::process::exit(7);
    }
    std::fs::write(
        profile.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"fixture-only-not-a-token"}}"#,
    )?;
    Ok(())
}

/// Prints the listing a test placed in the profile, or an empty one.
fn agents_json(profile: &Path) -> io::Result<()> {
    let listing = std::fs::read_to_string(profile.join("agents.json"))
        .unwrap_or_else(|_| "[]".to_owned());
    print!("{listing}");
    io::stdout().flush()
}

/// Stands in for the interactive agent view: records how it was started,
/// reads one line from the inherited terminal, and exits.
fn agent_view(profile: &Path, args: &[String]) -> io::Result<()> {
    let cwd = std::env::current_dir()?;
    let mut runs = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(profile.join("agent-view-runs"))?;
    writeln!(runs, "{}\t{}", args.join(" "), cwd.display())?;
    println!("AGENT_VIEW_READY");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    std::fs::write(profile.join("agent-view-input"), input)?;
    Ok(())
}
