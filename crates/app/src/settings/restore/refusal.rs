//! The Restore dialog's checklist and its refusals (SLI-71; `docs/DESIGN.md`
//! → "Settings" → **A check fails**).
//!
//! Three 24px rows, in order, stopping at the first that fails: the file is
//! intact, this version of Orre can read it, and AI access can be switched
//! off. Each refusal fails one row with its own label and detail, then a
//! body and, when the refusal has more to say than its row, a collapsed
//! "Details" block. Everything here is built from the typed check result
//! ([`Assessment`], [`Failure`]), never by reading an error's text, and
//! nothing taken from the file reaches the window unescaped: object names,
//! SQLite's messages, a chosen file's name and the `danger` caption all go
//! through [`inert`] (invisible characters escaped, `$` and `` ` `` made
//! inert for a shell, the output capped). SQL from the file is never shown.
//!
//! The third row can only fail when Restore is pressed: the restore
//! switches the backup's AI clients off and reads them back before it
//! replaces anything ([`orre_core::backup::restore`]); if that doesn't hold
//! it refuses with `Error::AiAccessNotOff` and the dialog goes back to its
//! checked state with that row failed.
//!
//! "Choose another file…" (only when opened from "Restore from a file…")
//! reopens the system's open dialog and runs the checks again in place.

use std::f32::consts::FRAC_PI_2;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, ClipboardItem, Context,
    FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Transformation, Window, div, px, radians,
};
use orre_core::backup::detail::Assessment;
use orre_core::backup::{self, Current};
use orre_core::db;

use crate::accounts::store::StoreError;
use crate::theme::tokens::{color, motion, radius, shadow, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant, GROUP, IconButton, IconButtonSize};
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

/// The Details block's copy button: its accessible name and its tooltip
/// (Rowling, SLI-71 review).
pub const COPY_DETAILS: &str = "Copy error details";

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
/// Rowling's body for a backup whose AI access can't be switched off. It
/// closes with "Your data hasn't changed." and not "Nothing has changed."
/// or "…you'll see one more backup in the list": by the time the read-back
/// fails, `backup::restore` has already backed the current database up,
/// and rotation can delete an old automatic backup then, so the list
/// doesn't always grow. The database is untouched (`backup::tests::
/// an_ai_access_refusal_leaves_the_data_but_not_always_one_more_backup`).
const AI_BODY: &str = "Orre only restores a backup if it can switch AI clients off afterwards. \
     It can't do that here. Your data hasn't changed.";

/// The longest object or file name shown, in characters of escaped output
/// (core caps a schema name at 64 input characters; this doesn't rely on
/// that).
const NAME_CAP: usize = 64;
/// The longest SQLite message the Details block shows, in characters of
/// escaped output.
const MESSAGE_CAP: usize = 160;
/// The longest error the dialog's `danger` caption shows, in characters of
/// escaped output.
const ERROR_CAP: usize = 300;
/// The Details chevron's turn: closed points right, open points down.
const CHEVRON_TURN: Duration = Duration::from_millis(120);
const CHEVRON_CLOSED: f32 = -FRAC_PI_2;
const CHEVRON_OPEN: f32 = 0.;
/// The Details block's height before it scrolls.
const DETAILS_MAX_HEIGHT: f32 = 96.;

/// The dialog's state beyond its stage.
pub(super) struct RefusalState {
    /// The last restore was refused because AI access couldn't be switched
    /// off: row ③ fails and Restore stays off.
    pub ai_refused: bool,
    /// Whether the Details block is open.
    pub details_open: bool,
    chevron: Chevron,
    /// "Choose another file…", focused first when it's there.
    pub choose: FocusHandle,
}

impl RefusalState {
    pub fn new(cx: &mut App) -> Self {
        Self {
            ai_refused: false,
            details_open: false,
            chevron: Chevron::default(),
            choose: cx.focus_handle(),
        }
    }

