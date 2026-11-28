//! The Restore dialog's checklist and its refusals (SLI-71; `docs/DESIGN.md`
//! → "Settings" → **A check fails**).
//!
//! Three 24px rows, in order, stopping at the first that fails: the file is
//! intact, this version of Orre can read it, and AI access can be switched
//! off. Each refusal fails one row with its own label and detail, then a
//! body and, when the refusal has more to say than its row, a collapsed
//! "Details" block. Everything here is built from the typed check result
//! ([`Assessment`], [`Failure`]), never by reading an error's text, and
//! nothing taken from the file reaches the window unescaped: object names
//! and SQLite's messages are capped and their control characters escaped
//! ([`inert`]). SQL from the file is never shown.
//!
//! The third row can only fail when Restore is pressed: the restore
//! switches the backup's AI clients off and reads them back before it
//! replaces anything ([`orre_core::backup::restore`]); if that doesn't hold
//! it refuses with `Error::AiAccessNotOff` and the dialog goes back to its
//! checked state with that row failed.
//!
//! "Choose another file…" (only when opened from "Restore from a file…")
//! reopens the system's open dialog and runs the checks again in place.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, div, px,
};
use orre_core::backup::detail::Assessment;
use orre_core::backup::{self, Current};
use orre_core::db;

use crate::accounts::store::StoreError;
use crate::theme::tokens::{color, radius, spacing, typography};
use crate::ui::button::{Button, ButtonVariant, IconButton, IconButtonSize};
use crate::ui::icon::Glyph;
use crate::ui::text::TextRoleExt as _;
use gpui_kit::component::Disableable as _;

use super::super::table::{Run, mono, plain};
use super::super::{RestoreTarget, sentence};
use super::{RestoreDialog, Stage, check_row};

/// Under the checks of a backup that can be restored, and nowhere else
/// (Rowling, SLI-71). `orre restore` still prints
/// [`super::AI_OFF_AFTER_RESTORE`]: keep the two saying the same thing.
pub const AI_OFF_PARAGRAPH: &str =
    "Every AI client is switched off after a restore. Turn each one back on under AI clients.";

/// The footer's leading action, from "Restore from a file…" only.
pub const CHOOSE_ANOTHER: &str = "Choose another file…";

/// The disclosure under a refusal.
pub const DETAILS: &str = "Details";

/// The Details block's copy button. String gap (SLI-71): the spec has no
/// accessible name for it, so it borrows the only existing copy button's
/// until one is written.
pub const COPY_DETAILS: &str = "Copy the command";

/// Row ①.
const INTACT: &str = "The file is intact";
const INTACT_DETAIL: &str = "integrity check";
/// Row ②.
const READABLE: &str = "This version of Orre can read it";
/// Row ③ (new, SLI-71).
const AI_OFF: &str = "AI access can be switched off";
const AI_NOT_OFF: &str = "AI access can't be switched off";
const AI_DETAIL: &str = "AI access";

/// Not a SQLite file, and (row ②) not an Orre database.
const NOT_AN_ORRE_BACKUP: &str = "It isn't an Orre backup";
const DAMAGED: &str = "The file is damaged";
const NEWER: &str = "It was made by a newer Orre";
const OTHER_BUILD: &str = "It's from a different build of Orre";
const OTHER_LAYOUT: &str = "It isn't laid out like an Orre backup";

/// The body under refusals that have no body of their own (as before
/// SLI-71).
const CANT_RESTORE: &str = "It can't be restored. Nothing has changed.";
const UPDATE_ORRE: &str = "Update Orre, then restore it. Nothing has changed.";
const SCHEMA_BODY: &str = "Orre only restores backups laid out the way it makes them. This one \
     may have been edited or come from another app. Nothing has changed.";
/// Rowling's body for a backup whose AI access can't be switched off, less
/// its last sentence, "Nothing has changed.": by the time the read-back
/// fails, the restore has already backed the current database up (and
/// rotation may have deleted an old automatic backup), so it isn't
/// strictly true. The current database itself is unchanged.
const AI_BODY: &str = "Orre only restores a backup if it can switch AI clients off afterwards. \
     It can't do that here.";

