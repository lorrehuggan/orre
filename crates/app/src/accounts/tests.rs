//! UI integration tests for accounts management: the shell's panel,
//! inspector and dialogs in a headless window, against a scratch database
//! through [`AccountStore`], as `orre` runs them.

use chrono::NaiveDate;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use orre_core::account::{self, AccountKind, EnteredBalance, EnteredSource, NewAccount};
use orre_core::db::Database;
use orre_core::id::AccountId;
use orre_core::money::{Currency, Money};
use tempfile::TempDir;

use super::describe::{
    AccountRow, AccountsDescription, Action, Dialog, Header, Inspector, PanelSection, Section,
};
use super::*;
use crate::shell::{ShellView, View};

/// A scratch database with Current account (an entered balance, a pot) and
/// the store reading it.
struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    current: AccountId,
    pot: AccountId,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        let mut db = Database::open(&path).unwrap();
        let mut current = NewAccount::new("Current account", AccountKind::Current, Currency::GBP);
        current.institution = Some("Example Bank".into());
        let balance = EnteredBalance {
            date: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            balance: Money::new(10_000, Currency::GBP),
            source: EnteredSource::Manual,
            note: None,
        };
        let current = account::create_with_balance(&mut db, &current, Some(&balance)).unwrap();
        let mut pot = NewAccount::new("Holiday", AccountKind::Pot, Currency::GBP);
        pot.parent = Some(current);
        let pot = account::create(&mut db, &pot).unwrap();
        Self {
            _dir: dir,
            path,
            current,
            pot,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (
    Entity<ShellView>,
    Entity<Accounts>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = AccountStore::new(scratch.path.clone());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::from_store(store));
        ShellView::with_accounts(View::Transactions, accounts, window, cx)
    });
    let accounts = cx.update(|_, cx| shell.read(cx).accounts().clone());
    redraw(cx);
    (shell, accounts, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn shows(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).is_some())
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

/// Clicks the field `id` and types `text` into it.
fn type_into(id: &str, text: &str, cx: &mut VisualTestContext) {
    click(id, cx);
    let text = text.to_owned();
    cx.update(move |window, cx| window.input(&text, cx));
    redraw(cx);
}

fn press(key: &str, cx: &mut VisualTestContext) {
    let key = key.to_owned();
    cx.update(move |window, cx| window.press(&key, cx));
    redraw(cx);
}

fn selected(accounts: &Entity<Accounts>, cx: &mut VisualTestContext) -> Option<AccountId> {
    cx.update(|_, cx| accounts.read(cx).selected())
}

/// What accounts management shows: the inspector while Transactions shows
/// it (an account selected and no transaction), and the shell's dialog.
fn describe(
    shell: &Entity<ShellView>,
    accounts: &Entity<Accounts>,
    cx: &mut VisualTestContext,
) -> AccountsDescription {
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        let transactions = shell.transactions().read(cx);
        let inspector = transactions
            .selected()
            .is_empty()
            .then(|| transactions.account_inspector(cx))
            .flatten();
        accounts
            .read(cx)
            .describe(inspector.as_ref().map(|inspector| inspector.read(cx)), cx)
            .with_dialog(shell.dialog(), cx)
    })
}

/// Every account row, ACCOUNTS then ASSETS AND LIABILITIES.
fn rows(shown: &AccountsDescription) -> Vec<&AccountRow> {
    shown
        .sections
        .iter()
        .flat_map(|section| &section.rows)
        .collect()
}

/// The panel's accounts by name, with the balance each shows.
fn listed(shown: &AccountsDescription) -> Vec<(&str, &str)> {
    rows(shown)
        .into_iter()
        .map(|row| (row.name.as_str(), row.balance.as_str()))
        .collect()
}

/// The selected row's name, and whether it draws the cursor.
fn selected_row(shown: &AccountsDescription) -> Option<(&str, bool)> {
    rows(shown)
        .into_iter()
        .find(|row| row.selected)
        .map(|row| (row.name.as_str(), row.cursor))
}

fn inspector(shown: &AccountsDescription) -> &Inspector {
    shown
        .inspector
        .as_ref()
        .expect("the account inspector shows")
}

/// The inspector's section titled `title` (`None`: Details, the first).
fn section<'a>(shown: &'a AccountsDescription, title: Option<&str>) -> &'a Section {
    let sections = &inspector(shown).sections;
    match title {
        None => &sections[0],
        Some(title) => sections
            .iter()
            .find(|section| section.title.as_deref() == Some(title))
            .unwrap_or_else(|| panic!("no {title} section: {sections:?}")),
    }
}

/// The value of the row labelled `label` in `section`.
fn value<'a>(section: &'a Section, label: &str) -> Option<&'a str> {
    section
        .rows
        .iter()
        .find(|(row, _)| row == label)
        .map(|(_, value)| value.as_str())
}

/// The footer's buttons.
fn footer(shown: &AccountsDescription) -> Vec<&str> {
    let footer = inspector(shown).sections.last().unwrap();
    footer.actions.iter().map(|a| a.label.as_str()).collect()
}

fn dialog(shown: &AccountsDescription) -> &Dialog {
    shown.dialog.as_ref().expect("a dialog is open")
}

/// The open dialog's field labelled `label`.
fn field<'a>(dialog: &'a Dialog, label: &str) -> Option<&'a str> {
    dialog
        .fields
        .iter()
        .find(|(field, _)| field == label)
        .map(|(_, value)| value.as_str())
}

fn labels(actions: &[Action]) -> Vec<&str> {
    actions.iter().map(|action| action.label.as_str()).collect()
}

/// Whether the panel key `key` does anything now.
fn key_works(shown: &AccountsDescription, key: &str) -> bool {
    shown
        .keys
        .iter()
        .find(|action| action.key == Some(key))
        .is_some_and(|action| action.enabled)
}

fn row(id: AccountId) -> String {
    format!("account-{id}")
}

#[gpui_kit::test]
fn the_panel_lists_the_database_and_a_click_shows_the_account(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        listed(&shown),
        [("Current account", "£100.00"), ("Holiday", "No balance")]
    );
    assert!(rows(&shown)[1].nested, "the pot is listed under it");
    assert_eq!(shown.net_worth, ["£100.00"]);
    // The Transactions fixture opens with a transaction selected.
    assert_eq!(shown.inspector, None);

    click(&row(scratch.current), cx);
    assert_eq!(selected(&accounts, cx), Some(scratch.current));
    let transactions_selected =
        cx.update(|_, cx| shell.read(cx).transactions().read(cx).selected().len());
    assert_eq!(transactions_selected, 0, "the account takes the inspector");
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(selected_row(&shown), Some(("Current account", false)));
    assert_eq!(inspector(&shown).header.title, "Current account");
    assert_eq!(
        value(section(&shown, None), "Name"),
        Some("Current account")
    );
    assert_eq!(value(section(&shown, None), "Bank"), Some("Example Bank"));
    assert_eq!(
        labels(&section(&shown, Some("Balances")).actions),
        ["Add balance…"]
    );
    assert!(shows("add-balance", cx), "drawn as described");

    // A second click keeps it (SLI-70); Escape goes back to all accounts.
    click(&row(scratch.current), cx);
    assert_eq!(selected(&accounts, cx), Some(scratch.current));
    press(panel::CLEAR_KEY, cx);
    assert_eq!(selected(&accounts, cx), None, "Escape: all accounts");
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(shown.inspector, None);
    assert_eq!(selected_row(&shown), None);
    assert!(!shows("add-balance", cx));
}

#[gpui_kit::test]
fn new_account_adds_and_selects_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(panel::ADD_ACCOUNT, cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(dialog(&shown).title, "New account");
    assert_eq!(labels(&dialog(&shown).actions), ["Cancel", "Add account"]);

    type_into("new-account-name", "Amex", cx);
    type_into("new-account-bank", "American Express", cx);
    type_into("new-account-balance", "-42.50", cx);
    let shown = describe(&shell, &accounts, cx);
    let fields: Vec<&str> = dialog(&shown)
        .fields
        .iter()
        .map(|(label, _)| label.as_str())
        .collect();
    assert_eq!(fields, ["Name", "Bank", "Kind", "Currency", "Balance", ""]);
    assert_eq!(field(dialog(&shown), "Name"), Some("Amex"));
    assert_eq!(field(dialog(&shown), "Bank"), Some("American Express"));
    assert_eq!(field(dialog(&shown), "Kind"), Some("Current"));
    assert_eq!(field(dialog(&shown), "Balance"), Some("-42.50"));
    click("new-account-add", cx);

    let shown = describe(&shell, &accounts, cx);
    assert_eq!(shown.dialog, None, "the dialog closes");
    let added = account::list(&scratch.db())
        .unwrap()
        .into_iter()
        .find(|account| account.name == "Amex")
        .expect("Amex was added");
    assert_eq!(added.institution.as_deref(), Some("American Express"));
    assert_eq!(selected(&accounts, cx), Some(added.id));
    let balances = account::entered_balances(&scratch.db(), added.id).unwrap();
    assert_eq!(balances.len(), 1);
    assert_eq!(balances[0].balance.minor(), -4_250);
    assert!(listed(&shown).contains(&("Amex", "−£42.50")), "{shown:?}");
    assert_eq!(selected_row(&shown).map(|(name, _)| name), Some("Amex"));
}