    /// Opens or closes Details. The chevron turns from wherever it is now,
    /// so a click mid-turn turns it back from there, not from the end.
    pub fn toggle(&mut self, now: Instant, reduce_motion: bool) {
        self.chevron.turn(self.details_open, now, reduce_motion);
        self.details_open = !self.details_open;
    }

    /// Closes Details at once, chevron and all (a new refusal or target).
    fn close(&mut self) {
        self.details_open = false;
        self.chevron = Chevron::default();
    }
}

/// The Details chevron's turn: where it started, in radians, and when; how
/// many turns there have been (each animates under its own id).
#[derive(Clone, Copy, Debug)]
struct Chevron {
    from: f32,
    turned_at: Option<Instant>,
    turns: u64,
}

impl Default for Chevron {
    fn default() -> Self {
        Self {
            from: CHEVRON_CLOSED,
            turned_at: None,
            turns: 0,
        }
    }
}

impl Chevron {
    /// The angle it's turning to, Details being `open`.
    fn target(open: bool) -> f32 {
        if open { CHEVRON_OPEN } else { CHEVRON_CLOSED }
    }

    /// Its angle at `now`, Details being `open`: along the turn, eased as
    /// drawn.
    fn angle(&self, open: bool, now: Instant, reduce_motion: bool) -> f32 {
        let target = Self::target(open);
        match self.turned_at {
            Some(at) if !reduce_motion => {
                let progress =
                    (now.duration_since(at).as_secs_f32() / CHEVRON_TURN.as_secs_f32()).min(1.);
                self.from + (target - self.from) * ease_out(progress)
            }
            _ => target,
        }
    }

    /// Starts a turn from where it is at `now` (Details was `was_open`).
    fn turn(&mut self, was_open: bool, now: Instant, reduce_motion: bool) {
        self.from = self.angle(was_open, now, reduce_motion);
        self.turned_at = Some(now);
        self.turns += 1;
    }
}

