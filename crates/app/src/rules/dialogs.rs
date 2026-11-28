//! The Rules view's dialogs (`docs/DESIGN.md` → "Rules" → states board;
//! Paper `HFH-0`), `--container-dialog` wide.
//!
//! - **Delete rule** "Delete the rule “Merchant contains UBER”?": its
//!   transactions keep what it set, new ones stay as they are unless
//!   another rule matches; it can't be undone (no transaction changes, so
//!   there is no change batch). Cancel / destructive solid "Delete rule".
//! - **Delete name** (composed from Delete rule): the transactions that
//!   showed it show the bank's cleaned text again, or another name. Cancel /
//!   destructive solid "Delete name".
//! - **Delete category** (composed from Delete rule; opened by the Rules
//!   view itself, not the shell): only for a category nothing uses, so
//!   nothing else changes. Cancel / destructive solid "Delete category".
//! - **Undo a change** "Undo “Applied rule TFL TRAVEL → Transport”?": what
//!   goes back and what stays because it changed since. Cancel / neutral
//!   primary "Undo change" (reversible by applying again).
//!
//! A refusal from core is said under the body in caption type, `danger`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Entity, EventEmitter, FocusHandle, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div,
};
use orre_core::change::{ChangeKind, UndoSummary};
use orre_core::id::{ChangeBatchId, MerchantAliasId, RuleId};
use orre_core::rule::RuleAction;

