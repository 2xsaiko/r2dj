use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::bail;
use chrono::{TimeZone, Utc};
use cmdparser::{CommandDispatcher, ExecSource, SimpleExecutor};
use sqlx::postgres::PgRow;
use sqlx::prelude::*;
use sqlx::types::chrono::{DateTime, NaiveDateTime};
use sqlx::{Execute, PgConnection, Postgres, Transaction};
use std::cmp::min;
use tokio::fs;
use tokio::io;
use tokio::stream::StreamExt;
use uuid::Uuid;

pub enum ApplyBehavior<'a> {
    All,
    Count(usize),
    Until(&'a str),
}

pub async fn apply_migration(
    db_url: &str,
    v: u64,
    b: &ApplyBehavior<'_>,
    dir: &Path,
    unapply: bool,
    pretend: bool,
) -> anyhow::Result<()> {
    let db: PgConnection = PgConnection::connect(db_url).await.unwrap();

    let mut available: Vec<Migration> = fs::read_dir(dir)
        .await?
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let p = entry.path();
            if p.is_dir() {
                let mig = load_migration(p).unwrap();
                Some(mig)
            } else {
                None
            }
        })
        .collect()
        .await;

    available.sort_unstable_by(|a, b| a.date.cmp(&b.date));

    let root_ta = db.begin().await?;

    let mut ta = root_ta.begin().await?;
    do_exec(&mut ta, include_str!("init.sql"), v >= 2).await?;
    let mut root_ta = ta.commit().await?;

    let applied: Vec<Uuid> = sqlx::query("SELECT id FROM __migtool_meta ORDER BY (run_at, id) ASC")
        .map(|row: PgRow| row.get::<Uuid, _>(0))
        .fetch(&mut root_ta)
        .fold(Ok(Vec::new()), |acc, a| match (acc, a) {
            (Ok(mut acc), Ok(a)) => {
                acc.push(a);
                Ok(acc)
            }
            (Ok(_), Err(a)) => Err(a),
            (x @ Err(_), _) => x,
        })
        .await?;

    let mut queue = Vec::new();

    {
        let mut i_avail = 0;
        let mut i_applied = 0;
        loop {
            if i_applied >= applied.len() {
                // we ran out of applied migrations to check
                if !unapply {
                    for m in available.iter().skip(i_avail) {
                        queue.push(m);
                    }
                }
                break;
            }
            if i_avail >= available.len() {
                // there's more applied than available migrations!
                for m in applied.iter().skip(i_applied) {
                    eprintln!("warning: No migration definition or unexpected order for migration {}! Can not unapply.", m.to_simple());
                }
                if unapply {
                    queue.clear();
                }
                break;
            }
            if available[i_avail].uuid != applied[i_applied] {
                eprintln!("warning: No migration definition or unexpected order for migration {}! Can not unapply.", applied[i_applied].to_simple());
                if unapply {
                    queue.clear();
                }
            } else {
                if unapply {
                    queue.push(&available[i_avail]);
                }
                i_avail += 1;
            }
            i_applied += 1;
        }
    }

    if unapply {
        queue.reverse();
    }

    let queue = match b {
        ApplyBehavior::Count(c) => &queue[..min(*c, queue.len())],
        ApplyBehavior::Until(e) => {
            let idx = queue
                .iter()
                .enumerate()
                .filter(|(_, m)| OsStr::new(e) == m.root.as_os_str())
                .map(|(idx, _)| idx)
                .next();
            match idx {
                None => bail!("Migration {} not available!", e),
                Some(idx) => &queue[..idx],
            }
        }
        ApplyBehavior::All => &queue,
    };

    for &item in queue {
        let name = item
            .name
            .as_deref()
            .map(Cow::Borrowed)
            .unwrap_or_else(|| item.root.to_string_lossy());
        if !unapply {
            println!("Applying migration {}", name);
        } else {
            println!("Unapplying migration {}", name);
        }
        match run_migration(item, root_ta, unapply, v).await {
            Err(e) => {
                bail!("Failed to run migration: {}", e);
            }
            Ok(a) => root_ta = a,
        }
    }

    if !pretend {
        root_ta.commit().await?;
    }

    Ok(())
}

async fn run_migration(
    migration: &Migration,
    db: Transaction<PgConnection>,
    unapply: bool,
    v: u64,
) -> anyhow::Result<Transaction<PgConnection>> {
    let src = if !unapply {
        migration.apply_source().await?
    } else {
        migration.unapply_source().await?
    };

    let name = migration
        .name
        .as_ref()
        .map(|s| s.as_str().into())
        .unwrap_or_else(|| migration.root.to_string_lossy());

    let mut ta = db.begin().await?;
    do_exec(&mut ta, src.as_str(), v >= 1).await?;
    if !unapply {
        do_exec(
            &mut ta,
            sqlx::query("INSERT INTO __migtool_meta (id) VALUES ($1)").bind(&migration.uuid),
            v >= 2,
        )
        .await?;
    } else {
        do_exec(
            &mut ta,
            sqlx::query("DELETE FROM __migtool_meta WHERE id = $1").bind(&migration.uuid),
            v >= 2,
        )
        .await?;
    }
    let db = ta.commit().await?;

    Ok(db)
}

async fn do_exec(
    mut db: impl Executor<Database = Postgres>,
    q: impl Execute<'_, Postgres>,
    verbose: bool,
) -> sqlx::Result<u64> {
    if verbose {
        println!("=> {}", q.query_string().replace('\n', "\n.. "));
    }
    let f = db.execute(q);
    match f.await as sqlx::Result<u64> {
        Ok(rows) => {
            if verbose {
                println!("{} rows affected.\n", rows);
            }
            Ok(rows)
        }
        Err(e) => {
            eprintln!("{}", e);
            Err(e)
        }
    }
}

#[derive(Debug)]
struct Migration {
    root: PathBuf,
    uuid: Uuid,
    date: DateTime<Utc>,
    name: Option<String>,
}

impl Migration {
    async fn apply_source(&self) -> io::Result<String> {
        let pb = self.root.join("apply.sql");
        fs::read_to_string(pb).await
    }

    async fn unapply_source(&self) -> io::Result<String> {
        let pb = self.root.join("unapply.sql");
        fs::read_to_string(pb).await
    }
}

fn load_migration(path: impl AsRef<Path>) -> anyhow::Result<Migration> {
    let mut uuid = None;
    let mut date = None;
    let mut name = None;

    let mut cd = CommandDispatcher::new(SimpleExecutor::new(|cmd, args| match cmd {
        "id" => uuid = Some(Uuid::parse_str(args[0]).expect("Invalid uuid")),
        "date" => {
            date = Some(Utc::from_utc_datetime(
                &Utc,
                &NaiveDateTime::from_timestamp(args[0].parse().expect("Invalid timestamp"), 0),
            ))
        }
        "name" => name = Some(args[0].to_string()),
        _ => {}
    }));
    cd.scheduler()
        .exec_path(path.as_ref().join("_props"), ExecSource::Other)?;
    cd.resume_until_empty();

    Ok(Migration {
        root: path.as_ref().to_path_buf(),
        uuid: uuid.expect("uuid not specified"),
        date: date.expect("date not specified"),
        name,
    })
}