#[gpui_kit::test]
fn new_account_says_what_is_wrong_and_adds_nothing(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(panel::ADD_ACCOUNT, cx);

    // No name.
    click("new-account-add", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        dialog(&shown).error.as_deref(),
        Some("An account needs a name.")
    );

    // A balance that would need rounding.
    type_into("new-account-name", "Amex", cx);
    type_into("new-account-balance", "12.345", cx);
    click("new-account-add", cx);
    let shown = describe(&shell, &accounts, cx);
    let error = dialog(&shown).error.clone().unwrap();
    assert!(error.starts_with("The balance isn't an amount"), "{error}");
    assert_eq!(account::list(&scratch.db()).unwrap().len(), 2);

    // Escape cancels.
    press("escape", cx);
    assert_eq!(describe(&shell, &accounts, cx).dialog, None);
    assert_eq!(account::list(&scratch.db()).unwrap().len(), 2);
}

#[gpui_kit::test]
fn the_inspector_saves_a_rename_and_refuses_a_blank_name(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);

    click("account-name", cx);
    press("end", cx);
    cx.update(|window, cx| window.input(" (Monzo)", cx));
    press("enter", cx);
    let name = account::get(&scratch.db(), scratch.current).unwrap().name;
    assert_eq!(name, "Current account (Monzo)");
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        selected_row(&shown).map(|(name, _)| name),
        Some("Current account (Monzo)"),
        "the panel reads it back"
    );
    assert_eq!(inspector(&shown).header.title, "Current account (Monzo)");

    // Clear the field: core refuses, and the stored name comes back.
    click("account-name", cx);
    press("secondary-a", cx);
    press("backspace", cx);
    press("enter", cx);
    assert_eq!(
        account::get(&scratch.db(), scratch.current).unwrap().name,
        "Current account (Monzo)"
    );
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        value(section(&shown, None), "Name"),
        Some("Current account (Monzo)"),
        "the stored name is back in the field"
    );
    let field = cx.update(|window, _| window.find("account-name").value().map(ToString::to_string));
    assert_eq!(
        field.as_deref(),
        Some("Current account (Monzo)"),
        "as drawn"
    );
}

#[gpui_kit::test]
fn add_balance_records_an_entered_balance(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click("add-balance", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(dialog(&shown).title, "Add a balance for Current account");

    type_into("add-balance-amount", "1,284.10", cx);
    // Replace today's date with one in the past.
    click("add-balance-date", cx);
    press("secondary-a", cx);
    cx.update(|window, cx| window.input("9 Nov 2026", cx));
    type_into("add-balance-note", "From the app", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        dialog(&shown).fields,
        [
            ("Balance".to_owned(), "1,284.10".to_owned()),
            (String::new(), "9 Nov 2026".to_owned()),
            ("Note".to_owned(), "From the app".to_owned()),
        ]
    );
    assert_eq!(labels(&dialog(&shown).actions), ["Cancel", "Add balance"]);
    click("add-balance-add", cx);

    let shown = describe(&shell, &accounts, cx);
    assert_eq!(shown.dialog, None, "the dialog closes");
    let balances = account::entered_balances(&scratch.db(), scratch.current).unwrap();
    assert_eq!(balances.len(), 2);
    assert_eq!(
        balances[0].date,
        NaiveDate::from_ymd_opt(2026, 11, 9).unwrap()
    );
    assert_eq!(balances[0].balance.minor(), 128_410);
    assert_eq!(balances[0].note.as_deref(), Some("From the app"));
    // The inspector lists it, newest first, and the panel shows it.
    let balances: Vec<&str> = section(&shown, Some("Balances"))
        .rows
        .iter()
        .map(|(_, value)| value.as_str())
        .collect();
    assert_eq!(
        balances,
        ["From the app · £1,284.10", "Entered by you · £100.00"]
    );
    assert_eq!(listed(&shown)[0], ("Current account", "£1,284.10"));
}

#[gpui_kit::test]
fn add_balance_says_before_a_click_that_the_day_has_a_balance(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click("add-balance", cx);

    // The account has an entered balance on 1 Nov.
    retype_date("add-balance-date", "1 Nov 2026", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        dialog(&shown).error.as_deref(),
        Some("This account already has a balance on 1 Nov 2026. Choose another day.")
    );
    let enabled = |shown: &AccountsDescription| -> Vec<bool> {
        dialog(shown)
            .actions
            .iter()
            .map(|action| action.enabled)
            .collect()
    };
    assert_eq!(enabled(&shown), [true, false], "Add balance waits (IRD-0)");

    // Nothing is added while it shows, and it stays up.
    type_into("add-balance-amount", "5.00", cx);
    click("add-balance-add", cx);
    let shown = describe(&shell, &accounts, cx);
    assert!(dialog(&shown).error.is_some());
    assert_eq!(
        account::entered_balances(&scratch.db(), scratch.current)
            .unwrap()
            .len(),
        1
    );

    // A free day, and a date that can't be read yet, say nothing.
    retype_date("add-balance-date", "2 Nov 2026", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(dialog(&shown).error, None);
    assert_eq!(enabled(&shown), [true, true]);
    retype_date("add-balance-date", "2 No", cx);
    assert_eq!(dialog(&describe(&shell, &accounts, cx)).error, None);
}

#[gpui_kit::test]
fn a_cash_count_says_before_a_click_that_the_day_is_counted(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let cash = cash_account(&scratch, true);
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(cash), cx);
    click("add-balance", cx);
    retype_date("count-date", "1 Nov 2026", cx);
    type_into("count-amount", "18.00", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        dialog(&shown).error.as_deref(),
        Some("This account already has a count on 1 Nov 2026. Choose another day.")
    );
    assert!(
        dialog(&shown).actions[1..]
            .iter()
            .all(|action| !action.enabled),
        "saving waits"
    );
    click("count-add", cx);
    assert!(
        describe(&shell, &accounts, cx).dialog.is_some(),
        "still open"
    );
    assert_eq!(
        account::entered_balances(&scratch.db(), cash)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(transaction_count(&scratch, cash), 1, "no difference added");
}

#[gpui_kit::test]
fn add_balance_clears_a_refusal_when_a_field_changes(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click("add-balance", cx);
    click("add-balance-add", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        dialog(&shown).error.as_deref(),
        Some("Type the balance, as 1,284.10.")
    );

    type_into("add-balance-amount", "1", cx);
    assert_eq!(dialog(&describe(&shell, &accounts, cx)).error, None);
}

#[gpui_kit::test]
fn add_balance_words_a_refusal_with_the_fields_dates(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click("add-balance", cx);
    // A balance added elsewhere since the dialog read the account.
    let mut db = scratch.db();
    let elsewhere = EnteredBalance {
        date: NaiveDate::from_ymd_opt(2026, 11, 17).unwrap(),
        balance: Money::new(1, Currency::GBP),
        source: EnteredSource::Manual,
        note: None,
    };
    account::add_balance(&mut db, scratch.current, &elsewhere).unwrap();

    type_into("add-balance-amount", "5.00", cx);
    retype_date("add-balance-date", "17 Nov 2026", cx);
    click("add-balance-add", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        dialog(&shown).error.as_deref(),
        Some("The account already has an entered balance on 17 Nov 2026; edit that one instead.")
    );
}

#[gpui_kit::test]
fn sample_data_cant_change(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let accounts = cx.new(|_| Accounts::fixture(fixture::accounts()));
        let result = accounts.update(cx, |accounts, cx| {
            accounts.create(
                &NewAccount::new("X", AccountKind::Cash, Currency::GBP),
                None,
                cx,
            )
        });
        assert!(result.is_err());
    });
}

