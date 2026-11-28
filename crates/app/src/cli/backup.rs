//! `orre backup`, `orre backup --list`, `orre backup --delete <file>`
//! and `orre restore <file>` (M1.28, R.12): local backups in `<data
//! dir>/backups` ([`orre_core::backup`]).

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::Args;
use orre_core::backup::{self, Current, Reason, RestoreCheck, detail};
use orre_core::db::{self, Database};

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
    let checked = backup::check(database, &file)
        .map_err(|error| anyhow::Error::msg(restore_failed(&file, &error)))?;
    write_check(database, &checked, out)?;

    if !args.yes {
        writeln!(out)?;
        writeln!(
            out,
            "Nothing was restored. Run the command again with --yes to restore it."
        )?;
        return Ok(());
    }

    // The reasons say whether anything changed.
    let restored = backup::restore(database, &file)
        .map_err(|error| anyhow::Error::msg(restore_failed(&file, &error)))?;
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

/// A db::Error as a restore reason: drop "the database" so it reads as
/// "can't restore {file}: it …" / "its …". Other variants keep their
/// existing wording.
fn db_reason(source: &db::Error) -> String {
    match source {
        db::Error::SchemaTooNew { found, supported } => format!(
            "it is at schema version {found}, newer than this version of Orre supports ({supported}); update Orre to open it"
        ),
        db::Error::MigrationHistory { version, problem } => format!(
            "its migration history doesn't match this version of Orre: migration {version} {problem}"
        ),
        _ => source.to_string(),
    }
}

