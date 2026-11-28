//! What accounts management shows, as plain data, for tests (ROADMAP U.3):
//! the accounts panel (net worth, the accounts with their pots, the assets
//! and liabilities, the flagged dots and the `+`s), the account inspector,
//! an asset's or liability's valuations canvas, the undo bar after closing,
//! a dialog, and the panel's keys with whether each does anything now.
//!
//! Compiled for tests and the dev driver only (the `dev-driver` feature,
//! debug builds): it isn't in a release build, and nothing but the tests and
//! the driver can reach it, `orre mcp`
//! included (`docs/architecture/app.md` → "Decision record: views that
//! describe themselves stay out of the release binary").
//!
//! Every string comes from the code the panel, the inspector, the canvas
//! and the dialogs draw with ([`super::panel`], [`super::inspector`],
//! [`super::valuations`], each dialog's `describe()`). Account and pot
//! names are described as they're shown, through [`super::name`] (made
//! plain: a name can come from an import or a preset); the inspector's Name
//! field holds the stored name as typed. Styling (icons, tones, mono runs) is
//! left out, but for an overdrawn balance's `danger`; runs are joined into
//! one string.
//!
//! The panel's cursor is its selection, drawn with the focus ring while the
//! panel has keyboard focus; whether it had is described as last drawn,
//! since the panel's focus is the shell's.
//!
//! What it leaves out: CATEGORIES (the Transactions register's counts,
//! which the shell keeps), the valuations chart (its scale has its own
//! tests), fixed explanatory captions, an open select's menu, and the
//! pointer's hover.

use gpui_kit::{AnyView, App};

pub use crate::ui::describe::{Action, Dialog, Header, Inspector, Section};

use super::add_balance::AddBalanceDialog;
use super::close::CloseAccountDialog;
use super::delete::DeleteAccountDialog;
use super::inspector::{
    self, AccountInspector, BalanceText, HeaderCaption, balance_rows, balances_words,
    header_caption, header_figure, import_text, kind_label, pot_balance, pots_total, preset_value,
};
use super::new_account::NewAccountDialog;
use super::valuations::{self, RowText, Words};
use super::{Accounts, panel};
use orre_core::account::AccountKind;
use orre_core::reconciliation::Status;
use orre_core::reporting::account_tree::AccountNode;

/// What accounts management shows ([`Accounts::describe`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountsDescription {
    /// Net worth, one line per currency ("—" with no accounts).
    pub net_worth: Vec<String>,
    /// ACCOUNTS, then ASSETS AND LIABILITIES.
    pub sections: Vec<PanelSection>,
    /// Why the accounts couldn't be read, under ACCOUNTS.
    pub error: Option<String>,
    /// The account inspector, while it's shown.
    pub inspector: Option<Inspector>,
    /// The valuations canvas, while an asset or liability is selected.
    pub valuations: Option<ValuationsDescription>,
    /// The bar after closing an account: what it says, and Undo.
    pub undo_bar: Option<(String, Vec<Action>)>,
    /// The context menu open on a panel row: each group's items, as a label
    /// and, for one that can't be chosen, its reason.
    pub context_menu: Option<Vec<Vec<ContextItem>>>,
    /// The dialog open over the window, when it's one of accounts'
    /// (`AccountsDescription::with_dialog`).
    pub dialog: Option<Dialog>,
    /// The panel's keys: ↑↓ through the accounts, Escape, Enter and ⌘N.
    pub keys: Vec<Action>,
}

/// A context menu item: its label and, when it can't be chosen, why.
pub type ContextItem = (String, Option<String>);

/// A section of the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelSection {
    pub title: &'static str,
    /// The `+` on its label.
    pub add: Action,
    pub rows: Vec<AccountRow>,
    /// What it says with no rows.
    pub note: Option<&'static str>,
}

/// An account's row in the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountRow {
    pub name: String,
    /// Its latest balance, or "No balance".
    pub balance: String,
    /// The balance is `danger`: an account that can be overdrawn is.
    pub overdrawn: bool,
    /// A pot, under its account.
    pub nested: bool,
    /// What follows its name while it closes on a later day: "· closes 29
    /// Nov".
    pub closes: Option<String>,
    /// The `danger` dot: its balance check failed.
    pub flagged: bool,
    pub selected: bool,
    /// The focus ring is drawn on it: it's selected and the panel has
    /// keyboard focus.
    pub cursor: bool,
}

/// The valuations canvas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValuationsDescription {
    /// "Valuations" / "Balances".
    pub title: &'static str,
    /// "2 values you entered · net worth uses …".
    pub caption: String,
    /// "Add valuation ⌘N".
    pub add: Action,
    /// The table, newest first.
    pub rows: Vec<ValuationRowDescription>,
    /// The caption under the table: from when it counts toward net worth.
    pub note: Option<String>,
    /// The empty state's title and body, with no values.
    pub empty: Option<(String, String)>,
}