#[gpui_kit::test]
fn the_arrow_keys_move_through_the_panel(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    let shown = describe(&shell, &accounts, cx);
    assert!(key_works(&shown, "down") && !key_works(&shown, "escape"));
    click(&row(scratch.current), cx);
    assert!(key_works(&describe(&shell, &accounts, cx), "escape"));
    press("down", cx);
    assert_eq!(
        selected_row(&describe(&shell, &accounts, cx)),
        Some(("Holiday", true)),
        "the pot is next, drawn with the cursor"
    );
    press("down", cx);
    assert_eq!(
        selected(&accounts, cx),
        Some(scratch.pot),
        "it stops at the end"
    );
    press("up", cx);
    assert_eq!(selected(&accounts, cx), Some(scratch.current));
    press("escape", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(selected_row(&shown), None, "Escape: all accounts");
    assert!(!key_works(&shown, "escape"));
}

#[gpui_kit::test]
fn the_kind_select_changes_the_kind_by_pointer_and_keyboard(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    // An account with nothing in it, so every kind can be chosen.
    let empty = account::create(
        &mut scratch.db(),
        &NewAccount::new("Card", AccountKind::Current, Currency::GBP),
    )
    .unwrap();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(empty), cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(value(section(&shown, None), "Kind"), Some("Current"));
    assert_eq!(
        section(&shown, None).actions,
        [
            Action::button("Kind", true),
            Action::button("Currency", true),
            Action::button("Preset", true),
        ],
        "nothing in it: every select opens"
    );

    click("inspector-kind", cx);
    assert!(shows("option-credit_card", cx), "the menu is open");
    click("option-credit_card", cx);
    assert!(!shows("option-credit_card", cx), "choosing closes it");
    assert_eq!(
        account::get(&scratch.db(), empty).unwrap().kind,
        AccountKind::CreditCard
    );

    // From the keyboard: the focused trigger opens on Enter, the arrows
    // move, Enter chooses (Credit card → Cash), Escape would close.
    press("enter", cx);
    assert!(shows("option-cash", cx));
    press("down", cx);
    press("enter", cx);
    assert_eq!(
        account::get(&scratch.db(), empty).unwrap().kind,
        AccountKind::Cash
    );
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(value(section(&shown, None), "Kind"), Some("Cash"));
    assert_eq!(
        value(section(&shown, None), "Bank"),
        None,
        "a cash account has no Bank"
    );
    assert!(!shows("option-cash", cx), "choosing closes it");
    // Focus is back on the select: Enter opens it again, Escape closes it.
    press("enter", cx);
    assert!(shows("option-cash", cx));
    press("escape", cx);
    assert!(!shows("option-cash", cx));

    // Pressing the field while the menu is open closes it.
    click("inspector-kind", cx);
    assert!(shows("option-cash", cx));
    click("inspector-kind", cx);
    assert!(!shows("option-cash", cx), "a second press closes it");
}

#[gpui_kit::test]
fn an_edit_left_by_clicking_another_account_stays_with_its_account(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click("account-name", cx);
    press("end", cx);
    cx.update(|window, cx| window.input(" 2", cx));
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        value(section(&shown, None), "Name"),
        Some("Current account 2"),
        "typed, not saved yet"
    );
    assert_eq!(
        selected_row(&shown).map(|(name, _)| name),
        Some("Current account")
    );
    // No Enter: the next click takes focus and changes the selection.
    click(&row(scratch.pot), cx);
    assert_eq!(selected(&accounts, cx), Some(scratch.pot));

    let db = scratch.db();
    assert_eq!(
        account::get(&db, scratch.current).unwrap().name,
        "Current account 2"
    );
    assert_eq!(
        account::get(&db, scratch.pot).unwrap().name,
        "Holiday",
        "the pot is untouched"
    );
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(value(section(&shown, None), "Name"), Some("Holiday"));
    assert!(listed(&shown).contains(&("Current account 2", "£100.00")));
}

// Assets, liabilities, closing and deleting (M3.17) ---------------------------

/// Adds a valued account to the scratch database, with one value.
fn valued(scratch: &Scratch, name: &str, kind: AccountKind, minor: i64) -> AccountId {
    let value = EnteredBalance {
        date: NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
        balance: Money::new(minor, Currency::GBP),
        source: EnteredSource::Valuation,
        note: None,
    };
    account::create_with_balance(
        &mut scratch.db(),
        &NewAccount::new(name, kind, Currency::GBP),
        Some(&value),
    )
    .unwrap()
}

/// Replaces the date in the field `id` with `date`.
fn retype_date(id: &str, date: &str, cx: &mut VisualTestContext) {
    click(id, cx);
    press("secondary-a", cx);
    let date = date.to_owned();
    cx.update(move |window, cx| window.input(&date, cx));
    redraw(cx);
}

