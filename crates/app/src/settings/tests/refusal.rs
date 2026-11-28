//! The Restore dialog's checklist and refusals (SLI-71), driven through the
//! shell on crafted files: which row fails, its detail, the body, the rows
//! not shown, Restore off, the footer, and the Details block.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::super::restore::RestoreDialog;
use super::super::restore::refusal::{self, Failure};
use super::*;

/// A copy of a fresh backup of `scratch`, as `name` beside it.
fn copy_of_a_backup(scratch: &Scratch, name: &str) -> PathBuf {
    let (taken, _) = SettingsStore::new(scratch.path.clone()).back_up().unwrap();
    let copy = scratch.path.with_file_name(name);
    fs::copy(&taken.path, &copy).unwrap();
    copy
}

/// Overwrites every page after the first of the SQLite file at `path`
/// past its 8-byte header, so it opens but fails the integrity check.
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

fn sql(path: &Path, batch: &str) {
    Connection::open(path)
        .unwrap()
        .execute_batch(batch)
        .unwrap();
}

/// A trigger whose body is the SQL that must never be shown.
const HOSTILE_TRIGGER: &str = "CREATE TRIGGER qa_evil AFTER INSERT ON account \
     BEGIN UPDATE mcp_client SET enabled = 1, access_level = 'standard'; END;";

/// Opens the Restore dialog on `path` through Settings, as "Restore from a
/// file…" does when `from_file`, and waits for its checks.
fn open_restore(
    shell: &Entity<ShellView>,
    settings: &Entity<SettingsView>,
    path: &Path,
    from_file: bool,
    cx: &mut VisualTestContext,
) -> crate::ui::describe::Dialog {
    let target = if from_file {
        RestoreTarget::chosen(path.to_path_buf())
    } else {
        RestoreTarget {
            path: path.to_path_buf(),
            taken_at: None,
            local: None,
            from_file: false,
        }
    };
    cx.update(|_, cx| {
        settings.update(cx, |_, cx| cx.emit(SettingsEvent::RestoreRequested(target)))
    });
    redraw(cx);
    describe(shell, settings, cx).dialog.expect("the dialog")
}

