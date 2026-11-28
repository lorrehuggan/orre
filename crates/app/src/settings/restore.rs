//! The Restore dialog (`docs/DESIGN.md` → "Settings" → **Restore dialog**,
//! **A check fails**; Paper `H6D-0`, `H73-0`), opened by "Restore…" on a
//! backup or "Restore from a file…".
//!
//! The M1.28 checks run first, on a copy, changing nothing
//! (`backup::detail::assess`): the file is intact (SQLite's integrity
//! check), and this version of Orre can read it (migration history, a
//! schema no newer). Each is a 24px row with a `check`, or a `danger` row
//! with `alert` when it fails; then what restoring undoes, and that the
//! current state is backed up first and AI clients are refused until it's
//! done. Cancel / "Restore" (neutral primary: restoring is undone by
//! restoring that backup). Restore stays disabled unless both checks pass.
//! Restoring runs `backup::restore`, which checks again, backs up, swaps
//! the files under the restore lock and migrates.
//!
//! Composed for M3.16 (not drawn): "Checking the file…" while the checks
//! run, and "Restoring…" (buttons disabled, Escape ignored) while it
//! restores; a damaged file fails the first row and doesn't show the
//! second; a current database that can't be read, or a restore that was
//! refused, is said in `danger` with what to do; an earlier restore that
//! stopped part-way is mentioned, as `orre restore` does. R.12 adds a
//! caption that user presets aren't in backups ([`PRESETS_NOT_IN_BACKUPS`]),
//! and SLI-48 one that AI clients are off afterwards ([`AI_OFF_AFTER_RESTORE`]).

use gpui_kit::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, Task, Window, div,
};
use orre_core::backup::Current;
use orre_core::backup::detail::{Assessment, Since};

use crate::accounts::DialogClosed;
use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::error_caption;
use crate::ui::icon::Glyph;
use crate::ui::text::TextRoleExt as _;
use gpui_kit::component::Disableable as _;

use super::store::SettingsStore;
use super::table::{Run, caption, count, mono, plain, plural, rich};
use super::{RestoreTarget, SettingsView, sentence};

/// Under the checks (composed, R.12): user presets live outside the
/// database, so a backup neither holds them nor changes them.
pub const PRESETS_NOT_IN_BACKUPS: &str = "Your own bank presets aren't in backups, so restoring \
     doesn't change them. An account whose preset isn't on this computer asks for its columns \
     again at the next import.";

/// Under the checks, and printed by `orre restore`: a restore switches
/// every AI client off (`backup::restore`).
pub const AI_OFF_AFTER_RESTORE: &str =
    "AI clients are switched off after a restore; turn them back on in Settings.";

/// Where the dialog is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The checks are running.
    Checking,
    /// The checks are done.
    Checked(Assessment),
    /// The checks couldn't run (the file can't be read).
    Unreadable(SharedString),
    /// Restoring.
    Restoring,
}

/// The Restore dialog.
pub struct RestoreDialog {
    settings: Entity<SettingsView>,
    target: RestoreTarget,
    stage: Stage,
    /// What restoring undoes, when the backup's time is known and the
    /// count, read in the background, has landed.
    since: Option<Since>,
    today: chrono::NaiveDate,
    /// Why the last restore was refused.
    error: Option<SharedString>,
    focus: FocusHandle,
    _task: Option<Task<()>>,
    /// Reading `since` in the background: it reads the backup, which may be
    /// large or hostile, so the dialog doesn't wait for it.
    _since_read: Option<Task<()>>,
}

impl EventEmitter<DialogClosed> for RestoreDialog {}

impl RestoreDialog {
    pub fn new(
        settings: Entity<SettingsView>,
        target: RestoreTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let (store, today) = {
            let view = settings.read(cx);
            (view.store().ok().cloned(), view.clock().today)
        };
        let mut dialog = Self {
            settings,
            target,
            stage: Stage::Checking,
            since: None,
            today,
            error: None,
            focus,
            _task: None,
            _since_read: None,
        };
        match store {
            Some(store) => {
                dialog.read_since(store.clone(), cx);
                dialog.check(store, cx);
            }
            None => {
                dialog.stage =
                    Stage::Unreadable("This is sample data, which can't be changed.".into())
            }
        }
        dialog
    }

    /// A dialog showing `stage` without running anything (snapshots).
    pub fn preview(
        settings: Entity<SettingsView>,
        target: RestoreTarget,
        stage: Stage,
        since: Option<Since>,
        cx: &mut Context<Self>,
    ) -> Self {
        let today = settings.read(cx).clock().today;
        Self {
            settings,
            target,
            stage,
            since,
            today,
            error: None,
            focus: cx.focus_handle(),
            _task: None,
            _since_read: None,
        }
    }

    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    /// What restoring undoes, once its background read has landed.
    #[cfg(test)]
    pub(crate) fn since(&self) -> Option<&Since> {
        self.since.as_ref()
    }