/// The longest object name the Details block shows, in characters (core
/// caps it at the same length; this doesn't rely on that).
const NAME_CAP: usize = 64;
/// The longest SQLite message the Details block shows, in characters.
const MESSAGE_CAP: usize = 160;
/// The longest error the dialog's `danger` caption shows, in characters.
const ERROR_CAP: usize = 300;
/// The Details block's height before it scrolls.
const DETAILS_MAX_HEIGHT: f32 = 96.;

/// The dialog's state beyond its stage.
pub(super) struct RefusalState {
    /// The last restore was refused because AI access couldn't be switched
    /// off: row ③ fails and Restore stays off.
    pub ai_refused: bool,
    /// Whether the Details block is open.
    pub details_open: bool,
    /// "Choose another file…", focused first when it's there.
    pub choose: FocusHandle,
}

impl RefusalState {
    pub fn new(cx: &mut App) -> Self {
        Self {
            ai_refused: false,
            details_open: false,
            choose: cx.focus_handle(),
        }
    }
}

/// Why a restore didn't happen, sorted from its typed error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A check failed (the file changed after the dialog checked it).
    Check(Assessment),
    /// The backup's AI access couldn't be switched off.
    AiAccessNotOff,
    /// Something else, said in the `danger` caption.
    Other(SharedString),
}

/// `error` (from the settings store's restore) as a [`Failure`].
pub fn failure(error: StoreError) -> Failure {
    match error.downcast::<backup::Error>() {
        Ok(error) => match *error {
            backup::Error::AiAccessNotOff { .. } => Failure::AiAccessNotOff,
            error => match Assessment::of_failed_check(error) {
                Ok(assessment) => Failure::Check(assessment),
                Err(error) => Failure::Other(error_text(&error)),
            },
        },
        Err(error) => Failure::Other(error_text(&*error)),
    }
}

/// An error that isn't a failed check, as the `danger` caption says it:
/// a sentence, capped, with control characters escaped (a SQLite message
/// can quote a name from the file).
pub fn error_text(error: &dyn std::error::Error) -> SharedString {
    sentence(&inert(&error.to_string(), ERROR_CAP, false))
}