fn dialog_entity(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> Entity<RestoreDialog> {
    cx.update(|_, cx| {
        shell
            .read(cx)
            .dialog()
            .cloned()
            .expect("a dialog")
            .downcast::<RestoreDialog>()
            .unwrap_or_else(|_| panic!("not the Restore dialog"))
    })
}

fn field(label: &str, value: &str) -> (String, String) {
    (label.to_owned(), value.to_owned())
}

const INTACT: (&str, &str) = ("check: The file is intact", "integrity check");
const CANT: &str = "It can't be restored. Nothing has changed.";

/// The rows and body of a refusal, Restore off, Details collapsed.
fn assert_refused(
    dialog: &crate::ui::describe::Dialog,
    rows: &[(&str, &str)],
    body: &str,
    details: bool,
) {
    let mut expected: Vec<(String, String)> = rows
        .iter()
        .map(|(label, value)| field(label, value))
        .collect();
    expected.push(field("body", body));
    assert_eq!(dialog.fields, expected, "{dialog:?}");
    assert!(dialog.fields.iter().all(|(label, _)| label != "caption"));
    assert!(
        dialog.actions.contains(&button("Restore", false)),
        "{dialog:?}"
    );
    assert_eq!(
        dialog.actions.contains(&button("Details", true)),
        details,
        "{dialog:?}"
    );
    assert_eq!(dialog.error, None);
}

#[gpui_kit::test]
fn each_refusal_fails_its_own_row(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, settings, cx) = open(cx, &scratch);
    let supported = Database::supported_schema_version();

    // Not a SQLite file: row ① as "isn't an Orre backup", nothing after.
    let junk = scratch.path.with_file_name("junk.db");
    fs::write(&junk, b"not a database at all, just some bytes......").unwrap();
    let dialog = open_restore(&shell, &settings, &junk, false, cx);
    assert_refused(
        &dialog,
        &[("alert: It isn't an Orre backup", "not a SQLite file")],
        CANT,
        false,
    );
    press("escape", cx);

    // A SQLite file with damaged pages: "The file is damaged".
    let corrupt = copy_of_a_backup(&scratch, "corrupt.db");
    corrupt_pages(&corrupt);
    let dialog = open_restore(&shell, &settings, &corrupt, false, cx);
    assert_refused(
        &dialog,
        &[("alert: The file is damaged", "integrity check")],
        CANT,
        true,
    );
    press("escape", cx);

    // Not Orre: row ① passes, row ② fails, row ③ isn't shown.
    let other = scratch.path.with_file_name("other.db");
    sql(&other, "CREATE TABLE t (x INTEGER)");
    let dialog = open_restore(&shell, &settings, &other, false, cx);
    assert_refused(
        &dialog,
        &[
            INTACT,
            ("alert: It isn't an Orre backup", "no migration history"),
        ],
        CANT,
        false,
    );
    press("escape", cx);

    // From a newer Orre: its own row and body, as before SLI-71.
    let newer = copy_of_a_backup(&scratch, "newer.db");
    Connection::open(&newer)
        .unwrap()
        .execute(
            "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) \
             VALUES (?1, 'from_the_future', '', '0')",
            [supported + 1],
        )
        .unwrap();
    let dialog = open_restore(&shell, &settings, &newer, false, cx);
    assert_refused(
        &dialog,
        &[
            INTACT,
            (
                "alert: It was made by a newer Orre",
                &format!("schema {}, this reads up to {supported}", supported + 1),
            ),
        ],
        "Update Orre, then restore it. Nothing has changed.",
        true,
    );
    press("escape", cx);

    // Another migration history.
    let edited = copy_of_a_backup(&scratch, "edited.db");
    sql(
        &edited,
        "UPDATE refinery_schema_history SET checksum = '1' WHERE version = 1",
    );
    let dialog = open_restore(&shell, &settings, &edited, false, cx);
    assert_refused(
        &dialog,
        &[
            INTACT,
            (
                "alert: It's from a different build of Orre",
                "migration history",
            ),
        ],
        CANT,
        false,
    );
    press("escape", cx);

    // A schema that isn't Orre's: a hostile trigger, a view, a table.
    let layout = (
        "alert: It isn't laid out like an Orre backup",
        "different layout",
    );
    let body = "Orre only restores backups laid out the way it makes them. This one may have \
                been edited or come from another app. Nothing has changed.";
    for (name, batch) in [
        ("trigger.db", HOSTILE_TRIGGER),
        (
            "view.db",
            "CREATE VIEW qa_view AS SELECT name FROM account WHERE 'SECRET_VIEW_SQL' <> ''",
        ),
        ("table.db", "CREATE TABLE qa_extra (x INTEGER)"),
    ] {
        let path = copy_of_a_backup(&scratch, name);
        sql(&path, batch);
        let dialog = open_restore(&shell, &settings, &path, false, cx);
        assert_refused(&dialog, &[INTACT, layout], body, true);
        press("escape", cx);
    }
}