#[gpui_kit::test]
fn the_assets_plus_adds_an_asset_shown_by_its_values(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(panel::ADD_VALUED_ACCOUNT, cx);
    // Kind is Asset: no Bank, Value for Balance, and a Note.
    let shown = describe(&shell, &accounts, cx);
    let fields: Vec<&str> = dialog(&shown)
        .fields
        .iter()
        .map(|(label, _)| label.as_str())
        .collect();
    assert_eq!(fields, ["Name", "Kind", "Currency", "Value", "", "Note"]);
    assert_eq!(field(dialog(&shown), "Kind"), Some("Asset"));
    assert!(!shows("new-account-bank", cx), "drawn as described");
    type_into("new-account-name", "House", cx);
    type_into("new-account-balance", "385,000", cx);
    retype_date("new-account-date", "12 Oct 2026", cx);
    type_into("new-account-note", "Remortgage survey", cx);
    click("new-account-add", cx);
    assert_eq!(
        describe(&shell, &accounts, cx).dialog,
        None,
        "the dialog closes"
    );

    let house = account::list(&scratch.db())
        .unwrap()
        .into_iter()
        .find(|account| account.name == "House")
        .expect("House was added");
    assert_eq!(house.kind, AccountKind::Asset);
    let values = account::entered_balances(&scratch.db(), house.id).unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].balance.minor(), 38_500_000);
    assert_eq!(
        values[0].source,
        orre_core::reconciliation::BalanceSource::Valuation
    );
    assert_eq!(values[0].note.as_deref(), Some("Remortgage survey"));
    assert_eq!(selected(&accounts, cx), Some(house.id));

    // Its values replace the transaction table, and ⌘N adds one.
    let shown = describe(&shell, &accounts, cx);
    let assets = &shown.sections[1];
    assert_eq!(assets.title, "Assets and liabilities");
    assert_eq!(assets.rows[0].name, "House");
    let canvas = shown.valuations.as_ref().expect("the valuations canvas");
    assert_eq!(canvas.title, "Valuations");
    assert_eq!(
        canvas.add,
        Action::keyed("Add valuation", panel::ADD_VALUE_KEY, true)
    );
    assert_eq!(canvas.rows.len(), 1);
    assert_eq!(canvas.rows[0].note, "Remortgage survey · first value");
    assert_eq!(canvas.rows[0].value, "£385,000.00");
    assert_eq!(canvas.rows[0].change, "—");
    assert!(shows("add-valuation", cx));
    // Clicking the row twice lets it go and selects it again, with the
    // panel focused: ⌘N works there.
    click(&row(house.id), cx);
    click(&row(house.id), cx);
    assert_eq!(selected(&accounts, cx), Some(house.id));
    assert!(key_works(
        &describe(&shell, &accounts, cx),
        panel::ADD_VALUE_KEY
    ));
    press("secondary-n", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        dialog(&shown).title,
        "Add a valuation for House",
        "⌘N opens Add valuation"
    );
    type_into("add-balance-amount", "390,000", cx);
    retype_date("add-balance-date", "1 Nov 2026", cx);
    click("add-balance-add", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(shown.dialog, None);
    let values = account::entered_balances(&scratch.db(), house.id).unwrap();
    assert_eq!(values[0].balance.minor(), 39_000_000);
    let canvas = shown.valuations.as_ref().unwrap();
    assert_eq!(
        canvas.rows[0],
        super::describe::ValuationRowDescription {
            date: "1 Nov 2026".to_owned(),
            note: "Valuation".to_owned(),
            change: "+£5,000.00".to_owned(),
            value: "£390,000.00".to_owned(),
        }
    );
    assert!(canvas.caption.starts_with("2 values you entered"));

    // A bank account goes back to the table.
    click(&row(scratch.current), cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(shown.valuations, None);
    assert!(!key_works(&shown, panel::ADD_VALUE_KEY));
    assert!(!shows("add-valuation", cx));
}

#[gpui_kit::test]
fn a_liability_stores_what_is_owed_below_zero(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let mortgage = valued(&scratch, "Mortgage", AccountKind::Liability, -21_691_000);
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(mortgage), cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        labels(&section(&shown, Some("Balances")).actions),
        ["Add balance…"]
    );
    assert!(
        section(&shown, Some("Balances")).rows.is_empty(),
        "the canvas lists them"
    );
    click("add-balance", cx);
    // A minus is refused: Orre stores what's owed below zero itself.
    type_into("add-balance-amount", "-1", cx);
    retype_date("add-balance-date", "1 Nov 2026", cx);
    click("add-balance-add", cx);
    let shown = describe(&shell, &accounts, cx);
    assert!(
        dialog(&shown)
            .error
            .as_deref()
            .unwrap()
            .contains("without a minus")
    );

    click("add-balance-amount", cx);
    press("secondary-a", cx);
    cx.update(|window, cx| window.input("212,640", cx));
    redraw(cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(field(dialog(&shown), "Owed"), Some("212,640"));
    let caption = &dialog(&shown).fields.last().unwrap().1;
    assert!(
        caption.contains("Orre stores it as −£212,640.00"),
        "the caption says what will be stored: {caption}"
    );
    click("add-balance-add", cx);
    assert_eq!(describe(&shell, &accounts, cx).dialog, None);
    let values = account::entered_balances(&scratch.db(), mortgage).unwrap();
    assert_eq!(values[0].balance.minor(), -21_264_000);
}

#[gpui_kit::test]
fn closing_an_asset_on_a_day_that_has_come_hides_it_at_once(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let house = valued(&scratch, "House", AccountKind::Asset, 38_500_000);
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(house), cx);
    click("close-account", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(dialog(&shown).title, "Close House?");
    assert_eq!(labels(&dialog(&shown).actions), ["Cancel", "Close account"]);

    // Not on or before its last value.
    retype_date("close-account-date", "12 Oct 2026", cx);
    click("close-account-close", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(field(dialog(&shown), "Sold on"), Some("12 Oct 2026"));
    let error = dialog(&shown).error.clone().expect("refused");
    assert!(error.contains("2026-10-13"), "{error}");
    assert_eq!(account::get(&scratch.db(), house).unwrap().closed_on, None);

    retype_date("close-account-date", "6 Nov 2026", cx);
    click("close-account-close", cx);
    assert_eq!(
        account::get(&scratch.db(), house).unwrap().closed_on,
        NaiveDate::from_ymd_opt(2026, 11, 6)
    );
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(shown.dialog, None, "the dialog closes");
    assert!(
        !listed(&shown).iter().any(|(name, _)| *name == "House"),
        "it leaves the panel"
    );
    assert_eq!(selected(&accounts, cx), None);
    // The day has come: the bar says so, with no Undo (SLI-77).
    assert_eq!(
        shown.undo_bar,
        Some(("House closed".to_owned(), Vec::new()))
    );
    assert!(!shows("undo-change", cx));
    assert!(shows("undo-dismiss", cx));
}

/// SLI-77: the panel and its net worth are as of today. A liability paid
/// off tomorrow stays, counted, with "· closes {day}" after its name, and
/// the bar's Undo reopens it; on the day itself it leaves.
#[gpui_kit::test]
fn closing_on_a_later_day_keeps_the_account_until_then(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let loan = valued(&scratch, "QA Loan", AccountKind::Liability, -500_000);
    let (shell, accounts, cx) = open(cx, &scratch);
    let today = crate::today();
    let tomorrow = today.succ_opt().unwrap();
    let day = crate::format::list_date(tomorrow, today);
    let before = describe(&shell, &accounts, cx).net_worth;
    assert_eq!(before, ["−£4,900.00"]);

    let close = |cx: &mut VisualTestContext| {
        click(&row(loan), cx);
        click("close-account", cx);
        retype_date(
            "close-account-date",
            &crate::format::field_date(tomorrow),
            cx,
        );
        click("close-account-close", cx);
    };
    close(cx);
    assert_eq!(
        account::get(&scratch.db(), loan).unwrap().closed_on,
        Some(tomorrow)
    );
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(shown.dialog, None, "the dialog closes");
    let row_of = |shown: &AccountsDescription| {
        rows(shown)
            .into_iter()
            .find(|row| row.name == "QA Loan")
            .cloned()
    };
    let shown_row = row_of(&shown).expect("it stays in the panel");
    assert_eq!(shown_row.balance, "−£5,000.00", "at what's owed today");
    assert_eq!(shown_row.closes, Some(format!("· closes {day}")));
    assert_eq!(shown.net_worth, before, "still counted");
    assert_eq!(
        shown.undo_bar,
        Some((
            format!("QA Loan closes on {day}"),
            vec![Action::button("Undo", true)]
        ))
    );
    // It can't close again; the context menu agrees.
    if let Some(inspector) = &shown.inspector {
        assert!(
            !inspector
                .sections
                .iter()
                .any(|section| labels(&section.actions).contains(&"Close account…")),
            "no Close account… while it closes"
        );
    }
    assert!(!shows("close-account", cx));

    // Undo reopens it, without the £0 value closing added.
    click("undo-change", cx);
    assert_eq!(account::get(&scratch.db(), loan).unwrap().closed_on, None);
    assert_eq!(
        account::entered_balances(&scratch.db(), loan)
            .unwrap()
            .len(),
        1
    );
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(row_of(&shown).unwrap().closes, None);
    assert_eq!(shown.undo_bar, None);

    // Closed again: on the day itself (the shell's clock tick passes the
    // new date), it leaves the panel and net worth rises by what was owed.
    close(cx);
    accounts.update(cx, |accounts, cx| accounts.follow_day(today, cx));
    redraw(cx);
    assert!(
        row_of(&describe(&shell, &accounts, cx)).is_some(),
        "same day: no read"
    );
    accounts.update(cx, |accounts, cx| accounts.follow_day(tomorrow, cx));
    redraw(cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(row_of(&shown), None, "it leaves on its closing day");
    assert_eq!(shown.net_worth, ["£100.00"]);
    assert_eq!(selected(&accounts, cx), None);
}

#[gpui_kit::test]
fn delete_shows_only_where_core_allows_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let house = valued(&scratch, "House", AccountKind::Asset, 38_500_000);
    let (shell, accounts, cx) = open(cx, &scratch);

    // Current account has a pot: close only.
    click(&row(scratch.current), cx);
    assert_eq!(footer(&describe(&shell, &accounts, cx)), ["Close account…"]);
    assert!(!shows("delete-account", cx), "drawn as described");

    click(&row(house), cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(footer(&shown), ["Close account…", "Delete account…"]);
    let note = &inspector(&shown).sections.last().unwrap().rows[0].1;
    assert!(
        note.starts_with("Sold it? Close it on the sale date"),
        "{note}"
    );
    click("delete-account", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(dialog(&shown).title, "Delete House?");
    assert_eq!(
        labels(&dialog(&shown).actions),
        ["Cancel", "Delete account"]
    );
    click("delete-account-delete", cx);
    assert!(account::get(&scratch.db(), house).is_err(), "deleted");
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(shown.dialog, None);
    assert!(!listed(&shown).iter().any(|(name, _)| *name == "House"));
    assert_eq!(selected(&accounts, cx), None);
}

/// A cash account counted at £50.00 on 1 Nov with £27.00 spent since: Orre
/// expects £23.00.
fn cash_account(scratch: &Scratch, counted: bool) -> AccountId {
    let mut db = scratch.db();
    let cash = account::create(
        &mut db,
        &NewAccount::new("Cash", AccountKind::Cash, Currency::GBP),
    )
    .unwrap();
    if counted {
        let on = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
        let count = EnteredBalance {
            date: on(1),
            balance: Money::new(5_000, Currency::GBP),
            source: EnteredSource::Manual,
            note: None,
        };
        account::count::record(&mut db, cash, &count, false).unwrap();
        let spent = orre_core::manual::ManualTransaction::new(
            cash,
            on(4),
            Money::new(-2_700, Currency::GBP),
            "Market",
        );
        orre_core::manual::create(&mut db, &spent).unwrap();
    }
    cash
}

fn count_dialog(
    shell: &Entity<ShellView>,
    cx: &mut VisualTestContext,
) -> Option<add_balance::Counting> {
    cx.update(|_, cx| {
        let dialog = shell.read(cx).dialog()?.clone();
        let dialog = dialog.downcast::<add_balance::AddBalanceDialog>().ok()?;
        dialog.read(cx).counting().copied()
    })
}

fn transaction_count(scratch: &Scratch, cash: AccountId) -> usize {
    account::usage(&scratch.db(), cash).unwrap().transactions
}

#[gpui_kit::test]
fn a_short_cash_count_offers_its_difference_and_saves_both(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let cash = cash_account(&scratch, true);
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(cash), cx);
    let shown = describe(&shell, &accounts, cx);
    let counts = section(&shown, Some("Counts"));
    assert_eq!(labels(&counts.actions), ["Count cash…"]);
    assert_eq!(counts.rows[0].1, "Starting point · £50.00");
    click("add-balance", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(dialog(&shown).title, "Count cash for Cash");
    assert_eq!(
        labels(&dialog(&shown).actions),
        ["Cancel", "Save count"],
        "nothing to add before a count"
    );

    type_into("count-amount", "18.00", cx);
    let counting = count_dialog(&shell, cx).expect("the count can be read");
    assert_eq!(counting.expectation.expected.unwrap().minor(), 2_300);
    let shown = describe(&shell, &accounts, cx);
    let expected = field(dialog(&shown), "Orre").unwrap();
    assert!(
        expected.starts_with("£23.00 from transactions since the 1 Nov"),
        "{expected}"
    );
    assert_eq!(field(dialog(&shown), "Difference"), Some("−£5.00"));
    assert_eq!(
        labels(&dialog(&shown).actions),
        ["Cancel", "Save count only", "Add −£5.00 and save"]
    );
    assert!(shows("count-add", cx), "drawn as described");

    click("count-add", cx);
    assert_eq!(
        describe(&shell, &accounts, cx).dialog,
        None,
        "the dialog closes"
    );
    let counts = account::entered_balances(&scratch.db(), cash).unwrap();
    assert_eq!(counts.len(), 2);
    assert_eq!(counts[0].balance.minor(), 1_800);
    assert_eq!(transaction_count(&scratch, cash), 2);
}

#[gpui_kit::test]
fn saving_a_cash_count_only_adds_no_transaction(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let cash = cash_account(&scratch, true);
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(cash), cx);
    click("add-balance", cx);
    type_into("count-amount", "18.00", cx);
    click("count-save-only", cx);
    assert_eq!(describe(&shell, &accounts, cx).dialog, None);
    assert_eq!(
        account::entered_balances(&scratch.db(), cash)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(transaction_count(&scratch, cash), 1);
}

#[gpui_kit::test]
fn a_matching_count_has_a_plain_save_and_the_first_count_too(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let cash = cash_account(&scratch, true);
    let empty = {
        let mut db = scratch.db();
        account::create(
            &mut db,
            &NewAccount::new("Wallet", AccountKind::Cash, Currency::GBP),
        )
        .unwrap()
    };
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(cash), cx);
    click("add-balance", cx);
    type_into("count-amount", "23.00", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(field(dialog(&shown), "Difference"), Some("£0.00"));
    assert_eq!(labels(&dialog(&shown).actions), ["Cancel", "Save count"]);
    press("escape", cx);

    click(&row(empty), cx);
    click("add-balance", cx);
    type_into("count-amount", "12.00", cx);
    let counting = count_dialog(&shell, cx).unwrap();
    assert_eq!(counting.expectation.expected, None, "the starting point");
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        dialog(&shown).fields,
        [
            ("Counted".to_owned(), "12.00".to_owned()),
            (
                String::new(),
                cx.update(|_, cx| crate::format::field_date(accounts.read(cx).data().today))
            ),
        ],
        "no expectation or difference on the first count"
    );
    assert_eq!(labels(&dialog(&shown).actions), ["Cancel", "Save count"]);
    click("count-save", cx);
    assert_eq!(
        account::entered_balances(&scratch.db(), empty)
            .unwrap()
            .len(),
        1
    );
}

/// The shell on Transactions over the states board's accounts, where Joint
/// account's balance check failed.
fn open_flagged(
    cx: &mut TestAppContext,
) -> (Entity<ShellView>, Entity<Accounts>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(fixture::with_states()));
        ShellView::with_accounts(View::Transactions, accounts, window, cx)
    });
    let accounts = cx.update(|_, cx| shell.read(cx).accounts().clone());
    redraw(cx);
    (shell, accounts, cx)
}

