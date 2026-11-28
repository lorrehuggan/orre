use crate::account::AccountKind;
use crate::money::Currency;

use super::super::test_ledger::TestLedger;
use super::*;

fn period(text: &str) -> Period {
    text.parse().unwrap()
}

/// `(name, balance, counted, unanchored)` per account, and the totals as
/// `(currency, minor)`.
type Month = (Vec<(String, i64, i64, bool)>, Vec<(String, i64)>);

fn months(history: &Capped<NetWorthMonth>) -> Vec<Month> {
    history
        .items
        .iter()
        .map(|month| {
            (
                month
                    .accounts
                    .iter()
                    .map(|a| {
                        (
                            a.name.clone(),
                            a.balance.minor(),
                            a.counted.minor(),
                            a.unanchored,
                        )
                    })
                    .collect(),
                month
                    .total
                    .iter()
                    .map(|m| (m.currency().code().to_owned(), m.minor()))
                    .collect(),
            )
        })
        .collect()
}

fn account(name: &str, balance: i64, counted: i64, unanchored: bool) -> (String, i64, i64, bool) {
    (name.to_owned(), balance, counted, unanchored)
}

#[test]
fn sums_month_end_balances_per_currency() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    ledger.balance(current, "2026-09-30", 100_000);
    // A move into a pot, paired: net worth doesn't change.
    let pot = ledger.pot(current, "Holiday");
    let out = ledger.row(current, "2026-10-05", -10_000, None);
    let into = ledger.row(pot, "2026-10-05", 10_000, None);
    ledger.transfer(out, into);
    // A shared purchase counts in full: balances are gross.
    let dinner = ledger.row(current, "2026-11-06", -4_000, Some("food.eating_out"));
    ledger.share(dinner, -2_000);
    // Valuation accounts.
    let house = ledger.valuation_account("House", AccountKind::Asset, Currency::GBP);
    ledger.valuation(house, "2026-10-15", 30_000_000);
    let mortgage = ledger.valuation_account("Mortgage", AccountKind::Liability, Currency::GBP);
    ledger.valuation(mortgage, "2026-09-01", -20_000_000);
    // A liability entered positive still subtracts.
    let loan = ledger.valuation_account("Loan", AccountKind::Liability, Currency::GBP);
    ledger.valuation(loan, "2026-09-01", 5_000);
    // Euros without a stated balance: unanchored, listed, not counted.
    let euros = ledger.account("Euros", Currency::EUR);
    ledger.row(euros, "2026-11-02", -500, None);
    // Closed on 20 October: in September, gone after.
    let old = ledger.account("Old savings", Currency::GBP);
    ledger.balance(old, "2026-09-01", 3_000);
    ledger
        .db
        .conn()
        .execute(
            "UPDATE account SET closed_on = '2026-10-20' WHERE id = ?1",
            [old],
        )
        .unwrap();

    let history =
        net_worth_history(&ledger.reader(), period("2026-09"), period("2026-11")).unwrap();
    assert!(!history.truncated);
    let dates: Vec<String> = history.items.iter().map(|m| m.date.to_string()).collect();
    assert_eq!(dates, ["2026-09-30", "2026-10-31", "2026-11-30"]);
    let gbp = |minor| ("GBP".to_owned(), minor);
    assert_eq!(
        months(&history),
        [
            (
                // The house has no valuation yet.
                vec![
                    account("Current", 100_000, 100_000, false),
                    account("Holiday", 0, 0, true),
                    account("Mortgage", -20_000_000, -20_000_000, false),
                    account("Loan", 5_000, -5_000, false),
                    account("Euros", 0, 0, true),
                    account("Old savings", 3_000, 3_000, false),
                ],
                vec![gbp(-19_902_000)],
            ),
            (
                vec![
                    account("Current", 90_000, 90_000, false),
                    account("Holiday", 10_000, 10_000, true),
                    account("House", 30_000_000, 30_000_000, false),
                    account("Mortgage", -20_000_000, -20_000_000, false),
                    account("Loan", 5_000, -5_000, false),
                    account("Euros", 0, 0, true),
                ],
                vec![gbp(10_095_000)],
            ),
            (
                vec![
                    account("Current", 86_000, 86_000, false),
                    account("Holiday", 10_000, 10_000, true),
                    account("House", 30_000_000, 30_000_000, false),
                    account("Mortgage", -20_000_000, -20_000_000, false),
                    account("Loan", 5_000, -5_000, false),
                    account("Euros", -500, 0, true),
                ],
                vec![gbp(10_091_000)],
            ),
        ]
    );
}