#[gpui_kit::test]
fn details_name_the_object_but_never_its_sql_or_the_path(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, settings, cx) = open(cx, &scratch);
    let supported = Database::supported_schema_version();
    let path = copy_of_a_backup(&scratch, "trigger.db");
    sql(&path, HOSTILE_TRIGGER);

    let dialog = open_restore(&shell, &settings, &path, false, cx);
    assert!(
        dialog.fields.iter().all(|(label, _)| label != "Details"),
        "collapsed by default: {dialog:?}"
    );
    click("restore-details", cx);
    let dialog = describe(&shell, &settings, cx).dialog.unwrap();
    let details = &dialog
        .fields
        .iter()
        .find(|(label, _)| label == "Details")
        .expect("open")
        .1;
    assert_eq!(
        details,
        &format!(
            "its schema has an extra trigger \"qa_evil\" (compared with Orre's migrations at \
             version {supported})"
        )
    );
    assert!(!details.contains("UPDATE"), "{details}");
    assert!(!details.contains("mcp_client"), "{details}");
    let dir = scratch
        .path
        .parent()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(!details.contains(&dir), "{details}");
    assert!(!details.contains("can't be restored"), "{details}");
    assert!(
        dialog
            .actions
            .contains(&button(refusal::COPY_DETAILS, true))
    );
    // Nowhere in the dialog either.
    let all = format!("{dialog:?}");
    assert!(!all.contains("UPDATE mcp_client"), "{all}");
    assert!(!all.contains(&dir), "{all}");
    press("escape", cx);

    // A hostile name is capped, its quotes and control characters escaped.
    let name = format!("qa\"\n\u{202E}\u{200B}{}", "x".repeat(300));
    let hostile = copy_of_a_backup(&scratch, "hostile.db");
    Connection::open(&hostile)
        .unwrap()
        .execute_batch(&format!(
            "CREATE TABLE \"{}\" (x INTEGER)",
            name.replace('"', "\"\"")
        ))
        .unwrap();
    open_restore(&shell, &settings, &hostile, false, cx);
    click("restore-details", cx);
    let dialog = describe(&shell, &settings, cx).dialog.unwrap();
    let details = &dialog
        .fields
        .iter()
        .find(|(l, _)| l == "Details")
        .unwrap()
        .1;
    assert!(
        details.starts_with("its schema has an extra table \"qa\\\"\\u{a}\\u{202e}\\u{200b}xxx"),
        "{details}"
    );
    assert!(!details.contains('\n') && !details.contains('\u{202E}'));
    assert!(!details.contains('\u{200B}'));
    // The name, escaped, stops at 64 characters and "…".
    let shown = details
        .strip_prefix("its schema has an extra table \"")
        .and_then(|rest| rest.split_once("\" (compared"))
        .unwrap()
        .0;
    // 64 characters, then "…": cap + 1, the ellipsis included.
    assert_eq!(shown.chars().count(), 64 + 1, "{shown}");
    assert!(shown.ends_with('…'), "{shown}");
    press("escape", cx);

    // David's shapes: a name made of tag characters and blanks, an index
    // whose name would run in a shell, and a 5000-character name in a
    // broken schema (SQLite's message for it is the Integrity "first").
    let tags = format!(
        "x{}\u{2800}\u{3164}\u{115F}",
        (0x20..0x7f_u32)
            .map(|c| char::from_u32(0xE0000 + c).unwrap())
            .collect::<String>()
    );
    let tagged = copy_of_a_backup(&scratch, "tags.db");
    sql(
        &tagged,
        &format!("CREATE TABLE \"{}\" (x INTEGER)", tags.replace('"', "\"\"")),
    );
    let details = open_details(&shell, &settings, &tagged, cx);
    assert!(details.contains("\"x\\u{e0020}\\u{e0021}"), "{details}");
    assert!(!details.contains('\u{E0041}') && !details.contains('\u{2800}'));
    press("escape", cx);

    let shell_name = copy_of_a_backup(&scratch, "idx_shell.db");
    index_mismatch(&shell_name, "x$(touch /tmp/david_pwn)");
    let details = open_details(&shell, &settings, &shell_name, cx);
    assert!(
        details.contains("missing from index x\\$(touch /tmp/david_pwn)"),
        "{details}"
    );
    press("escape", cx);

    let long = copy_of_a_backup(&scratch, "malformed_long.db");
    {
        let conn = Connection::open(&long).unwrap();
        conn.execute_batch("PRAGMA writable_schema = ON").unwrap();
        let name = "N".repeat(5000);
        conn.execute(
            "INSERT INTO sqlite_master (type, name, tbl_name, rootpage, sql) \
             VALUES ('table', ?1, ?1, 2, 'CREATE TABLE zz (q')",
            [&name],
        )
        .unwrap();
        bump_schema_version(&conn);
    }
    let dialog = open_restore(&shell, &settings, &long, false, cx);
    assert_eq!(
        dialog.fields[0].0, "alert: The file is damaged",
        "{dialog:?}"
    );
    click("restore-details", cx);
    let details = details_of(&shell, &settings, cx);
    let first = details.split_once("; the first: ").unwrap().1;
    assert!(
        first.chars().count() <= 160 + 1,
        "{}",
        first.chars().count()
    );
    assert!(first.ends_with('…'), "{first}");
}

/// Restores `path`'s dialog, opens Details and returns its text.
fn open_details(
    shell: &Entity<ShellView>,
    settings: &Entity<SettingsView>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> String {
    open_restore(shell, settings, path, false, cx);
    click("restore-details", cx);
    details_of(shell, settings, cx)
}

fn details_of(
    shell: &Entity<ShellView>,
    settings: &Entity<SettingsView>,
    cx: &mut VisualTestContext,
) -> String {
    let dialog = describe(shell, settings, cx).dialog.unwrap();
    dialog
        .fields
        .into_iter()
        .find(|(label, _)| label == "Details")
        .expect("Details open")
        .1
}

fn bump_schema_version(conn: &Connection) {
    let version: i64 = conn
        .query_row("PRAGMA schema_version", [], |row| row.get(0))
        .unwrap();
    conn.execute_batch(&format!("PRAGMA schema_version = {}", version + 1))
        .unwrap();
}

/// David's `idx_mismatch`: an index named `name` whose entries don't match
/// its table, so the integrity check names it ("row 1 missing from index
/// …").
fn index_mismatch(path: &Path, name: &str) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE t (a); CREATE INDEX i ON t (a); INSERT INTO t VALUES (1), (2), (3); \
         PRAGMA writable_schema = ON;",
    )
    .unwrap();
    conn.execute(
        "UPDATE sqlite_master SET name = ?1, sql = ?2 WHERE name = 'i'",
        [
            name,
            &format!(
                "CREATE INDEX \"{}\" ON t (a + 1)",
                name.replace('"', "\"\"")
            ),
        ],
    )
    .unwrap();
    bump_schema_version(&conn);
}