/// What the panel asked the shell for, as it happens.
fn requests(
    accounts: &Entity<Accounts>,
    cx: &mut VisualTestContext,
) -> std::rc::Rc<std::cell::RefCell<Vec<AccountsEvent>>> {
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        cx.subscribe(accounts, move |_, event: &AccountsEvent, _| {
            sink.borrow_mut().push(*event)
        })
        .detach()
    });
    seen
}

#[gpui_kit::test]
fn enter_on_a_flagged_account_opens_its_check_as_the_dot_does(cx: &mut TestAppContext) {
    let (shell, accounts, cx) = open_flagged(cx);
    let seen = requests(&accounts, cx);
    let shown = describe(&shell, &accounts, cx);
    let flagged: Vec<&str> = rows(&shown)
        .into_iter()
        .filter(|row| row.flagged)
        .map(|row| row.name.as_str())
        .collect();
    assert_eq!(flagged, ["Joint account"]);
    // Clicking the row selects it and focuses the panel.
    click(&row(fixture::JOINT), cx);
    assert_eq!(selected(&accounts, cx), Some(fixture::JOINT));
    assert!(key_works(
        &describe(&shell, &accounts, cx),
        panel::CHECK_KEY
    ));

    press("enter", cx);
    assert_eq!(
        seen.borrow().last(),
        Some(&AccountsEvent::ChecksRequested(fixture::JOINT)),
        "what the flagged dot asks for"
    );
    assert_eq!(cx.update(|_, cx| shell.read(cx).view()), View::Inbox);
}

#[gpui_kit::test]
fn enter_on_an_account_that_isnt_flagged_does_nothing(cx: &mut TestAppContext) {
    let (shell, accounts, cx) = open_flagged(cx);
    let seen = requests(&accounts, cx);
    click(&row(fixture::CURRENT), cx);
    let before = seen.borrow().len();
    assert!(!key_works(
        &describe(&shell, &accounts, cx),
        panel::CHECK_KEY
    ));

    press("enter", cx);
    assert_eq!(seen.borrow().len(), before, "nothing asked for");
    assert_eq!(cx.update(|_, cx| shell.read(cx).view()), View::Transactions);
    assert_eq!(selected(&accounts, cx), Some(fixture::CURRENT));

    // The arrows go on to Joint, and there Enter opens its check.
    let order = cx.update(|_, cx| panel::order(accounts.read(cx).data()));
    let from = order.iter().position(|id| *id == fixture::CURRENT).unwrap();
    let to = order.iter().position(|id| *id == fixture::JOINT).unwrap();
    for _ in from..to {
        press("down", cx);
    }
    assert_eq!(selected(&accounts, cx), Some(fixture::JOINT));
    press("enter", cx);
    assert_eq!(cx.update(|_, cx| shell.read(cx).view()), View::Inbox);
}

/// The whole description of the sample accounts with Current account
/// selected: the panel, the inspector and the keys, field by field.
#[gpui_kit::test]
fn the_sample_accounts_are_described_in_full(cx: &mut TestAppContext) {
    let (shell, accounts, cx) = open_flagged(cx);
    click(&row(fixture::CURRENT), cx);
    // A row: its name and balance, overdrawn, nested, flagged, selected.
    let row = |name: &str, balance: &str, overdrawn, nested, flagged, selected| AccountRow {
        name: name.to_owned(),
        balance: balance.to_owned(),
        overdrawn,
        nested,
        closes: None,
        flagged,
        selected,
        cursor: false,
    };
    let rows = |rows: &[(&str, &str)]| -> Vec<(String, String)> {
        rows.iter()
            .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
            .collect()
    };
    let button = |label: &str, enabled: bool| Action::button(label, enabled);
    let key = |label: &str, key: &'static str, enabled: bool| Action::keyed(label, key, enabled);
    assert_eq!(
        describe(&shell, &accounts, cx),
        AccountsDescription {
            net_worth: vec!["£18,240.55".to_owned()],
            sections: vec![
                PanelSection {
                    title: "Accounts",
                    add: button("Add account", true),
                    rows: vec![
                        row("Current account", "£1,284.10", false, false, false, true),
                        row("Holiday", "£1,200.00", false, true, false, false),
                        row("Rainy day", "£3,000.00", false, true, false, false),
                        row("Joint account", "−£24.10", true, false, true, false),
                        row("Savings", "£5,750.00", false, false, false, false),
                        row("Stocks ISA", "£7,030.55", false, false, false, false),
                        row("Amex", "No balance", false, false, false, false),
                    ],
                    note: None,
                },
                PanelSection {
                    title: "Assets and liabilities",
                    add: button("Add asset or liability", true),
                    rows: Vec::new(),
                    note: None,
                },
            ],
            error: None,
            inspector: Some(Inspector {
                header: Header {
                    title: "Current account".to_owned(),
                    figure: Some("£1,284.10".to_owned()),
                    caption: Some("Balance on 9 Nov · pots not included".to_owned()),
                },
                sections: vec![
                    Section {
                        title: None,
                        rows: rows(&[
                            ("Name", "Current account"),
                            ("Bank", "Monzo"),
                            ("Kind", "Current"),
                            ("Currency", "GBP"),
                            ("Preset", "Monzo"),
                        ]),
                        // It holds money: the currency is locked.
                        actions: vec![
                            button("Kind", true),
                            button("Currency", false),
                            button("Preset", true),
                        ],
                    },
                    Section {
                        title: Some("Balances".to_owned()),
                        rows: rows(&[("1 Sep", "Entered by you · £1,071.70")]),
                        actions: vec![button("Add balance…", true)],
                    },
                    Section {
                        title: Some("Pots".to_owned()),
                        rows: rows(&[
                            ("Holiday", "£1,200.00"),
                            ("Rainy day", "£3,000.00"),
                            ("Total", "£4,200.00"),
                        ]),
                        actions: Vec::new(),
                    },
                    Section {
                        title: Some("Imports".to_owned()),
                        rows: rows(&[
                            ("monzo-november.csv", "296 · 9 Nov"),
                            ("monzo-october.csv", "281 · 2 Nov"),
                        ]),
                        actions: Vec::new(),
                    },
                    // It has pots and imports: close only.
                    Section {
                        title: None,
                        rows: Vec::new(),
                        actions: vec![button("Close account…", true)],
                    },
                ],
            }),
            valuations: None,
            undo_bar: None,
            context_menu: None,
            dialog: None,
            keys: vec![
                key("Move up", "up", true),
                key("Move down", "down", true),
                key("All accounts", "escape", true),
                key("Open its balance check", "enter", false),
                key("Add valuation", "secondary-n", false),
            ],
        }
    );
}

