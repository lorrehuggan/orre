//! `orre backup`, `orre backup --list`, `orre backup --delete <file>`
//! and `orre restore <file>` (M1.28, R.12): local backups in `<data
//! dir>/backups` ([`orre_core::backup`]).

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::Args;
use orre_core::backup::{self, Current, Reason, RestoreCheck, detail};
use orre_core::db::Database;

use super::write_table;
use crate::settings::restore::{AI_OFF_AFTER_RESTORE, undoes};
use crate::settings::table::Run;

#[derive(Debug, Args)]
pub struct BackupArgs {
    /// List the backups instead of taking one.
    #[arg(long, short = 'l')]
    pub list: bool,

    /// Delete one backup, automatic or manual, for good. Without --yes,
    /// only shows what would be deleted.
    #[arg(long, value_name = "FILE", conflicts_with = "list")]
    pub delete: Option<PathBuf>,

    /// Delete it. Only with --delete.
    #[arg(long, short = 'y', requires = "delete")]
    pub yes: bool,
}

#[derive(Debug, Args)]
pub struct RestoreArgs {
    /// The backup: a path, or a file name from `orre backup --list`.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Restore it. Without this, only checks the backup and shows what
    /// restoring would do.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// `orre backup`: a manual backup of an existing database (kept until
/// the user deletes it), or with `--list` the backups, newest first, or
/// with `--delete` one backup deleted (`terminal` gates `--delete --yes`).
pub fn run(database: &Path, args: &BackupArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    if args.list {
        return list(database, out);
    }
    if let Some(file) = &args.delete {
        return delete(database, file, args.yes, out);
    }
    let db = super::open_existing(database)?;
    let backup = backup::create(&db, Reason::Manual).context("couldn't back up the database")?;
    writeln!(
        out,
        "Backed up the database to {} ({}).",
        backup.path.display(),
        human_size(backup.size)
    )?;
    Ok(())
}

fn list(database: &Path, out: &mut dyn Write) -> anyhow::Result<()> {
    let dir = backup::backups_dir(database);
    let backups = backup::list(database).context("couldn't list the backups")?;
    if backups.is_empty() {
        writeln!(
            out,
            "No backups in {} yet. Run `orre backup` to take one.",
            dir.display()
        )?;
        return Ok(());
    }
    writeln!(out, "Backups in {}, newest first:", dir.display())?;
    let rows: Vec<Vec<String>> = backups
        .iter()
        .map(|backup| {
            vec![
                backup.taken_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                backup.reason.key().to_owned(),
                human_size(backup.size),
                backup
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ]
        })
        .collect();
    write_table(out, &["TAKEN (UTC)", "REASON", "SIZE", "FILE"], &rows)?;
    writeln!(
        out,
        "Total: {} {}, {}.",
        backups.len(),
        if backups.len() == 1 {
            "backup"
        } else {
            "backups"
        },
        human_size(backup::total_size(&backups))
    )?;
    writeln!(
        out,
        "Automatic backups (import, migration, restore) rotate: the {} most recent are kept, \
         plus the newest of each of the {} months before this one. Manual ones stay until \
         you delete them (`orre backup --delete <FILE>`).",
        backup::KEEP_RECENT,
        backup::KEEP_MONTHS
    )?;
    Ok(())
}

/// `orre backup --delete <file>`. Without `--yes` it says which backup it
/// would delete and deletes nothing.
fn delete(database: &Path, file: &Path, yes: bool, out: &mut dyn Write) -> anyhow::Result<()> {
    let dir = backup::backups_dir(database);
    let in_dir = file
        .parent()
        .is_none_or(|parent| parent.as_os_str().is_empty() || parent == dir);
    let found = backup::list(database)
        .context("couldn't list the backups")?
        .into_iter()
        .find(|backup| in_dir && backup.path.file_name() == file.file_name());
    let Some(found) = found else {
        bail!(
            "there is no backup {} in {}; `orre backup --list` lists the backups",
            file.display(),
            dir.display()
        );
    };
    writeln!(
        out,
        "Backup: {}\n  taken {}, {}, {}",
        found.path.display(),
        found.taken_at.format("%Y-%m-%d %H:%M:%S UTC"),
        describe(found.reason),
        human_size(found.size)
    )?;
    if !yes {
        writeln!(out)?;
        writeln!(
            out,
            "Nothing was deleted. Run the command again with --yes to delete it for good: \
             it can't be undone."
        )?;
        return Ok(());
    }
    let deleted = backup::delete(database, &found.path)?;
    writeln!(out)?;
    writeln!(
        out,
        "Deleted {} ({} freed).",
        deleted.path.display(),
        human_size(deleted.size)
    )?;
    Ok(())
}

/// `orre restore`. Without `--yes` it checks the backup, says what
/// restoring would do, and succeeds without changing anything.
pub fn restore(database: &Path, args: &RestoreArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let file = resolve(database, &args.file)?;
    let checked = backup::check(database, &file).context("the backup can't be restored")?;
    write_check(database, &checked, out)?;

    if !args.yes {
        writeln!(out)?;
        writeln!(
            out,
            "Nothing was restored. Run the command again with --yes to restore it."
        )?;
        return Ok(());
    }

    // Core's errors say whether anything changed.
    let restored = backup::restore(database, &file)?;
    writeln!(out)?;
    if let Some(pre_restore) = &restored.pre_restore {
        writeln!(
            out,
            "Backed up the current database to {}.",
            pre_restore.path.display()
        )?;
    }
    match restored.migrated_to {
        Some(version) => writeln!(
            out,
            "Restored {} and updated it from schema version {} to {version}.",
            file.display(),
            restored.from_version
        )?,
        None => writeln!(out, "Restored {}.", file.display())?,
    }
    writeln!(out, "{AI_OFF_AFTER_RESTORE}")?;
    match (restored.log_rows_carried, &restored.pre_restore) {
        (Some(0), _) => {}
        (Some(rows), _) => writeln!(
            out,
            "Kept {rows} AI access-log {} newer than the backup.",
            if rows == 1 { "entry" } else { "entries" }
        )?,
        (None, Some(pre_restore)) => writeln!(
            out,
            "Couldn't keep the AI access-log entries newer than the backup; they're in {}.",
            pre_restore.path.display()
        )?,
        (None, None) => {}
    }
    Ok(())
}

/// The backup at `file` as given, or else a file of that name in the
/// backups directory.
fn resolve(database: &Path, file: &Path) -> anyhow::Result<PathBuf> {
    if file.exists() {
        return Ok(file.to_path_buf());
    }
    let dir = backup::backups_dir(database);
    if file.components().count() == 1 {
        let in_backups = dir.join(file);
        if in_backups.exists() {
            return Ok(in_backups);
        }
    }
    bail!(
        "there is no backup {} (nor in {}); `orre backup --list` lists the backups",
        file.display(),
        dir.display()
    );
}

/// What restoring the checked backup would undo, as the Restore dialog
/// says it, when it has a backup's name (so its time is known), the current
/// database is up to date, and there's something to undo. Counting is only
/// a preview: a failure leaves the line out.
fn since(database: &Path, checked: &RestoreCheck) -> Option<detail::Since> {
    let (_, taken_at) = checked.named?;
    let db = Database::open_existing(database).ok()?;
    detail::since(&db, &checked.path, taken_at)
        .ok()
        .filter(|since| !since.is_empty())
}

fn write_check(
    database: &Path,
    checked: &RestoreCheck,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(out, "Backup: {}", checked.path.display())?;
    if let Some((reason, taken_at)) = checked.named {
        writeln!(
            out,
            "  taken {}, {}",
            taken_at.format("%Y-%m-%d %H:%M:%S UTC"),
            describe(reason)
        )?;
    }
    if checked.schema_version < checked.supported_version {
        writeln!(
            out,
            "  schema version {}, older than this Orre's ({}): it is updated after restoring",
            checked.schema_version, checked.supported_version
        )?;
    } else {
        writeln!(out, "  schema version {}", checked.schema_version)?;
    }
    writeln!(
        out,
        "  passed SQLite's integrity check and matches this Orre's migration history"
    )?;

    match checked.current {
        Current::Missing => {
            writeln!(
                out,
                "There is no database at {} yet; restoring creates it.",
                database.display()
            )?;
        }
        Current::At(version) => {
            writeln!(
                out,
                "Current database: {}, schema version {version}",
                database.display()
            )?;
            writeln!(
                out,
                "Restoring replaces it with the backup. It is backed up first, so the \
                 restore can be undone by restoring that backup."
            )?;
            if let Some(since) = since(database, checked) {
                writeln!(
                    out,
                    "Restoring undoes everything since then: {}",
                    Run::joined(&undoes(&since))
                )?;
            }
        }
        Current::Unreadable => {
            writeln!(
                out,
                "Current database: {}, which SQLite can't read",
                database.display()
            )?;
            writeln!(
                out,
                "Restoring backs it up first, which needs it readable. Move it (and any \
                 -wal and -shm files beside it) aside to restore over it."
            )?;
        }
    }

    writeln!(
        out,
        "Your own bank presets ({}) aren't in backups: restoring doesn't change them, and an \
         account whose preset isn't on this computer needs its mapping chosen again at import.",
        orre_core::paths::presets_dir(database).display()
    )?;

    if let Some(interrupted) = &checked.interrupted {
        let started = interrupted
            .started
            .map(|time| format!(" started {}", time.format("%Y-%m-%d %H:%M:%S UTC")))
            .unwrap_or_default();
        let from = interrupted
            .backup
            .as_ref()
            .map(|backup| format!(" from {}", backup.display()))
            .unwrap_or_default();
        writeln!(out)?;
        writeln!(
            out,
            "Warning: an earlier restore{from}{started} stopped before it finished. The \
             current database is complete: either the one it was replacing or that backup. \
             Restoring now backs it up first, as always."
        )?;
    }
    Ok(())
}

fn describe(reason: Reason) -> &'static str {
    match reason {
        Reason::Import => "before an import",
        Reason::Migration => "before a database update",
        Reason::Restore => "before a restore",
        Reason::Manual => "by hand",
    }
}

/// `1234567` → `1.1 MB`: binary units, one decimal, integer arithmetic.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut unit = 0;
    let mut divisor: u64 = 1024;
    while unit + 1 < UNITS.len() && bytes >= divisor * 1024 {
        divisor *= 1024;
        unit += 1;
    }
    let tenths = (u128::from(bytes) * 10 + u128::from(divisor) / 2) / u128::from(divisor);
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::human_size;

    #[test]
    fn sizes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1_234_567), "1.2 MB");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0 GB");
    }
}