#[test]
fn refuses_a_backwards_range_and_caps_long_ones() {
    let mut ledger = TestLedger::new();
    ledger.account("Current", Currency::GBP);
    let reader = ledger.reader();
    assert!(matches!(
        net_worth_history(&reader, period("2026-11"), period("2026-10")),
        Err(Error::EmptyRange { .. })
    ));
    let long = net_worth_history(&reader, period("2000-01"), period("2026-12")).unwrap();
    assert!(long.truncated);
    assert_eq!(long.len(), Limit::MAX.get());
}

#[test]
fn an_unanchored_account_is_listed_but_counts_once_a_balance_is_entered() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    ledger.row(current, "2026-10-05", 4_000, None);
    let pot = ledger.pot(current, "Holiday");
    ledger.row(pot, "2026-10-06", 700, None);

    let reader = ledger.reader();
    let before = net_worth_history(&reader, period("2026-10"), period("2026-10")).unwrap();
    assert_eq!(
        months(&before),
        [(
            vec![
                account("Current", 4_000, 0, true),
                account("Holiday", 700, 700, true)
            ],
            vec![("GBP".to_owned(), 700)],
        )]
    );
    let worth = &before.items[0].accounts;
    assert!(!worth[0].is_counted());
    assert!(worth[1].is_counted());
    drop(reader);

    ledger.balance(current, "2026-10-10", 5_000);
    let after = net_worth_history(&ledger.reader(), period("2026-10"), period("2026-10")).unwrap();
    assert_eq!(
        months(&after),
        [(
            vec![
                account("Current", 5_000, 5_000, false),
                account("Holiday", 700, 700, true)
            ],
            vec![("GBP".to_owned(), 5_700)],
        )]
    );
    assert!(after.items[0].accounts.iter().all(AccountWorth::is_counted));
}

#[test]
fn history_and_the_accounts_panel_agree_on_net_worth() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    ledger.balance(current, "2026-10-01", 100_000);
    let pot = ledger.pot(current, "Holiday");
    let out = ledger.row(current, "2026-10-05", -10_000, None);
    let into = ledger.row(pot, "2026-10-05", 10_000, None);
    ledger.transfer(out, into);
    // No stated balance: left out of both.
    let card = ledger.account("Card", Currency::GBP);
    ledger.row(card, "2026-10-06", -3_000, None);
    let spare = ledger.account("Spare", Currency::GBP);
    ledger.row(spare, "2026-10-07", 800, None);
    let spare_pot = ledger.pot(spare, "Spare pot");
    ledger.row(spare_pot, "2026-10-08", 200, None);
    let house = ledger.valuation_account("House", AccountKind::Asset, Currency::GBP);
    ledger.valuation(house, "2026-10-09", 250_000);
    // The only euro account has no stated balance: no euro total in either.
    let euros = ledger.account("Euros", Currency::EUR);
    ledger.row(euros, "2026-10-06", 5_000, None);

    let check = |ledger: &TestLedger| {
        let tree = crate::reporting::account_tree::account_tree(
            &ledger.reader(),
            crate::reporting::test_ledger::date("2026-11-28"),
        )
        .unwrap();
        let history =
            net_worth_history(&ledger.reader(), period("2026-10"), period("2026-10")).unwrap();
        assert_eq!(history.items[0].total, tree.net_worth);
        tree.net_worth.iter().map(|m| m.minor()).collect::<Vec<_>>()
    };
    // 90,000 + 10,000 + 200 + 250,000.
    assert_eq!(check(&ledger), [350_200]);

    ledger.balance(card, "2026-10-10", -3_000);
    assert_eq!(check(&ledger), [347_200]);
}

#[test]
fn each_point_is_the_last_day_of_the_month_under_the_month_start() {
    use crate::reporting::test_ledger::date;
    use crate::reporting::{MonthStart, StartDay, Weekends};

    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    ledger.balance(current, "2026-10-01", 10_000);
    ledger.row(current, "2026-11-24", -1_000, None);
    ledger.row(current, "2026-11-25", -2_000, None);
    let start = MonthStart::new(StartDay::day(25).unwrap(), Weekends::Keep);
    crate::setting::set_month_start(&mut ledger.db, start).unwrap();

    let history =
        net_worth_history(&ledger.reader(), period("2026-11"), period("2026-12")).unwrap();
    let points: Vec<_> = history
        .items
        .iter()
        .map(|month| {
            assert_eq!(month.date, month.range.to());
            (
                month.period.key(),
                month.date,
                month.total.get(Currency::GBP),
            )
        })
        .collect();
    assert_eq!(
        points,
        [
            (
                "2026-11".to_owned(),
                date("2026-11-24"),
                Some(Money::new(9_000, Currency::GBP))
            ),
            (
                "2026-12".to_owned(),
                date("2026-12-24"),
                Some(Money::new(7_000, Currency::GBP))
            ),
        ]
    );
}