/// A row of the valuations table, lane by lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValuationRowDescription {
    pub date: String,
    /// The note, or "Valuation"; the oldest with "· first value" after it.
    pub note: String,
    pub change: String,
    pub value: String,
}

fn flagged(node: &AccountNode) -> bool {
    node.status == Status::Mismatch
}

fn account_row(
    node: &AccountNode,
    today: chrono::NaiveDate,
    nested: bool,
    selected: bool,
    ring: bool,
) -> AccountRow {
    let (balance, overdrawn) =
        panel::latest(node).unwrap_or_else(|| (panel::NO_BALANCE.to_owned(), false));
    AccountRow {
        name: super::name(&node.account.name),
        balance,
        overdrawn,
        nested,
        closes: panel::closes(node, today),
        flagged: flagged(node),
        selected,
        cursor: selected && ring,
    }
}

/// The inspector for `node`, with the Name and Bank fields as `fields`
/// holds them: the header, Details, the balances, Pots and Imports when
/// there are any, and the footer.
fn describe_inspector(
    accounts: &Accounts,
    node: &AccountNode,
    fields: &AccountInspector,
    cx: &App,
) -> Inspector {
    let data = accounts.data();
    let detail = data.detail(node.account.id).cloned().unwrap_or_default();
    let kind = node.account.kind;
    let caption = match header_caption(node, data) {
        HeaderCaption::Dated { verb, date, tail } => {
            format!("{verb}{date}{}", tail.unwrap_or_default())
        }
        HeaderCaption::Missing(words) => words.to_owned(),
    };
    let (name, bank) = fields.field_values(cx);
    let mut details = Section::default().row("Name", name);
    if kind.is_imported() {
        details = details.row("Bank", bank);
    }
    details = details
        .row("Kind", kind_label(kind))
        .row("Currency", node.account.currency.code());
    if kind.is_imported() {
        details = details.row("Preset", preset_value(node, data));
    }
    if let Some(error) = fields.error() {
        details = details.row("", error.to_string());
    }
    // The selects, which can't open on a locked value.
    details = details
        .action(Action::button("Kind", kind != AccountKind::Pot))
        .action(Action::button("Currency", !detail.usage.currency_locked()));
    if kind.is_imported() {
        details = details.action(Action::button("Preset", true));
    }

    let (title, add) = balances_words(kind);
    let mut balances = Section::titled(title).action(Action::button(add, true));
    if kind.holds_transactions() {
        for BalanceText {
            date,
            label,
            amount,
            ..
        } in balance_rows(node, &detail.balances, data)
        {
            balances = balances.row(date, format!("{label} · {amount}"));
        }
    }
    let mut sections = vec![details, balances];

    if !node.pots.is_empty() {
        let mut pots = node
            .pots
            .iter()
            .fold(Section::titled("Pots"), |section, pot| {
                section.row(super::name(&pot.account.name), pot_balance(pot))
            });
        if let Some(total) = pots_total(node) {
            pots = pots.row("Total", total);
        }
        sections.push(pots);
    }
    if !detail.imports.is_empty() {
        sections.push(
            detail
                .imports
                .iter()
                .fold(Section::titled("Imports"), |section, batch| {
                    let (file, line) = import_text(batch, data);
                    section.row(file, line)
                }),
        );
    }

    let mut footer = Section::default();
    if node.closes_on().is_none() {
        footer = footer.action(Action::button(inspector::CLOSE, true));
    }
    if detail.deletion.refused.is_none() {
        footer = footer.action(Action::button(inspector::DELETE, true));
    }
    if let Some(note) = inspector::footer_note(node) {
        footer = footer.row("", note);
    }
    sections.push(footer);

    Inspector {
        header: Header {
            title: super::name(&node.account.name),
            figure: header_figure(node),
            caption: Some(caption),
        },
        sections,
    }
}

/// The valuations canvas for `node`, an asset or liability.
fn describe_valuations(accounts: &Accounts, node: &AccountNode) -> ValuationsDescription {
    let detail = accounts
        .data()
        .detail(node.account.id)
        .cloned()
        .unwrap_or_default();
    let words = Words::of(node.account.kind);
    let rows = valuations::rows(&detail.balances);
    let last = rows.len().saturating_sub(1);
    ValuationsDescription {
        title: words.title,
        caption: valuations::toolbar_caption(words, rows.len()),
        add: Action::keyed(words.add, panel::ADD_VALUE_KEY, true),
        rows: rows
            .iter()
            .enumerate()
            .map(|(at, row)| {
                let RowText {
                    date,
                    note,
                    first,
                    change,
                    value,
                    ..
                } = RowText::of(row, words, at == last);
                ValuationRowDescription {
                    date,
                    note: match first {
                        Some(first) => format!("{note} {first}"),
                        None => note,
                    },
                    change,
                    value,
                }
            })
            .collect(),
        note: (!rows.is_empty()).then(|| valuations::table_note(node, &rows)),
        empty: rows
            .is_empty()
            .then(|| (valuations::empty_title(words), valuations::empty_body(node))),
    }
}

