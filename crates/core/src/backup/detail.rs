//! What the app's Backups screen shows about each backup (ROADMAP M3.16;
//! `docs/DESIGN.md` → "Settings" → **Backups**): what a backup holds, why
//! it was taken, which group it's listed in, what restoring it would undo,
//! and the restore checks one by one.
//!
//! A backup's metadata lives in its file name (decision record 2026-11-10,
//! point 6), so everything else is read: from the backup itself, opened
//! read-only and `immutable` so nothing (not even a `-wal` or `-shm`) is
//! written beside it, or from the current database, whose import batches,
//! change batches and suggestions carry their times.

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};

use super::{Backup, Error, KEEP_RECENT, Reason, RestoreCheck, SchemaProblem};
use crate::clock;
use crate::db::{self, Database, SchemaVersion};

/// What a backup holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contents {
    pub schema_version: SchemaVersion,
    /// Every transaction row, excluded ones and pot mirrors included.
    pub transactions: u64,
    /// Every account, closed ones included.
    pub accounts: u64,
    pub rules: u64,
}

/// `path` as an SQLite URI that opens it read-only and `immutable`: no
/// locks, no journal, nothing written next to it.
fn immutable_uri(path: &Path) -> String {
    let mut uri = String::from("file:");
    for c in path.to_string_lossy().chars() {
        match c {
            '%' => uri.push_str("%25"),
            '?' => uri.push_str("%3F"),
            '#' => uri.push_str("%23"),
            c => uri.push(c),
        }
    }
    uri.push_str("?immutable=1");
    uri
}

/// The backup at `path`, opened read-only and `immutable`, with
/// `trusted_schema` off: it may not be one of Orre's.
fn open_backup(path: &Path) -> Result<Connection, Error> {
    let conn = Connection::open_with_flags(
        immutable_uri(path),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.pragma_update(None, "trusted_schema", false)?;
    Ok(conn)
}

/// What the backup at `path` holds, read without changing the file.
pub fn contents(path: &Path) -> Result<Contents, Error> {
    let conn = open_backup(path)?;
    let schema_version = db::schema_version_on(&conn)?;
    let count = |table: &str| -> Result<u64, Error> {
        let count: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })?;
        Ok(count.unsigned_abs())
    };
    Ok(Contents {
        schema_version,
        transactions: count("\"transaction\"")?,
        accounts: count("account")?,
        rules: count("rule")?,
    })
}

/// For each of `backups` (as [`super::list`] returns them, newest first),
/// whether it's kept as a month's backup rather than as a recent one: an
/// automatic backup outside the [`KEEP_RECENT`] most recent automatic ones,
/// which rotation keeps only as the newest of its month. Manual backups
/// are never monthly.
pub fn monthly(backups: &[Backup]) -> Vec<bool> {
    let mut automatic = 0;
    backups
        .iter()
        .map(|backup| {
            if !backup.reason.is_automatic() {
                return false;
            }
            automatic += 1;
            automatic > KEEP_RECENT
        })
        .collect()
}

/// Why a backup was taken, with what can be read about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail {
    /// Before an import: the file imported, when the current database
    /// still has its batch (committed or undone).
    Import { file: Option<String> },
    /// Before a database update: from the backup's schema to the next one.
    Migration {
        from: Option<SchemaVersion>,
        to: Option<SchemaVersion>,
    },
    /// Before a restore.
    Restore,
    /// Asked for.
    Manual,
}

/// What each of `backups` (newest first) was taken before. `versions` are
/// their schema versions where read ([`contents`]), in the same order;
/// `db` is the current database.
///
/// An import backup is taken just before its import commits, so its import
/// is the first batch the current database recorded between it and the
/// next newer backup. A migration backup's update went to the schema of the
/// next newer backup, or the current database's when it's the newest.
pub fn details(
    db: &Database,
    backups: &[Backup],
    versions: &[Option<SchemaVersion>],
) -> Result<Vec<Detail>, Error> {
    let current = db.schema_version()?;
    let mut first_import = db.conn().prepare(
        "SELECT source_file FROM import_batch \
         WHERE imported_at >= ?1 AND (?2 IS NULL OR imported_at < ?2) \
         ORDER BY imported_at, id LIMIT 1",
    )?;
    backups
        .iter()
        .enumerate()
        .map(|(index, backup)| {
            let newer = index.checked_sub(1).map(|newer| &backups[newer]);
            Ok(match backup.reason {
                Reason::Import => {
                    let from = clock::stored(backup.taken_at);
                    let to = newer.map(|newer| clock::stored(newer.taken_at));
                    let file = first_import
                        .query_map(rusqlite::params![from, to], |row| row.get(0))?
                        .next()
                        .transpose()?;
                    Detail::Import { file }
                }
                Reason::Migration => Detail::Migration {
                    from: versions.get(index).copied().flatten(),
                    to: match newer {
                        Some(_) => versions.get(index - 1).copied().flatten(),
                        None => Some(current),
                    },
                },
                Reason::Restore => Detail::Restore,
                Reason::Manual => Detail::Manual,
            })
        })
        .collect()
}

/// An import committed after a backup was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSince {
    /// The imported file's name.
    pub file: String,
    /// The transactions it added.
    pub rows: u64,
}

