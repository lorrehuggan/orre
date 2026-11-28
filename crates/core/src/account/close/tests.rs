//! Tests for valuations, closing, reopening and deleting accounts.

use chrono::NaiveDate;
use rusqlite::params;
use tempfile::TempDir;

use crate::account::{
    self, AccountKind, EnteredBalance, EnteredSource, Error, NewAccount, date_key,
};
use crate::db::Database;
use crate::id::AccountId;
use crate::manual::{self, ManualTransaction};
use crate::money::{Currency, Money};
use crate::reconciliation::BalanceSource;
use crate::reporting::Period;
use crate::reporting::account_tree::account_tree;
use crate::reporting::net_worth::net_worth_history;

use super::*;

fn open() -> (Database, TempDir) {
    let dir = TempDir::new().unwrap();
    let db = Database::open(&dir.path().join("orre.db")).unwrap();
    (db, dir)
}

fn day(m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn value(date: NaiveDate, minor: i64) -> EnteredBalance {
    EnteredBalance {
        date,
        balance: gbp(minor),
        source: EnteredSource::Valuation,
        note: None,
    }
}

fn house(db: &mut Database) -> AccountId {
    let new = NewAccount::new("House", AccountKind::Asset, Currency::GBP);
    let mut first = value(day(3, 14), 35_200_000);
    first.note = Some("Purchase price".into());
    account::create_with_balance(db, &new, Some(&first)).unwrap()
}

fn spend(db: &mut Database, account: AccountId, date: NaiveDate, minor: i64) {
    manual::create(
        db,
        &ManualTransaction::new(account, date, gbp(minor), "Coffee"),
    )
    .unwrap();
}

fn count(db: &Database, sql: &str, account: AccountId) -> i64 {
    db.conn()
        .query_row(sql, [account], |row| row.get(0))
        .unwrap()
}

#[test]
fn valuations_are_for_asset_and_liability_accounts_only() {
    let (mut db, _dir) = open();
    let house = house(&mut db);
    account::add_balance(&mut db, house, &value(day(10, 12), 38_500_000)).unwrap();
    let stored = account::entered_balances(&db, house).unwrap();
    assert_eq!(
        stored
            .iter()
            .map(|b| (b.date, b.balance.minor(), b.source, b.note.as_deref()))
            .collect::<Vec<_>>(),
        [
            (day(10, 12), 38_500_000, BalanceSource::Valuation, None),
            (
                day(3, 14),
                35_200_000,
                BalanceSource::Valuation,
                Some("Purchase price")
            ),
        ]
    );
    // One a day, like any entered balance.
    assert!(matches!(
        account::add_balance(&mut db, house, &value(day(10, 12), 1)),
        Err(Error::BalanceExists(_))
    ));
    // An asset takes valuations only; a bank account none.
    let mut manual = value(day(11, 1), 1);
    manual.source = EnteredSource::Manual;
    assert!(matches!(
        account::add_balance(&mut db, house, &manual),
        Err(Error::HoldsValuations("asset"))
    ));
    let current = account::create(
        &mut db,
        &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
    )
    .unwrap();
    assert!(matches!(
        account::add_balance(&mut db, current, &value(day(11, 1), 1)),
        Err(Error::NotValued("current"))
    ));
    assert_eq!(
        EnteredSource::for_kind(AccountKind::Liability),
        EnteredSource::Valuation
    );
    assert_eq!(
        EnteredSource::for_kind(AccountKind::Cash),
        EnteredSource::Manual
    );

    // A liability is what's owed: at or below zero, never silently flipped.
    let mortgage = account::create(
        &mut db,
        &NewAccount::new("Mortgage", AccountKind::Liability, Currency::GBP),
    )
    .unwrap();
    assert!(matches!(
        account::add_balance(&mut db, mortgage, &value(day(10, 1), 21_264_000)),
        Err(Error::LiabilityAboveZero)
    ));
    account::add_balance(&mut db, mortgage, &value(day(10, 1), -21_264_000)).unwrap();

    // Net worth: the latest value of each, the liability subtracting.
    let tree = account_tree(&db.reader().unwrap(), day(11, 28)).unwrap();
    assert_eq!(
        tree.net_worth
            .iter()
            .map(|money| money.minor())
            .collect::<Vec<_>>(),
        [38_500_000 - 21_264_000]
    );
}

#[test]
fn a_valuation_account_closes_after_its_last_value_at_zero_and_reopens() {
    let (mut db, _dir) = open();
    let house = house(&mut db);
    account::add_balance(&mut db, house, &value(day(10, 12), 38_500_000)).unwrap();

    let plan = closing(&db, house).unwrap();
    assert_eq!(plan.earliest, Some(day(10, 13)));
    assert!(plan.pots.is_empty());
    assert!(!plan.allows(day(10, 12)) && plan.allows(day(10, 13)));
    assert!(matches!(
        close(&mut db, house, day(10, 12)),
        Err(Error::CloseTooEarly { earliest }) if earliest == day(10, 13)
    ));

    assert_eq!(close(&mut db, house, day(11, 6)).unwrap(), [house]);
    let stored = account::get(&db, house).unwrap();
    assert_eq!(stored.closed_on, Some(day(11, 6)));
    // The values stay, and a £0 one is added on the closing day.
    let values = account::entered_balances(&db, house).unwrap();
    assert_eq!(values.len(), 3);
    assert_eq!((values[0].date, values[0].balance.minor()), (day(11, 6), 0));
    assert!(matches!(closing(&db, house), Err(Error::AlreadyClosed)));
    assert!(matches!(
        close(&mut db, house, day(11, 7)),
        Err(Error::AlreadyClosed)
    ));
    // Nothing after the closing day.
    assert!(matches!(
        account::add_balance(&mut db, house, &value(day(11, 7), 1)),
        Err(Error::AfterClosing { closed_on }) if closed_on == day(11, 6)
    ));
    assert_eq!(list_closed(&db).unwrap()[0].id, house);

    // It leaves the panel on its closing day; earlier months keep its value.
    let reader = db.reader().unwrap();
    assert!(
        account_tree(&reader, day(11, 6))
            .unwrap()
            .accounts
            .is_empty()
    );
    let months = net_worth_history(
        &reader,
        Period::new(2026, 10).unwrap(),
        Period::new(2026, 11).unwrap(),
    )
    .unwrap();
    let totals: Vec<Vec<i64>> = months
        .items
        .iter()
        .map(|month| month.total.iter().map(|money| money.minor()).collect())
        .collect();
    assert_eq!(totals, [vec![38_500_000], vec![]]);
    drop(reader);

    // Reopening takes the £0 away and nothing else.
    assert_eq!(reopen(&mut db, house).unwrap(), [house]);
    assert_eq!(account::get(&db, house).unwrap().closed_on, None);
    assert_eq!(account::entered_balances(&db, house).unwrap().len(), 2);
    assert!(matches!(reopen(&mut db, house), Err(Error::NotClosed)));
    assert!(list_closed(&db).unwrap().is_empty());
}

#[test]
fn a_bank_account_closes_with_its_pots_and_keeps_its_history() {
    let (mut db, _dir) = open();
    let current = account::create(
        &mut db,
        &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
    )
    .unwrap();
    let mut pot = NewAccount::new("Holiday", AccountKind::Pot, Currency::GBP);
    pot.parent = Some(current);
    let pot = account::create(&mut db, &pot).unwrap();
    spend(&mut db, current, day(11, 2), -500);
    spend(&mut db, pot, day(11, 4), 500);
    let mut balance = value(day(11, 3), 10_000);
    balance.source = EnteredSource::Manual;
    account::add_balance(&mut db, current, &balance).unwrap();

    let plan = closing(&db, current).unwrap();
    assert_eq!(plan.pots.iter().map(|p| p.id).collect::<Vec<_>>(), [pot]);
    // The pot's last row is the latest thing either holds; that day is fine.
    assert_eq!(plan.earliest, Some(day(11, 4)));
    assert!(matches!(
        close(&mut db, current, day(11, 3)),
        Err(Error::CloseTooEarly { .. })
    ));

    // The lock-date guard: never before the lock.
    db.conn()
        .execute(
            "UPDATE account SET reconciled_through = ?1 WHERE id = ?2",
            params![date_key(day(11, 10)), current],
        )
        .unwrap();
    assert!(matches!(
        close(&mut db, current, day(11, 9)),
        Err(Error::CloseLocked { through, .. }) if through == day(11, 10)
    ));
    assert_eq!(
        close(&mut db, current, day(11, 10)).unwrap(),
        [current, pot]
    );
    // History stays; no £0 balance for a bank account.
    let held = "SELECT COUNT(*) FROM \"transaction\" WHERE account_id = ?1";
    assert_eq!(count(&db, held, current), 1);
    assert_eq!(count(&db, held, pot), 1);
    assert_eq!(account::entered_balances(&db, current).unwrap().len(), 1);
    assert_eq!(account::get(&db, pot).unwrap().closed_on, Some(day(11, 10)));

    // A pot can't reopen while its parent is closed; reopening the parent
    // reopens the pots that closed with it.
    assert!(matches!(reopen(&mut db, pot), Err(Error::ParentClosed)));
    assert_eq!(reopen(&mut db, current).unwrap(), [current, pot]);
    assert_eq!(account::get(&db, pot).unwrap().closed_on, None);

    // A pot that closed earlier, on its own, stays closed.
    close(&mut db, pot, day(11, 5)).unwrap();
    close(&mut db, current, day(11, 12)).unwrap();
    assert_eq!(reopen(&mut db, current).unwrap(), [current]);
    assert_eq!(account::get(&db, pot).unwrap().closed_on, Some(day(11, 5)));
}

#[test]
fn an_empty_account_closes_on_any_day() {
    let (mut db, _dir) = open();
    let cash = account::create(
        &mut db,
        &NewAccount::new("Cash", AccountKind::Cash, Currency::GBP),
    )
    .unwrap();
    assert_eq!(closing(&db, cash).unwrap().earliest, None);
    close(&mut db, cash, day(1, 1)).unwrap();
    // Closed means nothing new is added after the day.
    let mut later = value(day(1, 2), 100);
    later.source = EnteredSource::Manual;
    assert!(matches!(
        account::add_balance(&mut db, cash, &later),
        Err(Error::AfterClosing { .. })
    ));
}

#[test]
fn only_an_account_nothing_refers_to_can_be_deleted() {
    let (mut db, _dir) = open();
    let house = house(&mut db);
    account::add_balance(&mut db, house, &value(day(10, 12), 38_500_000)).unwrap();
    assert_eq!(
        deletion(&db, house).unwrap(),
        Deletion {
            values: 2,
            refused: None
        }
    );
    assert_eq!(delete(&mut db, house).unwrap(), 2);
    assert!(matches!(account::get(&db, house), Err(Error::NotFound(_))));
    assert_eq!(
        db.conn()
            .query_row("SELECT COUNT(*) FROM balance_snapshot", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );

    let current = account::create(
        &mut db,
        &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
    )
    .unwrap();
    assert_eq!(deletion(&db, current).unwrap().refused, None);
    spend(&mut db, current, day(11, 2), -500);
    assert_eq!(
        deletion(&db, current).unwrap().refused,
        Some(DeleteLock::Transactions)
    );
    assert!(matches!(
        delete(&mut db, current),
        Err(Error::Undeletable(DeleteLock::Transactions))
    ));
    assert!(account::get(&db, current).is_ok());

    let empty = account::create(
        &mut db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .unwrap();
    db.conn()
        .execute(
            "UPDATE account SET reconciled_through = '2026-11-01' WHERE id = ?1",
            [empty],
        )
        .unwrap();
    assert_eq!(
        deletion(&db, empty).unwrap().refused,
        Some(DeleteLock::Locked)
    );
    assert_eq!(
        Error::Undeletable(DeleteLock::Locked).to_string(),
        "the account can't be deleted: it is locked; close it instead"
    );
}