use crate::accounts::DialogClosed;
use crate::theme::tokens::{color, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::error_caption;
use crate::ui::text::TextRoleExt as _;

use super::Rules;
use super::text;

/// Every dialog's way out.
pub const CANCEL: &str = "Cancel";
/// Each dialog's confirming button.
pub const DELETE_RULE: &str = "Delete rule";
pub const DELETE_NAME: &str = "Delete name";
pub const DELETE_CATEGORY: &str = "Delete category";
pub const UNDO_CHANGE: &str = "Undo change";

fn title(words: String) -> impl IntoElement {
    div()
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .text_role(typography::TITLE)
        .text_color(color::TEXT)
        .child(words)
}

fn body(words: String) -> impl IntoElement {
    div()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(words)
}

/// "Delete the rule …?".
pub struct DeleteRuleDialog {
    rules: Entity<Rules>,
    rule: RuleId,
    title: String,
    body: String,
    focus: FocusHandle,
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for DeleteRuleDialog {}

impl DeleteRuleDialog {
    pub fn new(
        rules: Entity<Rules>,
        rule: RuleId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let (title, body) = {
            let state = rules.read(cx);
            let data = state.data();
            match data.rule(rule) {
                Some(summary) => {
                    let definition = summary.rule.definition();
                    let named = format!(
                        "{} {}",
                        text::condition(definition),
                        text::pattern(definition)
                    );
                    let rows = text::count(summary.decides, "transaction", "transactions");
                    let kept = match definition.action(orre_core::rule::ActionKind::SetCategory) {
                        Some(RuleAction::SetCategory(key)) => {
                            let label = data
                                .category(key)
                                .map_or_else(|| key.as_str().to_owned(), |c| c.label.clone());
                            format!("The {rows} it categorised keep {label}. ")
                        }
                        _ => format!("The {rows} it changed keep what it set. "),
                    };
                    (
                        format!("Delete the rule “{named}”?"),
                        format!(
                            "{kept}New transactions like them stay as they are unless another \
                             rule matches. This can’t be undone, but you can create the rule \
                             again."
                        ),
                    )
                }
                None => ("Delete the rule?".to_owned(), String::new()),
            }
        };
        Self {
            rules,
            rule,
            title,
            body,
            focus,
            error: None,
        }
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let rule = self.rule;
        match self
            .rules
            .update(cx, |rules, cx| rules.delete_rule(rule, cx))
        {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }
}

impl Render for DeleteRuleDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_cancel = cx.entity().downgrade();
        Dialog::with_title(title(self.title.clone()), &self.focus)
            .body(body(self.body.clone()))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("delete-rule-cancel", CANCEL)
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("delete-rule-confirm", DELETE_RULE)
                    .with_variant(ButtonVariant::DestructiveSolid)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

/// "Delete the name …?" (composed from Delete rule).
pub struct DeleteNameDialog {
    rules: Entity<Rules>,
    name: MerchantAliasId,
    title: String,
    body: String,
    focus: FocusHandle,
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for DeleteNameDialog {}

impl DeleteNameDialog {
    pub fn new(
        rules: Entity<Rules>,
        name: MerchantAliasId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let (title, body) = match rules.read(cx).data().name(name) {
            Some(summary) => {
                let shown =
                    text::plain(summary.alias.definition().name().as_str(), text::NAME_SHOWN);
                (
                    format!("Delete the name “{shown}”?"),
                    delete_name_words(summary.matches),
                )
            }
            None => ("Delete the name?".to_owned(), String::new()),
        };
        Self {
            rules,
            name,
            title,
            body,
            focus,
            error: None,
        }
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let name = self.name;
        match self
            .rules
            .update(cx, |rules, cx| rules.delete_name(name, cx))
        {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }
}

/// The delete-name body: what happens to the transactions that show it.
fn delete_name_words(matches: usize) -> String {
    let lead = match matches {
        0 => "No transactions show it, so nothing changes.".to_owned(),
        1 => "The transaction that shows it goes back to the bank’s cleaned text, or to another \
              name that matches."
            .to_owned(),
        n => format!(
            "The {n} transactions that show it go back to the bank’s cleaned text, or to \
             another name that matches."
        ),
    };
    format!("{lead} Names you typed on a transaction stay. You can add the name again.")
}

impl Render for DeleteNameDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_cancel = cx.entity().downgrade();
        Dialog::with_title(title(self.title.clone()), &self.focus)
            .body(body(self.body.clone()))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("delete-name-cancel", CANCEL)
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("delete-name-confirm", DELETE_NAME)
                    .with_variant(ButtonVariant::DestructiveSolid)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

/// "Delete the category …?" (composed from Delete rule). Offered only for a
/// category nothing uses; core checks again.
pub struct DeleteCategoryDialog {
    rules: Entity<Rules>,
    category: i64,
    title: String,
    body: String,
    focus: FocusHandle,
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for DeleteCategoryDialog {}

impl DeleteCategoryDialog {
    pub fn new(
        rules: Entity<Rules>,
        category: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let (title, body) = match rules.read(cx).data().category_entry(category) {
            Some((summary, _)) => {
                let category = &summary.category;
                let what = if category.is_group() {
                    "group"
                } else {
                    "category"
                };
                (
                    format!("Delete the {what} “{}”?", category.label()),
                    format!(
                        "No transaction, split, rule, budget or schedule uses it, so nothing else \
                         changes. This can’t be undone. You can add a category with this name \
                         again, under a new key: {} is never used again.",
                        category.key()
                    ),
                )
            }
            None => ("Delete the category?".to_owned(), String::new()),
        };
        Self {
            rules,
            category,
            title,
            body,
            focus,
            error: None,
        }
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let category = self.category;
        match self
            .rules
            .update(cx, |rules, cx| rules.delete_category(category, cx))
        {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }
}

impl Render for DeleteCategoryDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_cancel = cx.entity().downgrade();
        Dialog::with_title(title(self.title.clone()), &self.focus)
            .body(body(self.body.clone()))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("delete-category-cancel", CANCEL)
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("delete-category-confirm", DELETE_CATEGORY)
                    .with_variant(ButtonVariant::DestructiveSolid)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

/// "Undo …?".
pub struct UndoChangeDialog {
    rules: Entity<Rules>,
    batch: ChangeBatchId,
    title: String,
    body: String,
    focus: FocusHandle,
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for UndoChangeDialog {}

/// What goes back and what stays, as the dialog says it.
fn undo_words(summary: &UndoSummary, date: String) -> String {
    let total = summary.batch.transactions;
    let back = summary.transactions;
    let other = total.saturating_sub(back);
    let what = match summary.batch.kind {
        ChangeKind::RuleApply => " The rule stays.",
        _ => "",
    };
    let lead = if total == 1 && back == 1 {
        format!("The transaction goes back to the values it had before {date}.")
    } else if back == total {
        format!(
            "The {} go back to the values they had before {date}.",
            text::count(total, "transaction", "transactions")
        )
    } else {
        format!(
            "{back} of the {} go back to the values they had before {date}.",
            text::count(total, "transaction", "transactions")
        )
    };
    let since = match other {
        0 => String::new(),
        1 => " The other 1 has changed since, by your edit or another rule, and keeps its \
              current value."
            .to_owned(),
        n => format!(
            " The other {n} have changed since, by your edit or another rule, and keep their \
             current values."
        ),
    };
    format!("{lead}{since}{what}")
}

impl UndoChangeDialog {
    pub fn new(
        rules: Entity<Rules>,
        batch: ChangeBatchId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let (title, body, error) = {
            let state = rules.read(cx);
            let data = state.data();
            let today = state.today();
            let date = data
                .history_at
                .get(&batch)
                .map(|(made, _)| text::date(made.date(), today))
                .unwrap_or_else(|| "the change".to_owned());
            let title = match data.entry(batch) {
                Some(entry) => format!("Undo “{}”?", text::change_words(entry, data)),
                None => "Undo this change?".to_owned(),
            };
            match state.undo_preview(batch) {
                Ok(summary) => (title, undo_words(&summary, date), None),
                Err(error) => (title, String::new(), Some(error)),
            }
        };
        Self {
            rules,
            batch,
            title,
            body,
            focus,
            error,
        }
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let batch = self.batch;
        match self
            .rules
            .update(cx, |rules, cx| rules.undo_batch(batch, cx))
        {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }
}

impl Render for UndoChangeDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_cancel = cx.entity().downgrade();
        Dialog::with_title(title(self.title.clone()), &self.focus)
            .body(body(self.body.clone()))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("undo-change-cancel", CANCEL)
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("undo-change-confirm", UNDO_CHANGE)
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

/// What each dialog shows, for the test-only `describe()`
/// ([`super::describe`]): its title, body, refusal and buttons, as drawn.
#[cfg(any(test, feature = "dev-driver"))]
mod described {
    use super::super::describe::DialogDescription;
    use super::*;

    fn described(
        title: &str,
        body: &str,
        error: Option<&SharedString>,
        confirm: &str,
    ) -> DialogDescription {
        DialogDescription::new(title, body, error, confirm)
    }

    impl DeleteRuleDialog {
        pub fn describe(&self) -> DialogDescription {
            described(&self.title, &self.body, self.error.as_ref(), DELETE_RULE)
        }
    }

    impl DeleteNameDialog {
        pub fn describe(&self) -> DialogDescription {
            described(&self.title, &self.body, self.error.as_ref(), DELETE_NAME)
        }
    }

    impl DeleteCategoryDialog {
        pub fn describe(&self) -> DialogDescription {
            described(
                &self.title,
                &self.body,
                self.error.as_ref(),
                DELETE_CATEGORY,
            )
        }
    }

    impl UndoChangeDialog {
        pub fn describe(&self) -> DialogDescription {
            described(&self.title, &self.body, self.error.as_ref(), UNDO_CHANGE)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orre_core::change::{BatchInfo, BatchStatus};

    fn summary(transactions: usize, restored_on: usize) -> UndoSummary {
        UndoSummary {
            batch: BatchInfo {
                id: ChangeBatchId::new(1),
                kind: ChangeKind::RuleApply,
                created_at: String::new(),
                status: BatchStatus::Applied,
                undone_at: None,
                rule: None,
                items: transactions,
                transactions,
            },
            restored: restored_on,
            changed_since: transactions - restored_on,
            transactions: restored_on,
        }
    }

    #[test]
    fn the_delete_name_dialog_counts_in_the_singular_and_plural() {
        let tail = " Names you typed on a transaction stay. You can add the name again.";
        assert_eq!(
            delete_name_words(0),
            format!("No transactions show it, so nothing changes.{tail}")
        );
        assert_eq!(
            delete_name_words(1),
            format!(
                "The transaction that shows it goes back to the bank’s cleaned text, or to \
                 another name that matches.{tail}"
            )
        );
        assert_eq!(
            delete_name_words(3),
            format!(
                "The 3 transactions that show it go back to the bank’s cleaned text, or to \
                 another name that matches.{tail}"
            )
        );
    }

    #[test]
    fn the_undo_dialog_counts_what_goes_back() {
        assert_eq!(
            undo_words(&summary(58, 56), "2 Nov".into()),
            "56 of the 58 transactions go back to the values they had before 2 Nov. The other 2 \
             have changed since, by your edit or another rule, and keep their current values. \
             The rule stays."
        );
        assert_eq!(
            undo_words(&summary(4, 4), "9 Nov".into()),
            "The 4 transactions go back to the values they had before 9 Nov. The rule stays."
        );
    }
}