impl Accounts {
    /// Notes whether the panel drew its focus ring, as it draws it.
    pub(super) fn record_panel_ring(&self, ring: bool) {
        self.panel_ring.set(ring);
    }

    /// What accounts management shows now, as plain data (module docs),
    /// with `inspector`'s fields while it's shown (Transactions shows it
    /// while an account is selected and no transaction is). A dialog is
    /// the shell's: add it with `AccountsDescription::with_dialog`.
    pub fn describe(&self, inspector: Option<&AccountInspector>, cx: &App) -> AccountsDescription {
        let data = self.data();
        let selected = self.selected();
        let ring = self.panel_ring.get();
        let row = |node: &AccountNode, nested: bool| {
            account_row(
                node,
                self.data().today,
                nested,
                selected == Some(node.account.id),
                ring,
            )
        };
        let (kept, valued) = panel::sections(data);
        let accounts = PanelSection {
            title: panel::ACCOUNTS,
            add: Action::button(panel::ADD_ACCOUNT_LABEL, true),
            rows: kept
                .iter()
                .flat_map(|node| {
                    std::iter::once(row(node, false))
                        .chain(node.pots.iter().map(|pot| row(pot, true)))
                })
                .collect(),
            note: (self.loaded() && kept.is_empty() && self.load_error().is_none())
                .then_some(panel::NO_ACCOUNTS),
        };
        let valued_section = PanelSection {
            title: panel::VALUED,
            add: Action::button(panel::ADD_VALUED_LABEL, true),
            rows: valued.iter().map(|node| row(node, false)).collect(),
            note: None,
        };
        let net_worth = panel::net_worth(data);
        let node = self.selected_node();
        let valued_node = node.filter(|node| !node.account.kind.holds_transactions());
        AccountsDescription {
            net_worth: if net_worth.is_empty() {
                vec![panel::NO_NET_WORTH.to_owned()]
            } else {
                net_worth
            },
            sections: vec![accounts, valued_section],
            error: self.load_error().map(ToString::to_string),
            inspector: node
                .zip(inspector)
                .map(|(node, fields)| describe_inspector(self, node, fields, cx)),
            valuations: valued_node.map(|node| describe_valuations(self, node)),
            undo_bar: match (self.removed(), self.closed()) {
                (Some(removed), _) => Some(removed.message.clone()).map(|message| {
                    (
                        self.removed_error().unwrap_or(&message).to_string(),
                        vec![Action::button(crate::ui::undo_bar::UNDO, true)],
                    )
                }),
                (None, Some(closed)) => {
                    let message = self.closed_error().unwrap_or(&closed.message);
                    let undo = closed
                        .undoable
                        .then(|| Action::button(crate::ui::undo_bar::UNDO, true));
                    Some((message.to_string(), undo.into_iter().collect()))
                }
                (None, None) => None,
            },
            context_menu: self.context_menu().map(|menu| {
                menu.groups
                    .iter()
                    .map(|group| {
                        group
                            .options
                            .iter()
                            .map(|option| {
                                (
                                    option.label.to_string(),
                                    option.disabled.then(|| {
                                        option.hint.clone().unwrap_or_default().to_string()
                                    }),
                                )
                            })
                            .collect()
                    })
                    .collect()
            }),
            dialog: None,
            keys: Action::moves(&panel::KEYS, !panel::order(data).is_empty())
                .into_iter()
                .chain([
                    Action::keyed("All accounts", panel::CLEAR_KEY, selected.is_some()),
                    Action::keyed(
                        "Open its balance check",
                        panel::CHECK_KEY,
                        node.is_some_and(flagged),
                    ),
                    Action::keyed("Add valuation", panel::ADD_VALUE_KEY, valued_node.is_some()),
                ])
                .collect(),
        }
    }
}

impl AccountsDescription {
    /// With `dialog`, the shell's open dialog, when it's one of accounts'.
    pub fn with_dialog(mut self, dialog: Option<&AnyView>, cx: &App) -> Self {
        self.dialog = Self::dialog_of(dialog, cx);
        self
    }

    /// `dialog` described, when it's one of this view's.
    pub fn dialog_of(dialog: Option<&AnyView>, cx: &App) -> Option<crate::ui::describe::Dialog> {
        dialog.and_then(|dialog| {
            let dialog = dialog.clone();
            let dialog = match dialog.downcast::<NewAccountDialog>() {
                Ok(new) => return Some(new.read(cx).describe(cx)),
                Err(dialog) => dialog,
            };
            let dialog = match dialog.downcast::<AddBalanceDialog>() {
                Ok(add) => return Some(add.read(cx).describe(cx)),
                Err(dialog) => dialog,
            };
            let dialog = match dialog.downcast::<CloseAccountDialog>() {
                Ok(close) => return Some(close.read(cx).describe(cx)),
                Err(dialog) => dialog,
            };
            dialog
                .downcast::<DeleteAccountDialog>()
                .ok()
                .map(|delete| delete.read(cx).describe())
        })
    }
}