/// The one message a refused or failed restore prints:
/// `can't restore {file}: {reason}`. The reason is said here rather than
/// with core's message, which names the file and says it can't be
/// restored too. "Nothing was restored" ends only the reasons that come
/// before the backup replaces the database.
fn restore_failed(file: &Path, error: &backup::Error) -> String {
    use backup::{Error, SchemaProblem};

    const NOTHING: &str = "Nothing was restored.";
    let reason = match error {
        Error::Io {
            action,
            path,
            source,
        } => format!("couldn't {action} {}: {source}", path.display()),
        Error::Sqlite(source) => format!("database error: {source}"),
        Error::Database(source) => db_reason(source),
        Error::NotADatabase { .. } => "it isn't a SQLite database".to_owned(),
        Error::NotOrre { .. } => "it isn't an Orre database (no migration history)".to_owned(),
        Error::Integrity { count, first, .. } => format!(
            "it failed SQLite's integrity check ({count} {}; the first: {first}). {NOTHING}",
            if *count == 1 { "problem" } else { "problems" }
        ),
        Error::Incompatible { source, .. } => format!("{}. {NOTHING}", db_reason(source)),
        Error::Schema {
            version,
            problem,
            object_type,
            name,
            ..
        } => {
            let kind = match problem {
                SchemaProblem::Extra => "extra",
                SchemaProblem::Missing => "missing",
                SchemaProblem::Changed => "changed",
            };
            format!(
                "its schema doesn't match Orre's (version {version}): \
                 {kind} {object_type} {name:?}. {NOTHING}"
            )
        }
        Error::PreRestoreBackup { path, source } => format!(
            "the current database {} couldn't be backed up ({source}), so nothing was \
             restored; if it is damaged, move it and its -wal and -shm files aside first",
            path.display()
        ),
        Error::InUse { path } => format!(
            "{} is still open in another program (its -wal file remains); close Orre and \
             anything else using it, then restore again. {NOTHING}",
            path.display()
        ),
        Error::RestoredButNotOpened {
            path,
            pre_restore,
            source,
        } => format!(
            "it was restored to {}, but it couldn't be opened ({}){}",
            path.display(),
            db_reason(source),
            match pre_restore {
                Some(backup) => format!("; the database it replaced is in {}", backup.display()),
                None => String::new(),
            }
        ),
        Error::OrphanSidecar { sidecar, database } => format!(
            "{} exists without its database {}; it may hold changes, so move it aside \
             before restoring. {NOTHING}",
            sidecar.display(),
            database.display()
        ),
        Error::RestoreLocked { lock } => format!(
            "another restore is running (it holds the lock on {}); wait for it to finish, \
             then restore again. {NOTHING}",
            lock.display()
        ),
        Error::AiAccessNotOff { source, .. } => format!(
            "AI access in it couldn't be switched off{}. Nothing was restored; the current \
             database is unchanged.",
            match source {
                Some(source) => format!(" ({source})"),
                None => String::new(),
            }
        ),
        Error::NotABackup { name, dir } => {
            format!("{} isn't a backup in {}", name.display(), dir.display())
        }
    };
    format!("can't restore {}: {reason}", file.display())
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

    mod restore_failed {
        use std::io;
        use std::path::{Path, PathBuf};

        use orre_core::backup::{Error, SchemaProblem};
        use orre_core::db::{self, Database, HistoryProblem};

        fn file() -> PathBuf {
            PathBuf::from("/scratch/backups/x.db")
        }

        fn database() -> PathBuf {
            PathBuf::from("/scratch/orre.db")
        }

        /// The message for `error`, checked to be `expected` and to say
        /// "can't restore" once and never "can't be restored".
        fn assert_message(error: &Error, expected: &str) {
            let message = super::super::restore_failed(&file(), error);
            assert_eq!(message, expected);
            assert_eq!(message.matches("can't restore").count(), 1, "{message}");
            assert!(!message.contains("can't be restored"), "{message}");
        }

        fn io_error() -> io::Error {
            io::Error::other("disk full")
        }

        fn schema_too_new() -> db::Error {
            db::Error::SchemaTooNew {
                found: 99,
                supported: 20,
            }
        }

        /// A real SQLite error, from opening a file that isn't a database
        /// (the app doesn't depend on rusqlite to make one directly).
        fn sqlite_error() -> (String, Error) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("notes.db");
            std::fs::write(&path, "not a database ".repeat(10)).unwrap();
            match Database::open(&path) {
                Err(db::Error::Sqlite(source)) => (source.to_string(), Error::Sqlite(source)),
                Err(other) => panic!("not a SQLite error: {other:?}"),
                Ok(_) => panic!("opened a file that isn't a database"),
            }
        }

        fn at(path: &Path) -> String {
            path.display().to_string()
        }

        #[test]
        fn io() {
            let error = Error::Io {
                action: "read",
                path: file(),
                source: io_error(),
            };
            assert_message(
                &error,
                &format!(
                    "can't restore {0}: couldn't read {0}: disk full",
                    at(&file())
                ),
            );
        }

        #[test]
        fn sqlite() {
            let (source, error) = sqlite_error();
            assert_message(
                &error,
                &format!("can't restore {}: database error: {source}", at(&file())),
            );
        }

        #[test]
        fn database_error() {
            let error = Error::Database(schema_too_new());
            assert_message(
                &error,
                &format!(
                    "can't restore {}: it is at schema version 99, newer than this version of Orre supports (20); update Orre to open it",
                    at(&file())
                ),
            );
        }

        #[test]
        fn not_a_database() {
            let error = Error::NotADatabase { path: file() };
            assert_message(
                &error,
                &format!("can't restore {}: it isn't a SQLite database", at(&file())),
            );
        }

        #[test]
        fn not_orre() {
            let error = Error::NotOrre { path: file() };
            assert_message(
                &error,
                &format!(
                    "can't restore {}: it isn't an Orre database (no migration history)",
                    at(&file())
                ),
            );
        }

        #[test]
        fn integrity() {
            let error = Error::Integrity {
                path: file(),
                count: 2,
                first: "page 3 is never used".to_owned(),
            };
            assert_message(
                &error,
                &format!(
                    "can't restore {}: it failed SQLite's integrity check (2 problems; the \
                     first: page 3 is never used). Nothing was restored.",
                    at(&file())
                ),
            );
            let one = Error::Integrity {
                path: file(),
                count: 1,
                first: "page 3 is never used".to_owned(),
            };
            assert_message(
                &one,
                &format!(
                    "can't restore {}: it failed SQLite's integrity check (1 problem; the \
                     first: page 3 is never used). Nothing was restored.",
                    at(&file())
                ),
            );
        }

        #[test]
        fn incompatible() {
            let error = Error::Incompatible {
                path: file(),
                source: db::Error::MigrationHistory {
                    version: 3,
                    problem: HistoryProblem::Changed,
                },
            };
            assert_message(
                &error,
                &format!(
                    "can't restore {}: its migration history doesn't match this version of Orre: migration 3 was changed after it was applied. Nothing was restored.",
                    at(&file())
                ),
            );
            let too_new = Error::Incompatible {
                path: file(),
                source: schema_too_new(),
            };
            assert_message(
                &too_new,
                &format!(
                    "can't restore {}: it is at schema version 99, newer than this version of Orre supports (20); update Orre to open it. Nothing was restored.",
                    at(&file())
                ),
            );
        }

        fn schema(problem: SchemaProblem) -> Error {
            Error::Schema {
                path: file(),
                version: 20,
                problem,
                object_type: "table".to_owned(),
                name: "qa_extra".to_owned(),
            }
        }

        #[test]
        fn schema_extra() {
            assert_message(
                &schema(SchemaProblem::Extra),
                &format!(
                    "can't restore {}: its schema doesn't match Orre's (version 20): extra \
                     table \"qa_extra\". Nothing was restored.",
                    at(&file())
                ),
            );
        }

        #[test]
        fn schema_missing() {
            assert_message(
                &schema(SchemaProblem::Missing),
                &format!(
                    "can't restore {}: its schema doesn't match Orre's (version 20): missing \
                     table \"qa_extra\". Nothing was restored.",
                    at(&file())
                ),
            );
        }

        #[test]
        fn schema_changed() {
            assert_message(
                &schema(SchemaProblem::Changed),
                &format!(
                    "can't restore {}: its schema doesn't match Orre's (version 20): changed \
                     table \"qa_extra\". Nothing was restored.",
                    at(&file())
                ),
            );
        }

        #[test]
        fn pre_restore_backup() {
            let error = Error::PreRestoreBackup {
                path: database(),
                source: Box::new(Error::Io {
                    action: "copy",
                    path: database(),
                    source: io_error(),
                }),
            };
            assert_message(
                &error,
                &format!(
                    "can't restore {}: the current database {1} couldn't be backed up \
                     (couldn't copy {1}: disk full), so nothing was restored; if it is \
                     damaged, move it and its -wal and -shm files aside first",
                    at(&file()),
                    at(&database())
                ),
            );
        }

        #[test]
        fn in_use() {
            let error = Error::InUse { path: database() };
            assert_message(
                &error,
                &format!(
                    "can't restore {}: {} is still open in another program (its -wal file \
                     remains); close Orre and anything else using it, then restore again. \
                     Nothing was restored.",
                    at(&file()),
                    at(&database())
                ),
            );
        }

        #[test]
        fn restored_but_not_opened() {
            let pre_restore = PathBuf::from("/scratch/backups/orre-pre.db");
            let error = Error::RestoredButNotOpened {
                path: database(),
                pre_restore: Some(pre_restore.clone()),
                source: schema_too_new(),
            };
            let message = super::super::restore_failed(&file(), &error);
            assert!(!message.contains("othing was restored"), "{message}");
            assert_message(
                &error,
                &format!(
                    "can't restore {}: it was restored to {}, but it couldn't be opened (it is at schema version 99, newer than this version of Orre supports (20); update Orre to open it); the database it replaced is in {}",
                    at(&file()),
                    at(&database()),
                    at(&pre_restore)
                ),
            );
            let without = Error::RestoredButNotOpened {
                path: database(),
                pre_restore: None,
                source: schema_too_new(),
            };
            assert_message(
                &without,
                &format!(
                    "can't restore {}: it was restored to {}, but it couldn't be opened (it is at schema version 99, newer than this version of Orre supports (20); update Orre to open it)",
                    at(&file()),
                    at(&database())
                ),
            );
        }

        #[test]
        fn orphan_sidecar() {
            let sidecar = PathBuf::from("/scratch/orre.db-wal");
            let error = Error::OrphanSidecar {
                sidecar: sidecar.clone(),
                database: database(),
            };
            assert_message(
                &error,
                &format!(
                    "can't restore {}: {} exists without its database {}; it may hold \
                     changes, so move it aside before restoring. Nothing was restored.",
                    at(&file()),
                    at(&sidecar),
                    at(&database())
                ),
            );
        }

        #[test]
        fn restore_locked() {
            let lock = PathBuf::from("/scratch/orre.db.restore-lock");
            let error = Error::RestoreLocked { lock: lock.clone() };
            assert_message(
                &error,
                &format!(
                    "can't restore {}: another restore is running (it holds the lock on {}); \
                     wait for it to finish, then restore again. Nothing was restored.",
                    at(&file()),
                    at(&lock)
                ),
            );
        }

        #[test]
        fn ai_access_not_off() {
            let error = Error::AiAccessNotOff {
                path: file(),
                source: None,
            };
            assert_message(
                &error,
                &format!(
                    "can't restore {}: AI access in it couldn't be switched off. Nothing was \
                     restored; the current database is unchanged.",
                    at(&file())
                ),
            );
            let (source, Error::Sqlite(sqlite)) = sqlite_error() else {
                unreachable!()
            };
            let with_source = Error::AiAccessNotOff {
                path: file(),
                source: Some(sqlite),
            };
            assert_message(
                &with_source,
                &format!(
                    "can't restore {}: AI access in it couldn't be switched off ({source}). \
                     Nothing was restored; the current database is unchanged.",
                    at(&file())
                ),
            );
        }

        #[test]
        fn not_a_backup() {
            let dir = PathBuf::from("/scratch/backups");
            let error = Error::NotABackup {
                name: PathBuf::from("notes.db"),
                dir: dir.clone(),
            };
            assert_message(
                &error,
                &format!(
                    "can't restore {}: notes.db isn't a backup in {}",
                    at(&file()),
                    at(&dir)
                ),
            );
        }
    }
}
