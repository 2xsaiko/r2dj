use std::path::Path;
use std::process::Command;

use cmdparser::{CommandDispatcher, ExecSource, SimpleExecutor};

fn main() {
    let output = Command::new("git").args(&["rev-parse", "HEAD"]).output().unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=DATABASE_URL={}", read_config("../srvrc"));
}

#[allow(clippy::single_match)]
fn read_config(path: impl AsRef<Path>) -> String {
    let mut db_url = None;

    let mut cd = CommandDispatcher::new(SimpleExecutor::new(|cmd, args| match cmd {
        "db_url" => db_url = Some(args[0].to_string()),
        _ => {}
    }));
    cd.scheduler().exec_path(path, ExecSource::Other).expect("Could not open config file");
    cd.resume_until_empty();

    db_url.expect("db_url not set in config file")
}
