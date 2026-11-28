//! The Close account dialog (`docs/DESIGN.md` → "Accounts you keep
//! yourself" → **Close House?**; Paper `FT1-0` on the states board
//! `EPF-0`), opened by "Close account…" in the account inspector, on every
//! kind of account (M3.17).
//!
//! The body first (what closing does: the account leaves the panel and net
//! worth from the day, its history stays, and the command that reopens it,
//! in Geist Mono: [`reopen_command`]), then the day
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
    /// Its ID, which the reopen command names when its name can't be
    /// pasted safely ([`reopen_command`]).
    pub id: AccountId,
    /// Its name as shown ([`super::name`]).
    pub name: String,
    /// Its name as stored, which `orre accounts reopen` matches exactly.
    pub stored_name: String,
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
            id: account,
            name: super::name(&node.account.name),
            stored_name: node.account.name.clone(),
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
        let reopen = reopen_line(&self.reopen_command());
        match (self.kind, self.balance) {
            (AccountKind::Asset, Some(value)) if !value.is_zero() => format!(
                "{name} leaves the Accounts panel and counts as {zero} from {day}, which lowers net worth by {value} (the sale money arrives in your bank account). Its valuations stay, so earlier net worth doesn't change. {reopen}",
                zero = zero(value),
                value = amount(value),
            ),
            (AccountKind::Liability, Some(owed)) if !owed.is_zero() => format!(
                "{name} leaves the Accounts panel and counts as {zero} from {day}, which raises net worth by {owed}. Its balances stay, so earlier net worth doesn't change. {reopen}",
                zero = zero(owed),
                owed = amount(owed),
            ),
            (AccountKind::Asset | AccountKind::Liability, _) => format!(
                "{name} leaves the Accounts panel and counts as nothing from {day}. Its earlier values stay, so earlier net worth doesn't change. {reopen}"
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
                    " Its transactions, balances and imports stay, so earlier net worth doesn't change. {reopen}"
                ));
                body
            }
        }
    }

    /// The command that reopens it, as the body's last sentence names it
    /// ([`reopen_command`]).
    pub fn reopen_command(&self) -> String {
        reopen_command(&self.stored_name, self.id)
    }
}

/// What follows the command in the body's last sentence.
const REOPEN_TAIL: &str = " in a terminal.";

/// Every body's last sentence (SLI-77, Rowling): "To reopen it, run
/// {command} in a terminal." Plain text: `describe()` and tests carry the
/// body as one string; the dialog sets the command in Geist Mono
/// ([`body_text`]).
fn reopen_line(command: &str) -> String {
    format!("To reopen it, run {command}{REOPEN_TAIL}")
}

/// The command that reopens the account `stored` (its name as stored) with
/// ID `id`, to paste into a terminal: `orre accounts reopen "House"`, or
/// `orre accounts reopen 7` when the name can't be pasted safely in double
/// quotes or wouldn't find the account. The ID is the one `orre accounts
/// list` shows and `reopen` takes.
///
/// The name is used only when every one of these holds:
///
/// - It's shown as stored ([`super::name`] changes nothing): no invisible
///   or bidi characters stripped, no spaces collapsed or trimmed, not cut.
///   `reopen` matches the stored name exactly.
/// - No character a shell acts on inside double quotes: `"` `$` `` ` ``
///   `\` (POSIX shells, fish, PowerShell), `!` (history expansion in an
///   interactive bash or zsh) and `%` (cmd.exe variables).
/// - No control character (C0, DEL, C1, which covers U+0085), no
///   whitespace but the plain space (tab, newline, U+2028, U+2029,
///   no-break and other spaces), and nothing [`envelope::is_invisible`]
///   strips (zero-width characters, bidi overrides).
/// - It doesn't read as an ID (`reopen` tries `2024` as an ID first) and
///   doesn't start with `-` (it would read as an option).
///
/// [`envelope::is_invisible`]: orre_core::envelope::is_invisible
pub fn reopen_command(stored: &str, id: AccountId) -> String {
    let unsafe_char = |c: char| {
        matches!(c, '"' | '$' | '`' | '\\' | '!' | '%')
            || c.is_control()
            || (c.is_whitespace() && c != ' ')
            || orre_core::envelope::is_invisible(c)
    };
    let safe = !stored.is_empty()
        && super::name(stored) == stored
        && !stored.chars().any(unsafe_char)
        && stored.trim().parse::<i64>().is_err()
        && !stored.starts_with('-');
    if safe {
        format!("orre accounts reopen \"{stored}\"")
    } else {
        format!("orre accounts reopen {id}")
    }
}