    /// Reads what restoring undoes on a background thread, beside the
    /// checks; the sentence shows once it lands (and the checks passed). A
    /// dialog closed before then drops the read with it.
    fn read_since(&mut self, store: SettingsStore, cx: &mut Context<Self>) {
        let Some(taken_at) = self.target.taken_at else {
            return;
        };
        let path = self.target.path.clone();
        let work = cx
            .background_executor()
            .spawn(async move { store.since(&path, taken_at).ok() });
        self._since_read = Some(cx.spawn(async move |this, cx| {
            let since = work.await;
            let _ = this.update(cx, |this, cx| {
                this.since = since;
                cx.notify();
            });
        }));
    }

    fn check(&mut self, store: SettingsStore, cx: &mut Context<Self>) {
        let path = self.target.path.clone();
        let work = cx
            .background_executor()
            .spawn(async move { store.assess(&path).map_err(|error| error.to_string()) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.stage = match result {
                    Ok(assessment) => Stage::Checked(assessment),
                    Err(error) => Stage::Unreadable(sentence(&error)),
                };
                cx.notify();
            });
        }));
    }

    /// Whether Restore can be pressed.
    pub fn can_restore(&self) -> bool {
        match &self.stage {
            Stage::Checked(Assessment::Ready(check)) => check.current != Current::Unreadable,
            _ => false,
        }
    }

    /// Restores on a background thread, then closes and tells Settings.
    pub fn restore(&mut self, cx: &mut Context<Self>) {
        if !self.can_restore() {
            return;
        }
        let Ok(store) = self.settings.read(cx).store().cloned() else {
            return;
        };
        let previous = std::mem::replace(&mut self.stage, Stage::Restoring);
        self.error = None;
        self.settings
            .update(cx, |settings, cx| settings.restoring(cx));
        cx.notify();
        let path = self.target.path.clone();
        let work = cx
            .background_executor()
            .spawn(async move { store.restore(&path).map_err(|error| error.to_string()) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(restored) => {
                    this.settings
                        .update(cx, |settings, cx| settings.restored(&restored, cx));
                    cx.emit(DialogClosed);
                }
                Err(error) => {
                    this.stage = previous;
                    this.error = Some(sentence(&error));
                    this.settings
                        .update(cx, |settings, cx| settings.restore_failed(cx));
                    cx.notify();
                }
            });
        }));
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.stage == Stage::Restoring {
            return;
        }
        cx.emit(DialogClosed);
    }

    /// "Restore the backup from 10 Nov, 14:02?", or the file's name.
    fn title_runs(&self) -> Vec<Run> {
        match self.target.local {
            Some(local) => {
                use chrono::Datelike as _;
                let when = if local.year() == self.today.year() {
                    local.format("%-d %b, %H:%M").to_string()
                } else {
                    local.format("%-d %b %Y, %H:%M").to_string()
                };
                vec![plain("Restore the backup from "), mono(when), plain("?")]
            }
            None => vec![
                plain("Restore "),
                mono(
                    self.target
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                ),
                plain("?"),
            ],
        }
    }

    fn title(&self) -> impl IntoElement {
        rich(typography::TITLE, color::TEXT, self.title_runs()).whitespace_nowrap()
    }

    /// What the dialog shows, as plain data (test-only, ROADMAP U.3): its
    /// title, the error, and Cancel and Restore (off while it restores,
    /// Restore until the checks pass).
    #[cfg(any(test, feature = "dev-driver"))]
    pub fn describe(&self) -> crate::ui::describe::Dialog {
        use crate::ui::describe::{Action, Dialog};
        let restoring = self.stage == Stage::Restoring;
        Dialog {
            title: Run::joined(&self.title_runs()),
            fields: Vec::new(),
            error: self.error.as_ref().map(ToString::to_string),
            actions: vec![
                Action::button("Cancel", !restoring),
                Action::button("Restore", self.can_restore() && !restoring),
            ],
        }
    }
}

/// A check's row: 24px, the icon, what it found, and the check's name on
/// the right in caption type (Paper `H6G-0`, `H7E-0`).
fn check_row(passed: bool, finding: &str, name: Vec<Run>) -> AnyElement {
    let tone = if passed { color::TEXT } else { color::DANGER };
    let glyph = if passed { Glyph::Check } else { Glyph::Alert };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL_SM)
        .child(glyph.render().text_color(tone))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_role(typography::BODY)
                .text_color(tone)
                .child(finding.to_owned()),
        )
        .child(
            rich(
                typography::CAPTION,
                if passed {
                    color::TEXT_MUTED
                } else {
                    color::DANGER
                },
                name,
            )
            .whitespace_nowrap(),
        )
        .into_any_element()
}