/// `text` safe to show: at most `cap` characters (then "…"), with control
/// and invisible formatting characters (bidirectional overrides,
/// zero-width ones) written as `\u{…}`. With `quoted` (a name that goes
/// between quotes) `"` and `\` are escaped too; without it (a message), a
/// line break or tab is a space.
pub fn inert(text: &str, cap: usize, quoted: bool) -> String {
    let mut out = String::new();
    for (index, c) in text.chars().enumerate() {
        if index == cap {
            out.push('…');
            break;
        }
        if !quoted && (c == '\n' || c == '\t') {
            // A line break in SQLite's own message reads as a space.
            out.push(' ');
        } else if invisible(c) {
            out.push_str(&format!("\\u{{{:x}}}", u32::from(c)));
        } else if quoted && (c == '"' || c == '\\') {
            out.push('\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

fn invisible(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
        )
}

/// "28 Nov, 11:40", with the year only when it isn't this year (B7).
pub fn when(local: chrono::NaiveDateTime, today: chrono::NaiveDate) -> String {
    use chrono::Datelike as _;
    if local.year() == today.year() {
        local.format("%-d %b, %H:%M").to_string()
    } else {
        local.format("%-d %b %Y, %H:%M").to_string()
    }
}

/// A checklist row as drawn.
#[derive(Debug, Clone)]
pub struct Row {
    pub passed: bool,
    pub label: &'static str,
    pub detail: Vec<Run>,
}

fn passed(label: &'static str, detail: Vec<Run>) -> Row {
    Row {
        passed: true,
        label,
        detail,
    }
}

fn failed(label: &'static str, detail: Vec<Run>) -> Row {
    Row {
        passed: false,
        label,
        detail,
    }
}

/// The rows for `assessment`, stopping at the first that fails; row ③
/// fails when `ai_refused`.
pub fn rows(assessment: &Assessment, ai_refused: bool) -> Vec<Row> {
    let intact = || passed(INTACT, vec![plain(INTACT_DETAIL)]);
    match assessment {
        Assessment::Ready(check) => vec![
            intact(),
            passed(
                READABLE,
                vec![plain("schema "), mono(check.schema_version.to_string())],
            ),
            if ai_refused {
                failed(AI_NOT_OFF, vec![plain(AI_DETAIL)])
            } else {
                passed(AI_OFF, vec![plain(AI_DETAIL)])
            },
        ],
        Assessment::NotADatabase => {
            vec![failed(NOT_AN_ORRE_BACKUP, vec![plain("not a SQLite file")])]
        }
        Assessment::Integrity { .. } => vec![failed(DAMAGED, vec![plain(INTACT_DETAIL)])],
        Assessment::NotOrre => vec![
            intact(),
            failed(NOT_AN_ORRE_BACKUP, vec![plain("no migration history")]),
        ],
        Assessment::TooNew { found, supported } => vec![
            intact(),
            failed(
                NEWER,
                vec![
                    plain("schema "),
                    mono(found.to_string()),
                    plain(", this reads up to "),
                    mono(supported.to_string()),
                ],
            ),
        ],
        Assessment::OtherHistory => vec![
            intact(),
            failed(OTHER_BUILD, vec![plain("migration history")]),
        ],
        Assessment::Schema { .. } => vec![
            intact(),
            failed(OTHER_LAYOUT, vec![plain("different layout")]),
        ],
    }
}

/// What a refusal says under its rows; `None` when nothing is refused.
pub fn body(assessment: &Assessment, ai_refused: bool) -> Option<&'static str> {
    match assessment {
        Assessment::Ready(_) if ai_refused => Some(AI_BODY),
        Assessment::Ready(_) => None,
        Assessment::TooNew { .. } => Some(UPDATE_ORRE),
        Assessment::Schema { .. } => Some(SCHEMA_BODY),
        Assessment::NotADatabase
        | Assessment::Integrity { .. }
        | Assessment::NotOrre
        | Assessment::OtherHistory => Some(CANT_RESTORE),
    }
}

/// The Details block's text: the reason beyond the row, without the file's
/// path or "can't be restored", built from typed fields. `None` when the
/// row says it all (not a SQLite file, not Orre, another history), and for
/// AI access, whose only extra would be SQLite's message about a write to
/// the backup (which a trigger in it could write).
pub fn details(assessment: &Assessment) -> Option<String> {
    match assessment {
        Assessment::Schema {
            version,
            problem,
            object_type,
            name,
        } => Some(format!(
            "its schema {problem} {} \"{}\" (compared with Orre's migrations at version {version})",
            inert(object_type, NAME_CAP, false),
            inert(name, NAME_CAP, true),
        )),
        Assessment::Integrity { count, first } => Some(format!(
            "{count} {}; the first: {}",
            if *count == 1 { "problem" } else { "problems" },
            inert(first, MESSAGE_CAP, false),
        )),
        Assessment::TooNew { found, supported } => Some(
            db::Error::SchemaTooNew {
                found: *found,
                supported: *supported,
            }
            .to_string(),
        ),
        Assessment::Ready(_)
        | Assessment::NotADatabase
        | Assessment::NotOrre
        | Assessment::OtherHistory => None,
    }
}

impl RestoreTarget {
    /// A file chosen from disk ("Restore from a file…"): its time, when it
    /// has a backup's name.
    pub fn chosen(path: std::path::PathBuf) -> Self {
        let taken_at = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(backup::parse_file_name)
            .map(|(_, taken_at)| taken_at);
        RestoreTarget {
            path,
            taken_at,
            local: taken_at.map(super::super::store::local),
            from_file: true,
        }
    }
}

impl RestoreDialog {
    /// The assessment the rows show, when the checks are done.
    fn assessment(&self) -> Option<&Assessment> {
        match &self.stage {
            Stage::Checked(assessment) => Some(assessment),
            _ => None,
        }
    }

    /// Whether the checks found nothing to refuse (Restore can be pressed
    /// unless the current database can't be read).
    pub(super) fn passed(&self) -> bool {
        matches!(self.assessment(), Some(Assessment::Ready(_))) && !self.refusal.ai_refused
    }

    /// Whether the current database can't be read (Restore stays off).
    pub(super) fn current_unreadable(&self) -> bool {
        matches!(self.assessment(), Some(Assessment::Ready(check)) if check.current == Current::Unreadable)
    }

    /// The Details text now shown, when the block is open.
    fn details_text(&self) -> Option<String> {
        self.assessment().and_then(details)
    }

    /// After a restore that didn't happen: a failed check is shown in its
    /// row, AI access in row ③, anything else in the `danger` caption.
    pub(super) fn refused(&mut self, failure: Failure, previous: Stage) {
        self.refusal.details_open = false;
        match failure {
            Failure::Check(assessment) => self.stage = Stage::Checked(assessment),
            Failure::AiAccessNotOff => {
                self.stage = previous;
                self.refusal.ai_refused = true;
            }
            Failure::Other(error) => {
                self.stage = previous;
                self.error = Some(error);
            }
        }
    }

    pub fn toggle_details(&mut self, cx: &mut Context<Self>) {
        self.refusal.details_open = !self.refusal.details_open;
        cx.notify();
    }

    /// "Choose another file…": the system's open dialog, then the checks
    /// on the file chosen, in this dialog.
    pub fn choose_another(&mut self, cx: &mut Context<Self>) {
        if self.stage == Stage::Restoring || !self.target.from_file {
            return;
        }
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Restore".into()),
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.retarget(RestoreTarget::chosen(path), cx)
            });
        }));
    }

    /// Checks `target` instead, in place: the title, the rows and what
    /// restoring undoes follow it.
    pub fn retarget(&mut self, target: RestoreTarget, cx: &mut Context<Self>) {
        if self.stage == Stage::Restoring {
            return;
        }
        let store = self.settings.read(cx).store().ok().cloned();
        self.target = target;
        self.since = None;
        self._since_read = None;
        self.error = None;
        self.refusal.ai_refused = false;
        self.refusal.details_open = false;
        self.stage = Stage::Checking;
        if let Some(store) = store {
            self.read_since(store.clone(), cx);
            self.check(store, cx);
        }
        cx.notify();
    }

    /// The rows, the refusal's body and its Details, 12px apart; `None`
    /// while the checks run.
    pub(super) fn render_checks(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let assessment = self.assessment()?;
        let ai_refused = self.refusal.ai_refused;
        let rows = rows(assessment, ai_refused)
            .into_iter()
            .map(|row| check_row(row.passed, row.label, row.detail));
        let mut column = div()
            .flex()
            .flex_col()
            .child(div().flex().flex_col().children(rows));
        let Some(body) = body(assessment, ai_refused) else {
            return Some(column.into_any_element());
        };
        column = column.gap(spacing::S3).child(
            div()
                .text_role(typography::BODY)
                .text_color(color::TEXT_SECONDARY)
                .child(body),
        );
        if let Some(text) = details(assessment) {
            column = column.child(self.render_details(text, cx));
        }
        Some(column.into_any_element())
    }

    fn render_details(&self, text: String, cx: &mut Context<Self>) -> AnyElement {
        let open = self.refusal.details_open;
        let copied = text.clone();
        div()
            .flex()
            .flex_col()
            .gap(spacing::S1)
            .child(
                div().flex().child(
                    Button::new("restore-details", DETAILS)
                        .with_variant(ButtonVariant::Ghost)
                        .icon(Glyph::ChevronDown)
                        .with_surface(color::FLOAT)
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_details(cx))),
                ),
            )
            .when(open, |this| {
                this.child(
                    div()
                        .relative()
                        .child(
                            div()
                                .id("restore-details-text")
                                .max_h(px(DETAILS_MAX_HEIGHT))
                                .overflow_y_scroll()
                                .py(spacing::S2)
                                .pl(spacing::S3)
                                .pr(spacing::S6 + spacing::S2)
                                .rounded(radius::SURFACE)
                                .bg(color::FIELD)
                                .text_role(typography::MONO_VALUE)
                                .text_color(color::TEXT_SECONDARY)
                                .child(text),
                        )
                        .child(
                            div().absolute().top(spacing::S1).right(spacing::S1).child(
                                IconButton::new("restore-details-copy", Glyph::Copy, COPY_DETAILS)
                                    .with_size(IconButtonSize::Small)
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copied.clone(),
                                        ))
                                    }),
                            ),
                        ),
                )
            })
            .into_any_element()
    }

    /// "Choose another file…", for the footer's leading edge.
    pub(super) fn render_choose(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.target.from_file {
            return None;
        }
        Some(
            div()
                .flex()
                .child(
                    Button::new("restore-choose", CHOOSE_ANOTHER)
                        .with_variant(ButtonVariant::Outline)
                        .with_surface(color::FLOAT)
                        .track_focus(&self.refusal.choose)
                        .disabled(self.stage == Stage::Restoring)
                        .on_click(cx.listener(|this, _, _, cx| this.choose_another(cx))),
                )
                .into_any_element(),
        )
    }

    /// The rows, body and Details as the description lists them: each row
    /// as `check: <label>` or `alert: <label>` with its detail, then
    /// `body`, then `Details` when open.
    #[cfg(any(test, feature = "dev-driver"))]
    pub(super) fn describe_checks(&self) -> Vec<(String, String)> {
        let Some(assessment) = self.assessment() else {
            return Vec::new();
        };
        let ai_refused = self.refusal.ai_refused;
        let mut fields: Vec<(String, String)> = rows(assessment, ai_refused)
            .into_iter()
            .map(|row| {
                (
                    format!(
                        "{}: {}",
                        if row.passed { "check" } else { "alert" },
                        row.label
                    ),
                    Run::joined(&row.detail),
                )
            })
            .collect();
        if let Some(body) = body(assessment, ai_refused) {
            fields.push(("body".to_owned(), body.to_owned()));
        }
        if self.refusal.details_open
            && let Some(text) = self.details_text()
        {
            fields.push((DETAILS.to_owned(), text));
        }
        fields
    }

    /// The actions after Cancel and Restore: Details (and its copy button
    /// when open), and "Choose another file…".
    #[cfg(any(test, feature = "dev-driver"))]
    pub(super) fn describe_actions(&self) -> Vec<crate::ui::describe::Action> {
        use crate::ui::describe::Action;
        let mut actions = Vec::new();
        if self.details_text().is_some() {
            actions.push(Action::button(DETAILS, true));
            if self.refusal.details_open {
                actions.push(Action::button(COPY_DETAILS, true));
            }
        }
        if self.target.from_file {
            actions.push(Action::button(
                CHOOSE_ANOTHER,
                self.stage != Stage::Restoring,
            ));
        }
        actions
    }

    /// [`Self::refused`], for tests that can't make a restore fail that way.
    #[cfg(test)]
    pub(crate) fn refused_for_test(&mut self, failure: Failure, previous: Stage) {
        self.refused(failure, previous);
    }

    /// Whether "Choose another file…" has focus (tests).
    #[cfg(test)]
    pub(crate) fn choose_focused(&self, window: &gpui_kit::Window) -> bool {
        self.refusal.choose.is_focused(window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_text_is_capped_and_its_control_characters_escaped() {
        assert_eq!(inert("qa_evil", NAME_CAP, true), "qa_evil");
        assert_eq!(
            inert("a\"b\\c\n\u{202E}d\u{1b}[31m", NAME_CAP, true),
            "a\\\"b\\\\c\\u{a}\\u{202e}d\\u{1b}[31m"
        );
        assert_eq!(inert("near \"x\"", MESSAGE_CAP, false), "near \"x\"");
        assert_eq!(inert("a\nb\u{202E}", MESSAGE_CAP, false), "a b\\u{202e}");
        let long = "x".repeat(500);
        let capped = inert(&long, NAME_CAP, true);
        assert_eq!(capped.chars().count(), NAME_CAP + 1);
        assert!(capped.ends_with('…'));
    }

    #[test]
    fn dates_name_the_year_only_when_it_isnt_this_one() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 28).unwrap();
        let at = |year| {
            chrono::NaiveDate::from_ymd_opt(year, 11, 28)
                .unwrap()
                .and_hms_opt(11, 40, 0)
                .unwrap()
        };
        assert_eq!(when(at(2026), today), "28 Nov, 11:40");
        assert_eq!(when(at(2025), today), "28 Nov 2025, 11:40");
    }

    #[test]
    fn the_schema_refusal_names_the_object_never_its_sql() {
        let assessment = Assessment::Schema {
            version: 24,
            problem: backup::SchemaProblem::Extra,
            object_type: "trigger".into(),
            name: "qa_evil".into(),
        };
        assert_eq!(
            details(&assessment).unwrap(),
            "its schema has an extra trigger \"qa_evil\" (compared with Orre's migrations at \
             version 24)"
        );
        let hostile = Assessment::Schema {
            version: 24,
            problem: backup::SchemaProblem::Extra,
            object_type: "view".into(),
            name: format!("x\" ; DROP TABLE account; --\n{}", "y".repeat(200)),
        };
        let text = details(&hostile).unwrap();
        assert!(
            text.contains("x\\\" ; DROP TABLE account; --\\u{a}"),
            "{text}"
        );
        assert!(!text.contains('\n'), "{text}");
        assert!(text.contains('…'), "{text}");
    }
}