/// `body` cut around `command` in its last sentence: the text before the
/// command, the command, and the text after it. Cut from the end, where
/// the sentence always is, so a name holding the command can't move it.
fn split_reopen<'a>(body: &'a str, command: &str) -> Option<(&'a str, &'a str, &'a str)> {
    let before_tail = body.strip_suffix(REOPEN_TAIL)?;
    let before = before_tail.strip_suffix(command)?;
    let tail_at = before_tail.len();
    Some((before, &body[before.len()..tail_at], &body[tail_at..]))
}

/// The body as drawn: a caption in which the reopen command is set in
/// Geist Mono, as Settings sets `orre mcp --client` in prose (Paper
/// `CRE-0`). A body without it is plain.
fn body_text(body: String, command: &str) -> gpui_kit::Div {
    use crate::settings::table::{mono, plain, rich};

    match split_reopen(&body, command) {
        Some((before, command, after)) => rich(
            crate::theme::tokens::typography::CAPTION,
            color::TEXT_MUTED,
            vec![plain(before), mono(command), plain(after)],
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
        let command = self
            .facts
            .as_ref()
            .map(CloseFacts::reopen_command)
            .unwrap_or_default();
        let cancel = cx.listener(|this, _, _, cx| this.cancel(cx));
        let close = cx.listener(|this, _, window, cx| this.submit(window, cx));
        let on_cancel = cx.entity().downgrade();
        Dialog::new(title, &self.focus)
            .body(body_text(body, &command))
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

    /// Account 7, "House", of `kind`, worth `minor` pence, with `pots`.
    fn facts(kind: AccountKind, minor: i64, pots: &[&str]) -> CloseFacts {
        CloseFacts {
            id: AccountId::new(7),
            name: "House".into(),
            stored_name: "House".into(),
            kind,
            balance: Some(Money::new(minor, Currency::GBP)),
            pots: pots.iter().map(|pot| (*pot).to_owned()).collect(),
            earliest: None,
        }
    }

    /// `facts` named `stored`, shown as the dialog shows it.
    fn named(mut facts: CloseFacts, stored: &str) -> CloseFacts {
        facts.name = crate::accounts::name(stored);
        facts.stored_name = stored.to_owned();
        facts
    }

    #[test]
    fn the_asset_body_is_paper_s() {
        assert_eq!(
            facts(AccountKind::Asset, 38_500_000, &[]).body(Some(day(11, 6))),
            "House leaves the Accounts panel and counts as £0 from 6 Nov 2026, which lowers net worth by £385,000.00 (the sale money arrives in your bank account). Its valuations stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"House\" in a terminal."
        );
        assert_eq!(facts(AccountKind::Asset, 1, &[]).day_label(), "Sold on");
        assert_eq!(
            facts(AccountKind::Liability, -1, &[]).day_label(),
            "Paid off on"
        );
    }

    #[test]
    fn a_bank_account_names_its_balance_and_pots() {
        let savings = named(
            facts(AccountKind::Current, 128_410, &["Holiday", "Rainy day"]),
            "Current account",
        );
        // SLI-77 (Rowling): the day with its year, then the balance, then
        // the pots, then what stays, then how to reopen it.
        assert_eq!(
            savings.body(Some(day(11, 29))),
            "Current account leaves the Accounts panel and net worth from 29 Nov 2026. Its balance of £1,284.10 goes with it. Its pots, Holiday and Rainy day, close too. Its transactions, balances and imports stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"Current account\" in a terminal."
        );
        assert_eq!(savings.day_label(), "Closed on");
        assert_eq!(list(&["A".into(), "B".into()]), "A and B");
        assert_eq!(list(&["A".into(), "B".into(), "C".into()]), "A, B and C");
        // A day that doesn't read.
        let mortgage = named(facts(AccountKind::Liability, -21_264_000, &[]), "Mortgage");
        assert_eq!(
            mortgage.body(None),
            "Mortgage leaves the Accounts panel and counts as £0 from that day, which raises net worth by £212,640.00. Its balances stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"Mortgage\" in a terminal."
        );
    }

    /// SLI-77 (Rowling): every shape of the bank sentence, exactly.
    #[test]
    fn the_bank_body_in_each_shape() {
        let bank = |minor: Option<i64>, pots: &[&str]| {
            let mut facts = named(facts(AccountKind::Current, 0, pots), "Everyday account");
            facts.balance = minor.map(|minor| Money::new(minor, Currency::GBP));
            facts.body(Some(day(11, 29)))
        };
        // No balance, no pots.
        assert_eq!(
            bank(None, &[]),
            "Everyday account leaves the Accounts panel and net worth from 29 Nov 2026. Its transactions, balances and imports stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"Everyday account\" in a terminal."
        );
        // A balance, no pots.
        assert_eq!(
            bank(Some(120_000), &[]),
            "Everyday account leaves the Accounts panel and net worth from 29 Nov 2026. Its balance of £1,200.00 goes with it. Its transactions, balances and imports stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"Everyday account\" in a terminal."
        );
        // A balance and one pot.
        assert_eq!(
            bank(Some(120_000), &["Holiday"]),
            "Everyday account leaves the Accounts panel and net worth from 29 Nov 2026. Its balance of £1,200.00 goes with it. Its pot, Holiday, closes too. Its transactions, balances and imports stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"Everyday account\" in a terminal."
        );
        // A balance and three pots.
        assert_eq!(
            bank(Some(120_000), &["Holiday", "Rainy day", "Car"]),
            "Everyday account leaves the Accounts panel and net worth from 29 Nov 2026. Its balance of £1,200.00 goes with it. Its pots, Holiday, Rainy day and Car, close too. Its transactions, balances and imports stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"Everyday account\" in a terminal."
        );
        // Pots but a zero balance (and the same with no balance): no
        // balance sentence.
        let zero_with_pots = "Everyday account leaves the Accounts panel and net worth from 29 Nov 2026. Its pots, Holiday and Car, close too. Its transactions, balances and imports stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"Everyday account\" in a terminal.";
        assert_eq!(bank(Some(0), &["Holiday", "Car"]), zero_with_pots);
        assert_eq!(bank(None, &["Holiday", "Car"]), zero_with_pots);
        // An overdrawn balance keeps its sign, as before.
        assert!(bank(Some(-2_410), &[]).contains(" Its balance of −£24.10 goes with it. "));
    }

    /// SLI-77: the liability sentence QA saw, with the year even in the
    /// current year, and a day in another year; and the shared arm for an
    /// asset or liability with nothing left.
    #[test]
    fn the_liability_body_has_the_year() {
        let loan = named(facts(AccountKind::Liability, -500_000, &[]), "QA Loan");
        assert_eq!(
            loan.body(Some(day(11, 29))),
            "QA Loan leaves the Accounts panel and counts as £0 from 29 Nov 2026, which raises net worth by £5,000.00. Its balances stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"QA Loan\" in a terminal."
        );
        assert_eq!(
            loan.body(NaiveDate::from_ymd_opt(2027, 1, 4)),
            "QA Loan leaves the Accounts panel and counts as £0 from 4 Jan 2027, which raises net worth by £5,000.00. Its balances stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"QA Loan\" in a terminal."
        );
        let empty = named(facts(AccountKind::Asset, 0, &[]), "Car");
        assert_eq!(
            empty.body(Some(day(11, 29))),
            "Car leaves the Accounts panel and counts as nothing from 29 Nov 2026. Its earlier values stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"Car\" in a terminal."
        );
        // A liability with nothing owed takes the same arm, as does one
        // with no value at all.
        let paid = named(facts(AccountKind::Liability, 0, &[]), "Old loan");
        let mut never_valued = paid.clone();
        never_valued.balance = None;
        let nothing = "Old loan leaves the Accounts panel and counts as nothing from that day. Its earlier values stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen \"Old loan\" in a terminal.";
        assert_eq!(paid.body(None), nothing);
        assert_eq!(never_valued.body(None), nothing);
    }

    /// A name that can't be pasted safely in double quotes: the body names
    /// the account by its ID, in every kind of body.
    #[test]
    fn an_unsafe_name_is_reopened_by_its_id() {
        let bank = named(
            facts(AccountKind::Current, 120_000, &["Holiday"]),
            "Joint \"bills\"",
        );
        assert_eq!(
            bank.body(Some(day(11, 29))),
            "Joint \"bills\" leaves the Accounts panel and net worth from 29 Nov 2026. Its balance of £1,200.00 goes with it. Its pot, Holiday, closes too. Its transactions, balances and imports stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen 7 in a terminal."
        );
        let loan = named(facts(AccountKind::Liability, -500_000, &[]), "Loan $HOME");
        assert_eq!(
            loan.body(Some(day(11, 29))),
            "Loan $HOME leaves the Accounts panel and counts as £0 from 29 Nov 2026, which raises net worth by £5,000.00. Its balances stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen 7 in a terminal."
        );
        let house = named(facts(AccountKind::Asset, 38_500_000, &[]), "House `rm`");
        assert_eq!(
            house.body(Some(day(11, 6))),
            "House `rm` leaves the Accounts panel and counts as £0 from 6 Nov 2026, which lowers net worth by £385,000.00 (the sale money arrives in your bank account). Its valuations stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen 7 in a terminal."
        );
        // A newline is shown as a space; the stored name differs, so the
        // ID is used.
        let car = named(facts(AccountKind::Asset, 0, &[]), "Car\nloan");
        assert_eq!(
            car.body(Some(day(11, 29))),
            "Car loan leaves the Accounts panel and counts as nothing from 29 Nov 2026. Its earlier values stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen 7 in a terminal."
        );
    }

    #[test]
    fn the_reopen_command_quotes_only_a_safe_name() {
        let id = AccountId::new(12);
        let quoted = |name: &str| format!("orre accounts reopen \"{name}\"");
        for safe in [
            "House",
            "Joint account",
            "Sam's ISA",
            "Rainy day £",
            "Épargne Zoë",
            "Сбережения",
            "貯金",
            "Savings 2",
            "Pot-1",
        ] {
            assert_eq!(reopen_command(safe, id), quoted(safe), "{safe:?}");
        }
        for unsafe_name in [
            // Shell syntax inside double quotes.
            "Say \"hi\"",
            "Cost $5",
            "Run `id`",
            "Back\\slash",
            "Hi!there",
            "50% pot",
            // Line breaks, tabs and other control characters.
            "Two\nlines",
            "Carriage\rreturn",
            "Tab\there",
            "Esc\u{1b}[31m",
            "Del\u{7f}",
            "Next\u{85}line",
            "Line\u{2028}separator",
            "Para\u{2029}separator",
            "No\u{a0}break",
            // Invisible and bidi characters.
            "Zero\u{200b}width",
            "Bidi\u{202e}override",
            // What `reopen` would read otherwise: an ID, an option, or a
            // name the dialog shows differently.
            "2024",
            " 7",
            "+7",
            "-Savings",
            " Padded",
            "Double  space",
        ] {
            assert_eq!(
                reopen_command(unsafe_name, id),
                "orre accounts reopen 12",
                "{unsafe_name:?}"
            );
        }
        // A name past what the dialog shows is cut there: the ID.
        let long = "a".repeat(crate::inbox::text::NAME_SHOWN + 1);
        assert_eq!(reopen_command(&long, id), "orre accounts reopen 12");
        let longest = "a".repeat(crate::inbox::text::NAME_SHOWN);
        assert_eq!(reopen_command(&longest, id), quoted(&longest));
    }

    /// The mono run is exactly the command, found from the end, so a name
    /// that holds the command (or the sentence) can't move it.
    #[test]
    fn the_reopen_command_is_split_from_the_end() {
        let pieces = |facts: &CloseFacts| {
            let body = facts.body(Some(day(11, 29)));
            let command = facts.reopen_command();
            let (before, mono, after) = split_reopen(&body, &command).expect("split");
            (before.to_owned(), mono.to_owned(), after.to_owned())
        };
        let loan = named(facts(AccountKind::Liability, -500_000, &[]), "QA Loan");
        let (before, mono, after) = pieces(&loan);
        assert!(before.starts_with("QA Loan leaves"));
        assert!(before.ends_with("doesn't change. To reopen it, run "));
        assert_eq!(mono, "orre accounts reopen \"QA Loan\"");
        assert_eq!(after, " in a terminal.");

        // The name holds the quoted command's words: it's in the run once,
        // at the end.
        let odd = named(
            facts(AccountKind::Current, 0, &[]),
            "orre accounts reopen in a terminal",
        );
        let (before, mono, after) = pieces(&odd);
        assert!(before.starts_with("orre accounts reopen in a terminal leaves"));
        assert_eq!(
            mono,
            "orre accounts reopen \"orre accounts reopen in a terminal\""
        );
        assert_eq!(after, " in a terminal.");

        // An unsafe name that holds the ID form.
        let by_id = named(
            facts(AccountKind::Current, 0, &[]),
            "orre accounts reopen 7 in a terminal.$",
        );
        let (before, mono, after) = pieces(&by_id);
        assert!(before.starts_with("orre accounts reopen 7 in a terminal.$ leaves"));
        assert!(before.ends_with("To reopen it, run "));
        assert_eq!(mono, "orre accounts reopen 7");
        assert_eq!(after, " in a terminal.");

        // A body without the command isn't split.
        assert_eq!(
            split_reopen("No command here.", "orre accounts reopen 7"),
            None
        );
    }
}
