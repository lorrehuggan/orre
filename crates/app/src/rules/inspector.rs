//! The Rules view's inspector (`docs/DESIGN.md` → "Rules"; Paper `B1J-0`,
//! `EFT-0`, `G2O-0`, `GZT-0`, `HFH-0`): 14px sections divided by hairlines.
//!
//! - **A rule**: the rule as title, what it decides as the figure; Never
//!   reached (the card naming the rule that catches its matches); When
//!   (Field, Test, Text, Account, Amount); Then (one row per action, "+ Add
//!   action"); Past transactions (Apply, and an agent's rule over this one
//!   from the Inbox); the footer (where it came from, Delete rule…). A new
//!   rule has Order (Position) instead of Past transactions and no footer.
//! - **A merchant name**: Match (Test, Text, Shows as), the bank text it
//!   catches, the footer (Added on, Delete name…).
//! - **A change**: what changed, row by row, and Undo this change….
//! - **A category** (composed, not drawn: the name inspector's layout): the
//!   label as title, the transactions filed under it as the figure;
//!   Category (Name, saved on blur and Enter as a rename; Group and Kind
//!   locked, as the account inspector's Currency); Used by (the five
//!   counts); In pickers (Hide or Show); the footer (the key in mono, and
//!   "Delete category…" only when nothing uses it). A new category has
//!   Name, Group ("New group" or a group) and Kind (a segmented control for
//!   a new group, else the group's, locked), the key it will get, and
//!   "Create category".
//!
//! Composed, not drawn (`DESIGN.md` → "Rules" → "Built in M3.13"): the
//! Amount condition's second row, the Split action's lines, the Exclude
//! reason row, the Tag action's pills, New rule and New name from scratch.

use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use orre_core::change::BatchStatus;
use orre_core::rule::{ActionKind, RuleOrigin};

use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::{color, font, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant, IconButton, IconButtonSize};
use crate::ui::field::{
    self as fields, FieldRow, LockedValue, SelectField, error_caption, text_input,
};
use crate::ui::icon::Glyph;
use crate::ui::inspector::{caption, section, section_title_row};
use crate::ui::marker::Marker;
use crate::ui::menu::{self, Select};
use crate::ui::segmented::Segmented;
use crate::ui::tag::{AddTagPill, TagPill};
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::form::{ActionForm, AmountChoice, CategoryError, Direction, FormError, NameError};
use super::text::{self, Part, rich};
use super::view::{Input, Menu, RulesView, amount_label};
use super::{CategorySelection, NameSelection, Rules, Section};

/// The inspector for the open section.
pub fn render(
    view: &RulesView,
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let section_kind = entity.read(cx).section();
    let sections: Vec<AnyElement> = match section_kind {
        Section::Rules => rule_sections(view, entity, window, cx),
        Section::Names => name_sections(view, entity, window, cx),
        Section::History => change_sections(entity, window, cx),
        Section::Categories => category_sections(view, entity, window, cx),
    };
    div()
        .id("rules-inspector")
        .flex()
        .flex_col()
        .flex_none()
        .w(size::INSPECTOR)
        .h_full()
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_l(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .children(sections)
        .into_any_element()
}

/// What the inspector says with nothing selected, by section.
pub const NO_RULE: &str =
    "Select a rule to see what it does and edit it, or start one with New rule.";
pub const NO_NAME: &str = "Select a name to edit it, or start one with New name.";
pub const NO_CHANGE: &str = "Select a change to see what it did and undo it.";
pub const NO_CATEGORY: &str =
    "Select a category to rename, hide or delete it, or add one with New category.";
/// A stored category selected but gone.
pub const CATEGORY_GONE: &str = "Select a category to rename, hide or delete it.";

/// What the header says: what `header` draws and the test-only
/// `describe()` reads.
pub struct HeaderText {
    /// The title, with mono parts.
    pub title: Vec<Part>,
    /// The figure, and whether it's faint.
    pub figure: Option<(String, bool)>,
    pub caption: Option<String>,
}

/// The header: a title with mono parts, the figure and a caption.
fn header(window: &Window, text: HeaderText) -> AnyElement {
    let HeaderText {
        title,
        figure,
        caption: caption_text,
    } = text;
    let caption_text = caption_text.map(caption_line);
    section(false)
        .gap(spacing::S1)
        .child(
            div()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(rich(window, typography::TITLE, color::TEXT, title)),
        )
        .when_some(figure, |this, (figure, faint)| {
            this.child(
                div()
                    .flex()
                    .text_color(if faint {
                        color::TEXT_FAINT
                    } else {
                        color::TEXT
                    })
                    .child(Tracked::new(typography::DISPLAY_FIGURE, figure)),
            )
        })
        .children(caption_text)
        .into_any_element()
}

fn caption_line(words: String) -> AnyElement {
    caption(div().child(SharedString::from(words))).into_any_element()
}

/// Nothing selected: one caption.
fn nothing(words: &'static str) -> Vec<AnyElement> {
    vec![
        section(false)
            .child(fields::caption(words))
            .into_any_element(),
    ]
}

/// A select field in a row, wired to the view's menus.
fn select(
    view: &RulesView,
    menu: Menu,
    label: &'static str,
    field: SelectField,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let groups = if view.menu == Some(menu) {
        view.visible_groups(cx)
    } else {
        view.menu_groups(menu, cx)
    };
    let (toggled, chosen, moved) = (
        cx.entity().downgrade(),
        cx.entity().downgrade(),
        cx.entity().downgrade(),
    );
    let mut select = Select::new(
        SharedString::from(format!("rules-select-{menu:?}")),
        label,
        field,
        groups,
        view.chosen_key(menu, cx),
        view.state(menu),
    )
    .on_toggle(move |open, window, cx| {
        toggled
            .update(cx, |this, cx| {
                if open != (this.menu == Some(menu)) {
                    if open {
                        this.open(menu, window, cx);
                    } else {
                        this.close(window, cx);
                    }
                }
            })
            .ok();
    })
    .on_choose(move |key, window, cx| {
        chosen
            .update(cx, |this, cx| this.choose(key, window, cx))
            .ok();
    })
    .on_move(move |step, _, cx| {
        moved
            .update(cx, |this, cx| this.move_highlight(step, cx))
            .ok();
    });
    if matches!(menu, Menu::Category | Menu::SplitLine(_)) {
        select = select
            .filter(&view.menu_filter)
            .max_height(size::CONTROL * super::view::MENU_ROWS);
    }
    if matches!(menu, Menu::Position | Menu::CategoryGroup) {
        select = select.max_height(size::CONTROL * super::view::MENU_ROWS);
    }
    select.into_any_element()
}

/// A text field, mono when it holds a pattern or a number.
fn field(view: &RulesView, input: Input, mono: bool) -> AnyElement {
    match view.input(input) {
        Some(state) => text_input(state)
            .w_full()
            .when(mono, |this| this.font_family(font::MONO))
            .into_any_element(),
        None => div().into_any_element(),
    }
}

/// The ghost "+ Add action" / "+ Add line" (24px, radius 6, 8px padding
/// pulled 8px left, a 12px plus, caption 500 `text-secondary`), as M3.9's
/// "Add line".
fn ghost(
    id: &'static str,
    label: &'static str,
    focus: &gpui_kit::FocusHandle,
    open: bool,
    window: &Window,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .track_focus(focus)
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1_5)
        .h(size::CONTROL_SM)
        .px(spacing::S2)
        .ml(-spacing::S2)
        .rounded(radius::MD)
        .text_color(color::TEXT_SECONDARY)
        .when(open, |this| this.bg(color::CONTROL_ACTIVE))
        .hover(|style| style.bg(color::HOVER))
        .active(|style| style.bg(color::CONTROL_ACTIVE))
        .child(Glyph::Plus.render().text_color(color::TEXT_SECONDARY))
        .child(
            div()
                .whitespace_nowrap()
                .text_role(typography::CAPTION)
                .font_weight(weight::MEDIUM)
                .child(label),
        )
        .focus_visible_ring(focus, FocusRingPlacement::Outside, window)
}