#[gpui_kit::test]
fn a_backup_that_can_be_restored_passes_three_rows(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, settings, cx) = open(cx, &scratch);
    let supported = Database::supported_schema_version();
    let (taken, _) = SettingsStore::new(scratch.path.clone()).back_up().unwrap();

    let dialog = open_restore(&shell, &settings, &taken.path, false, cx);
    assert_eq!(
        dialog.fields,
        [
            field(INTACT.0, INTACT.1),
            field(
                "check: This version of Orre can read it",
                &format!("schema {supported}")
            ),
            field("check: AI access can be switched off", "AI access"),
            field(
                "caption",
                "Orre backs up how things are now before it swaps the files, so you can \
                 restore that backup to come back."
            ),
            field(
                "caption",
                "Every AI client is switched off after a restore. Turn each one back on under \
                 AI clients."
            ),
            field("caption", super::super::restore::PRESETS_NOT_IN_BACKUPS),
        ]
    );
    assert_eq!(
        dialog.actions,
        [button("Cancel", true), button("Restore", true)],
        "no Details, no Choose another file… for a listed backup"
    );
    let all = format!("{dialog:?}");
    assert!(!all.contains("refused until it's done"), "{all}");
}

/// AI access can't be switched off only once Restore is pressed, and only
/// for a backup the schema check would already refuse (`backup::restore`),
/// so the refusal is driven directly: the typed error is sorted to row ③.
#[gpui_kit::test]
fn ai_access_that_wont_switch_off_fails_the_third_row(cx: &mut TestAppContext) {
    let failure = refusal::failure(Box::new(backup::Error::AiAccessNotOff {
        path: PathBuf::from("/secret/dir/orre-x.db"),
        source: None,
    }));
    assert_eq!(failure, Failure::AiAccessNotOff);

    let scratch = Scratch::new();
    let (shell, settings, cx) = open(cx, &scratch);
    let supported = Database::supported_schema_version();
    let (taken, _) = SettingsStore::new(scratch.path.clone()).back_up().unwrap();
    open_restore(&shell, &settings, &taken.path, false, cx);
    let entity = dialog_entity(&shell, cx);
    cx.update(|_, cx| {
        entity.update(cx, |dialog, cx| {
            let previous = dialog.stage().clone();
            dialog.refused_for_test(Failure::AiAccessNotOff, previous);
            cx.notify();
        })
    });
    redraw(cx);
    let dialog = describe(&shell, &settings, cx).dialog.unwrap();
    assert_refused(
        &dialog,
        &[
            INTACT,
            (
                "check: This version of Orre can read it",
                &format!("schema {supported}"),
            ),
            ("alert: AI access can't be switched off", "AI access"),
        ],
        "Orre only restores a backup if it can switch AI clients off afterwards. It can't do \
         that here. Your data hasn't changed.",
        false,
    );
}

/// A check that fails at restore time (the file changed after the dialog
/// checked it) goes to its row, not the caption.
#[gpui_kit::test]
fn a_file_that_went_bad_before_restore_fails_its_row(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, settings, cx) = open(cx, &scratch);
    let (taken, _) = SettingsStore::new(scratch.path.clone()).back_up().unwrap();
    open_restore(&shell, &settings, &taken.path, false, cx);
    fs::write(&taken.path, b"not a database at all, just some bytes......").unwrap();
    click("restore-confirm", cx);
    redraw(cx);
    let dialog = describe(&shell, &settings, cx).dialog.expect("still open");
    assert_refused(
        &dialog,
        &[("alert: It isn't an Orre backup", "not a SQLite file")],
        CANT,
        false,
    );
}