/// Account and pot names can come from an import or a preset, so the
/// panel, the inspector, the dialogs, the undo bar and the description
/// show them made plain ([`super::name`]: bidi, zero-width and control
/// characters stripped), while the Name field holds the stored name as
/// typed and an edit there round-trips it.
#[gpui_kit::test]
fn account_names_are_shown_and_described_plain(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let name = "Bills\u{202E} pot\u{200B} <b>ignore</b>";
    let plain = "Bills pot <b>ignore</b>";
    assert_eq!(super::name(name), plain);
    let id = account::create(
        &mut scratch.db(),
        &NewAccount::new(name, AccountKind::Current, Currency::GBP),
    )
    .unwrap();
    let mut pot = NewAccount::new("Rainy\u{7}\u{2066}day", AccountKind::Pot, Currency::GBP);
    pot.parent = Some(id);
    account::create(&mut scratch.db(), &pot).unwrap();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(id), cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(selected_row(&shown).map(|(name, _)| name), Some(plain));
    assert!(
        rows(&shown)
            .iter()
            .any(|row| row.nested && row.name == "Rainyday"),
        "the pot's row: {:?}",
        listed(&shown)
    );
    assert_eq!(inspector(&shown).header.title, plain);
    assert!(
        value(section(&shown, Some("Pots")), "Rainyday").is_some(),
        "the Pots section"
    );
    assert_eq!(
        value(section(&shown, None), "Name"),
        Some(name),
        "the Name field holds the stored name"
    );
    let drawn = cx.update(|window, _| window.find("account-name").value().map(ToString::to_string));
    assert_eq!(drawn.as_deref(), Some(name), "the field holds it as typed");

    // An edit in the field keeps the stored text: it round-trips.
    click("account-name", cx);
    press("end", cx);
    cx.update(|window, cx| window.input(" 2", cx));
    press("enter", cx);
    let stored = format!("{name} 2");
    assert_eq!(account::get(&scratch.db(), id).unwrap().name, stored);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        selected_row(&shown).map(|(name, _)| name),
        Some(format!("{plain} 2").as_str())
    );
    assert_eq!(value(section(&shown, None), "Name"), Some(stored.as_str()));

    // The Close dialog's title and the undo bar after it.
    click("close-account", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(dialog(&shown).title, format!("Close {plain} 2?"));
    click("close-account-close", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(shown.dialog, None, "it closed");
    assert_eq!(
        shown.undo_bar.map(|(message, _)| message),
        Some(format!("{plain} 2 closed"))
    );
}

/// An [`Accounts::unloaded`] store as the panel describes it before any read
/// is back, then a shell over it (which starts the read, so it is read by the
/// time the window is up).
fn open_unloaded<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (
    AccountsDescription,
    Entity<ShellView>,
    Entity<Accounts>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let unread = cx.new(|_| Accounts::unloaded(AccountStore::new(scratch.path.clone())));
    let before = cx.update(|cx| {
        assert!(!unread.read(cx).loaded());
        unread.read(cx).describe(None, cx)
    });
    let (shell, cx) = cx.add_window_view(|window, cx| {
        ShellView::with_accounts(View::Transactions, unread.clone(), window, cx)
    });
    (before, shell, unread, cx)
}

/// Records the events `accounts` emits.
fn record_events(
    accounts: &Entity<Accounts>,
    cx: &mut VisualTestContext,
) -> std::rc::Rc<std::cell::RefCell<Vec<AccountsEvent>>> {
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(accounts, move |_, event: &AccountsEvent, _| {
            sink.borrow_mut().push(*event)
        })
        .detach()
    });
    events
}

#[gpui_kit::test]
fn reads_in_the_background_and_says_nothing_until_it_is_back(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (before, shell, accounts, cx) = open_unloaded(cx, &scratch);
    assert!(rows(&before).is_empty());
    assert_eq!(before.sections[0].note, None, "not \"No accounts yet\"");
    assert_eq!(before.sections[1].note, None);
    assert_eq!(before.net_worth, [panel::NO_NET_WORTH]);

    redraw(cx);
    assert!(cx.update(|_, cx| accounts.read(cx).loaded()));
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        listed(&shown),
        [("Current account", "£100.00"), ("Holiday", "No balance")]
    );
    assert_eq!(shown.error, None);
}

#[gpui_kit::test]
fn a_database_with_no_accounts_says_so_once_it_is_read(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let scratch = Scratch {
        path: dir.path().join("orre.db"),
        _dir: dir,
        current: AccountId::new(0),
        pot: AccountId::new(0),
    };
    Database::open(&scratch.path).unwrap();
    let (before, shell, accounts, cx) = open_unloaded(cx, &scratch);
    assert_eq!(before.sections[0].note, None);
    redraw(cx);
    assert_eq!(
        describe(&shell, &accounts, cx).sections[0].note,
        Some(panel::NO_ACCOUNTS)
    );
}

#[gpui_kit::test]
fn a_reload_keeps_showing_the_last_read_until_the_new_one_lands(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    let house = valued(&scratch, "House", AccountKind::Asset, 38_500_000);
    cx.update(|_, cx| accounts.update(cx, |accounts, cx| accounts.reload(cx)));
    let before = describe(&shell, &accounts, cx);
    assert!(!listed(&before).iter().any(|(name, _)| *name == "House"));
    assert!(!cx.update(|_, cx| accounts.read(cx).data().contains(house)));

    redraw(cx);
    let after = describe(&shell, &accounts, cx);
    assert!(listed(&after).iter().any(|(name, _)| *name == "House"));
    assert!(cx.update(|_, cx| accounts.read(cx).data().contains(house)));
}

#[gpui_kit::test]
fn a_selected_account_deleted_elsewhere_is_let_go_when_the_read_lands(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let house = valued(&scratch, "House", AccountKind::Asset, 38_500_000);
    let (_shell, accounts, cx) = open(cx, &scratch);
    let events = record_events(&accounts, cx);
    click(&row(house), cx);
    assert_eq!(selected(&accounts, cx), Some(house));
    events.borrow_mut().clear();

    account::close::delete(&mut scratch.db(), house).unwrap();
    cx.update(|_, cx| accounts.update(cx, |accounts, cx| accounts.reload(cx)));
    assert_eq!(selected(&accounts, cx), Some(house), "until the read lands");
    assert!(events.borrow().is_empty());

    redraw(cx);
    assert_eq!(selected(&accounts, cx), None);
    assert_eq!(*events.borrow(), [AccountsEvent::Selected(None)]);
}

/// Another process (an `orre import`, say) adds a balance to `current` and
/// a new account, as the shell's change watcher will find; the panel reads
/// again.
fn change_elsewhere_and_reload(
    scratch: &Scratch,
    accounts: &Entity<Accounts>,
    cx: &mut VisualTestContext,
) {
    let entered = EnteredBalance {
        date: NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
        balance: Money::new(25_000, Currency::GBP),
        source: EnteredSource::Manual,
        note: Some("From a terminal".into()),
    };
    let mut db = scratch.db();
    account::add_balance(&mut db, scratch.current, &entered).unwrap();
    account::create(
        &mut db,
        &NewAccount::new("Amex", AccountKind::CreditCard, Currency::GBP),
    )
    .unwrap();
    cx.update(|_, cx| accounts.update(cx, |accounts, cx| accounts.reload(cx)));
    redraw(cx);
}

#[gpui_kit::test]
fn a_reload_from_outside_keeps_the_selection_and_a_name_being_typed(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click("account-name", cx);
    press("end", cx);
    cx.update(|window, cx| window.input(" 2", cx));

    change_elsewhere_and_reload(&scratch, &accounts, cx);

    assert_eq!(selected(&accounts, cx), Some(scratch.current));
    let shown = describe(&shell, &accounts, cx);
    assert!(
        listed(&shown).iter().any(|(name, _)| *name == "Amex"),
        "the new account is listed"
    );
    assert_eq!(
        selected_row(&shown).map(|(name, _)| name),
        Some("Current account")
    );
    assert_eq!(
        value(section(&shown, None), "Name"),
        Some("Current account 2"),
        "typed, not saved, and not put back"
    );
    let field = cx.update(|window, _| window.find("account-name").value().map(ToString::to_string));
    assert_eq!(field.as_deref(), Some("Current account 2"), "as drawn");
    let balances: Vec<&str> = section(&shown, Some("Balances"))
        .rows
        .iter()
        .map(|(_, value)| value.as_str())
        .collect();
    assert_eq!(
        balances,
        ["From a terminal · £250.00", "Entered by you · £100.00"],
        "the balance entered elsewhere is listed"
    );
    assert_eq!(
        account::get(&scratch.db(), scratch.current).unwrap().name,
        "Current account",
        "the reload saved nothing"
    );
}

#[gpui_kit::test]
fn a_reload_from_outside_keeps_an_open_dialog_and_what_was_typed_in_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click("add-balance", cx);
    type_into("add-balance-amount", "1,284.10", cx);
    type_into("add-balance-note", "Half typed", cx);
    let before = describe(&shell, &accounts, cx);
    assert_eq!(dialog(&before).title, "Add a balance for Current account");

    change_elsewhere_and_reload(&scratch, &accounts, cx);

    let after = describe(&shell, &accounts, cx);
    assert_eq!(dialog(&after), dialog(&before), "the dialog is as it was");
    assert_eq!(selected(&accounts, cx), Some(scratch.current));
    assert!(listed(&after).iter().any(|(name, _)| *name == "Amex"));

    // It still saves, to the account it was opened for.
    click("add-balance-add", cx);
    assert_eq!(describe(&shell, &accounts, cx).dialog, None);
    let balances = account::entered_balances(&scratch.db(), scratch.current).unwrap();
    assert!(balances.iter().any(|b| b.balance.minor() == 128_410));
}