// A rule -------------------------------------------------------------------------

fn rule_sections(
    view: &RulesView,
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> Vec<AnyElement> {
    let rules = entity.read(cx);
    let Some(form) = rules.form().cloned() else {
        return nothing(NO_RULE);
    };
    let head = rule_header(rules);
    let data = rules.data().clone();
    let is_new = rules.is_new();
    let summary = rules.selected_rule().cloned();
    let apply = rules.apply().cloned();
    let error = rules.form_error();
    let touching = rules.touching().to_vec();
    let new_position = rules.new_position();
    let today = rules.today();
    let total = data.rules.len();

    let mut sections: Vec<AnyElement> = head.map(|head| header(window, head)).into_iter().collect();
    if let Some(summary) = &summary
        && let Some(card) = NeverReachedText::of(&data, summary)
    {
        sections.push(never_reached(entity, summary, card, window));
    }

    sections.push(when_section(view, &form, &data, error.as_ref(), cx));
    sections.push(then_section(view, &form, &data, error.as_ref(), window, cx));

    if is_new {
        let overlaps = apply.as_ref().map_or(0, |apply| apply.overlaps.len());
        let position_label = match new_position {
            0 => "First".to_owned(),
            at if at >= total => "Last".to_owned(),
            at => format!("After rule {at}"),
        };
        let field = SelectField::new(
            "rules-position-field",
            format!(
                "{position_label} · rule {} of {}",
                new_position.min(total) + 1,
                total + 1
            ),
        );
        let overlap_words = match overlaps {
            0 => format!(
                "This one overlaps none of your {}.",
                text::count(total, "rule", "rules")
            ),
            1 => "It overlaps 1 of your rules: the first to match a field sets it.".to_owned(),
            n => format!("It overlaps {n} of your rules: the first to match a field sets it."),
        };
        sections.push(
            section(true)
                .gap(spacing::S2)
                .child(section_title_row("Order", None))
                .child(FieldRow::new(
                    "Position",
                    select(view, Menu::Position, "Position", field, cx),
                ))
                .child(fields::caption(format!(
                    "A new rule goes first, so it wins over broader rules. {overlap_words}"
                )))
                .into_any_element(),
        );
    } else if let Some(summary) = &summary {
        sections.push(past_section(entity, &touching, apply.is_some(), window));
        sections.push(rule_footer(entity, summary, &data, today));
    }
    sections
}

/// The rule's header: the rule as title (what's typed, while it isn't
/// valid), what it decides as the figure, and the caption. `None` with no
/// rule in the inspector.
pub fn rule_header(rules: &Rules) -> Option<HeaderText> {
    let form = rules.form()?;
    let data = rules.data();
    let title = match form.definition(data.currency(form.account)) {
        Ok(rule) => vec![
            Part::text(format!("{} ", text::condition(&rule))),
            Part::mono(text::pattern(&rule)).weight(weight::MEDIUM),
        ],
        Err(_) if form.text.is_empty() => vec![Part::text("New rule")],
        Err(_) => vec![
            Part::text(format!(
                "{} {} ",
                text::field(form.field),
                text::test(form.kind)
            )),
            Part::mono(text::plain(&form.text, text::PATTERN_SHOWN)).weight(weight::MEDIUM),
        ],
    };
    let total = data.rules.len();
    if rules.is_new() {
        let rows = rules.apply().map_or(0, |apply| apply.rows.len());
        let lead = if rules.new_origin() == RuleOrigin::Edit {
            "earlier transactions match"
        } else {
            "transactions match"
        };
        return Some(HeaderText {
            title,
            figure: Some((rows.to_string(), false)),
            caption: Some(format!("{lead} · new rule, not saved yet")),
        });
    }
    let summary = rules.selected_rule()?;
    let position = data.position(summary.rule.id()).map_or(0, |at| at + 1);
    let caption = if summary.never_reached() {
        format!("transactions decided by this rule · rule {position} of {total}")
    } else {
        let last = summary
            .last
            .map(|date| format!(" · last on {}", text::date(date, rules.today())))
            .unwrap_or_default();
        format!("transactions match{last} · rule {position} of {total}")
    };
    Some(HeaderText {
        title,
        figure: Some((summary.decides.to_string(), summary.never_reached())),
        caption: Some(caption),
    })
}

/// The Never reached card in words: what `never_reached` draws and the
/// test-only `describe()` reads.
pub struct NeverReachedText {
    /// "Rule 2, Merchant contains `UBER`, comes first and catches all 9
    /// transactions this rule would match."
    pub body: Vec<Part>,
    pub caption: String,
    /// The rule above it, 1 first, and its ID.
    pub position: usize,
    pub above: orre_core::id::RuleId,
}

/// The card's buttons.
pub fn move_above(position: usize) -> String {
    format!("Move above rule {position}")
}

pub fn show_rule(position: usize) -> String {
    format!("Show rule {position}")
}

impl NeverReachedText {
    /// The card for `summary`, if it's never reached and the rule that
    /// catches its matches is there.
    pub fn of(
        data: &super::store::RulesData,
        summary: &orre_core::rule::RuleSummary,
    ) -> Option<Self> {
        let catch = summary.caught_by.filter(|_| summary.never_reached())?;
        let above = data.rule(catch.rule)?;
        let rows = catch.rows;
        let above_rule = above.rule.definition();
        let position = data.position(above.rule.id()).map_or(0, |at| at + 1);
        let all = if rows >= summary.matches {
            format!(
                "all {}",
                text::count(summary.matches, "transaction", "transactions")
            )
        } else {
            format!("{rows} of the {} transactions", summary.matches)
        };
        Some(Self {
            body: vec![
                Part::text(format!("Rule {position}, {} ", text::condition(above_rule))),
                Part::mono(text::pattern(above_rule)),
                Part::text(format!(
                    ", comes first and catches {all} this rule would match."
                )),
            ],
            caption: format!(
                "Move this rule above rule {position} to let it decide them, or delete it."
            ),
            position,
            above: above.rule.id(),
        })
    }
}

fn never_reached(
    entity: &Entity<Rules>,
    summary: &orre_core::rule::RuleSummary,
    card: NeverReachedText,
    window: &Window,
) -> AnyElement {
    let NeverReachedText {
        body,
        caption: card_caption,
        position,
        above: above_id,
    } = card;
    let (moved, shown) = (entity.clone(), entity.clone());
    let (id, target) = (summary.rule.id(), position - 1);
    section(true)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(spacing::S2)
                .p(spacing::S3)
                .rounded(radius::SURFACE)
                .bg(color::FLOAT)
                .border(size::HAIRLINE)
                .border_color(color::HAIRLINE)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing::S2)
                        .child(Glyph::Alert.render().text_color(color::WARNING))
                        .child(
                            div()
                                .text_role(typography::SECTION_TITLE)
                                .text_color(color::TEXT)
                                .child("Never reached"),
                        ),
                )
                .child(div().text_role(typography::BODY).child(rich(
                    window,
                    typography::BODY,
                    color::TEXT_STRONG,
                    body,
                )))
                .child(fields::caption(card_caption))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing::S1_5)
                        .pt(spacing::S1)
                        .child(
                            Button::new("rules-move-above", move_above(position))
                                .with_variant(ButtonVariant::Outline)
                                .on_click(move |_, _, cx| {
                                    moved.update(cx, |rules, cx| rules.move_rule(id, target, cx))
                                }),
                        )
                        .child(
                            Button::new("rules-show-above", show_rule(position))
                                .with_variant(ButtonVariant::Outline)
                                .on_click(move |_, _, cx| {
                                    shown.update(cx, |rules, cx| {
                                        rules.select_rule(Some(above_id), cx)
                                    })
                                }),
                        ),
                ),
        )
        .into_any_element()
}