#[gpui_kit::test]
fn choose_another_file_is_offered_only_for_a_file(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, settings, cx) = open(cx, &scratch);
    let junk = scratch.path.with_file_name("junk.db");
    fs::write(&junk, b"not a database at all, just some bytes......").unwrap();

    let dialog = open_restore(&shell, &settings, &junk, true, cx);
    assert_eq!(dialog.title, "Restore junk.db?");
    assert!(
        dialog
            .actions
            .contains(&button("Choose another file…", true))
    );
    let entity = dialog_entity(&shell, cx);
    assert!(
        cx.update(|window, cx| entity.read(cx).choose_focused(window)),
        "focused first"
    );

    // Choosing another file checks it in the same dialog.
    let (taken, _) = SettingsStore::new(scratch.path.clone()).back_up().unwrap();
    cx.update(|_, cx| {
        entity.update(cx, |dialog, cx| {
            dialog.retarget(RestoreTarget::chosen(taken.path.clone()), cx)
        })
    });
    redraw(cx);
    assert_eq!(dialog_entity(&shell, cx), entity, "the same dialog");
    let dialog = describe(&shell, &settings, cx).dialog.unwrap();
    assert!(
        dialog.title.starts_with("Restore the backup from "),
        "{dialog:?}"
    );
    assert!(
        dialog.actions.contains(&button("Restore", true)),
        "{dialog:?}"
    );
    assert!(
        dialog
            .actions
            .contains(&button("Choose another file…", true))
    );

    // Escape is Cancel.
    press("escape", cx);
    assert_eq!(describe(&shell, &settings, cx).dialog, None);

    // A listed backup: no such button.
    let dialog = open_restore(&shell, &settings, &taken.path, false, cx);
    assert!(
        !dialog
            .actions
            .iter()
            .any(|action| action.label == "Choose another file…"),
        "{dialog:?}"
    );
}

#[gpui_kit::test]
fn the_bar_after_a_restore_says_when_and_offers_ai_clients(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, settings, cx) = open(cx, &scratch);
    click("settings-backups", cx);
    click("back-up-now", cx);
    click("restore-backup", cx);
    cx.run_until_parked();
    let row_local = cx.update(|_, cx| settings.read(cx).backups().rows[0].local);
    click("restore-confirm", cx);
    cx.run_until_parked();
    redraw(cx);
    let shown = describe(&shell, &settings, cx);
    let notice = shown.notice.expect("the bar");
    assert_eq!(
        notice.message,
        format!(
            "Restored the backup from {} · AI clients are off",
            refusal::when(row_local)
        )
    );
    assert!(
        notice
            .message
            .contains(&row_local.format(" %Y, ").to_string())
    );
    assert_eq!(
        notice.actions,
        [button("AI clients", true), button("Show in folder", true)]
    );
    click("notice-ai-clients", cx);
    let shown = describe(&shell, &settings, cx);
    assert_eq!(
        cx.update(|_, cx| settings.read(cx).section()),
        super::super::Section::Clients
    );
    assert_eq!(shown.notice, None);
}

/// What the restore did beyond the database (the access-log entries it
/// kept) shows on the backup it took first, under "Since this backup", not
/// in the bar.
#[gpui_kit::test]
fn what_a_restore_kept_shows_on_the_backup_it_took_first(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, settings, cx) = open(cx, &scratch);
    click("settings-backups", cx);
    click("back-up-now", cx);
    // A call logged after the backup: the restore keeps it.
    access_log::append(
        &mut scratch.db(),
        &NewEntry {
            client: name("claude-code"),
            tool: "search_transactions".into(),
            access_level: Some(AccessLevel::Standard),
            outcome: Outcome::Ok,
            purpose: None,
            args_redacted_json: "{}".into(),
            row_count: 1,
            result_hash: Some("cd".into()),
            withheld_count: 0,
        },
    )
    .unwrap();
    click("restore-backup", cx);
    cx.run_until_parked();
    click("restore-confirm", cx);
    cx.run_until_parked();
    redraw(cx);
    let notice = describe(&shell, &settings, cx).notice.expect("the bar");
    assert!(!notice.message.contains("kept"), "{notice:?}");
    assert!(!notice.message.contains("updated"), "{notice:?}");

    let taken_first = backup::list(&scratch.path)
        .unwrap()
        .into_iter()
        .find(|backup| backup.reason == backup::Reason::Restore)
        .expect("the backup taken first");
    click(
        &format!("backup-{}", taken_first.taken_at.timestamp_millis()),
        cx,
    );
    let inspector = describe(&shell, &settings, cx)
        .inspector
        .expect("the backup's inspector");
    let since = inspector
        .sections
        .iter()
        .find(|section| section.title.as_deref() == Some("Since this backup"))
        .unwrap();
    assert!(
        since
            .rows
            .contains(&("Access log".to_owned(), "kept 1 newer entry".to_owned())),
        "{since:?}"
    );
}