#[gpui_kit::test]
fn a_new_account_is_selected_at_once_and_stays_selected_when_the_read_lands(
    cx: &mut TestAppContext,
) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    let events = record_events(&accounts, cx);
    let id = cx
        .update(|_, cx| {
            accounts.update(cx, |accounts, cx| {
                let account = NewAccount::new("Amex", AccountKind::Current, Currency::GBP);
                accounts.create(&account, None, cx)
            })
        })
        .unwrap();
    assert_eq!(selected(&accounts, cx), Some(id), "at once");
    assert_eq!(*events.borrow(), [AccountsEvent::Selected(Some(id))]);

    redraw(cx);
    assert_eq!(selected(&accounts, cx), Some(id));
    assert_eq!(*events.borrow(), [AccountsEvent::Selected(Some(id))]);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(selected_row(&shown).map(|(name, _)| name), Some("Amex"));
}

#[gpui_kit::test]
fn clicking_a_new_account_s_row_keeps_it_selected(cx: &mut TestAppContext) {
    // SLI-70: the new account is selected at once, so the first click on
    // its row must not clear the selection.
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    let id = cx
        .update(|_, cx| {
            accounts.update(cx, |accounts, cx| {
                let account = NewAccount::new("Amex", AccountKind::Current, Currency::GBP);
                accounts.create(&account, None, cx)
            })
        })
        .unwrap();
    redraw(cx);
    assert_eq!(selected(&accounts, cx), Some(id));

    click(&format!("account-{id}"), cx);
    assert_eq!(selected(&accounts, cx), Some(id), "still selected");
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(selected_row(&shown).map(|(name, _)| name), Some("Amex"));
    assert_eq!(
        value(section(&shown, None), "Name"),
        Some("Amex"),
        "the inspector stays filled"
    );
}

#[gpui_kit::test]
fn a_new_account_s_inspector_fills_in_when_its_read_lands(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(panel::ADD_ACCOUNT, cx);
    type_into("new-account-name", "Amex", cx);
    type_into("new-account-bank", "American Express", cx);
    click("new-account-add", cx);
    let added = account::list(&scratch.db())
        .unwrap()
        .into_iter()
        .find(|account| account.name == "Amex")
        .expect("Amex was added");
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(value(section(&shown, None), "Name"), Some("Amex"));
    assert_eq!(
        value(section(&shown, None), "Bank"),
        Some("American Express")
    );
    // An edit in the inspector changes only what was edited.
    click("account-name", cx);
    press("end", cx);
    cx.update(|window, cx| window.input(" Gold", cx));
    press("enter", cx);
    let stored = account::get(&scratch.db(), added.id).unwrap();
    assert_eq!(stored.name, "Amex Gold");
    assert_eq!(stored.institution.as_deref(), Some("American Express"));
}

// Editing and removing entered balances (M3.P9) ---------------------------------

fn entered_row(scratch: &Scratch) -> String {
    let id = account::entered_balances(&scratch.db(), scratch.current).unwrap()[0]
        .id
        .expect("an entered balance");
    inspector::balance_row_id(id).to_string()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

#[gpui_kit::test]
fn clicking_an_entered_balance_opens_the_edit_dialog_filled_in(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click(&entered_row(&scratch), cx);
    let shown = describe(&shell, &accounts, cx);
    let dialog = dialog(&shown);
    assert_eq!(dialog.title, "Edit a balance for Current account");
    assert_eq!(
        dialog.fields,
        [
            ("Balance".to_owned(), "100.00".to_owned()),
            (String::new(), "1 Nov 2026".to_owned()),
            ("Note".to_owned(), String::new()),
        ]
    );
    assert_eq!(
        labels(&dialog.actions),
        ["Remove balance", "Cancel", "Save"]
    );
    // Its own day isn't taken, so Save waits for nothing.
    assert_eq!(dialog.error, None);
    assert!(dialog.actions.iter().all(|action| action.enabled));
}

#[gpui_kit::test]
fn save_changes_the_amount_date_and_note(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click(&entered_row(&scratch), cx);
    retype_date("add-balance-amount", "250.00", cx);
    retype_date("add-balance-date", "2 Nov 2026", cx);
    type_into("add-balance-note", "Corrected", cx);
    click("add-balance-add", cx);

    assert_eq!(describe(&shell, &accounts, cx).dialog, None);
    let balances = account::entered_balances(&scratch.db(), scratch.current).unwrap();
    assert_eq!(balances.len(), 1, "edited in place, not added");
    assert_eq!(balances[0].date, day(2));
    assert_eq!(balances[0].balance.minor(), 25_000);
    assert_eq!(balances[0].note.as_deref(), Some("Corrected"));
}

#[gpui_kit::test]
fn editing_says_before_a_click_that_another_balance_has_the_day(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let other = EnteredBalance {
        date: day(5),
        balance: Money::new(9_000, Currency::GBP),
        source: EnteredSource::Manual,
        note: None,
    };
    account::add_balance(&mut scratch.db(), scratch.current, &other).unwrap();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    // The older one is the second row.
    let older = account::entered_balances(&scratch.db(), scratch.current).unwrap()[1]
        .id
        .unwrap();
    click(&inspector::balance_row_id(older), cx);
    assert_eq!(dialog(&describe(&shell, &accounts, cx)).error, None);

    retype_date("add-balance-date", "5 Nov 2026", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        dialog(&shown).error.as_deref(),
        Some("This account already has a balance on 5 Nov 2026. Choose another day.")
    );
    assert!(!dialog(&shown).actions[2].enabled, "Save waits");
}

#[gpui_kit::test]
fn remove_balance_removes_it_and_undo_adds_it_back(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let before = account::entered_balances(&scratch.db(), scratch.current).unwrap();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click(&entered_row(&scratch), cx);
    click("remove-balance", cx);

    assert!(
        account::entered_balances(&scratch.db(), scratch.current)
            .unwrap()
            .is_empty()
    );
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(shown.dialog, None, "no confirmation: the dialog closes");
    assert_eq!(
        shown.undo_bar,
        Some((
            "Removed the balance on 1 Nov 2026".to_owned(),
            vec![Action::button("Undo", true)]
        ))
    );

    click("undo-change", cx);
    let after = account::entered_balances(&scratch.db(), scratch.current).unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].date, before[0].date);
    assert_eq!(after[0].balance, before[0].balance);
    assert_eq!(after[0].source, before[0].source);
    assert_eq!(describe(&shell, &accounts, cx).undo_bar, None);
}

#[gpui_kit::test]
fn undoing_a_removal_tells_the_shell_the_balances_changed(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, accounts, cx) = open(cx, &scratch);
    let changed = std::rc::Rc::new(std::cell::Cell::new(0));
    let _subscription = cx.update(|_, cx| {
        let changed = changed.clone();
        cx.subscribe(&accounts, move |_, event: &AccountsEvent, _| {
            if *event == AccountsEvent::BalancesChanged {
                changed.set(changed.get() + 1);
            }
        })
    });
    click(&row(scratch.current), cx);
    click(&entered_row(&scratch), cx);
    click("remove-balance", cx);
    assert_eq!(changed.get(), 0, "the dialog closing reads the checks");

    click("undo-change", cx);
    assert_eq!(changed.get(), 1, "Undo has no dialog, so it says so itself");
}

#[gpui_kit::test]
fn an_edited_count_refuses_a_minus_as_a_new_one_does(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let cash = cash_account(&scratch, true);
    let count = account::entered_balances(&scratch.db(), cash).unwrap()[0]
        .id
        .unwrap();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(cash), cx);
    click(&inspector::balance_row_id(count), cx);
    retype_date("add-balance-amount", "-5.00", cx);
    click("add-balance-add", cx);

    let shown = describe(&shell, &accounts, cx);
    assert_eq!(
        dialog(&shown).error.as_deref(),
        Some("Type what you counted without a minus.")
    );
    let counts = account::entered_balances(&scratch.db(), cash).unwrap();
    assert_eq!(counts[0].balance.minor(), 5_000, "the count is unchanged");
}

#[gpui_kit::test]
fn undoing_a_removal_says_why_when_the_day_was_taken_since(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(scratch.current), cx);
    click(&entered_row(&scratch), cx);
    click("remove-balance", cx);
    let again = EnteredBalance {
        date: day(1),
        balance: Money::new(1, Currency::GBP),
        source: EnteredSource::Manual,
        note: None,
    };
    account::add_balance(&mut scratch.db(), scratch.current, &again).unwrap();

    click("undo-change", cx);
    let shown = describe(&shell, &accounts, cx);
    let (message, _) = shown.undo_bar.expect("the bar stays");
    assert!(message.contains("1 Nov 2026"), "{message}");
    assert!(!message.contains("2026-11-01"), "{message}");
    assert_eq!(
        account::entered_balances(&scratch.db(), scratch.current)
            .unwrap()
            .len(),
        1
    );
}