fn when_section(
    view: &RulesView,
    form: &super::form::RuleForm,
    data: &super::store::RulesData,
    error: Option<&FormError>,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let account = form.account.and_then(|id| data.account(id)).map_or_else(
        || "Any account".to_owned(),
        |account| text::plain(&account.name, text::NAME_SHOWN),
    );
    let pattern_error = error.filter(|error| error.is_pattern() && !form.text.is_empty());
    let pattern_label = if form.kind == orre_core::rule::MatchKind::Regex {
        "Pattern"
    } else {
        "Text"
    };
    let text_field = match view.input(Input::Pattern) {
        Some(state) => text_input(state)
            .w_full()
            .font_family(font::MONO)
            .when(pattern_error.is_some(), |this| {
                this.border(size::HAIRLINE)
                    .border_color(color::DANGER)
                    .suffix(Glyph::Alert.render().text_color(color::DANGER))
            })
            .into_any_element(),
        None => div().into_any_element(),
    };
    let mut section = section(true)
        .gap(spacing::S2)
        .child(section_title_row("When", None))
        .child(FieldRow::new(
            "Field",
            select(
                view,
                Menu::Field,
                "Field",
                SelectField::new("rules-field-field", text::field(form.field)),
                cx,
            ),
        ))
        .child(FieldRow::new(
            "Test",
            select(
                view,
                Menu::Test,
                "Test",
                SelectField::new("rules-test-field", text::test(form.kind)),
                cx,
            ),
        ))
        .child(FieldRow::new(pattern_label, text_field));
    if let Some(FormError::Pattern(problem)) = pattern_error {
        section = section.child(
            div()
                .pl(size::FIELD_LABEL + spacing::S2)
                .child(error_caption(text::pattern_error(problem))),
        );
    }
    section = section
        .child(FieldRow::new(
            "Account",
            select(
                view,
                Menu::Account,
                "Account",
                SelectField::new("rules-account-field", account),
                cx,
            ),
        ))
        .child(FieldRow::new(
            "Amount",
            select(
                view,
                Menu::Amount,
                "Amount",
                SelectField::new("rules-amount-field", amount_label(form.amount.choice)),
                cx,
            ),
        ));
    if matches!(
        form.amount.choice,
        AmountChoice::Over | AmountChoice::Under | AmountChoice::Between
    ) {
        let direction = Segmented::new(
            "rules-amount-direction",
            match form.amount.direction {
                Direction::Out => "out",
                Direction::In => "in",
            },
        )
        .segment("out", "Out")
        .segment("in", "In")
        .on_choose({
            let view = cx.entity().downgrade();
            move |key, _, cx| {
                let direction = if key.as_ref() == "in" {
                    Direction::In
                } else {
                    Direction::Out
                };
                let _ = view.update(cx, |this, cx| this.set_direction(direction, cx));
            }
        });
        let currency = data.currency(form.account);
        let symbol = crate::format::currency_prefix(currency);
        let amount = |input: Input| match view.input(input) {
            Some(state) => crate::ui::field::amount_input(state, symbol.clone(), cx)
                .flex_1()
                .min_w_0()
                .into_any_element(),
            None => div().into_any_element(),
        };
        let mut fields_row = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(spacing::S1_5)
            .child(direction)
            .child(amount(Input::Low));
        if form.amount.choice == AmountChoice::Between {
            fields_row = fields_row
                .child(
                    div()
                        .flex_none()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_MUTED)
                        .child("and"),
                )
                .child(amount(Input::High));
        }
        section = section.child(FieldRow::new("", fields_row));
    }
    if form.amount.choice == AmountChoice::Custom {
        section = section.child(fields::caption(
            "This rule has an amount range the choices above can’t say. Choosing one replaces \
             it.",
        ));
    }
    if let Some(FormError::Amount(problem)) = error {
        section = section.child(
            div()
                .pl(size::FIELD_LABEL + spacing::S2)
                .child(error_caption(problem.clone())),
        );
    }
    let caption_words = match form.kind {
        orre_core::rule::MatchKind::Regex => {
            "Matches and the preview update as you type once the pattern is valid. Matching \
             always finishes quickly, whatever the pattern."
        }
        _ => "Matching ignores case. Merchant is the cleaned name, before any rename.",
    };
    section
        .child(fields::caption(caption_words))
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn then_section(
    view: &RulesView,
    form: &super::form::RuleForm,
    data: &super::store::RulesData,
    error: Option<&FormError>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let mut section = section(true)
        .gap(spacing::S2)
        .child(section_title_row("Then", None));
    for action in &form.actions {
        let kind = action.kind();
        let control: AnyElement = match action {
            ActionForm::Category(value) => {
                let field = match value {
                    Some(key) => SelectField::new(
                        "rules-category-field",
                        data.category(key)
                            .map_or_else(|| key.as_str().to_owned(), |c| c.label.clone()),
                    )
                    .marker(super::table::marker(data, key)),
                    None => SelectField::new("rules-category-field", "Choose a category")
                        .marker(Marker::None)
                        .placeholder(true),
                };
                select(view, Menu::Category, "Category", field, cx)
            }
            ActionForm::Merchant(_) => field(view, Input::Merchant, false),
            ActionForm::Note(_) => field(view, Input::Note, false),
            ActionForm::Share(_) => match view.input(Input::Share) {
                Some(state) => text_input(state)
                    .w_full()
                    .font_family(font::MONO)
                    .suffix(
                        div()
                            .text_role(typography::MONO_AMOUNT)
                            .text_color(color::TEXT_FAINT)
                            .child("%"),
                    )
                    .into_any_element(),
                None => div().into_any_element(),
            },
            ActionForm::Exclude(reason) => {
                let field = match reason {
                    Some(reason) => SelectField::new("rules-exclude-field", text::reason(*reason)),
                    None => {
                        SelectField::new("rules-exclude-field", "Choose a reason").placeholder(true)
                    }
                };
                select(view, Menu::Exclude, "Reason", field, cx)
            }
            ActionForm::Tags(tags) => tag_control(view, tags, cx),
            ActionForm::Split(lines) => split_control(view, lines, data, window, cx),
        };
        let remove = IconButton::new(
            SharedString::from(format!("rules-remove-{}", kind.as_str())),
            Glyph::Reject,
            format!("Remove {}", text::action_label(kind)),
        )
        .with_size(IconButtonSize::Small)
        .on_click(cx.listener(move |this, _, _, cx| this.remove_action(kind, cx)));
        let tall = matches!(action, ActionForm::Split(_) | ActionForm::Tags(_));
        section = section.child(
            div()
                .flex()
                .flex_none()
                .gap(spacing::S2)
                .w_full()
                .map(|this| {
                    if tall {
                        this.items_start()
                    } else {
                        this.items_center().h(size::CONTROL)
                    }
                })
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .h(size::CONTROL)
                        .w(size::FIELD_LABEL)
                        .whitespace_nowrap()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_MUTED)
                        .child(text::action_label(kind)),
                )
                .child(div().flex().flex_1().min_w_0().child(control))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .h(size::CONTROL)
                        .child(remove),
                ),
        );
    }
    let open = view.menu == Some(Menu::AddAction);
    let add = ghost(
        "rules-add-action",
        "Add action",
        &view.add_action_focus,
        open,
        window,
    )
    .on_click(cx.listener(RulesView::keyboard_open(Menu::AddAction)));
    let add = div()
        .relative()
        .flex_none()
        .child(view.popup_trigger(Menu::AddAction, add, cx))
        .when(open, |this| {
            this.child(menu::below(
                size::CONTROL_SM + spacing::S1,
                view.popup_list(Menu::AddAction, cx),
            ))
        });
    section = section.child(div().flex().child(add));
    match error {
        Some(FormError::Action { message, .. }) => {
            section = section.child(error_caption(message.clone()));
        }
        Some(FormError::NoActions) => {
            section = section.child(fields::caption(
                "Add what the rule does: a category, a name, a tag, a note, a share, a split or \
                 an exclusion.",
            ));
        }
        _ => {}
    }
    if form.actions.len() > 1 {
        section = section.child(fields::caption(
            "Each kind of action once per rule. A rule sets either Category or Split, not \
             both: a split sets a category on each line.",
        ));
    }
    section.into_any_element()
}

