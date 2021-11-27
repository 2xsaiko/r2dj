use std::path::Path;

use crate::apply::ApplyBehavior;
use clap::{app_from_crate, App, Arg};
use cmdparser::{CommandDispatcher, ExecSource, SimpleExecutor};

mod apply;
mod create;

fn main() -> anyhow::Result<()> {
    let matches =
        app_from_crate!()
            .subcommand(
                App::new("create")
                    .about("Create a new migration")
                    .arg(Arg::new("name").value_name("NAME").required(true)),
            )
            .subcommand(
                App::new("apply")
                    .about("Apply and unapply migrations")
                    .arg(
                        Arg::new("until")
                            .short('u')
                            .long("until")
                            .value_name("DIRECTORY")
                            .about("Apply until a certain migration")
                            .conflicts_with("all"),
                    )
                    .arg(
                        Arg::new("unapply")
                            .short('r')
                            .long("unapply")
                            .about("Unapply migrations"),
                    )
                    .arg(Arg::new("all").short('a').long("all").about(
                        "Apply until the newest state or unapply until before the first state",
                    ))
                    .arg(
                        Arg::new("pretend")
                            .short('p')
                            .long("pretend")
                            .about("Do not actually modify the database"),
                    ),
            )
            .arg(
                Arg::new("migration-dir")
                    .short('d')
                    .long("migration-dir")
                    .value_name("DIRECTORY")
                    .default_value("migrations")
                    .about("Migration directory")
                    .global(true),
            )
            .arg(
                Arg::new("rc")
                    .short('C')
                    .long("rc")
                    .value_name("FILE")
                    .default_value("srvrc")
                    .about("Path to server configuration file containing database URL")
                    .global(true),
            )
            .arg(
                Arg::new("verbose")
                    .short('v')
                    .long("verbose")
                    .multiple_occurrences(true)
                    .global(true),
            )
            .get_matches();

    match matches.subcommand() {
        Some(("create", args)) => {
            let name = args.value_of("name").unwrap();
            let dir = args.value_of_os("migration-dir").unwrap();
            create::create_migration(name, Path::new(dir))?;
        }
        Some(("apply", args)) => {
            let rc = args.value_of_os("rc").unwrap();
            let verbosity = args.occurrences_of("verbose");
            let dir = args.value_of_os("migration-dir").unwrap();
            let unapply = args.is_present("unapply");
            let all = args.is_present("all");
            let until = args.value_of("until");
            let pretend = args.is_present("pretend");

            let db_url = read_config(rc);
            let b = if all {
                ApplyBehavior::All
            } else if let Some(until) = until {
                ApplyBehavior::Until(until)
            } else {
                ApplyBehavior::Count(1)
            };

            let mut runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(apply::apply_migration(
                &db_url,
                verbosity,
                &b,
                Path::new(dir),
                unapply,
                pretend,
            ))?
        }
        _ => {}
    }

    Ok(())
}

#[allow(clippy::single_match)]
fn read_config(path: impl AsRef<Path>) -> String {
    let mut db_url = None;

    let mut cd = CommandDispatcher::new(SimpleExecutor::new(|cmd, args| match cmd {
        "db_url" => db_url = Some(args[0].to_string()),
        _ => {}
    }));
    cd.scheduler()
        .exec_path(path, ExecSource::Other)
        .expect("Could not open config file");
    cd.resume_until_empty();

    db_url.expect("db_url not set in config file")
}