/// The entering curve (`motion::EASE_ENTER`, an ease-out) at `progress`.
fn ease_out(progress: f32) -> f32 {
    let curve = motion::EASE_ENTER;
    gpui_kit::base::Easing::cubic_bezier(curve.x1, curve.y1, curve.x2, curve.y2)
        .map_or(progress, |easing| easing.sample(progress))
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

/// `text` safe to show, as every window path that shows text from a file
/// (or an error about one) does, and as the copy button copies it:
///
/// - a character Rust's `char::escape_debug` would escape (controls,
///   format characters such as bidirectional overrides, zero-width and tag
///   characters, variation selectors, private use, combining marks, which
///   are Grapheme_Extend, so a decomposed accent escapes, a precomposed
///   "é" doesn't), and the two blanks it counts as printable (U+2800,
///   U+FFFC), is written `\u{…}`; without `quoted` (a message) a line
///   break or tab is a space instead;
/// - `$` and `` ` `` get a backslash, so a copy pasted into a shell can't
///   expand or run them; with `quoted` (a name between `"…"`) so do `"`
///   and `\`;
/// - the output stops before it would pass `cap` characters, never inside
///   an escape, and ends with "…" then.
pub fn inert(text: &str, cap: usize, quoted: bool) -> String {
    let mut out = String::new();
    let mut length = 0;
    for c in text.chars() {
        let piece = if !quoted && (c == '\n' || c == '\t') {
            // A line break in SQLite's own message reads as a space.
            " ".to_owned()
        } else if hidden(c) {
            format!("\\u{{{:x}}}", u32::from(c))
        } else if c == '$' || c == '`' || (quoted && (c == '"' || c == '\\')) {
            format!("\\{c}")
        } else {
            c.to_string()
        };
        let added = piece.chars().count();
        if length + added > cap {
            out.push('…');
            break;
        }
        out.push_str(&piece);
        length += added;
    }
    out
}

/// Whether `c` would be invisible, or isn't a character in its own right:
/// what `char::escape_debug` escapes (bar the quotes and backslash it
/// escapes as syntax), and the blanks it lets through.
fn hidden(c: char) -> bool {
    const PRINTABLE_BLANKS: [char; 2] = ['\u{2800}', '\u{FFFC}'];
    let syntax = matches!(c, '"' | '\'' | '\\');
    (!syntax && c.escape_debug().next() == Some('\\')) || PRINTABLE_BLANKS.contains(&c)
}

/// A chosen file's name as the title shows it ([`inert`], capped).
pub fn file_title(name: &str) -> String {
    inert(name, NAME_CAP, false)
}

/// "28 Nov 2026, 16:35": a backup's time in the Restore dialog's title and
/// the notification after a restore, always with its year (Pablo, SLI-71
/// review).
pub fn when(local: chrono::NaiveDateTime) -> String {
    local.format("%-d %b %Y, %H:%M").to_string()
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

/// A tooltip of plain words, drawn as the transactions table's
/// suggested-transfer tip is.
struct TextTip(&'static str);

impl Render for TextTip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .m(spacing::S2)
            .px(spacing::S2_5)
            .py(spacing::S2)
            .bg(color::HOVER)
            .border_1()
            .border_color(color::TOOLTIP_BORDER)
            .rounded(radius::SURFACE)
            .shadow(vec![shadow::TOOLTIP.box_shadow()])
            .text_role(typography::CAPTION)
            .text_color(color::TEXT_SECONDARY)
            .child(self.0)
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
    #[cfg(any(test, feature = "dev-driver"))]
    fn details_text(&self) -> Option<String> {
        self.assessment().and_then(details)
    }

    /// After a restore that didn't happen: a failed check is shown in its
    /// row, AI access in row ③, anything else in the `danger` caption.
    pub(super) fn refused(&mut self, failure: Failure, previous: Stage) {
        self.refusal.close();
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
        self.refusal.toggle(Instant::now(), cx.reduce_motion());
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
        self.refusal.close();
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

    /// The chevron: 12px, `text-muted`, `text-secondary` with the button's
    /// hover; turning over 120ms on the entering curve from wherever it was
    /// (`with_animation` honours `App::reduce_motion`).
    fn chevron(&self) -> AnyElement {
        let chevron = self.refusal.chevron;
        let to = Chevron::target(self.refusal.details_open);
        let from = chevron.from;
        let glyph = Glyph::ChevronDown
            .render_at(size::ICON_SM)
            .text_color(color::TEXT_MUTED)
            .group_hover(GROUP, |style| style.text_color(color::TEXT_SECONDARY));
        if chevron.turned_at.is_none() || from == to {
            return glyph
                .with_transformation(Transformation::rotate(radians(to)))
                .into_any_element();
        }
        glyph
            .with_animation(
                SharedString::from(format!("restore-details-chevron-{}", chevron.turns)),
                Animation::new(CHEVRON_TURN).with_easing(ease_out),
                move |glyph, delta| {
                    glyph.with_transformation(Transformation::rotate(radians(
                        from + (to - from) * delta,
                    )))
                },
            )
            .into_any_element()
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
                        .with_variant(ButtonVariant::Disclosure)
                        .leading(self.chevron())
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
                            div()
                                .id("restore-details-copy-tip")
                                .absolute()
                                .top(spacing::S1)
                                .right(spacing::S1)
                                .tooltip(|_, cx| cx.new(|_| TextTip(COPY_DETAILS)).into())
                                .child(
                                    IconButton::new(
                                        "restore-details-copy",
                                        Glyph::Copy,
                                        COPY_DETAILS,
                                    )
                                    .with_size(IconButtonSize::Small)
                                    .on_click(
                                        move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copied.clone(),
                                            ))
                                        },
                                    ),
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

    /// Every result fits its cap, then "…".
    fn assert_fits(text: &str, cap: usize) {
        let length = text.chars().count();
        let ellipsis = text.ends_with('…');
        assert!(
            length <= cap + usize::from(ellipsis),
            "{length} > {cap}: {text}"
        );
    }

    #[test]
    fn hostile_text_is_capped_and_its_control_characters_escaped() {
        assert_eq!(inert("qa_evil", NAME_CAP, true), "qa_evil");
        assert_eq!(
            inert("a\"b\\c\n\u{202E}d\u{1b}[31m", NAME_CAP, true),
            "a\\\"b\\\\c\\u{a}\\u{202e}d\\u{1b}[31m"
        );
        assert_eq!(inert("near \"x\"", MESSAGE_CAP, false), "near \"x\"");
        assert_eq!(inert("a\nb\u{202E}", MESSAGE_CAP, false), "a b\\u{202e}");
        // A zero-width space, in a name and in a message.
        assert_eq!(inert("a\u{200B}b", NAME_CAP, true), "a\\u{200b}b");
        assert_eq!(inert("a\u{200B}b", MESSAGE_CAP, false), "a\\u{200b}b");
        let long = "x".repeat(500);
        let capped = inert(&long, NAME_CAP, true);
        assert_eq!(capped.chars().count(), NAME_CAP + 1);
        assert!(capped.ends_with('…'));
        // An over-long Integrity message ("first"), as SQLite words it.
        let first = format!("row 1 missing from index {}", "i".repeat(5000));
        let details = details(&Assessment::Integrity { count: 3, first }).unwrap();
        let shown = details.strip_prefix("3 problems; the first: ").unwrap();
        assert_fits(shown, MESSAGE_CAP);
        assert!(shown.ends_with('…'), "{shown}");
    }

    /// Characters `char::escape_debug` escapes (and the two blanks it
    /// doesn't) are written `\u{…}`; letters, accents and emoji aren't.
    #[test]
    fn invisible_characters_are_escaped_and_letters_kept() {
        for c in [
            '\u{2800}',
            '\u{3164}',
            '\u{115F}',
            '\u{1160}',
            '\u{FFFC}',
            '\u{034F}',
            '\u{FE0F}',
            '\u{E0041}',
            '\u{E007F}',
            '\u{FE00}',
            '\u{0600}',
            '\u{06DD}',
            '\u{070F}',
            '\u{08E2}',
            '\u{1BCA0}',
            '\u{1D173}',
            '\u{00A0}',
            '\u{200D}',
            '\u{0301}',
        ] {
            let shown = inert(&format!("a{c}b"), NAME_CAP, true);
            assert_eq!(
                shown,
                format!("a\\u{{{:x}}}b", u32::from(c)),
                "U+{:04X}",
                u32::from(c)
            );
            assert_eq!(inert(&format!("a{c}b"), ERROR_CAP, false), shown);
        }
        // Precomposed accents and other scripts are letters, as they are.
        let names = "café Straße Жук 中文 naïve 🧑";
        assert_eq!(inert(names, NAME_CAP, true), names);
        // A decomposed accent escapes its combining mark.
        assert_eq!(inert("cafe\u{301}", NAME_CAP, true), "cafe\\u{301}");
        // A flood of combining marks can't stack: each is an escape.
        let zalgo = format!("a{}", "\u{301}".repeat(500));
        let shown = inert(&zalgo, NAME_CAP, true);
        assert!(shown.chars().all(|c| c.is_ascii() || c == '…'), "{shown}");
        assert_fits(&shown, NAME_CAP);
    }

    /// The copy button copies what Details shows: `$` and `` ` `` can't
    /// expand or run when it's pasted into a shell.
    #[test]
    fn shell_characters_are_inert_in_what_is_copied() {
        let first = "row 1 missing from index x$(touch /tmp/david_pwn)`id`".to_owned();
        let text = details(&Assessment::Integrity { count: 3, first }).unwrap();
        assert_eq!(
            text,
            "3 problems; the first: row 1 missing from index x\\$(touch /tmp/david_pwn)\\`id\\`"
        );
        let unescaped = |text: &str| {
            let chars: Vec<char> = text.chars().collect();
            (0..chars.len())
                .any(|i| matches!(chars[i], '$' | '`') && (i == 0 || chars[i - 1] != '\\'))
        };
        assert!(!unescaped(&text), "{text}");
        let name = Assessment::Schema {
            version: 24,
            problem: backup::SchemaProblem::Extra,
            object_type: "table".into(),
            name: "x$(id>/tmp/pwn)`id`;rm -rf ~".into(),
        };
        let text = details(&name).unwrap();
        assert!(!unescaped(&text), "{text}");
        assert!(
            text.contains("\"x\\$(id>/tmp/pwn)\\`id\\`;rm -rf ~\""),
            "{text}"
        );
    }

    /// The cap is on what's shown: escapes count, and none is cut in two.
    #[test]
    fn the_cap_counts_the_escaped_output() {
        let bidi = "\u{202E}".repeat(64);
        let shown = inert(&bidi, NAME_CAP, true);
        assert_fits(&shown, NAME_CAP);
        assert_eq!(shown, format!("{}…", "\\u{202e}".repeat(NAME_CAP / 8)));
        let caption = inert(&"\u{202E}".repeat(300), ERROR_CAP, false);
        assert_fits(&caption, ERROR_CAP);
        assert!(
            caption.trim_end_matches('…').ends_with("\\u{202e}"),
            "{caption}"
        );
        let long = inert(&"N".repeat(5000), NAME_CAP, true);
        assert_fits(&long, NAME_CAP);
        assert_eq!(long.chars().count(), NAME_CAP + 1);
        // Multibyte and a ZWJ right at the cap: whole characters, whole
        // escapes.
        let people = "\u{1F9D1}\u{200D}\u{1F4BB}".repeat(40);
        let shown = inert(&people, NAME_CAP, true);
        assert_fits(&shown, NAME_CAP);
        let body = shown.trim_end_matches('…');
        assert!(
            body.ends_with('\u{1F9D1}')
                || body.ends_with('\u{1F4BB}')
                || body.ends_with("\\u{200d}"),
            "{shown}"
        );
        let error = error_text(&std::io::Error::other("\u{202E}".repeat(300)));
        assert_fits(&error, ERROR_CAP + 1);
    }

    #[test]
    fn dates_always_name_the_year() {
        let at = |year| {
            chrono::NaiveDate::from_ymd_opt(year, 11, 28)
                .unwrap()
                .and_hms_opt(16, 35, 0)
                .unwrap()
        };
        assert_eq!(when(at(2026)), "28 Nov 2026, 16:35");
        assert_eq!(when(at(2025)), "28 Nov 2025, 16:35");
    }

    /// A click mid-turn turns the chevron back from where it is, not from
    /// the end; without motion it's at its end at once.
    #[test]
    fn the_chevron_turns_back_from_where_it_is() {
        let mut chevron = Chevron::default();
        let mut open = false;
        let start = Instant::now();
        assert_eq!(chevron.angle(open, start, false), CHEVRON_CLOSED);
        chevron.turn(open, start, false);
        open = true;
        assert_eq!(chevron.angle(open, start, false), CHEVRON_CLOSED);
        assert_eq!(
            chevron.angle(open, start + CHEVRON_TURN, false),
            CHEVRON_OPEN
        );
        let halfway = start + CHEVRON_TURN / 2;
        let angle = chevron.angle(open, halfway, false);
        assert!(CHEVRON_CLOSED < angle && angle < CHEVRON_OPEN, "{angle}");
        chevron.turn(open, halfway, false);
        open = false;
        assert_eq!(
            chevron.angle(open, halfway, false),
            angle,
            "from where it was"
        );
        assert_eq!(
            chevron.angle(open, halfway + CHEVRON_TURN, false),
            CHEVRON_CLOSED
        );
        chevron.turn(open, halfway, true);
        open = true;
        assert_eq!(
            chevron.angle(open, halfway, true),
            CHEVRON_OPEN,
            "reduced motion"
        );
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