/// The Tag action: the tags as pills, then "+ Tag" opening the tag menu
/// (D6.3's tag row).
fn tag_control(view: &RulesView, tags: &[String], cx: &mut Context<RulesView>) -> AnyElement {
    let private = |name: &str| {
        view.rules()
            .read(cx)
            .data()
            .tags
            .iter()
            .any(|tag| tag.private && tag.name.eq_ignore_ascii_case(name))
    };
    let pills: Vec<AnyElement> = tags
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let removed = name.clone();
            TagPill::new(
                SharedString::from(format!("rules-tag-{index}")),
                text::plain(name, text::NAME_SHOWN),
            )
            .private(private(name))
            .on_remove(cx.listener(move |this, _, _, cx| {
                let removed = removed.clone();
                this.rules().clone().update(cx, |rules, cx| {
                    rules.edit(
                        true,
                        |form| {
                            if let Some(ActionForm::Tags(tags)) =
                                form.action_mut(ActionKind::AddTag)
                            {
                                tags.retain(|tag| *tag != removed);
                            }
                        },
                        cx,
                    )
                });
            }))
            .into_any_element()
        })
        .collect();
    let open = view.menu == Some(Menu::Tags);
    let add = AddTagPill::new("rules-add-tag", &view.tag_focus)
        .open(open)
        .on_click(cx.listener(RulesView::keyboard_open(Menu::Tags)));
    let add = div()
        .relative()
        .flex_none()
        .child(view.popup_trigger(Menu::Tags, add, cx))
        .when(open, |this| {
            this.child(menu::below(
                size::CONTROL_SM + spacing::S1,
                view.popup_list(Menu::Tags, cx),
            ))
        });
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .flex_wrap()
        .items_center()
        .gap(spacing::S1)
        .min_h(size::CONTROL)
        .py(spacing::S0_5)
        .children(pills)
        .child(add)
        .into_any_element()
}