/// What restoring a backup taken at a given time would undo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Since {
    /// Imports committed since, oldest first.
    pub imports: Vec<ImportSince>,
    /// Changes to many transactions made since and not undone (rule
    /// applies, bulk edits, duplicate merges).
    pub edits: u64,
    /// Transactions added by hand since (`origin = 'manual'`), which
    /// restoring deletes.
    pub manual: u64,
    /// Transactions added by hand that the backup holds and the current
    /// database doesn't (deleted since), which restoring brings back. At
    /// most [`DELETED_CAP`]: past it, counting stops.
    pub deleted: u64,
    /// Whether `deleted` is where counting stopped ([`DELETED_CAP`]), so
    /// there are more: "at least".
    pub deleted_at_least: bool,
    /// Whether counting them ran out of its budget ([`DELETED_BUDGET`]), so
    /// how many is unknown (`deleted` is then 0 and means nothing).
    pub deleted_unknown: bool,
    /// Inbox suggestions filed since, whatever has happened to them.
    pub suggestions: u64,
}

impl Since {
    pub fn is_empty(&self) -> bool {
        self.imports.is_empty()
            && self.edits == 0
            && self.manual == 0
            && self.deleted == 0
            && !self.deleted_unknown
            && self.suggestions == 0
    }
}

/// What the current database has gained since `taken_at`, when `backup`
/// was taken, and the transactions added by hand it has lost since:
/// those `backup` holds and it doesn't.
pub fn since(db: &Database, backup: &Path, taken_at: DateTime<Utc>) -> Result<Since, Error> {
    let at = clock::stored(taken_at);
    let conn = db.conn();
    let imports = conn
        .prepare(
            "SELECT b.source_file, \
                    (SELECT count(*) FROM \"transaction\" t WHERE t.batch_id = b.id) \
             FROM import_batch b WHERE b.imported_at >= ?1 AND b.status = 'committed' \
             ORDER BY b.imported_at, b.id",
        )?
        .query_map([&at], |row| {
            Ok(ImportSince {
                file: row.get(0)?,
                rows: row.get::<_, i64>(1)?.unsigned_abs(),
            })
        })?
        .collect::<Result<_, _>>()?;
    let count = |sql: &str| -> Result<u64, Error> {
        let count: i64 = conn.query_row(sql, [&at], |row| row.get(0))?;
        Ok(count.unsigned_abs())
    };
    // A count that can't be made (the budget ran out, or the backup can't
    // be read as one) is unknown; the other counts stand.
    let counted = deleted_by_hand(db, backup, DELETED_CAP, DELETED_BUDGET).unwrap_or(None);
    let (deleted, deleted_at_least) = counted.unwrap_or((0, false));
    Ok(Since {
        imports,
        edits: count(
            "SELECT count(*) FROM change_batch WHERE created_at >= ?1 AND status = 'applied'",
        )?,
        manual: count(
            "SELECT count(*) FROM \"transaction\" WHERE origin = 'manual' AND created_at >= ?1",
        )?,
        deleted,
        deleted_at_least,
        deleted_unknown: counted.is_none(),
        suggestions: count("SELECT count(*) FROM suggestion WHERE created_at >= ?1")?,
    })
}

/// Where counting transactions deleted by hand stops. The count runs on
/// the UI thread when a backup is selected or the Restore dialog opens, and
/// the backup may be a hostile file, so the work is bounded: far more than
/// anyone deletes by hand between two backups, and a few milliseconds of
/// lookups.
pub const DELETED_CAP: u64 = 10_000;

/// How much work counting them may do in the backup: progress-handler
/// ticks of [`BUDGET_TICK`] SQLite instructions each. Past it the query is
/// interrupted and the count is unknown. The cap alone doesn't bound the
/// scan: a backup's other rows are read too (`origin` has no index).
/// Measured (SLI-57, release build): a backup with [`DELETED_CAP`]
/// transactions deleted takes 131 ticks, 431 with 100,000 imported rows
/// beside them and 1,631 with 500,000 (about 3 per 1,000 rows), so this
/// covers a few million rows; a hostile file is stopped in about 0.15 s
/// (0.3 s in a debug build).
pub const DELETED_BUDGET: u64 = 10_000;

/// SQLite instructions between progress-handler calls: one tick of
/// [`DELETED_BUDGET`].
pub const BUDGET_TICK: std::ffi::c_int = 1_000;

/// How many of `backup`'s manual transactions the current database
/// doesn't hold (SLI-57). Only [`crate::manual::delete`] removes a manual
/// row, and it records no change batch, so the backup is the record. A
/// row is the same row when its ID and insert time match: IDs are
/// `AUTOINCREMENT` and neither changes (`transaction_source_fields_read_only`),
/// and the time tells a row apart from a later one given the same ID after
/// a restore set the sequence back.
///
/// The backup may not be Orre's: without a plain `"transaction"` table,
/// with the columns read, it holds nothing to bring back. A virtual table
/// (SQLite lists one as a table) is refused before anything reads it: its
/// module could produce rows without end from a small file. Only those two columns
/// are read, never a description, and the scan is one pass over its
/// manual rows, as [`contents`] counts every row.
///
/// Counting stops past `cap`, which returns `(cap, true)`. At most the
/// current database's manual rows plus `cap + 1` backup rows are read and
/// looked up: in an Orre backup each held row matches a different current
/// one (IDs are unique), so reading more than that means more than `cap`
/// are deleted.
///
/// The backup's connection carries a progress handler: past `budget` ticks
/// the query is interrupted and the count is unknown, `None`. Other errors
/// are returned.
fn deleted_by_hand(
    db: &Database,
    backup: &Path,
    cap: u64,
    budget: u64,
) -> Result<Option<(u64, bool)>, Error> {
    let conn = open_backup(backup)?;
    let mut ticks = 0_u64;
    conn.progress_handler(
        BUDGET_TICK,
        Some(move || {
            ticks += 1;
            ticks > budget
        }),
    )?;
    match count_deleted(db, &conn, cap) {
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::OperationInterrupted =>
        {
            Ok(None)
        }
        counted => Ok(Some(counted?)),
    }
}