/// "the import of `monzo-november.csv` (296 rows), 4 edits and 12 Inbox
/// suggestions" (Paper `H6T-0`); `orre restore` prints it too.
pub(crate) fn undoes(since: &Since) -> Vec<Run> {
    let mut parts: Vec<Vec<Run>> = Vec::new();
    for import in &since.imports {
        parts.push(vec![
            plain("the import of "),
            mono(import.file.clone()),
            plain(" ("),
            mono(count(import.rows)),
            plain(format!(" {})", plural(import.rows, "row"))),
        ]);
    }
    if since.edits > 0 {
        parts.push(vec![
            mono(count(since.edits)),
            plain(format!(" {}", plural(since.edits, "edit"))),
        ]);
    }
    if since.manual > 0 {
        parts.push(vec![
            mono(count(since.manual)),
            plain(format!(
                " {} added by you",
                plural(since.manual, "transaction")
            )),
        ]);
    }
    if since.deleted_unknown {
        parts.push(vec![plain(
            "the deletion of an unknown number of transactions",
        )]);
    } else if since.deleted > 0 {
        parts.push(vec![
            plain(format!(
                "the deletion of {}",
                super::backups::at_least(since)
            )),
            mono(count(since.deleted)),
            plain(format!(" {}", plural(since.deleted, "transaction"))),
        ]);
    }
    if since.suggestions > 0 {
        parts.push(vec![
            mono(count(since.suggestions)),
            plain(format!(
                " Inbox {}",
                plural(since.suggestions, "suggestion")
            )),
        ]);
    }
    let mut runs = Vec::new();
    let total = parts.len();
    for (index, part) in parts.into_iter().enumerate() {
        if index > 0 {
            runs.push(plain(if index + 1 == total { " and " } else { ", " }));
        }
        runs.extend(part);
    }
    runs.push(plain("."));
    runs
}