/// The Split action: D6.3's lines without amounts, each a category and a
/// share in percent, "+ Add line" and the total.
fn split_control(
    view: &RulesView,
    lines: &[super::form::SplitLineForm],
    data: &super::store::RulesData,
    window: &Window,
    cx: &mut Context<RulesView>,
) -> AnyElement {
    let rows: Vec<AnyElement> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let field = match &line.category {
                Some(key) => SelectField::new(
                    SharedString::from(format!("rules-split-category-{index}")),
                    data.category(key)
                        .map_or_else(|| key.as_str().to_owned(), |c| c.label.clone()),
                )
                .marker(super::table::marker(data, key)),
                None => SelectField::new(
                    SharedString::from(format!("rules-split-category-{index}")),
                    "Category",
                )
                .marker(Marker::None)
                .placeholder(true),
            };
            let percent = match view.input(Input::SplitPercent(index)) {
                Some(state) => text_input(state)
                    .flex_none()
                    .w(size::CONTROL_XL + spacing::S6)
                    .font_family(font::MONO)
                    .text_right()
                    .suffix(
                        div()
                            .text_role(typography::MONO_AMOUNT)
                            .text_color(color::TEXT_FAINT)
                            .child("%"),
                    )
                    .into_any_element(),
                None => div().into_any_element(),
            };
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S1_5)
                .h(size::CONTROL)
                .child(div().flex().flex_1().min_w_0().child(select(
                    view,
                    Menu::SplitLine(index),
                    "Line category",
                    field,
                    cx,
                )))
                .child(percent)
                .when(lines.len() > 2, |this| {
                    this.child(
                        IconButton::new(
                            SharedString::from(format!("rules-split-remove-{index}")),
                            Glyph::Close,
                            format!("Remove line {}", index + 1),
                        )
                        .with_size(IconButtonSize::Small)
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.remove_split_line(index, window, cx),
                        )),
                    )
                })
                .into_any_element()
        })
        .collect();
    let total: u64 = lines
        .iter()
        .filter_map(|line| super::form::parse_percent(&line.percent))
        .sum();
    let balanced = total == 10_000;
    let focus = view.add_action_focus.clone();
    let add_line = ghost("rules-split-add-line", "Add line", &focus, false, window)
        .on_click(cx.listener(|this, _, window, cx| this.add_split_line(window, cx)));
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(spacing::S1_5)
        .children(rows)
        .child(
            div()
                .flex()
                .items_center()
                .h(size::CONTROL_SM)
                .child(add_line)
                .child(div().flex_1())
                .child(
                    div()
                        .text_role(typography::MONO_VALUE)
                        .text_color(if balanced {
                            color::TEXT_MUTED
                        } else {
                            color::WARNING
                        })
                        .child(format!(
                            "{}%",
                            super::form::percent_text(
                                orre_core::rule::Share::new(total.min(10_000))
                                    .unwrap_or(orre_core::rule::Share::WHOLE)
                            )
                        )),
                ),
        )
        .into_any_element()
}

fn past_section(
    entity: &Entity<Rules>,
    touching: &[orre_core::inbox::Suggestion],
    applying: bool,
    window: &Window,
) -> AnyElement {
    let apply = {
        let entity = entity.clone();
        Button::new("rules-apply", APPLY_TO_PAST)
            .with_variant(ButtonVariant::Outline)
            .disabled(applying)
            .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.apply_selected(cx)))
    };
    let mut section = section(true)
        .gap(spacing::S2)
        .child(section_title_row("Past transactions", None))
        .child(fields::caption(
            "Changes to this rule apply to new imports. To change past transactions too, apply \
             it: you see every change before anything is saved.",
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap(spacing::S1_5)
                .pt(spacing::S1)
                .child(apply),
        );
    if let Some(suggestion) = touching.first() {
        let id = suggestion.id;
        let review = entity.clone();
        section = section.child(
            div()
                .flex()
                .items_start()
                .gap(spacing::S1_5)
                .pt(spacing::S1)
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .h(crate::theme::tokens::leading::CAPTION)
                        .child(
                            Glyph::Sparkle
                                .render()
                                .size(size::ICON_XS)
                                .text_color(color::MOLDAVITE),
                        ),
                )
                .child(
                    div()
                        .id("rules-review-in-inbox")
                        .flex_1()
                        .min_w_0()
                        .text_role(typography::CAPTION)
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            review.update(cx, |rules, cx| rules.review_suggestion(id, cx))
                        })
                        .child(rich(
                            window,
                            typography::CAPTION,
                            color::MOLDAVITE_TEXT_MUTED,
                            touching_parts(suggestion),
                        )),
                ),
        );
    }
    section.into_any_element()
}

/// The Past transactions section's button.
pub const APPLY_TO_PAST: &str = "Apply to past transactions…";
/// The footers' and the change's buttons.
pub const DELETE_RULE: &str = "Delete rule…";
pub const DELETE_NAME: &str = "Delete name…";
pub const UNDO_CHANGE: &str = "Undo this change…";
pub const DELETE_CATEGORY: &str = "Delete category…";
pub const CREATE_CATEGORY: &str = "Create category";

/// The line under Apply when an agent's pending rule would take over this
/// rule's rows: "claude-code suggests a rule for `UBER` above this one.
/// Review in the Inbox". The pattern goes through [`text::pattern`].
pub fn touching_parts(suggestion: &orre_core::inbox::Suggestion) -> Vec<Part> {
    let pattern = match &suggestion.payload {
        orre_core::inbox::Payload::Rule(rule) => text::pattern(rule),
        _ => String::new(),
    };
    let (client, _) = crate::inbox::text::source_name(&suggestion.source_client);
    vec![
        Part::text(format!("{client} suggests a rule for ")),
        Part::mono(pattern).color(color::MOLDAVITE_TEXT),
        Part::text(" above this one. "),
        Part::text("Review in the Inbox").color(color::MOLDAVITE_TEXT),
    ]
}

/// The rule's footer: "Created on " and the date, if it's known.
pub fn rule_created(
    summary: &orre_core::rule::RuleSummary,
    data: &super::store::RulesData,
    today: chrono::NaiveDate,
) -> (&'static str, Option<String>) {
    let lead = match summary.rule.origin() {
        Some(RuleOrigin::Edit) => "Created from an edit on ",
        Some(RuleOrigin::Inbox) => "Created from a suggestion on ",
        _ => "Created on ",
    };
    let created = data
        .rule_created
        .get(&summary.rule.id())
        .map(|date| text::date(*date, today));
    (lead, created)
}

fn rule_footer(
    entity: &Entity<Rules>,
    summary: &orre_core::rule::RuleSummary,
    data: &super::store::RulesData,
    today: chrono::NaiveDate,
) -> AnyElement {
    let (lead, created) = rule_created(summary, data, today);
    let delete = {
        let entity = entity.clone();
        Button::new("rules-delete", DELETE_RULE)
            .with_variant(ButtonVariant::DestructiveCompact)
            .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.request_delete_rule(cx)))
    };
    footer(lead, created, delete.into_any_element())
}

/// The footer: "Created on 12 Mar" (the date in mono) and a destructive
/// action.
fn footer(lead: &'static str, date: Option<String>, action: AnyElement) -> AnyElement {
    section(true)
        .flex_row()
        .items_center()
        .justify_between()
        .gap(spacing::S2)
        .child(
            div()
                .flex()
                .items_center()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .when_some(date, |this, date| {
                    this.child(lead)
                        .child(div().font_family(font::MONO).child(date))
                }),
        )
        .child(action)
        .into_any_element()
}

// A merchant name ----------------------------------------------------------------

