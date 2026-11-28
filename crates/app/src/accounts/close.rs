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
                "{name} leaves the Accounts panel and counts as {zero} from {day}, which raises net worth by {owed}. Its balances stay, so earlier net worth doesn't change. {REOPEN_LINE}",
                zero = zero(owed),
                owed = amount(owed),
            ),
            (AccountKind::Asset | AccountKind::Liability, _) => format!(
                "{name} leaves the Accounts panel and counts as nothing from {day}. Its values stay, so earlier net worth doesn't change. You can reopen it."
            ),
            (_, balance) => {
                let mut body =
                    format!("{name} leaves the Accounts panel and net worth from {day}.");
                if let Some(balance) = balance.filter(|balance| !balance.is_zero()) {
                    body.push_str(&format!(
                        " Its balance of {} goes with it.",
                        money(balance, Sign::Negative)
                    ));
                }
                match self.pots.as_slice() {
                    [] => {}
                    [pot] => body.push_str(&format!(" Its pot, {pot}, closes too.")),
                    pots => body.push_str(&format!(" Its pots, {}, close too.", list(pots))),
                }
                body.push_str(&format!(
                    " Its transactions, balances and imports stay, so earlier net worth doesn't change. {REOPEN_LINE}"
                ));
                body
            }
        }
    }
}

/// The command that reopens a closed account, as the body names it; the
/// dialog sets it in Geist Mono ([`body_text`]).
const REOPEN_COMMAND: &str = "orre accounts reopen";

/// The bank and liability bodies' last sentence (SLI-77, Rowling). Plain
/// text: `describe()` and tests carry the body as one string.
const REOPEN_LINE: &str = "You can reopen it with orre accounts reopen.";

/// The body as drawn: a caption in which [`REOPEN_COMMAND`] is set in
/// Geist Mono, as Settings sets `orre mcp --client` in prose (Paper
/// `CRE-0`). Any other body is plain.
fn body_text(body: String) -> gpui_kit::Div {
    use crate::settings::table::{mono, plain, rich};

    match body.rsplit_once(REOPEN_COMMAND) {
        Some((before, after)) => rich(
            crate::theme::tokens::typography::CAPTION,
            color::TEXT_MUTED,
            vec![plain(before), mono(REOPEN_COMMAND), plain(after)],
        ),
        None => caption(body),
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
            .body(body_text(body))
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
        // SLI-77 (Rowling): the day with its year, then the balance, then
        // the pots, then what stays, then how to reopen it.
        assert_eq!(
            savings.body(Some(day(11, 29))),
            "Current account leaves the Accounts panel and net worth from 29 Nov 2026. Its balance of £1,284.10 goes with it. Its pots, Holiday and Rainy day, close too. Its transactions, balances and imports stay, so earlier net worth doesn't change. You can reopen it with orre accounts reopen."
        );
        assert_eq!(savings.day_label(), "Closed on");
        assert_eq!(list(&["A".into(), "B".into()]), "A and B");
        assert_eq!(list(&["A".into(), "B".into(), "C".into()]), "A, B and C");
        let mortgage = facts(AccountKind::Liability, -21_264_000, &[]);
        assert!(
            mortgage
                .body(None)
                .contains("counts as £0 from that day, which raises net worth by £212,640.00.")
        );
    }

    /// SLI-77 (Rowling): every shape of the bank sentence, exactly.
    #[test]
    fn the_bank_body_in_each_shape() {
        let bank = |minor: Option<i64>, pots: &[&str]| {
            let mut facts = facts(AccountKind::Current, 0, pots);
            facts.name = "Everyday account".into();
            facts.balance = minor.map(|minor| Money::new(minor, Currency::GBP));
            facts.body(Some(day(11, 29)))
        };
        // No balance, no pots.
        assert_eq!(
            bank(None, &[]),
            "Everyday account leaves the Accounts panel and net worth from 29 Nov 2026. Its transactions, balances and imports stay, so earlier net worth doesn't change. You can reopen it with orre accounts reopen."
        );
        // A balance, no pots.
        assert_eq!(
            bank(Some(120_000), &[]),
            "Everyday account leaves the Accounts panel and net worth from 29 Nov 2026. Its balance of £1,200.00 goes with it. Its transactions, balances and imports stay, so earlier net worth doesn't change. You can reopen it with orre accounts reopen."
        );
        // A balance and one pot.
        assert_eq!(
            bank(Some(120_000), &["Holiday"]),
            "Everyday account leaves the Accounts panel and net worth from 29 Nov 2026. Its balance of £1,200.00 goes with it. Its pot, Holiday, closes too. Its transactions, balances and imports stay, so earlier net worth doesn't change. You can reopen it with orre accounts reopen."
        );
        // A balance and three pots.
        assert_eq!(
            bank(Some(120_000), &["Holiday", "Rainy day", "Car"]),
            "Everyday account leaves the Accounts panel and net worth from 29 Nov 2026. Its balance of £1,200.00 goes with it. Its pots, Holiday, Rainy day and Car, close too. Its transactions, balances and imports stay, so earlier net worth doesn't change. You can reopen it with orre accounts reopen."
        );
        // Pots but a zero balance (and the same with no balance): no
        // balance sentence.
        let zero_with_pots = "Everyday account leaves the Accounts panel and net worth from 29 Nov 2026. Its pots, Holiday and Car, close too. Its transactions, balances and imports stay, so earlier net worth doesn't change. You can reopen it with orre accounts reopen.";
        assert_eq!(bank(Some(0), &["Holiday", "Car"]), zero_with_pots);
        assert_eq!(bank(None, &["Holiday", "Car"]), zero_with_pots);
        // An overdrawn balance keeps its sign, as before.
        assert!(bank(Some(-2_410), &[]).contains(" Its balance of −£24.10 goes with it. "));
    }

    /// SLI-77: the liability sentence QA saw, with the year even in the
    /// current year, and a day in another year. The asset bodies keep
    /// their wording (Rowling gave none for them).
    #[test]
    fn the_liability_body_has_the_year() {
        let mut loan = facts(AccountKind::Liability, -500_000, &[]);
        loan.name = "QA Loan".into();
        assert_eq!(
            loan.body(Some(day(11, 29))),
            "QA Loan leaves the Accounts panel and counts as £0 from 29 Nov 2026, which raises net worth by £5,000.00. Its balances stay, so earlier net worth doesn't change. You can reopen it with orre accounts reopen."
        );
        assert!(
            loan.body(NaiveDate::from_ymd_opt(2027, 1, 4))
                .contains("counts as £0 from 4 Jan 2027, which")
        );
        let mut empty = facts(AccountKind::Asset, 0, &[]);
        empty.name = "Car".into();
        assert_eq!(
            empty.body(Some(day(11, 29))),
            "Car leaves the Accounts panel and counts as nothing from 29 Nov 2026. Its values stay, so earlier net worth doesn't change. You can reopen it."
        );
    }

    /// The command in the body is found for the Geist Mono run, from the
    /// end, so a name that holds it doesn't move the run.
    #[test]
    fn the_reopen_command_is_split_from_the_end() {
        let mut odd = facts(AccountKind::Current, 0, &[]);
        odd.name = "orre accounts reopen".into();
        odd.balance = None;
        let body = odd.body(Some(day(11, 29)));
        let (before, after) = body.rsplit_once(REOPEN_COMMAND).unwrap();
        assert!(before.starts_with("orre accounts reopen leaves"));
        assert!(before.ends_with("You can reopen it with "));
        assert_eq!(after, ".");
    }
}