/// [`deleted_by_hand`]'s count on the backup's connection `conn`.
fn count_deleted(db: &Database, conn: &Connection, cap: u64) -> rusqlite::Result<(u64, bool)> {
    let virtual_table: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'transaction' \
         AND ltrim(sql, ' ' || char(9, 10, 13)) LIKE 'CREATE VIRTUAL TABLE%')",
        [],
        |row| row.get(0),
    )?;
    if virtual_table {
        return Ok((0, false));
    }
    let readable: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'transaction') \
         AND (SELECT count(*) FROM pragma_table_info('transaction') \
              WHERE name IN ('id', 'origin', 'created_at')) = 3",
        [],
        |row| row.get(0),
    )?;
    if !readable {
        return Ok((0, false));
    }
    let current: i64 = db.conn().query_row(
        "SELECT count(*) FROM \"transaction\" WHERE origin = 'manual'",
        [],
        |row| row.get(0),
    )?;
    let limit = current.unsigned_abs().saturating_add(cap).saturating_add(1);
    let mut held = db.conn().prepare(
        "SELECT EXISTS (SELECT 1 FROM \"transaction\" \
         WHERE id = ?1 AND created_at = ?2 AND origin = 'manual')",
    )?;
    let mut rows = conn.prepare(
        "SELECT id, created_at FROM \"transaction\" \
         WHERE origin = 'manual' AND typeof(id) = 'integer' AND typeof(created_at) = 'text' \
         AND octet_length(created_at) = 24 \
         AND created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at) \
         LIMIT ?1",
    )?;
    let mut rows = rows.query([i64::try_from(limit).unwrap_or(i64::MAX)])?;
    let (mut read, mut deleted) = (0_u64, 0_u64);
    while let Some(row) = rows.next()? {
        read += 1;
        if read == limit {
            return Ok((cap, true));
        }
        let id: i64 = row.get(0)?;
        let created_at: String = row.get(1)?;
        if !held.query_row(rusqlite::params![id, created_at], |row| {
            row.get::<_, bool>(0)
        })? {
            deleted += 1;
            if deleted > cap {
                return Ok((cap, true));
            }
        }
    }
    Ok((deleted, false))
}

/// Saves a copy of the backup at `from` as `to`, a new file readable only
/// by the user (`0600`, as backups are), for "Save a copy…". An existing
/// `to` is replaced (the save dialog has asked), written beside it first
/// and renamed over it; the database itself and its sidecar files are
/// refused.
pub fn save_copy(db_path: &Path, from: &Path, to: &Path) -> Result<(), Error> {
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    for suffix in ["", "-wal", "-shm", "-journal"] {
        if same(&db::sidecar(db_path, suffix), to) {
            return Err(Error::Io {
                action: "save a copy over",
                path: to.to_path_buf(),
                source: std::io::Error::other("it is Orre's database"),
            });
        }
    }
    if same(from, to) {
        return Ok(());
    }
    let name = to
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = to.with_file_name(format!(".{name}.orre-copy.tmp"));
    let _ = std::fs::remove_file(&partial);
    super::create_private_file(&partial)?;
    let copied = super::copy_into(from, &partial)
        .and_then(|()| super::finish_file(&partial))
        .and_then(|()| {
            std::fs::rename(&partial, to).map_err(|source| Error::Io {
                action: "save",
                path: to.to_path_buf(),
                source,
            })
        });
    if copied.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    copied
}

/// What the restore checks found, check by check: the file is intact
/// (SQLite's integrity check), then this version of Orre can read it
/// (Orre's migration history, a schema no newer than this build's and laid
/// out as the migrations make it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessment {
    /// Both passed: it can be restored.
    Ready(RestoreCheck),
    /// The file isn't a SQLite database ([`Error::NotADatabase`]). The
    /// second check didn't run.
    NotADatabase,
    /// A SQLite database that fails the integrity check
    /// ([`Error::Integrity`]): how many problems, and SQLite's message for
    /// the first, which can name objects in the file. The second check
    /// didn't run.
    Integrity { count: usize, first: String },
    /// Intact, but not an Orre database.
    NotOrre,
    /// Intact, but from a newer Orre.
    TooNew {
        found: SchemaVersion,
        supported: SchemaVersion,
    },
    /// Intact, but its migrations differ from this build's.
    OtherHistory,
    /// Intact, but its schema isn't what the migrations create at its
    /// version ([`Error::Schema`]): the first object that differs, its type
    /// and name capped as there; never its SQL.
    Schema {
        version: SchemaVersion,
        problem: SchemaProblem,
        object_type: String,
        name: String,
    },
}