/// The name's header: the name it shows as title, its matches as the
/// figure. `None` with no name in the inspector.
pub fn name_header(rules: &Rules) -> Option<HeaderText> {
    let selection = rules.selected_name()?;
    let form = rules.name_form()?;
    let title = vec![Part::text(if form.name.trim().is_empty() {
        "New name".to_owned()
    } else {
        text::plain(&form.name, text::NAME_SHOWN)
    })];
    let summary = match selection {
        NameSelection::Stored(id) => rules.data().name(id),
        NameSelection::New => None,
    };
    Some(match summary {
        Some(summary) => {
            let last = summary
                .last
                .map(|date| format!(" · last on {}", text::date(date, rules.today())))
                .unwrap_or_default();
            HeaderText {
                title,
                figure: Some((summary.matches.to_string(), false)),
                caption: Some(format!("{}{last}", name_matches_caption(summary.matches))),
            }
        }
        None => HeaderText {
            title,
            figure: None,
            caption: Some("new name, saved once both fields are filled in".to_owned()),
        },
    })
}

/// The caption under the name's match count: "transaction shows this name"
/// or "transactions show this name".
fn name_matches_caption(matches: usize) -> &'static str {
    if matches == 1 {
        "transaction shows this name"
    } else {
        "transactions show this name"
    }
}

/// The name's footer date: "Added on " it.
pub fn name_added(
    data: &super::store::RulesData,
    summary: &orre_core::merchant::AliasSummary,
    today: chrono::NaiveDate,
) -> Option<String> {
    data.name_added
        .get(&summary.alias.id())
        .map(|date| text::date(*date, today))
}

fn name_sections(
    view: &RulesView,
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> Vec<AnyElement> {
    let rules = entity.read(cx);
    let (Some(selection), Some(form), Some(head)) = (
        rules.selected_name(),
        rules.name_form().cloned(),
        name_header(rules),
    ) else {
        return nothing(NO_NAME);
    };
    let data = rules.data().clone();
    let today = rules.today();
    let error = rules.name_error();
    let summary = match selection {
        NameSelection::Stored(id) => data.name(id).cloned(),
        NameSelection::New => None,
    };
    let mut sections = vec![header(window, head)];
    let pattern_error = match &error {
        Some(NameError::Pattern(Some(problem))) if !form.pattern.is_empty() => {
            Some(problem.clone())
        }
        _ => None,
    };
    let text_field = match view.input(Input::NamePattern) {
        Some(state) => text_input(state)
            .w_full()
            .font_family(font::MONO)
            .when(pattern_error.is_some(), |this| {
                this.border(size::HAIRLINE)
                    .border_color(color::DANGER)
                    .suffix(Glyph::Alert.render().text_color(color::DANGER))
            })
            .into_any_element(),
        None => div().into_any_element(),
    };
    let mut matching = section(true)
        .gap(spacing::S2)
        .child(section_title_row("Match", None))
        .child(FieldRow::new(
            "Test",
            select(
                view,
                Menu::NameTest,
                "Test",
                SelectField::new("rules-name-test-field", text::alias_test(form.kind)),
                cx,
            ),
        ))
        .child(FieldRow::new("Text", text_field));
    if let Some(problem) = pattern_error {
        matching = matching.child(
            div()
                .pl(size::FIELD_LABEL + spacing::S2)
                .child(error_caption(text::pattern_error(&problem))),
        );
    }
    matching = matching
        .child(FieldRow::new(
            "Shows as",
            field(view, Input::NameShows, false),
        ))
        .child(fields::caption(
            "Matches the bank’s merchant text after clean-up, ignoring case. A name you typed on \
             a transaction isn’t replaced.",
        ));
    sections.push(matching.into_any_element());

    if let Some(summary) = &summary {
        const SHOWN: usize = 3;
        let caught: Vec<AnyElement> = summary
            .catches
            .iter()
            .take(SHOWN)
            .map(|caught| {
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(spacing::S2)
                    .h(size::CONTROL_SM)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_role(typography::MONO_VALUE)
                            .text_color(color::TEXT_SECONDARY)
                            .child(text::plain(&caught.text, text::NAME_SHOWN)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_role(typography::MONO_VALUE)
                            .text_color(color::TEXT_MUTED)
                            .child(text::date(caught.last, today)),
                    )
                    .into_any_element()
            })
            .collect();
        let more = summary.catches.len().saturating_sub(SHOWN);
        let mut catches = section(true)
            .gap(spacing::S2)
            .child(section_title_row("Bank text it catches", None));
        if caught.is_empty() {
            catches = catches.child(fields::caption("No transactions show this name yet."));
        } else {
            catches = catches.child(div().flex().flex_col().gap(spacing::S1).children(caught));
        }
        if more > 0 {
            catches = catches.child(
                div()
                    .pt(spacing::S1)
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child(format!("and {more} more")),
            );
        }
        sections.push(catches.into_any_element());
        let added = name_added(&data, summary, today);
        let delete = {
            let entity = entity.clone();
            Button::new("rules-delete-name", DELETE_NAME)
                .with_variant(ButtonVariant::DestructiveCompact)
                .on_click(move |_, _, cx| {
                    entity.update(cx, |rules, cx| rules.request_delete_name(cx))
                })
        };
        sections.push(footer("Added on ", added, delete.into_any_element()));
    }
    sections
}

// A change -----------------------------------------------------------------------

/// The change's header: what it did as title, the transactions it changed
/// as the figure, and when (wall-clock time). `None` with no change
/// selected.
pub fn change_header(rules: &Rules) -> Option<HeaderText> {
    let entry = rules.selected_batch()?;
    let data = rules.data();
    let made = data.history_at.get(&entry.batch.id).map(|(made, _)| *made);
    Some(HeaderText {
        title: text::change_parts(entry, data),
        figure: Some((entry.batch.transactions.to_string(), false)),
        caption: Some(match made {
            Some(at) => format!(
                "transactions changed · {}",
                text::when_at(at, rules.today())
            ),
            None => "transactions changed".to_owned(),
        }),
    })
}

fn change_sections(
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> Vec<AnyElement> {
    let rules = entity.read(cx);
    let (Some(entry), Some(head)) = (rules.selected_batch(), change_header(rules)) else {
        return nothing(NO_CHANGE);
    };
    let data = rules.data();
    let today = rules.today();
    let undone = entry.batch.status == BatchStatus::Undone;
    let undone_at = data
        .history_at
        .get(&entry.batch.id)
        .and_then(|(_, undone)| *undone);
    let mut sections = vec![header(window, head)];
    const SHOWN: usize = 12;
    let currency = data.currency(None);
    let items: Vec<AnyElement> = rules
        .batch_items()
        .iter()
        .take(SHOWN)
        .map(|item| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S2)
                .h(size::CONTROL_SM)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_SECONDARY)
                        .child(text::plain(&item.name, text::NAME_SHOWN)),
                )
                .child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_role(typography::CAPTION)
                        .child(rich(
                            window,
                            typography::CAPTION,
                            color::TEXT_MUTED,
                            vec![
                                Part::text(format!(
                                    "{} → ",
                                    text::shown(&item.old, data, currency)
                                )),
                                Part::text(text::shown(&item.new, data, currency))
                                    .color(color::TEXT),
                            ],
                        )),
                )
                .into_any_element()
        })
        .collect();
    let more = rules.batch_items().len().saturating_sub(SHOWN);
    let mut changed = section(true)
        .gap(spacing::S2)
        .child(section_title_row("What changed", None))
        .child(div().flex().flex_col().gap(spacing::S1).children(items));
    if more > 0 {
        changed = changed.child(
            div()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(format!("and {more} more")),
        );
    }
    if undone {
        let when = undone_at.map_or(String::new(), |at| {
            format!(" on {}", text::when_at(at, today))
        });
        changed = changed.child(fields::caption(format!(
            "Undone{when}. Each value went back where it still held this change’s."
        )));
    } else {
        let rule_words = if entry.batch.kind == orre_core::change::ChangeKind::RuleApply {
            " The rule itself stays."
        } else {
            ""
        };
        let undo = {
            let entity = entity.clone();
            Button::new("rules-undo-change", UNDO_CHANGE)
                .with_variant(ButtonVariant::Outline)
                .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.request_undo(cx)))
        };
        changed = changed
            .child(fields::caption(format!(
                "Undo puts each one back to the value it had before. A transaction you have \
                 edited since keeps your edit.{rule_words}"
            )))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::S1_5)
                    .pt(spacing::S1)
                    .child(undo),
            );
    }
    sections.push(changed.into_any_element());
    sections
}

