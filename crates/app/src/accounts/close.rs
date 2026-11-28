//! The Close account dialog (`docs/DESIGN.md` → "Accounts you keep
//! yourself" → **Close House?**; Paper `FT1-0` on the states board
//! `EPF-0`), opened by "Close account…" in the account inspector, on every
//! kind of account (M3.17).
//!
//! The body first (what closing does: the account leaves the panel and net
//! worth from the day, its history stays, it can be reopened), then the day
//! in a 96px date field, then Cancel and "Close account" (neutral primary:
//! it can be undone). Core decides the rest
//! ([`orre_core::account::close`]): never before what the account holds,
//! nor before its lock date. A refusal is said in `danger` under the form.
//!
//! Drawn for an asset ("Sold on"). Composed for M3.17 (decided with the
//! user, 2026-11-14): a liability's day is "Paid off on" and net worth rises
//! by what was owed; a bank, cash or pot account's is "Closed on", its
//! balance leaves net worth from the day (SLI-77: on it, as the panel is
//! as of today), and its open pots close with it. The day starts at today,
//! or at the earliest day it can close if that's later. The body follows
//! the day as it's typed, always with its year ("29 Nov 2026").

use chrono::NaiveDate;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};
use orre_core::account::AccountKind;
use orre_core::id::AccountId;
use orre_core::money::Money;

use crate::format::{Sign, currency_prefix, field_date, money, parse_date};
use crate::theme::tokens::{color, spacing};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{FieldRow, caption, date_input, error_caption};

use super::data::AccountsData;
use super::new_account::sentence;
use super::{Accounts, DialogClosed};

/// What the dialog says about one account, read when it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseFacts {
    pub name: String,
    pub kind: AccountKind,
    /// Its latest balance or value, if it has one.
    pub balance: Option<Money>,
    /// Its open pots' names, which close with it.
    pub pots: Vec<String>,
    /// The earliest day it can close.
    pub earliest: Option<NaiveDate>,
}

impl CloseFacts {
    /// The facts for `account`, if it's open.
    pub fn of(data: &AccountsData, account: AccountId) -> Option<CloseFacts> {
        let node = data.node(account)?;
        Some(CloseFacts {
            name: super::name(&node.account.name),
            kind: node.account.kind,
            balance: node.latest.map(|latest| latest.balance),
            pots: node
                .pots
                .iter()
                .map(|pot| super::name(&pot.account.name))
                .collect(),
            earliest: data.detail(account).and_then(|detail| detail.close_from),
        })
    }

    /// The day field's label.
    pub fn day_label(&self) -> &'static str {
        match self.kind {
            AccountKind::Asset => "Sold on",
            AccountKind::Liability => "Paid off on",
            _ => "Closed on",
        }
    }

    /// The body for closing on `on` (`None`: a day that doesn't read). The
    /// day always has its year ("29 Nov 2026"), and every kind leaves from
    /// it: the panel is as of today, so the account leaves on that day
    /// (SLI-77).
    pub fn body(&self, on: Option<NaiveDate>) -> String {
        let name = &self.name;
        let day = on.map_or_else(|| "that day".to_owned(), field_date);
        let amount =
            |balance: Money| money(balance.checked_abs().unwrap_or(balance), Sign::Negative);
        match (self.kind, self.balance) {
            (AccountKind::Asset, Some(value)) if !value.is_zero() => format!(
                "{name} leaves the Accounts panel and counts as {zero} from {day}, so net worth drops by {value} from then (the sale money arrives in your bank account). Its valuations stay, so earlier net worth doesn't change. You can reopen it.",
                zero = zero(value),
                value = amount(value),
            ),
            (AccountKind::Liability, Some(owed)) if !owed.is_zero() => format!(
                "{name} leaves the Accounts panel and counts as {zero} from {day}, so net worth rises by {owed} from then. Its balances stay, so earlier net worth doesn't change. You can reopen it.",
                zero = zero(owed),
                owed = amount(owed),
            ),
            (AccountKind::Asset | AccountKind::Liability, _) => format!(
                "{name} leaves the Accounts panel and counts as nothing from {day}. Its values stay, so earlier net worth doesn't change. You can reopen it."
            ),
            (_, balance) => {
                let mut body = format!(
                    "{name} leaves the Accounts panel and net worth from {day}. Its transactions, balances and imports stay, so earlier net worth doesn't change. You can reopen it."
                );
                if let Some(balance) = balance.filter(|balance| !balance.is_zero()) {
                    body.push_str(&format!(
                        " Its balance, {}, leaves net worth with it.",
                        money(balance, Sign::Negative)
                    ));
                }
                match self.pots.as_slice() {
                    [] => {}
                    [pot] => body.push_str(&format!(" Its pot, {pot}, closes with it.")),
                    pots => body.push_str(&format!(" Its pots, {}, close with it.", list(pots))),
                }
                body
            }
        }
    }
}