impl Assessment {
    pub fn is_ready(&self) -> bool {
        matches!(self, Assessment::Ready(_))
    }

    /// `error` as the check it fails, when it is one: what [`assess`]
    /// makes of [`super::check`]'s errors, and of [`super::restore`]'s
    /// when the file changed after it was checked. Any other error is
    /// returned as it is.
    pub fn of_failed_check(error: Error) -> Result<Self, Error> {
        match error {
            Error::NotADatabase { .. } => Ok(Assessment::NotADatabase),
            Error::Integrity { count, first, .. } => Ok(Assessment::Integrity { count, first }),
            Error::NotOrre { .. } => Ok(Assessment::NotOrre),
            Error::Incompatible {
                source: db::Error::SchemaTooNew { found, supported },
                ..
            } => Ok(Assessment::TooNew { found, supported }),
            Error::Incompatible { .. } => Ok(Assessment::OtherHistory),
            Error::Schema {
                version,
                problem,
                object_type,
                name,
                ..
            } => Ok(Assessment::Schema {
                version,
                problem,
                object_type,
                name,
            }),
            other => Err(other),
        }
    }
}

/// Runs [`super::check`] on `backup` and sorts its answer into the checks.
/// Errors that aren't a check failing (the file can't be read, the data
/// directory can't be written) are returned as they are.
pub fn assess(db_path: &Path, backup: &Path) -> Result<Assessment, Error> {
    match super::check(db_path, backup) {
        Ok(check) => Ok(Assessment::Ready(check)),
        Err(error) => Assessment::of_failed_check(error),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::TimeZone as _;
    use tempfile::TempDir;

    use super::*;
    use crate::backup;

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, hour, 0, 0).unwrap()
    }

    fn named(reason: Reason, taken_at: DateTime<Utc>) -> Backup {
        Backup {
            path: backup::file_name(reason, taken_at).into(),
            reason,
            taken_at,
            size: 0,
        }
    }

    #[test]
    fn reads_what_a_backup_holds_without_writing_beside_it() {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("orre.db")).unwrap();
        db.conn()
            .execute(
                "INSERT INTO account (name, kind, currency) VALUES ('Monzo', 'current', 'GBP')",
                [],
            )
            .unwrap();
        let taken = backup::create(&db, Reason::Manual).unwrap();
        let before: Vec<_> = fs::read_dir(taken.path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        let contents = contents(&taken.path).unwrap();
        assert_eq!(
            contents.schema_version,
            Database::supported_schema_version()
        );
        assert_eq!(
            (contents.accounts, contents.transactions, contents.rules),
            (1, 0, 0)
        );
        let after: Vec<_> = fs::read_dir(taken.path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(before, after);

        assert!(super::contents(&dir.path().join("missing.db")).is_err());
    }

    #[test]
    fn automatic_backups_past_the_recent_ones_are_monthly() {
        let mut backups: Vec<Backup> = (0..12)
            .map(|n| named(Reason::Import, at(20 - n, 9)))
            .collect();
        backups.insert(3, named(Reason::Manual, at(17, 12)));
        let monthly = monthly(&backups);
        assert_eq!(monthly.iter().filter(|monthly| **monthly).count(), 2);
        assert!(!monthly[3]);
        assert!(monthly[11] && monthly[12]);
    }

    #[test]
    fn names_the_import_and_the_update() {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("orre.db")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO account (id, name, kind, currency) VALUES (1, 'Monzo', 'current', 'GBP'); \
                 INSERT INTO import_batch (account_id, source_file, format, imported_at) \
                     VALUES (1, 'monzo-october.csv', 'csv', '2026-11-02T09:00:00.120Z'), \
                            (1, 'monzo-november.csv', 'csv', '2026-11-10T14:02:00.500Z');",
            )
            .unwrap();
        let current = db.schema_version().unwrap();
        let backups = vec![
            named(Reason::Import, at(10, 14)),
            named(Reason::Manual, at(9, 9)),
            named(Reason::Migration, at(5, 8)),
            named(Reason::Import, at(2, 9)),
            named(Reason::Import, at(1, 9)),
        ];
        let versions = [Some(current), Some(current), Some(6), Some(6), Some(6)];
        let details = details(&db, &backups, &versions).unwrap();
        assert_eq!(
            details,
            [
                Detail::Import {
                    file: Some("monzo-november.csv".into())
                },
                Detail::Manual,
                Detail::Migration {
                    from: Some(6),
                    to: Some(current)
                },
                Detail::Import {
                    file: Some("monzo-october.csv".into())
                },
                // Its import didn't commit before the next backup.
                Detail::Import { file: None },
            ]
        );
    }

    #[test]
    fn counts_what_restoring_would_undo() {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("orre.db")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO account (id, name, kind, currency) VALUES (1, 'Monzo', 'current', 'GBP'); \
                 INSERT INTO import_batch (id, account_id, source_file, format, imported_at) \
                     VALUES (1, 1, 'old.csv', 'csv', '2026-11-01T09:00:00.000Z'), \
                            (2, 1, 'monzo-november.csv', 'csv', '2026-11-10T14:02:00.500Z'); \
                 INSERT INTO change_batch (kind, created_at) \
                     VALUES ('bulk_edit', '2026-11-11T10:00:00.000Z'), \
                            ('bulk_edit', '2026-10-01T10:00:00.000Z'); \
                 INSERT INTO suggestion (kind, payload_json, source_client, created_at) \
                     VALUES ('categorization', '[1]', 'claude-code', '2026-11-12T10:00:00.000Z');",
            )
            .unwrap();
        for n in 0..3 {
            db.conn()
                .execute(
                    "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, origin, \
                     currency, description_raw, description_normalized, fingerprint) \
                     VALUES (1, 2, '2026-11-09', ?1, 'imported', 'GBP', 'x', 'X', ?2)",
                    rusqlite::params![-100 - n, format!("f{n}")],
                )
                .unwrap();
        }
        let taken = backup::create(&db, Reason::Manual).unwrap();
        let since = since(&db, &taken.path, at(10, 14)).unwrap();
        assert_eq!(
            since.imports,
            [ImportSince {
                file: "monzo-november.csv".into(),
                rows: 3
            }]
        );
        assert_eq!((since.edits, since.suggestions), (1, 1));
        assert!(
            super::since(&db, &taken.path, at(13, 0))
                .unwrap()
                .is_empty()
        );
    }

    /// A database with one account, and a way to add a transaction to it
    /// by hand.
    fn with_account(dir: &TempDir) -> (Database, crate::id::AccountId) {
        use crate::account::{self, AccountKind, NewAccount};
        use crate::money::Currency;

        let mut db = Database::open(&dir.path().join("orre.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        (db, account)
    }

    fn add_by_hand(
        db: &mut Database,
        account: crate::id::AccountId,
        description: &str,
    ) -> crate::id::TransactionId {
        use crate::manual::{self, ManualTransaction};
        use crate::money::{Currency, Money};

        let entry = ManualTransaction::new(
            account,
            chrono::NaiveDate::from_ymd_opt(2026, 11, 3).unwrap(),
            Money::new(-1250, Currency::GBP),
            description,
        );
        manual::create(db, &entry).unwrap()
    }

    /// SLI-57: a transaction added by hand before the backup and deleted
    /// since comes back when the backup is restored.
    #[test]
    fn counts_transactions_deleted_by_hand_since() {
        let dir = TempDir::new().unwrap();
        let (mut db, account) = with_account(&dir);
        let gone = add_by_hand(&mut db, account, "QA Pre Backup");
        add_by_hand(&mut db, account, "Kept");
        let taken = backup::create(&db, Reason::Manual).unwrap();
        let unchanged = since(&db, &taken.path, taken.taken_at).unwrap();
        assert_eq!(unchanged.deleted, 0);
        assert!(unchanged.is_empty());

        crate::manual::delete(&mut db, gone).unwrap();
        let since = since(&db, &taken.path, taken.taken_at).unwrap();
        assert_eq!((since.deleted, since.manual), (1, 0));
        assert!(!since.is_empty());
    }

    /// SLI-57: counting stops past the cap and says so, rather than giving
    /// a number that's too small; up to the cap it's exact. (The cap is
    /// 2 here, to keep the test small.)
    #[test]
    fn counting_deletions_stops_past_the_cap() {
        let dir = TempDir::new().unwrap();
        let (mut db, account) = with_account(&dir);
        let ids: Vec<_> = (0..5)
            .map(|n| add_by_hand(&mut db, account, &format!("Row {n}")))
            .collect();
        let taken = backup::create(&db, Reason::Manual).unwrap();
        let count = |db: &Database| {
            deleted_by_hand(db, &taken.path, 2, DELETED_BUDGET)
                .unwrap()
                .unwrap()
        };

        crate::manual::delete(&mut db, ids[0]).unwrap();
        assert_eq!(count(&db), (1, false));
        crate::manual::delete(&mut db, ids[1]).unwrap();
        assert_eq!(count(&db), (2, false));
        crate::manual::delete(&mut db, ids[2]).unwrap();
        assert_eq!(count(&db), (2, true));
        for id in &ids[3..] {
            crate::manual::delete(&mut db, *id).unwrap();
        }
        assert_eq!(count(&db), (2, true));

        // Through `since`, under the real cap: exact.
        let since = since(&db, &taken.path, taken.taken_at).unwrap();
        assert_eq!((since.deleted, since.deleted_at_least), (5, false));
    }

    /// A crafted table can repeat a held row without a primary key to stop
    /// it; reading stops anyway.
    #[test]
    fn counting_deletions_reads_a_bounded_number_of_rows() {
        let dir = TempDir::new().unwrap();
        let (mut db, account) = with_account(&dir);
        let id = add_by_hand(&mut db, account, "Held");
        let created_at: String = db
            .conn()
            .query_row(
                "SELECT created_at FROM \"transaction\" WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        let crafted = dir.path().join("crafted.db");
        let conn = Connection::open(&crafted).unwrap();
        conn.execute_batch(
            "CREATE TABLE \"transaction\" (id INTEGER, origin TEXT, created_at TEXT)",
        )
        .unwrap();
        for _ in 0..50 {
            conn.execute(
                "INSERT INTO \"transaction\" VALUES (?1, 'manual', ?2)",
                rusqlite::params![id, created_at],
            )
            .unwrap();
        }
        drop(conn);
        // One current manual row and a cap of 2: the fourth row read stops it.
        assert_eq!(
            deleted_by_hand(&db, &crafted, 2, DELETED_BUDGET).unwrap(),
            Some((2, true))
        );
        assert_eq!(
            deleted_by_hand(&db, &crafted, 100, DELETED_BUDGET).unwrap(),
            Some((0, false))
        );
    }

    /// SLI-57: a virtual table named `transaction` (SQLite lists it as a
    /// table, with the columns read) is refused unread, as a view is, even
    /// with rows that would count.
    #[test]
    fn a_virtual_transaction_table_is_not_read() {
        let dir = TempDir::new().unwrap();
        let (db, _) = with_account(&dir);
        let crafted = dir.path().join("virtual.db");
        Connection::open(&crafted)
            .unwrap()
            .execute_batch(
                "CREATE VIRTUAL TABLE \"transaction\" USING fts5(id, origin, created_at); \
                 INSERT INTO \"transaction\" VALUES (1, 'manual', '2026-01-01T00:00:00.000Z');",
            )
            .unwrap();
        // The row would count: it's what the guard keeps from being read.
        let conn = open_backup(&crafted).unwrap();
        let shape: (String, String) = conn
            .query_row(
                "SELECT typeof(id), typeof(created_at) FROM \"transaction\"",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(shape, ("integer".to_owned(), "text".to_owned()));

        assert_eq!(
            deleted_by_hand(&db, &crafted, DELETED_CAP, DELETED_BUDGET).unwrap(),
            Some((0, false))
        );
        let since = since(&db, &crafted, at(1, 0)).unwrap();
        assert_eq!(
            (since.deleted, since.deleted_at_least, since.deleted_unknown),
            (0, false, false)
        );
    }

    /// SLI-57: a count that runs out of its budget is unknown, not a hang,
    /// a panic, an error or a wrong number; the rest of `since` stands.
    #[test]
    fn a_count_past_its_budget_is_unknown() {
        let dir = TempDir::new().unwrap();
        let (mut db, account) = with_account(&dir);
        let gone = add_by_hand(&mut db, account, "Gone");
        let taken = backup::create(&db, Reason::Manual).unwrap();
        crate::manual::delete(&mut db, gone).unwrap();

        assert_eq!(
            deleted_by_hand(&db, &taken.path, DELETED_CAP, 0).unwrap(),
            None
        );
        assert_eq!(
            deleted_by_hand(&db, &taken.path, DELETED_CAP, DELETED_BUDGET).unwrap(),
            Some((1, false))
        );
    }

    /// The budget covers a real backup with [`DELETED_CAP`] transactions
    /// deleted by hand, with room to spare: a tenth of it is enough.
    #[test]
    fn the_budget_covers_a_backup_at_the_cap() {
        let dir = TempDir::new().unwrap();
        let (mut db, account) = with_account(&dir);
        let tx = db.conn_mut().transaction().unwrap();
        for n in 0..DELETED_CAP {
            tx.execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, origin, currency, \
                 description_raw, description_normalized) \
                 VALUES (?1, '2026-11-03', ?2, 'manual', 'GBP', 'x', 'X')",
                rusqlite::params![account, -1 - i64::try_from(n).unwrap()],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let taken = backup::create(&db, Reason::Manual).unwrap();
        db.conn()
            .execute("DELETE FROM \"transaction\" WHERE origin = 'manual'", [])
            .unwrap();

        assert_eq!(
            deleted_by_hand(&db, &taken.path, DELETED_CAP, DELETED_BUDGET / 10).unwrap(),
            Some((DELETED_CAP, false))
        );
    }

    /// SLI-57: a backup that can't be read as one (missing, or not a
    /// database) makes only the deletions unknown; what the current
    /// database has gained since is still counted.
    #[test]
    fn an_unreadable_backup_leaves_only_the_deletions_unknown() {
        let dir = TempDir::new().unwrap();
        let (mut db, account) = with_account(&dir);
        db.conn()
            .execute_batch(&format!(
                "INSERT INTO import_batch (id, account_id, source_file, format, imported_at) \
                     VALUES (1, {account}, 'monzo-november.csv', 'csv', '2026-11-10T14:02:00.500Z'); \
                 INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, origin, \
                     currency, description_raw, description_normalized, fingerprint) \
                     VALUES ({account}, 1, '2026-11-09', -100, 'imported', 'GBP', 'x', 'X', 'f'); \
                 INSERT INTO change_batch (kind, created_at) \
                     VALUES ('bulk_edit', '2026-11-11T10:00:00.000Z'); \
                 INSERT INTO suggestion (kind, payload_json, source_client, created_at) \
                     VALUES ('categorization', '[1]', 'claude-code', '2026-11-12T10:00:00.000Z');"
            ))
            .unwrap();
        add_by_hand(&mut db, account, "Added since");
        let garbage = dir.path().join("garbage.db");
        fs::write(&garbage, b"not a database at all, just some bytes......").unwrap();

        for backup in [dir.path().join("missing.db"), garbage] {
            assert!(deleted_by_hand(&db, &backup, DELETED_CAP, DELETED_BUDGET).is_err());
            let since = since(&db, &backup, at(10, 14)).unwrap();
            assert_eq!(
                (since.deleted, since.deleted_at_least, since.deleted_unknown),
                (0, false, true)
            );
            assert_eq!(
                since.imports,
                [ImportSince {
                    file: "monzo-november.csv".into(),
                    rows: 1
                }]
            );
            assert_eq!((since.edits, since.manual, since.suggestions), (1, 1, 1));
        }
    }

    /// SLI-57: only `created_at` values as Orre writes them (24 bytes,
    /// `YYYY-MM-DDTHH:MM:SS.SSSZ`, what the column's CHECK allows) are
    /// read; an oversized or misformatted one is dropped unread, and a real
    /// one beside them still counts.
    #[test]
    fn only_created_at_values_orre_writes_are_read() {
        let dir = TempDir::new().unwrap();
        let (db, _) = with_account(&dir);
        let crafted = dir.path().join("crafted.db");
        let conn = Connection::open(&crafted).unwrap();
        conn.execute_batch(
            "CREATE TABLE \"transaction\" (id INTEGER, origin TEXT, created_at TEXT)",
        )
        .unwrap();
        let oversized = format!("2026-01-01T00:00:00.000Z{}", "9".repeat(100_000));
        for (id, created_at) in [
            (1, "2026-01-01T00:00:00.000Z"),
            (2, oversized.as_str()),
            (3, "2026-01-01T00:00:00Z"),
            (4, "2026-01-01 00:00:00.000"),
            (5, "2026-13-01T00:00:00.000Z"),
            (6, "2026-01-01T00:00:00.000+"),
        ] {
            conn.execute(
                "INSERT INTO \"transaction\" VALUES (?1, 'manual', ?2)",
                rusqlite::params![id, created_at],
            )
            .unwrap();
        }
        drop(conn);
        assert_eq!(
            deleted_by_hand(&db, &crafted, DELETED_CAP, DELETED_BUDGET).unwrap(),
            Some((1, false))
        );

        // Every row Orre itself stores passes: the insert time comes from
        // the column's default.
        let (mut db, account) = with_account(&TempDir::new().unwrap());
        let gone = add_by_hand(&mut db, account, "Real");
        let taken = backup::create(&db, Reason::Manual).unwrap();
        crate::manual::delete(&mut db, gone).unwrap();
        assert_eq!(
            deleted_by_hand(&db, &taken.path, DELETED_CAP, DELETED_BUDGET).unwrap(),
            Some((1, false))
        );
    }

    /// One added and deleted after the backup isn't in it: restoring
    /// brings nothing back and has nothing to delete.
    #[test]
    fn a_transaction_added_and_deleted_since_counts_as_neither() {
        let dir = TempDir::new().unwrap();
        let (mut db, account) = with_account(&dir);
        let taken = backup::create(&db, Reason::Manual).unwrap();
        let brief = add_by_hand(&mut db, account, "Typo");
        crate::manual::delete(&mut db, brief).unwrap();
        let since = since(&db, &taken.path, taken.taken_at).unwrap();
        assert_eq!((since.deleted, since.manual), (0, 0));
        assert!(since.is_empty());
    }

    /// Imported rows leave only by undoing their import, which isn't a
    /// deletion by hand.
    #[test]
    fn imported_rows_removed_since_are_not_counted_as_deleted() {
        let dir = TempDir::new().unwrap();
        let (db, account) = with_account(&dir);
        db.conn()
            .execute_batch(&format!(
                "INSERT INTO import_batch (id, account_id, source_file, format, imported_at) \
                     VALUES (1, {account}, 'old.csv', 'csv', '2026-11-01T09:00:00.000Z'); \
                 INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, origin, \
                     currency, description_raw, description_normalized, fingerprint) \
                     VALUES ({account}, 1, '2026-11-01', -100, 'imported', 'GBP', 'x', 'X', 'f');"
            ))
            .unwrap();
        let taken = backup::create(&db, Reason::Manual).unwrap();
        db.conn()
            .execute("DELETE FROM \"transaction\" WHERE batch_id = 1", [])
            .unwrap();
        assert_eq!(since(&db, &taken.path, taken.taken_at).unwrap().deleted, 0);
    }

    /// IDs aren't reused (`AUTOINCREMENT`), and a row is matched by its ID
    /// and insert time, so a later row at a deleted one's ID (after a
    /// restore set the sequence back) doesn't hide the deletion.
    #[test]
    fn a_reused_id_does_not_hide_a_deletion() {
        let dir = TempDir::new().unwrap();
        let (mut db, account) = with_account(&dir);
        add_by_hand(&mut db, account, "First");
        let highest = add_by_hand(&mut db, account, "Highest");
        let taken = backup::create(&db, Reason::Manual).unwrap();

        crate::manual::delete(&mut db, highest).unwrap();
        let next = add_by_hand(&mut db, account, "Next");
        assert!(next > highest);
        let since = since(&db, &taken.path, taken.taken_at).unwrap();
        assert_eq!((since.deleted, since.manual), (1, 1));

        // As if a restore had set the sequence back: a new row at the
        // deleted one's ID, inserted later.
        crate::manual::delete(&mut db, next).unwrap();
        db.conn()
            .execute(
                "INSERT INTO \"transaction\" (id, account_id, date, amount_minor, origin, \
                 currency, description_raw, description_normalized, created_at) \
                 VALUES (?1, ?2, '2026-11-04', -1, 'manual', 'GBP', 'x', 'X', \
                         '2099-01-01T00:00:00.000Z')",
                rusqlite::params![highest, account],
            )
            .unwrap();
        assert_eq!(
            super::since(&db, &taken.path, taken.taken_at)
                .unwrap()
                .deleted,
            1
        );
    }

    /// A backup from an older schema (`origin` is in the first one) is
    /// read the same; a file without Orre's transactions table, or with a
    /// view in its place, has nothing to bring back.
    #[test]
    fn reads_older_backups_and_ignores_foreign_files() {
        let dir = TempDir::new().unwrap();
        let (db, _) = with_account(&dir);
        let old = dir.path().join("old.db");
        let old_db = db::open_at_version(&old, 1).unwrap();
        old_db
            .conn()
            .execute_batch(
                "INSERT INTO account (id, name, kind, currency) VALUES (9, 'Old', 'current', 'GBP'); \
                 INSERT INTO \"transaction\" (account_id, date, amount_minor, origin, currency, \
                     description_raw, description_normalized) \
                     VALUES (9, '2026-01-01', -1, 'manual', 'GBP', 'x', 'X');",
            )
            .unwrap();
        drop(old_db);
        assert_eq!(since(&db, &old, at(1, 0)).unwrap().deleted, 1);

        let foreign = dir.path().join("foreign.db");
        Connection::open(&foreign)
            .unwrap()
            .execute_batch(
                "CREATE TABLE t (id INTEGER, origin TEXT, created_at TEXT); \
                 INSERT INTO t VALUES (1, 'manual', '2026-01-01T00:00:00.000Z'); \
                 CREATE VIEW \"transaction\" AS SELECT * FROM t;",
            )
            .unwrap();
        assert_eq!(since(&db, &foreign, at(1, 0)).unwrap().deleted, 0);
        let empty = dir.path().join("empty.db");
        Connection::open(&empty)
            .unwrap()
            .execute_batch("CREATE TABLE x (y INTEGER)")
            .unwrap();
        assert_eq!(since(&db, &empty, at(1, 0)).unwrap().deleted, 0);
    }

    #[test]
    fn saves_a_private_copy_but_never_over_the_database() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("orre.db");
        let db = Database::open(&db_path).unwrap();
        let taken = backup::create(&db, Reason::Manual).unwrap();
        let copy = dir.path().join("elsewhere.db");
        save_copy(&db_path, &taken.path, &copy).unwrap();
        assert_eq!(fs::read(&copy).unwrap(), fs::read(&taken.path).unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        // Replaced when asked again.
        save_copy(&db_path, &taken.path, &copy).unwrap();
        let before = fs::read(&db_path).unwrap();
        assert!(save_copy(&db_path, &taken.path, &db_path).is_err());
        assert_eq!(fs::read(&db_path).unwrap(), before);
    }

    #[test]
    fn sorts_check_failures_into_the_checks() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("orre.db");
        let db = Database::open(&db_path).unwrap();
        let good = backup::create(&db, Reason::Manual).unwrap();
        drop(db);
        assert!(assess(&db_path, &good.path).unwrap().is_ready());

        let junk = dir.path().join("junk.db");
        fs::write(&junk, b"not a database at all, just some bytes......").unwrap();
        assert_eq!(assess(&db_path, &junk).unwrap(), Assessment::NotADatabase);

        // A real SQLite file with a page overwritten: damaged, not "not a
        // database".
        let corrupt = dir.path().join("corrupt.db");
        fs::copy(&good.path, &corrupt).unwrap();
        corrupt_pages(&corrupt);
        assert!(
            matches!(
                assess(&db_path, &corrupt).unwrap(),
                Assessment::Integrity { count, .. } if count >= 1
            ),
            "{:?}",
            assess(&db_path, &corrupt)
        );

        // A trigger the migrations don't create: named, typed, no SQL.
        let extra = dir.path().join("extra.db");
        fs::copy(&good.path, &extra).unwrap();
        Connection::open(&extra)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER qa_evil AFTER INSERT ON account \
                 BEGIN UPDATE mcp_client SET enabled = 1; END;",
            )
            .unwrap();
        let version = Database::supported_schema_version();
        assert_eq!(
            assess(&db_path, &extra).unwrap(),
            Assessment::Schema {
                version,
                problem: SchemaProblem::Extra,
                object_type: "trigger".to_owned(),
                name: "qa_evil".to_owned(),
            }
        );

        let empty = dir.path().join("empty.db");
        Connection::open(&empty)
            .unwrap()
            .execute_batch("CREATE TABLE t (x INTEGER)")
            .unwrap();
        assert_eq!(assess(&db_path, &empty).unwrap(), Assessment::NotOrre);

        assert!(assess(&db_path, &dir.path().join("missing.db")).is_err());
    }

    /// Overwrites every page after the first (the schema's) of the SQLite
    /// file at `path` past its 8-byte header, so the file still opens but
    /// fails the integrity check.
    fn corrupt_pages(path: &Path) {
        let mut bytes = fs::read(path).unwrap();
        let page = u16::from_be_bytes([bytes[16], bytes[17]]) as usize;
        let len = bytes.len();
        for start in (page..len).step_by(page) {
            for byte in &mut bytes[start + 8..(start + page).min(len)] {
                *byte = 0xA5;
            }
        }
        fs::write(path, bytes).unwrap();
    }
}