// A category ---------------------------------------------------------------------

/// The Name field, in a 1px `danger` border with the `alert` icon when
/// `error` (the pattern field's error state).
fn category_name_field(view: &RulesView, error: bool) -> AnyElement {
    match view.input(Input::CategoryName) {
        Some(state) => text_input(state)
            .w_full()
            .when(error, |this| {
                this.border(size::HAIRLINE)
                    .border_color(color::DANGER)
                    .suffix(Glyph::Alert.render().text_color(color::DANGER))
            })
            .into_any_element(),
        None => div().into_any_element(),
    }
}

/// The error under a field, lined up with the controls.
fn under_field(words: &'static str) -> AnyElement {
    div()
        .pl(size::FIELD_LABEL + spacing::S2)
        .child(error_caption(words))
        .into_any_element()
}

fn category_sections(
    view: &RulesView,
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> Vec<AnyElement> {
    match entity.read(cx).selected_category() {
        None => nothing(NO_CATEGORY),
        Some(CategorySelection::New) => new_category_sections(view, entity, window, cx),
        Some(CategorySelection::Stored(_)) => stored_category_sections(view, entity, window, cx),
    }
}

/// The stored category's header: its label as title, what's filed under
/// it as the figure (faint at 0), where it is and whether it's hidden.
/// `None` unless a stored category is selected.
pub fn stored_category_header(rules: &Rules) -> Option<HeaderText> {
    let (summary, group) = rules.selected_category_entry()?;
    let group = &group.group.category;
    let category = &summary.category;
    let is_group = category.is_group();
    let group_hidden = !is_group && group.is_hidden();
    let rows = super::table::filed(&summary.usage);
    let place = if is_group {
        format!(
            "transactions · a group of {}",
            text::count(
                subcategories(rules, summary),
                "subcategory",
                "subcategories"
            )
        )
    } else {
        format!("transactions · in {}", group.label())
    };
    let hidden_words = if category.is_hidden() || group_hidden {
        " · hidden"
    } else {
        ""
    };
    Some(HeaderText {
        title: vec![Part::text(category.label().to_owned())],
        figure: Some((rows.to_string(), rows == 0)),
        caption: Some(format!("{place}{hidden_words}")),
    })
}

fn subcategories(rules: &Rules, summary: &orre_core::category::CategorySummary) -> usize {
    rules
        .data()
        .category_group(summary.category.key())
        .map_or(0, |group| group.children.len())
}

/// Whether "Delete category…" is offered: nothing uses it and it has no
/// subcategories.
pub fn deletable(rules: &Rules, summary: &orre_core::category::CategorySummary) -> bool {
    summary.usage.is_unused() && subcategories(rules, summary) == 0
}

/// The In pickers button: "Hide category", or "Show category" for a hidden
/// one.
pub fn hide_label(category: &orre_core::category::Category) -> &'static str {
    if category.is_hidden() {
        "Show category"
    } else {
        "Hide category"
    }
}