#[gpui_kit::test]
fn a_balance_in_the_locked_period_is_not_clickable(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    // 2 Nov repeats 1 Nov's £100.00 with nothing between: a passing check to
    // lock through. 5 Nov is after the lock.
    for (date, minor) in [(2, 10_000), (5, 9_000)] {
        let entered = EnteredBalance {
            date: day(date),
            balance: Money::new(minor, Currency::GBP),
            source: EnteredSource::Manual,
            note: None,
        };
        account::add_balance(&mut scratch.db(), scratch.current, &entered).unwrap();
    }
    orre_core::reconciliation::lock(&mut scratch.db(), scratch.current, day(2)).unwrap();
    let balances = account::entered_balances(&scratch.db(), scratch.current).unwrap();
    let (free, locked) = (balances[0].id.unwrap(), balances[1].id.unwrap());
    let (shell, accounts, cx) = open(cx, &scratch);
    let seen = requests(&accounts, cx);
    click(&row(scratch.current), cx);

    click(&inspector::balance_row_id(locked), cx);
    assert_eq!(describe(&shell, &accounts, cx).dialog, None);
    assert!(
        seen.borrow()
            .iter()
            .all(|event| !matches!(event, AccountsEvent::EditBalanceRequested(..)))
    );

    click(&inspector::balance_row_id(free), cx);
    assert_eq!(
        dialog(&describe(&shell, &accounts, cx)).title,
        "Edit a balance for Current account"
    );
}

#[gpui_kit::test]
fn a_value_on_the_valuations_table_opens_its_edit_dialog(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let house = valued(&scratch, "House", AccountKind::Asset, 38_500_000);
    let (shell, accounts, cx) = open(cx, &scratch);
    click(&row(house), cx);
    click("valuation-2026-10-12", cx);
    let shown = describe(&shell, &accounts, cx);
    assert_eq!(dialog(&shown).title, "Edit a valuation for House");
    assert_eq!(field(dialog(&shown), "Value"), Some("385,000.00"));
    retype_date("add-balance-amount", "400,000.00", cx);
    click("add-balance-add", cx);
    let values = account::entered_balances(&scratch.db(), house).unwrap();
    assert_eq!(values[0].balance.minor(), 40_000_000);
}

// Context menus on the accounts panel (M3.P10) -----------------------------------

fn right_click(id: &str, cx: &mut VisualTestContext) {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, cx| window.right_click(id, cx));
    redraw(cx);
}

type ShownMenu = Vec<Vec<describe::ContextItem>>;

fn menu(
    shell: &Entity<ShellView>,
    accounts: &Entity<Accounts>,
    cx: &mut VisualTestContext,
) -> ShownMenu {
    describe(shell, accounts, cx)
        .context_menu
        .expect("a context menu is open")
}

fn items(menu: &ShownMenu) -> Vec<Vec<&str>> {
    menu.iter()
        .map(|group| group.iter().map(|(label, _)| label.as_str()).collect())
        .collect()
}

#[gpui_kit::test]
fn right_clicking_an_account_opens_its_menu_without_selecting_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    assert_eq!(describe(&shell, &accounts, cx).context_menu, None);
    right_click(&row(scratch.current), cx);

    let menu = menu(&shell, &accounts, cx);
    assert_eq!(
        items(&menu),
        [
            vec![
                "Show transactions",
                "New transaction…",
                "Import into this account…",
                "Add balance…",
                "Edit in the inspector",
            ],
            vec!["Close account…", "Delete account…"],
        ]
    );
    // It has a pot, so core won't delete it: the reason is the item's caption.
    assert_eq!(menu[1][0].1, None);
    assert_eq!(menu[1][1].1.as_deref(), Some("Has pots"));
    assert_eq!(selected(&accounts, cx), None, "the selection stays");
    assert!(shows("option-show-transactions", cx));
}

#[gpui_kit::test]
fn the_menu_follows_the_kind_of_row(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let house = valued(&scratch, "House", AccountKind::Asset, 38_500_000);
    let mortgage = valued(&scratch, "Mortgage", AccountKind::Liability, -21_264_000);
    let cash = cash_account(&scratch, false);
    let (shell, accounts, cx) = open(cx, &scratch);

    right_click(&row(scratch.pot), cx);
    assert_eq!(
        items(&menu(&shell, &accounts, cx)),
        [vec!["Show transactions", "Add balance…"]]
    );
    press("escape", cx);

    right_click(&row(house), cx);
    let shown = menu(&shell, &accounts, cx);
    assert_eq!(
        items(&shown),
        [
            vec!["Add value…", "Edit in the inspector"],
            vec!["Close account…", "Delete account…"],
        ]
    );
    assert_eq!(shown[1][1].1, None, "an account of values only can go");
    press("escape", cx);

    right_click(&row(mortgage), cx);
    assert_eq!(items(&menu(&shell, &accounts, cx))[0][0], "Add balance…");
    press("escape", cx);

    // Cash isn't imported into.
    right_click(&row(cash), cx);
    let shown = menu(&shell, &accounts, cx);
    assert!(!items(&shown)[0].contains(&"Import into this account…"));
    assert!(items(&shown)[0].contains(&"New transaction…"));
    press("escape", cx);

    right_click(panel::UNCATEGORISED_ROW, cx);
    assert_eq!(
        items(&menu(&shell, &accounts, cx)),
        [vec!["Show transactions"]]
    );
}

#[gpui_kit::test]
fn escape_closes_the_menu_and_a_chosen_item_closes_it_too(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    let seen = requests(&accounts, cx);
    right_click(&row(scratch.current), cx);
    press("escape", cx);
    assert_eq!(describe(&shell, &accounts, cx).context_menu, None);
    assert!(seen.borrow().is_empty());

    right_click(&row(scratch.current), cx);
    click("option-show-transactions", cx);
    assert_eq!(describe(&shell, &accounts, cx).context_menu, None);
    assert_eq!(
        seen.borrow().first(),
        Some(&AccountsEvent::ShowTransactionsRequested(scratch.current))
    );
    assert_eq!(selected(&accounts, cx), Some(scratch.current));
}

#[gpui_kit::test]
fn the_keys_move_through_the_menu_and_enter_chooses(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    let seen = requests(&accounts, cx);
    right_click(&row(scratch.current), cx);
    press("down", cx);
    press("down", cx);
    press("enter", cx);
    assert_eq!(describe(&shell, &accounts, cx).context_menu, None);
    assert_eq!(
        seen.borrow().first(),
        Some(&AccountsEvent::NewTransactionRequested(scratch.current))
    );
    // New transaction opens its dialog.
    assert!(cx.update(|_, cx| shell.read(cx).dialog().is_some()));
}

#[gpui_kit::test]
fn a_disabled_item_cant_be_chosen(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, accounts, cx) = open(cx, &scratch);
    let seen = requests(&accounts, cx);
    right_click(&row(scratch.current), cx);
    click("option-delete", cx);
    assert!(seen.borrow().is_empty());
    assert_eq!(menu(&shell, &accounts, cx).len(), 2, "the menu stays open");
}

#[gpui_kit::test]
fn the_items_ask_for_what_the_inspector_buttons_do(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, accounts, cx) = open(cx, &scratch);
    let seen = requests(&accounts, cx);
    for (key, event) in [
        (
            "add-balance",
            AccountsEvent::AddBalanceRequested(scratch.current),
        ),
        ("close", AccountsEvent::CloseRequested(scratch.current)),
        ("inspect", AccountsEvent::InspectRequested(scratch.current)),
    ] {
        seen.borrow_mut().clear();
        right_click(&row(scratch.current), cx);
        click(&format!("option-{key}"), cx);
        assert_eq!(seen.borrow().first(), Some(&event), "{key}");
        // The shell opens a dialog for the first two: leave it.
        press("escape", cx);
    }
}

#[gpui_kit::test]
fn a_stored_amount_beyond_the_cap_shows_an_error_under_accounts(cx: &mut TestAppContext) {
    use orre_core::money::MAX_MINOR;
    let scratch = Scratch::new();
    // As an older build could have written it: `Money::new` doesn't check
    // the cap, so `manual::create` stores it as is.
    let entry = orre_core::manual::ManualTransaction::new(
        scratch.current,
        NaiveDate::from_ymd_opt(2026, 11, 4).unwrap(),
        Money::new(MAX_MINOR + 1, Currency::GBP),
        "Argus",
    );
    orre_core::manual::create(&mut scratch.db(), &entry).unwrap();

    let (shell, accounts, cx) = open(cx, &scratch);
    let shown = describe(&shell, &accounts, cx);
    let error = shown
        .error
        .expect("an error under ACCOUNTS, not a blank panel");
    println!("ACCOUNTS ERROR: {error}");
    assert!(
        error.contains(
            "the stored amount_minor is out of range: amounts can't exceed 10,000,000,000.00 GBP"
        ),
        "{error}"
    );
}
