use std::path::{Path, PathBuf};

use cmdparser::{CommandDispatcher, ExecSource, SimpleExecutor};
use log::warn;

pub struct Config {}

pub fn load<P: AsRef<Path>>(path: P) {
    let mut nickname = "r2dj".to_string();
    let mut mumble_cert = None;

    let mut cd = CommandDispatcher::new(SimpleExecutor::new(|cmd, args| match cmd {
        "nickname" => nickname = args[0].to_string(),
        "mumble_cert" => mumble_cert = Some(PathBuf::from(args[1])),
        _ => warn!("Unknown config command '{}'", cmd),
    }));
    cd.scheduler()
        .exec_path(path, ExecSource::Event)
        .expect("failed to load configuration file");
    cd.resume_until_empty();
}