fn stored_category_sections(
    view: &RulesView,
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> Vec<AnyElement> {
    let rules = entity.read(cx);
    let (Some((summary, group)), Some(head)) = (
        rules.selected_category_entry(),
        stored_category_header(rules),
    ) else {
        return nothing(CATEGORY_GONE);
    };
    let (summary, group) = (summary.clone(), group.group.category.clone());
    let error = rules.category_error();
    let category = &summary.category;
    let usage = summary.usage;
    let is_group = category.is_group();
    let group_hidden = !is_group && group.is_hidden();
    let deletable = deletable(rules, &summary);
    let mut sections = vec![header(window, head)];

    // Category.
    let mut fields_section = section(true)
        .gap(spacing::S2)
        .child(section_title_row("Category", None))
        .child(FieldRow::new(
            "Name",
            category_name_field(view, error.is_some()),
        ));
    if let Some(error) = &error {
        fields_section = fields_section.child(under_field(error.message()));
    }
    fields_section = fields_section
        .child(FieldRow::new(
            "Group",
            LockedValue::new(if is_group {
                "None: it’s a group".to_owned()
            } else {
                group.label().to_owned()
            }),
        ))
        .child(FieldRow::new(
            "Kind",
            LockedValue::new(text::category_kind(category.kind())),
        ))
        .child(fields::caption(
            "Renaming changes only what you see. Rules, presets and AI clients use the key, which \
             never changes, so a category can’t move to another group or change its kind.",
        ));
    sections.push(fields_section.into_any_element());

    // Used by.
    let count_row = |label: &'static str, count: usize| {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .h(size::CONTROL_SM)
            .child(
                div()
                    .flex_1()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_SECONDARY)
                    .child(label),
            )
            .child(
                div()
                    .flex_none()
                    .text_role(typography::MONO_VALUE)
                    .text_color(if count == 0 {
                        color::TEXT_FAINT
                    } else {
                        color::TEXT
                    })
                    .child(count.to_string()),
            )
    };
    let use_words = if deletable {
        "Nothing uses it, so it can be deleted."
    } else if usage.only_suggestions() {
        "Only pending Inbox suggestions use it. Approve or reject them in the Inbox, then it \
         can be deleted."
    } else if !usage.is_unused() {
        "In use, so it can’t be deleted. Hide it to stop offering it; what’s filed under it \
         stays."
    } else {
        "A group with subcategories can’t be deleted. Delete or hide them first."
    };
    sections.push(
        section(true)
            .gap(spacing::S2)
            .child(section_title_row("Used by", None))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::S1)
                    .child(count_row("Transactions", usage.transactions))
                    .child(count_row("Split lines", usage.split_lines))
                    .child(count_row("Rules", usage.rules))
                    .child(count_row("Budgets", usage.budgets))
                    .child(count_row("Schedules", usage.schedules))
                    .child(count_row("Inbox suggestions", usage.suggestions)),
            )
            .child(fields::caption(use_words))
            .into_any_element(),
    );

    // In pickers.
    let hidden = category.is_hidden();
    let toggle_label = hide_label(category);
    let picker_words = match (hidden, group_hidden) {
        (_, true) => format!(
            "Hidden with its group, {}: it isn’t offered when you choose a category. Show the \
             group to offer it again.",
            group.label()
        ),
        (true, false) => "Hidden: it isn’t offered when you choose a category. Its transactions, \
                          rules and reports stay as they are."
            .to_owned(),
        (false, false) if is_group => "Offered when you choose a category. Hiding it keeps its \
                                       transactions and reports, and hides its subcategories \
                                       with it."
            .to_owned(),
        (false, false) => "Offered when you choose a category. Hiding it keeps its transactions \
                           and reports as they are."
            .to_owned(),
    };
    let toggle = {
        let entity = entity.clone();
        Button::new("rules-category-hide", toggle_label)
            .with_variant(ButtonVariant::Outline)
            .on_click(move |_, _, cx| {
                entity.update(cx, |rules, cx| rules.set_category_hidden(!hidden, cx))
            })
    };
    sections.push(
        section(true)
            .gap(spacing::S2)
            .child(section_title_row("In pickers", None))
            .child(fields::caption(picker_words))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::S1_5)
                    .pt(spacing::S1)
                    .child(toggle),
            )
            .into_any_element(),
    );

    // Footer.
    let action = if deletable {
        Button::new("rules-delete-category", DELETE_CATEGORY)
            .with_variant(ButtonVariant::DestructiveCompact)
            .on_click(cx.listener(|this, _, window, cx| this.request_delete_category(window, cx)))
            .into_any_element()
    } else {
        div().into_any_element()
    };
    sections.push(footer(
        "Key ",
        Some(category.key().as_str().to_owned()),
        action,
    ));
    sections
}

/// The new category's header: the name typed, or "New category".
pub fn new_category_header(form: &super::form::CategoryForm) -> HeaderText {
    let title = if form.name.trim().is_empty() {
        "New category".to_owned()
    } else {
        form.name.trim().to_owned()
    };
    HeaderText {
        title: vec![Part::text(title)],
        figure: None,
        caption: Some("new category, not added yet".to_owned()),
    }
}

fn new_category_sections(
    view: &RulesView,
    entity: &Entity<Rules>,
    window: &mut Window,
    cx: &mut Context<RulesView>,
) -> Vec<AnyElement> {
    let rules = entity.read(cx);
    let Some(form) = rules.category_form().cloned() else {
        return Vec::new();
    };
    let error = rules.category_error();
    let data = rules.data();
    let group = form
        .group
        .as_ref()
        .and_then(|key| data.category_group(key))
        .map(|group| group.group.category.clone());
    let key = (!form.name.trim().is_empty()).then(|| {
        orre_core::category::derive_key(&form.name, group.as_ref().map(|g| g.key()), |key| {
            data.category(key).is_some() || data.retired_keys.contains(key)
        })
    });
    let mut sections = vec![header(window, new_category_header(&form))];

    // A blank name only disables Create; a taken one says so.
    let taken = error == Some(CategoryError::Taken);
    let group_field = match &group {
        Some(group) => SelectField::new("rules-category-group-field", group.label().to_owned())
            .marker(super::table::marker(data, group.key())),
        None => SelectField::new("rules-category-group-field", "New group"),
    };
    let kind_control = match &group {
        Some(group) => LockedValue::new(text::category_kind(group.kind())).into_any_element(),
        None => {
            let view_handle = cx.entity().downgrade();
            let mut control = Segmented::new("rules-category-kind", form.kind.as_str()).fill();
            for kind in text::CATEGORY_KINDS {
                control = control.segment(kind.as_str(), text::category_kind(kind));
            }
            control
                .on_choose(move |key, _, cx| {
                    if let Ok(kind) = key.parse::<orre_core::category::CategoryKind>() {
                        let _ = view_handle.update(cx, |this, cx| this.set_category_kind(kind, cx));
                    }
                })
                .into_any_element()
        }
    };
    let key_words = match &key {
        Some(key) => format!(
            "Its key will be {key}. The key never changes, so rules, presets and AI clients keep \
             working when you rename it."
        ),
        None => "Its key is made from the name when it’s added, and never changes.".to_owned(),
    };
    let kind_words = if group.is_some() {
        " A subcategory has its group’s kind."
    } else {
        " A group’s kind can’t change later."
    };
    let mut fields_section = section(true)
        .gap(spacing::S2)
        .child(section_title_row("Category", None))
        .child(FieldRow::new("Name", category_name_field(view, taken)));
    if taken {
        fields_section = fields_section.child(under_field(CategoryError::Taken.message()));
    }
    fields_section = fields_section
        .child(FieldRow::new(
            "Group",
            select(view, Menu::CategoryGroup, "Group", group_field, cx),
        ))
        .child(FieldRow::new("Kind", kind_control))
        .child(fields::caption(format!("{key_words}{kind_words}")));
    sections.push(fields_section.into_any_element());

    let create = {
        let entity = entity.clone();
        Button::new("rules-create-category", CREATE_CATEGORY)
            .with_variant(ButtonVariant::Primary)
            .disabled(error.is_some())
            .on_click(move |_, _, cx| entity.update(cx, |rules, cx| rules.create_category(cx)))
    };
    sections.push(
        section(true)
            .child(div().flex().items_center().gap(spacing::S1_5).child(create))
            .into_any_element(),
    );
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_names_match_caption_agrees_with_its_count() {
        assert_eq!(name_matches_caption(1), "transaction shows this name");
        assert_eq!(name_matches_caption(3), "transactions show this name");
        assert_eq!(name_matches_caption(0), "transactions show this name");
    }
}