/// Nothing in `amount`'s currency, as the body writes it: `£0`.
pub(super) fn zero(amount: Money) -> String {
    let prefix = currency_prefix(amount.currency());
    if prefix.chars().count() > 1 {
        format!("{prefix} 0")
    } else {
        format!("{prefix}0")
    }
}

/// `A, B and C`.
fn list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The dialog's button that closes.
const CLOSE: &str = "Close account";

/// Close account, for one account.
pub struct CloseAccountDialog {
    accounts: Entity<Accounts>,
    account: AccountId,
    facts: Option<CloseFacts>,
    focus: FocusHandle,
    date: Entity<InputState>,
    error: Option<SharedString>,
    today: NaiveDate,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DialogClosed> for CloseAccountDialog {}

impl CloseAccountDialog {
    /// The dialog for `account`, which must be open.
    pub fn new(
        accounts: Entity<Accounts>,
        account: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let data = accounts.read(cx).data();
        let today = data.today;
        let facts = CloseFacts::of(data, account);
        let start = facts
            .as_ref()
            .and_then(|facts| facts.earliest)
            .map_or(today, |earliest| earliest.max(today));
        let date = cx.new(|cx| InputState::new(window, cx).default_value(field_date(start)));
        let subscriptions = vec![cx.subscribe_in(
            &date,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )];
        date.read(cx).focus_handle(cx).focus(window, cx);
        Self {
            accounts,
            account,
            facts,
            focus: cx.focus_handle(),
            date,
            error: None,
            today,
            _subscriptions: subscriptions,
        }
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    fn typed(&self, cx: &gpui_kit::App) -> Option<NaiveDate> {
        parse_date(&self.date.read(cx).value(), self.today)
    }

    /// The title, the body (following the typed day) and the day's label.
    fn text(&self, cx: &gpui_kit::App) -> (String, String, &'static str) {
        match &self.facts {
            Some(facts) => (
                format!("Close {}?", facts.name),
                facts.body(self.typed(cx)),
                facts.day_label(),
            ),
            None => ("Close account?".to_owned(), String::new(), "Closed on"),
        }
    }

    /// What the dialog shows, as plain data (test-only, ROADMAP U.3): its
    /// body, which names the day and what leaves net worth, is a field
    /// labelled "" before the day's.
    #[cfg(any(test, feature = "dev-driver"))]
    pub fn describe(&self, cx: &gpui_kit::App) -> crate::ui::describe::Dialog {
        use crate::ui::describe::{Action, Dialog};
        let (title, body, label) = self.text(cx);
        Dialog {
            title,
            fields: vec![
                (String::new(), body),
                (label.to_owned(), self.date.read(cx).value().to_string()),
            ],
            error: self.error.as_ref().map(ToString::to_string),
            actions: vec![Action::button("Cancel", true), Action::button(CLOSE, true)],
        }
    }

    /// Closes the account on the typed day, or says why it can't.
    pub fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(on) = self.typed(cx) else {
            self.error = Some("The date isn't one Orre can read. Type it as 6 Nov 2026.".into());
            cx.notify();
            return;
        };
        let account = self.account;
        let result = self
            .accounts
            .update(cx, |accounts, cx| accounts.close(account, on, cx));
        match result {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(sentence(&error));
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }
}

impl Render for CloseAccountDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, body, label) = self.text(cx);
        let cancel = cx.listener(|this, _, _, cx| this.cancel(cx));
        let close = cx.listener(|this, _, window, cx| this.submit(window, cx));
        let on_cancel = cx.entity().downgrade();
        Dialog::new(title, &self.focus)
            .body(caption(body))
            .body(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::S2)
                    .child(FieldRow::new(
                        label,
                        date_input(&self.date).id("close-account-date"),
                    )),
            )
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("close-account-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cancel),
            )
            .action(
                Button::new("close-account-close", CLOSE)
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .on_click(close),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

#[cfg(test)]
mod tests {
    use orre_core::money::Currency;

    use super::*;

    fn day(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    fn facts(kind: AccountKind, minor: i64, pots: &[&str]) -> CloseFacts {
        CloseFacts {
            name: "House".into(),
            kind,
            balance: Some(Money::new(minor, Currency::GBP)),
            pots: pots.iter().map(|pot| (*pot).to_owned()).collect(),
            earliest: None,
        }
    }

    #[test]
    fn the_asset_body_is_paper_s() {
        assert_eq!(
            facts(AccountKind::Asset, 38_500_000, &[]).body(Some(day(11, 6))),
            "House leaves the Accounts panel and counts as £0 from 6 Nov 2026, so net worth drops by £385,000.00 from then (the sale money arrives in your bank account). Its valuations stay, so earlier net worth doesn't change. You can reopen it."
        );
        assert_eq!(facts(AccountKind::Asset, 1, &[]).day_label(), "Sold on");
        assert_eq!(
            facts(AccountKind::Liability, -1, &[]).day_label(),
            "Paid off on"
        );
    }

    #[test]
    fn a_bank_account_names_its_balance_and_pots() {
        let mut savings = facts(AccountKind::Current, 128_410, &["Holiday", "Rainy day"]);
        savings.name = "Current account".into();
        // SLI-77: the bank sentence says "from" the close day itself, with
        // its year even in the current year, as the liability's does.
        assert_eq!(
            savings.body(Some(day(11, 29))),
            "Current account leaves the Accounts panel and net worth from 29 Nov 2026. Its transactions, balances and imports stay, so earlier net worth doesn't change. You can reopen it. Its balance, £1,284.10, leaves net worth with it. Its pots, Holiday and Rainy day, close with it."
        );
        assert_eq!(savings.day_label(), "Closed on");
        assert_eq!(list(&["A".into(), "B".into(), "C".into()]), "A, B and C");
        let mortgage = facts(AccountKind::Liability, -21_264_000, &[]);
        assert!(
            mortgage
                .body(None)
                .contains("from that day, so net worth rises by £212,640.00 from then")
        );
    }

    /// SLI-77: the liability sentence QA saw, with the year even in the
    /// current year, and a day in another year.
    #[test]
    fn the_liability_body_has_the_year() {
        let mut loan = facts(AccountKind::Liability, -500_000, &[]);
        loan.name = "QA Loan".into();
        assert_eq!(
            loan.body(Some(day(11, 29))),
            "QA Loan leaves the Accounts panel and counts as £0 from 29 Nov 2026, so net worth rises by £5,000.00 from then. Its balances stay, so earlier net worth doesn't change. You can reopen it."
        );
        assert!(
            loan.body(NaiveDate::from_ymd_opt(2027, 1, 4))
                .contains("counts as £0 from 4 Jan 2027, so")
        );
        let mut empty = facts(AccountKind::Asset, 0, &[]);
        empty.name = "Car".into();
        assert_eq!(
            empty.body(Some(day(11, 29))),
            "Car leaves the Accounts panel and counts as nothing from 29 Nov 2026. Its values stay, so earlier net worth doesn't change. You can reopen it."
        );
    }
}