impl Render for RestoreDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body: Vec<AnyElement> = Vec::new();
        let mut checks: Vec<AnyElement> = Vec::new();
        let intact = || vec![plain("integrity check")];
        let mut note: Option<SharedString> = None;
        match &self.stage {
            Stage::Checking => body.push(caption("Checking the file…").into_any_element()),
            Stage::Unreadable(error) => body.push(error_caption(error.clone()).into_any_element()),
            Stage::Restoring | Stage::Checked(_) => {}
        }
        let assessment = match &self.stage {
            Stage::Checked(assessment) => Some(assessment.clone()),
            _ => None,
        };
        match &assessment {
            Some(Assessment::Ready(check)) => {
                checks.push(check_row(true, "The file is intact", intact()));
                checks.push(check_row(
                    true,
                    "This version of Orre can read it",
                    vec![plain("schema "), mono(check.schema_version.to_string())],
                ));
                if check.current == Current::Unreadable {
                    note = Some(
                        "The current database can't be read, so it can't be backed up first. \
                         Move it (and any -wal and -shm files beside it) aside to restore over \
                         it."
                        .into(),
                    );
                }
            }
            Some(Assessment::Damaged { .. }) => {
                checks.push(check_row(false, "The file is damaged", intact()));
            }
            Some(Assessment::NotOrre) => {
                checks.push(check_row(true, "The file is intact", intact()));
                checks.push(check_row(
                    false,
                    "It isn't an Orre backup",
                    vec![plain("no migration history")],
                ));
            }
            Some(Assessment::TooNew { found, supported }) => {
                checks.push(check_row(true, "The file is intact", intact()));
                checks.push(check_row(
                    false,
                    "It was made by a newer Orre",
                    vec![
                        plain("schema "),
                        mono(found.to_string()),
                        plain(", this reads up to "),
                        mono(supported.to_string()),
                    ],
                ));
            }
            Some(Assessment::OtherHistory) => {
                checks.push(check_row(true, "The file is intact", intact()));
                checks.push(check_row(
                    false,
                    "It's from a different build of Orre",
                    vec![plain("migration history")],
                ));
            }
            None => {}
        }
        if !checks.is_empty() {
            body.push(div().flex().flex_col().children(checks).into_any_element());
        }

        let ready =
            matches!(&assessment, Some(Assessment::Ready(_))) || self.stage == Stage::Restoring;
        if ready {
            if let Some(since) = self.since.as_ref().filter(|since| !since.is_empty()) {
                body.push(
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::S1)
                        .child(caption("Restoring undoes everything since then:"))
                        .child(rich(typography::BODY, color::TEXT, undoes(since)))
                        .into_any_element(),
                );
            }
            body.push(
                caption(if self.stage == Stage::Restoring {
                    "Restoring… Orre backs up how things are now first. AI clients are \
                     refused until it's done."
                } else {
                    "Orre backs up how things are now before it swaps the files, so you can \
                     restore that backup to come back. AI clients are refused until it's done."
                })
                .into_any_element(),
            );
            body.push(caption(AI_OFF_AFTER_RESTORE).into_any_element());
            body.push(caption(PRESETS_NOT_IN_BACKUPS).into_any_element());
            if let Some(Assessment::Ready(check)) = &assessment
                && check.interrupted.is_some()
            {
                body.push(
                    caption(
                        "An earlier restore stopped before it finished. The current database is \
                         complete, and it's backed up first, as always.",
                    )
                    .into_any_element(),
                );
            }
        } else if let Some(assessment) = &assessment {
            body.push(
                caption(match assessment {
                    Assessment::TooNew { .. } => {
                        "Update Orre, then restore it. Nothing has changed."
                    }
                    _ => "It can't be restored. Nothing has changed.",
                })
                .into_any_element(),
            );
        }
        if let Some(note) = note {
            body.push(error_caption(note).into_any_element());
        }
        if let Some(error) = self.error.clone() {
            body.push(error_caption(error).into_any_element());
        }

        let restoring = self.stage == Stage::Restoring;
        let on_cancel = cx.entity().downgrade();
        let dialog = Dialog::with_title(self.title(), &self.focus);
        body.into_iter()
            .fold(dialog, |dialog, part| dialog.body(part))
            .action(
                Button::new("restore-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .disabled(restoring)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("restore-confirm", "Restore")
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .disabled(!self.can_restore() || restoring)
                    .on_click(cx.listener(|this, _, _, cx| this.restore(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

#[cfg(test)]
mod tests {
    use orre_core::backup::detail::ImportSince;

    use super::*;

    fn text(runs: Vec<Run>) -> String {
        format!("{runs:?}")
    }

    #[test]
    fn says_what_restoring_undoes() {
        let since = Since {
            imports: vec![ImportSince {
                file: "monzo-november.csv".into(),
                rows: 296,
            }],
            edits: 4,
            manual: 2,
            deleted: 1,
            deleted_at_least: false,
            deleted_unknown: false,
            suggestions: 12,
        };
        let text = text(undoes(&since));
        for part in [
            "the import of ",
            "monzo-november.csv",
            "296",
            " rows)",
            " transactions added by you",
            "the deletion of ",
            " transaction",
            " and ",
            " Inbox suggestions",
        ] {
            assert!(text.contains(part), "{part} in {text}");
        }
        assert_eq!(
            Run::joined(&undoes(&since)),
            "the import of monzo-november.csv (296 rows), 4 edits, 2 transactions added by \
             you, the deletion of 1 transaction and 12 Inbox suggestions."
        );
    }

    /// SLI-57: a count that ran out of its budget says it's unknown, never
    /// 0 or the cap.
    #[test]
    fn says_when_the_deletions_are_unknown() {
        let since = Since {
            deleted_unknown: true,
            ..Since::default()
        };
        assert!(!since.is_empty());
        assert_eq!(
            Run::joined(&undoes(&since)),
            "the deletion of an unknown number of transactions."
        );
        let lines: Vec<(String, String)> = super::super::backups::since_lines(&since)
            .into_iter()
            .map(|(label, value)| (Run::joined(&label), value))
            .collect();
        assert_eq!(lines, [("Deleted".to_owned(), "couldn't count".to_owned())]);
        let with_edits = Since {
            edits: 2,
            deleted_unknown: true,
            ..Since::default()
        };
        assert_eq!(
            Run::joined(&undoes(&with_edits)),
            "2 edits and the deletion of an unknown number of transactions."
        );
    }

    /// SLI-57: past the cap, the count says "at least".
    #[test]
    fn says_at_least_when_counting_deletions_stopped() {
        use orre_core::backup::detail::DELETED_CAP;

        let since = Since {
            deleted: DELETED_CAP,
            deleted_at_least: true,
            ..Since::default()
        };
        assert_eq!(
            Run::joined(&undoes(&since)),
            "the deletion of at least 10,000 transactions."
        );
        let lines: Vec<(String, String)> = super::super::backups::since_lines(&since)
            .into_iter()
            .map(|(label, value)| (Run::joined(&label), value))
            .collect();
        assert_eq!(
            lines,
            [(
                "Deleted".to_owned(),
                "at least 10,000 transactions".to_owned()
            )]
        );
        let exact = Since {
            deleted: DELETED_CAP,
            ..Since::default()
        };
        assert_eq!(
            Run::joined(&undoes(&exact)),
            "the deletion of 10,000 transactions."
        );
    }
}
