use std::fs;
use std::fs::File;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;

use sqlx::types::chrono::Utc;
use uuid::Uuid;

pub fn create_migration(name: &str, dir: &Path) -> anyhow::Result<()> {
    let now = Utc::now();
    let dirname = format!(
        "{}-{}",
        now.format("%Y%m%d%H%M%S"),
        name.replace(|c: char| !c.is_ascii_alphanumeric(), "-")
            .to_lowercase()
    );
    println!("Creating migration '{}' at '{}'", name, dirname);

    let uuid = Uuid::new_v4();

    let migration_dir = dir.join(dirname);
    match fs::create_dir(dir) {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        x @ Err(_) => x.unwrap(),
    }
    fs::create_dir(&migration_dir)?;

    let mut props = File::create(migration_dir.join("_props"))?;
    let mut apply = File::create(migration_dir.join("apply.sql"))?;
    let mut unapply = File::create(migration_dir.join("unapply.sql"))?;

    writeln!(props, "// Auto-generated migration metadata. Do not edit.")?;
    writeln!(props, "id   {}", uuid.to_simple())?;
    writeln!(props, "name {}", cmdparser::escape(name))?;
    writeln!(props, "date {}", now.timestamp())?;

    writeln!(
        apply,
        "-- Write SQL here that applies the changes to the database you want, starting"
    )?;
    writeln!(apply, "-- from the previous migration point.")?;

    writeln!(
        unapply,
        "-- Write SQL here that undoes the changes done in apply.sql, back to the"
    )?;
    writeln!(unapply, "-- previous migration point.")?;

    Ok(())
}
