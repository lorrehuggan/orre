//! The shell's live updates over a scratch database: another process's
//! writes reach the views on a check, the app's own writes reload every
//! view of what they changed and aren't reported again, a rebase never
//! drops a write, and a restore pauses the checks.
//!
//! "Another process" is a second connection, as `orre import` or the MCP
//! server would open. Checks are run by hand ([`ShellView::check_changes`]),
//! except in the timer's tests, which start it ([`ShellView::keep_watching`])
//! and advance the test clock.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use chrono::NaiveDate;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use orre_core::account::{self, AccountEdit, AccountKind, NewAccount};
use orre_core::db::Database;
use orre_core::exclusion::ExclusionReason;
use orre_core::inbox::{self as core_inbox, Filter, NewSuggestion, SuggestionId, SuggestionKind};
use orre_core::manual::{self, ManualTransaction};
use orre_core::mcp_client::{self, AccessLevel, ClientName};
use orre_core::merchant::{self, AliasDefinition, AliasKind, DisplayName};
use orre_core::money::{Currency, Money};
use orre_core::watch::Topic;
use tempfile::TempDir;

use super::super::status::Ai;
use super::*;
use crate::inbox::fixture;
use crate::settings::RestoreTarget;
use crate::settings::restore::RestoreDialog;
use crate::settings::store::SettingsStore;

struct Scratch {
    _dir: TempDir,
    path: PathBuf,
}

impl Scratch {
    /// The Inbox's sample data: accounts, transactions, rules, suggestions
    /// and two enabled clients.
    fn seeded() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        fixture::seed(&mut Database::open(&path).unwrap());
        Self { _dir: dir, path }
    }

    /// A connection of another process's.
    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn pending(&self) -> Vec<SuggestionId> {
        core_inbox::list(
            &self.db(),
            &Filter {
                status: Some(core_inbox::SuggestionStatus::Pending),
                kind: None,
            },
        )
        .unwrap()
        .into_iter()
        .filter(|suggestion| suggestion.source_client == fixture::AGENT)
        .map(|suggestion| suggestion.id)
        .collect()
    }

    /// Files a rule suggestion as the MCP server would.
    fn file_outside(&self, pattern: &str) -> SuggestionId {
        core_inbox::file_suggestion(
            &mut self.db(),
            &ClientName::new(fixture::AGENT).unwrap(),
            &NewSuggestion {
                kind: SuggestionKind::Rule,
                payload: serde_json::json!({"match_field": "description", "match_kind": "contains",
                    "pattern": pattern, "actions": [{"kind": "set_category", "value": "bills"}]}),
                rationale: Some("From outside.".to_owned()),
            },
        )
        .unwrap()
        .id
    }

    /// Renames an account as `orre accounts` would.
    fn rename_outside(&self, from: &str, to: &str) {
        let mut db = self.db();
        let account = account::list(&db)
            .unwrap()
            .into_iter()
            .find(|account| account.name == from)
            .expect("the fixture has it");
        let mut edit = AccountEdit::of(&account);
        edit.name = to.to_owned();
        account::update(&mut db, account.id, &edit).unwrap();
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
}

/// The shell on `view` over `scratch`, as `open` builds it: the baseline
/// first, then the views' first reads, everything read.
fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
    view: View,
) -> (Entity<ShellView>, &'a mut VisualTestContext) {
    let watched = baseline(&scratch.path);
    open_watched(cx, scratch, view, watched)
}

fn open_watched<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
    view: View,
    watched: Watched,
) -> (Entity<ShellView>, &'a mut VisualTestContext) {
    init(cx);
    let database = scratch.path.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let mut shell = ShellView::from_database(view, Ok(database), window, cx);
        shell.watch(watched, cx);
        shell
    });
    settle(cx);
    (shell, cx)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Runs one check and what it reloads; the topics it reported.
fn check(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> BTreeSet<Topic> {
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.check_changes(cx)));
    settle(cx);
    last_reported(shell, cx).expect("a check ran")
}

fn last_reported(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> Option<BTreeSet<Topic>> {
    cx.update(|_, cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .expect("watching")
            .last_reported
            .clone()
    })
}

fn topics(topics: &[Topic]) -> BTreeSet<Topic> {
    topics.iter().copied().collect()
}

fn account_names(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        let accounts = shell.read(cx).accounts().read(cx);
        accounts
            .data()
            .tree
            .rows()
            .map(|node| node.account.name.clone())
            .collect()
    })
}

fn listed(shell: &Entity<ShellView>, id: SuggestionId, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| shell.read(cx).inbox().read(cx).entry(id).is_some())
}

fn rows(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| {
        shell
            .read(cx)
            .transactions()
            .read(cx)
            .register()
            .rows()
            .len()
    })
}

/// The Overview reads the ledger again when another process changes it.
#[gpui_kit::test]
fn the_overview_reloads_on_a_ledger_change(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Overview);
    let overview = cx.update(|_, cx| shell.read(cx).overview().clone());
    let november = orre_core::reporting::Period::new(2026, 11).unwrap();
    // The row added below is on 12 November: read as of the 14th, not the
    // clock's day.
    let day = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
    cx.update(|_, cx| {
        overview.update(cx, |overview, cx| {
            overview.hold_day(day);
            overview.select_month(november, cx);
            // Read again on the held day: the shell's first read was on the
            // clock's, and picking the shown month starts none.
            overview.reload(cx);
        })
    });
    settle(cx);
    let spent = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            overview
                .read(cx)
                .data()
                .map(|data| data.selected.spent.minor())
        })
    };
    let before = spent(cx).expect("the first read landed");
    add_row_outside(&scratch, &shell, cx);
    assert!(check(&shell, cx).contains(&Topic::Ledger));
    // The row is £12.34 out on 12 November.
    assert_eq!(spent(cx), Some(before + 1234));
}

/// Setting the month start in Settings (M4.11) applies at once: the
/// Overview's month and the Date chip's presets are cut again by the next
/// reads, with no check needed, and the next check has nothing to report.
#[gpui_kit::test]
fn a_new_month_start_recuts_the_overview_and_the_date_chip(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use orre_core::reporting::period::{DateRange, MonthStart, StartDay, Weekends};

    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let (overview, transactions, settings) = cx.update(|_, cx| {
        let shell = shell.read(cx);
        (
            shell.overview().clone(),
            shell.transactions().clone(),
            shell.settings().clone(),
        )
    });
    // The fixture day, Tue 10 Nov 2026.
    let tuesday = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
    cx.update(|_, cx| {
        overview.update(cx, |overview, cx| {
            overview.hold_day(tuesday);
            overview.reload(cx);
        });
        transactions.update(cx, |transactions, _| transactions.hold_day(tuesday));
    });
    settle(cx);
    for id in ["filter-date", "option-this_month"] {
        cx.update(|window, cx| window.click(id, cx));
        settle(cx);
    }
    // The chip's label, its range, and the Overview's switcher dates.
    let shown = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let transactions = transactions.read(cx);
            let chip = transactions.describe(cx).toolbar.chips[2].label.clone();
            let range = transactions.filter().dates;
            let overview = overview.read(cx).describe(cx);
            (chip, range, overview.month, overview.dates)
        })
    };
    let set = |start: MonthStart, cx: &mut VisualTestContext| {
        cx.update(|_, cx| settings.update(cx, |settings, cx| settings.set_month_start(start, cx)));
        settle(cx);
    };
    let range = |from: &str, to: &str| DateRange::parse(from, to).ok();

    assert_eq!(
        shown(cx),
        (
            "This month".to_owned(),
            range("2026-11-01", "2026-11-30"),
            "November 2026".to_owned(),
            None
        ),
        "calendar months say no dates"
    );

    // The 25th, Friday before: 25 Oct is a Sunday, so November starts on
    // Friday 23 Oct.
    set(
        MonthStart::new(StartDay::day(25).unwrap(), Weekends::FridayBefore),
        cx,
    );
    assert_eq!(
        shown(cx),
        (
            "This month · 23 Oct – 24 Nov".to_owned(),
            range("2026-10-23", "2026-11-24"),
            "November 2026".to_owned(),
            Some("23 Oct – 24 Nov".to_owned())
        )
    );
    assert_eq!(check(&shell, cx), topics(&[]), "the app's own write");
    // The open chip menu says every month preset's dates, chosen or not.
    cx.update(|window, cx| window.click("filter-date", cx));
    settle(cx);
    let details = cx.update(|_, cx| {
        let menu = transactions
            .read(cx)
            .describe(cx)
            .menu
            .expect("the Date menu");
        menu.groups[0]
            .options
            .iter()
            .map(|option| (option.label.clone(), option.detail.clone()))
            .collect::<Vec<_>>()
    });
    let detail = |label: &str| {
        details
            .iter()
            .find(|(shown, _)| shown == label)
            .unwrap_or_else(|| panic!("no {label} in {details:?}"))
            .1
            .as_deref()
    };
    assert_eq!(detail("This month"), Some("23 Oct – 24 Nov"));
    assert_eq!(detail("Last month"), Some("25 Sep – 22 Oct"));
    assert_eq!(detail("This year"), None, "years keep their own dates");
    cx.simulate_keystrokes("escape");
    settle(cx);

    // The last day, Friday before: 31 Oct is a Saturday.
    set(MonthStart::new(StartDay::LAST, Weekends::FridayBefore), cx);
    assert_eq!(
        shown(cx),
        (
            "This month · 30 Oct – 29 Nov".to_owned(),
            range("2026-10-30", "2026-11-29"),
            "November 2026".to_owned(),
            Some("30 Oct – 29 Nov".to_owned())
        )
    );

    // Back to the 1st: the dates go again.
    set(MonthStart::CALENDAR, cx);
    assert_eq!(shown(cx).0, "This month");
    assert_eq!(shown(cx).3, None);
}

// The topic map ------------------------------------------------------------------

#[test]
fn each_topic_reloads_its_views() {
    let none = Views::default();
    assert_eq!(
        views(&topics(&[Topic::Inbox]), false),
        Views {
            inbox: true,
            ..none
        }
    );
    let ledger = Views {
        transactions: true,
        overview: true,
        accounts: true,
        checks: true,
        ..none
    };
    assert_eq!(views(&topics(&[Topic::Ledger]), false), ledger);
    assert_eq!(
        views(&topics(&[Topic::Ledger]), true),
        Views {
            rules: true,
            ..ledger
        },
        "a Ledger change reloads Rules only while it's on show"
    );
    let rules = Views {
        transactions: true,
        overview: true,
        rules: true,
        ..none
    };
    assert_eq!(views(&topics(&[Topic::Rules]), false), rules);
    assert_eq!(views(&topics(&[Topic::Rules]), true), rules);
    assert_eq!(
        views(&topics(&[Topic::Settings]), false),
        Views {
            settings: true,
            // The month start is a setting: the views it cuts months for.
            inbox: true,
            transactions: true,
            overview: true,
            ..none
        }
    );
    let every = Views {
        inbox: true,
        transactions: true,
        overview: true,
        accounts: true,
        checks: true,
        rules: true,
        settings: true,
    };
    assert_eq!(views(&Topic::ALL.into_iter().collect(), false), every);
    assert_eq!(views(&BTreeSet::new(), true), none);
}

#[test]
fn it_checks_every_second_while_active_and_every_five_when_not() {
    assert_eq!(poll_interval(true), Duration::from_secs(1));
    assert_eq!(poll_interval(false), Duration::from_secs(5));
}

// Another process's writes --------------------------------------------------------

/// An MCP client's suggestion shows in the Inbox and its badge on the next
/// check, and the suggestion the user had selected stays selected.
#[gpui_kit::test]
fn a_suggestion_filed_by_a_client_shows_with_the_badge(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let inbox = cx.update(|_, cx| shell.read(cx).inbox().clone());
    let selected = scratch.pending()[0];
    cx.update(|_, cx| inbox.update(cx, |inbox, cx| inbox.select(Some(selected), cx)));
    let badge = cx.update(|_, cx| inbox.read(cx).badge());
    assert_eq!(check(&shell, cx), topics(&[]), "nothing changed yet");

    let filed = scratch.file_outside("FROMOUTSIDE");
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox]));
    assert!(listed(&shell, filed, cx));
    assert_eq!(cx.update(|_, cx| inbox.read(cx).badge()), badge + 1);
    assert_eq!(
        cx.update(|_, cx| inbox.read(cx).selected()),
        Some(selected),
        "the new suggestion takes nothing"
    );
}

/// A row added in a terminal shows in Transactions, and the accounts panel
/// and the status bar follow; the selected transaction stays selected.
#[gpui_kit::test]
fn a_row_added_outside_shows_in_transactions_and_the_status_bar(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let transactions = cx.update(|_, cx| shell.read(cx).transactions().clone());
    let first = cx.update(|_, cx| transactions.read(cx).register().rows()[0].id);
    cx.update(|_, cx| transactions.update(cx, |view, cx| view.select([first], cx)));
    let before = rows(&shell, cx);
    let status_before = cx.update(|_, cx| shell.read(cx).status(cx).transactions);
    let account = cx.update(|_, cx| {
        shell.read(cx).accounts().read(cx).data().tree.accounts[0]
            .account
            .id
    });

    let entry = ManualTransaction::new(
        account,
        NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(),
        Money::new(-1234, Currency::GBP),
        "OUTSIDE SHOP",
    );
    manual::create(&mut scratch.db(), &entry).unwrap();
    assert_eq!(check(&shell, cx), topics(&[Topic::Ledger]));

    assert_eq!(rows(&shell, cx), before + 1);
    let status_after = cx.update(|_, cx| shell.read(cx).status(cx).transactions);
    assert_ne!(status_after, status_before, "the status bar counts it");
    let selected: Vec<_> = cx.update(|_, cx| {
        transactions
            .read(cx)
            .selected()
            .iter()
            .map(|row| row.id)
            .collect()
    });
    assert_eq!(selected, [first]);
}

/// Rules counts the ledger, so a Ledger change reloads it, but only while
/// it's the view on show.
#[gpui_kit::test]
fn a_ledger_change_reloads_rules_only_while_it_is_on_show(cx: &mut TestAppContext) {
    let renamed_in_rules = |shell: &Entity<ShellView>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            shell
                .read(cx)
                .rules()
                .read(cx)
                .data()
                .accounts
                .iter()
                .any(|account| account.name == "Renamed outside")
        })
    };

    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    scratch.rename_outside("Savings", "Renamed outside");
    assert_eq!(check(&shell, cx), topics(&[Topic::Ledger]));
    assert!(account_names(&shell, cx).contains(&"Renamed outside".to_owned()));
    assert!(!renamed_in_rules(&shell, cx), "Rules isn't on show");

    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Rules);
    scratch.rename_outside("Savings", "Renamed outside");
    assert_eq!(check(&shell, cx), topics(&[Topic::Ledger]));
    assert!(renamed_in_rules(&shell, cx));
}

/// A merchant name added elsewhere (an approved suggestion, `orre`)
/// moves only the Rules fingerprint, and Transactions shows it.
#[gpui_kit::test]
fn a_merchant_name_added_outside_shows_in_transactions(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let named = |shell: &Entity<ShellView>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            shell
                .read(cx)
                .transactions()
                .read(cx)
                .register()
                .rows()
                .iter()
                .filter(|row| row.merchant == "Wasabi Sushi")
                .count()
        })
    };
    assert_eq!(named(&shell, cx), 0);

    let alias = AliasDefinition::new(
        AliasKind::Prefix,
        "WASABI",
        DisplayName::new("Wasabi Sushi").unwrap(),
    )
    .unwrap();
    merchant::add(&mut scratch.db(), &alias).unwrap();
    assert_eq!(check(&shell, cx), topics(&[Topic::Rules]));
    assert!(named(&shell, cx) > 0);
}

/// A client enabled from a terminal shows in Settings' list, and the
/// status bar counts it.
#[gpui_kit::test]
fn a_client_enabled_outside_shows_in_settings_and_the_status_bar(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Settings);
    let name = ClientName::new("cursor").unwrap();
    let enabled = |shell: &Entity<ShellView>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| shell.read(cx).settings().read(cx).ai().enabled())
    };
    let before = enabled(&shell, cx);

    mcp_client::enable_at(&mut scratch.db(), &name, AccessLevel::Aggregate).unwrap();
    assert_eq!(check(&shell, cx), topics(&[Topic::Settings]));
    assert_eq!(enabled(&shell, cx), before + 1);
    assert!(cx.update(|_, cx| {
        shell
            .read(cx)
            .settings()
            .read(cx)
            .ai()
            .client(&name)
            .is_some()
    }));
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).status(cx).ai),
        Ai::Connected(before + 1)
    );
}

// The app's own writes ---------------------------------------------------------------

/// After the app's own approval or rejection, the next check reports
/// nothing: the shell rebased what the decision changed before the views
/// read it.
#[gpui_kit::test]
fn the_apps_own_decisions_are_not_reported_again(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let inbox = cx.update(|_, cx| shell.read(cx).inbox().clone());
    let pending = scratch.pending();
    assert_eq!(check(&shell, cx), topics(&[]));

    cx.update(|_, cx| inbox.update(cx, |inbox, cx| inbox.reject(pending[0], cx)));
    settle(cx);
    assert!(!listed(&shell, pending[0], cx));
    assert_eq!(check(&shell, cx), topics(&[]));

    cx.update(|_, cx| inbox.update(cx, |inbox, cx| inbox.approve(pending[1], cx)));
    settle(cx);
    assert!(!listed(&shell, pending[1], cx));
    assert_eq!(check(&shell, cx), topics(&[]));
}

/// The trap a rebase per view falls into: a write from another process
/// lands, then the app makes its own change to the same topic. The shell
/// rebases before every view of the topic reads, so the outside write is in
/// those reads, and the next check has nothing left to report.
#[gpui_kit::test]
fn a_rebase_never_drops_another_processs_write(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    assert_eq!(check(&shell, cx), topics(&[]));

    scratch.rename_outside("Savings", "Renamed outside");
    let transactions = cx.update(|_, cx| shell.read(cx).transactions().clone());
    let row = cx.update(|_, cx| transactions.read(cx).register().rows()[0].id);
    cx.update(|_, cx| {
        transactions.update(cx, |view, cx| {
            view.exclude(row, ExclusionReason::Other, Some("Testing"), cx)
        })
    })
    .unwrap();
    settle(cx);

    assert!(
        account_names(&shell, cx).contains(&"Renamed outside".to_owned()),
        "the accounts read after the rebase"
    );
    let excluded = cx.update(|_, cx| {
        transactions
            .read(cx)
            .register()
            .row(row)
            .and_then(|row| row.excluded)
    });
    assert_eq!(excluded, Some(ExclusionReason::Other));
    assert!(
        !check(&shell, cx).contains(&Topic::Ledger),
        "nothing about the ledger is left to report"
    );
}

/// R.19: a new account is selected even with a read of the accounts in
/// flight when it's created: that read started before the write, so it's
/// dropped, and the shell's read after the write has the account.
#[gpui_kit::test]
fn a_created_account_is_selected_with_a_read_in_flight(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let accounts = cx.update(|_, cx| shell.read(cx).accounts().clone());
    let created = cx
        .update(|_, cx| {
            accounts.update(cx, |accounts, cx| {
                accounts.reload(cx);
                accounts.create(
                    &NewAccount::new("Holiday fund", AccountKind::Savings, Currency::GBP),
                    None,
                    cx,
                )
            })
        })
        .unwrap();
    settle(cx);
    let (selected, listed) = cx.update(|_, cx| {
        let accounts = accounts.read(cx);
        (accounts.selected(), accounts.data().contains(created))
    });
    assert_eq!(selected, Some(created));
    assert!(listed);
    assert!(!check(&shell, cx).contains(&Topic::Ledger));
}

/// R.19: typing into a rule's form after a save and before the shell's
/// read lands is kept.
#[gpui_kit::test]
fn typing_after_a_rules_save_is_kept_when_the_read_lands(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Rules);
    let rules = cx.update(|_, cx| shell.read(cx).rules().clone());
    let rule = cx.update(|_, cx| rules.read(cx).data().rules[0].rule.id());
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(rule), cx)));
    settle(cx);
    let saved = cx.update(|_, cx| rules.read(cx).form().unwrap().text.clone()) + " LTD";

    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            let text = saved.clone();
            rules.edit(true, move |form| form.text = text, cx);
            rules.edit(false, |form| form.text = "TYPED".into(), cx);
        })
    });
    settle(cx);

    let text = cx.update(|_, cx| rules.read(cx).form().map(|form| form.text.clone()));
    assert_eq!(text.as_deref(), Some("TYPED"));
    let stored = orre_core::rule::get(&scratch.db(), rule).unwrap();
    assert_eq!(stored.definition().pattern().as_str(), saved);
    assert!(!check(&shell, cx).contains(&Topic::Rules));
}

// Restore, the baseline, the timer --------------------------------------------------

/// The app's own restore: no checks while it runs; afterwards every view
/// shows the restored data, and the next check reports nothing.
#[gpui_kit::test]
fn a_restore_pauses_the_checks_and_every_view_reads_the_restored_data(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let store = SettingsStore::new(scratch.path.clone());
    let (backup, _) = store.back_up().unwrap();
    let filed = scratch.file_outside("AFTERTHEBACKUP");
    scratch.rename_outside("Savings", "Renamed outside");
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox, Topic::Ledger]));
    assert!(listed(&shell, filed, cx));

    let settings = cx.update(|_, cx| shell.read(cx).settings().clone());
    cx.update(|_, cx| settings.update(cx, |settings, cx| settings.restoring(cx)));
    settle(cx);
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.check_changes(cx)));
    let checking = cx.update(|_, cx| shell.read(cx).live.as_ref().unwrap().checking.is_some());
    assert!(!checking, "no check while the restore runs");

    let restored = store.restore(&backup.path).unwrap();
    cx.update(|_, cx| settings.update(cx, |settings, cx| settings.restored(&restored, cx)));
    settle(cx);

    assert!(!listed(&shell, filed, cx));
    assert!(account_names(&shell, cx).contains(&"Savings".to_owned()));
    assert!(!cx.update(|_, cx| shell.read(cx).live.as_ref().unwrap().paused));
    assert_eq!(
        check(&shell, cx),
        topics(&[]),
        "the restored file is the baseline"
    );
}

/// A restore that fails resumes the checks at once.
#[gpui_kit::test]
fn a_failed_restore_resumes_the_checks(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let settings = cx.update(|_, cx| shell.read(cx).settings().clone());
    cx.update(|_, cx| settings.update(cx, |settings, cx| settings.restoring(cx)));
    settle(cx);
    let filed = scratch.file_outside("WHILEPAUSED");
    cx.update(|_, cx| settings.update(cx, |settings, cx| settings.restore_failed(cx)));
    settle(cx);
    assert_eq!(last_reported(&shell, cx), Some(topics(&[Topic::Inbox])));
    assert!(listed(&shell, filed, cx));
}

/// A Restore dialog over `backup`, checked and ready to press Restore.
fn restore_dialog(
    shell: &Entity<ShellView>,
    backup: &std::path::Path,
    cx: &mut VisualTestContext,
) -> Entity<RestoreDialog> {
    let settings = cx.update(|_, cx| shell.read(cx).settings().clone());
    let target = RestoreTarget {
        path: backup.to_path_buf(),
        taken_at: None,
        local: None,
    };
    let dialog =
        cx.update(|window, cx| cx.new(|cx| RestoreDialog::new(settings, target, window, cx)));
    settle(cx);
    assert!(cx.update(|_, cx| dialog.read(cx).can_restore()), "checked");
    dialog
}

fn paused(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| shell.read(cx).live.as_ref().unwrap().paused)
}

/// Pressing Restore in the dialog pauses the checks, and a restore that
/// fails (the file went bad after the dialog checked it) leaves them
/// running: an outside write is reported.
#[gpui_kit::test]
fn a_restore_the_dialog_could_not_do_leaves_the_checks_running(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let (backup, _) = SettingsStore::new(scratch.path.clone()).back_up().unwrap();
    let dialog = restore_dialog(&shell, &backup.path, cx);
    std::fs::write(&backup.path, b"not a database").unwrap();

    cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.restore(cx)));
    assert!(paused(&shell, cx), "paused while it runs");
    settle(cx);
    assert!(
        cx.update(|_, cx| dialog.read(cx).stage() != &crate::settings::restore::Stage::Restoring),
        "the dialog is back and says why"
    );
    assert!(!paused(&shell, cx), "the failure resumed the checks");

    let filed = scratch.file_outside("AFTERTHEFAILURE");
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox]));
    assert!(listed(&shell, filed, cx));
}

/// A restore done through the dialog leaves the checks running once the
/// round after it has finished.
#[gpui_kit::test]
fn a_restore_through_the_dialog_leaves_the_checks_running(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let (backup, _) = SettingsStore::new(scratch.path.clone()).back_up().unwrap();
    let dialog = restore_dialog(&shell, &backup.path, cx);
    let filed = scratch.file_outside("AFTERTHEBACKUP");
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox]));

    cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.restore(cx)));
    assert!(paused(&shell, cx), "paused while it runs");
    settle(cx);
    assert!(!paused(&shell, cx), "resumed after the round");
    assert!(!listed(&shell, filed, cx), "the restored data is shown");
    assert_eq!(check(&shell, cx), topics(&[]));
    let again = scratch.file_outside("AFTERTHERESTORE");
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox]));
    assert!(listed(&shell, again, cx));
}

/// A write between the baseline and the views' first reads is reported.
#[gpui_kit::test]
fn a_write_between_the_baseline_and_the_first_reads_is_reported(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let watched = baseline(&scratch.path);
    let filed = scratch.file_outside("BEFORETHEWINDOW");
    let (shell, cx) = open_watched(cx, &scratch, View::Inbox, watched);
    assert!(listed(&shell, filed, cx));
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox]));
}

/// Without a baseline (no database file yet when it was taken), the first
/// check that compares reloads every view.
#[gpui_kit::test]
fn without_a_baseline_the_first_check_reloads_every_view(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("orre.db");
    let watched = baseline(&path);
    fixture::seed(&mut Database::open(&path).unwrap());
    let scratch = Scratch { _dir: dir, path };
    let (shell, cx) = open_watched(cx, &scratch, View::Inbox, watched);
    assert_eq!(check(&shell, cx), Topic::ALL.into_iter().collect());
    assert_eq!(check(&shell, cx), topics(&[]));
}

/// The window becoming active checks at once.
#[gpui_kit::test]
fn becoming_active_checks_at_once(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let filed = scratch.file_outside("ONACTIVATION");
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.activation_changed(false, cx)));
    settle(cx);
    assert_eq!(
        last_reported(&shell, cx),
        None,
        "going inactive doesn't check"
    );
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.activation_changed(true, cx)));
    settle(cx);
    assert_eq!(last_reported(&shell, cx), Some(topics(&[Topic::Inbox])));
    assert!(listed(&shell, filed, cx));
}

/// Tests (and the snapshot example) never start the timer: time passing
/// checks nothing.
#[gpui_kit::test]
fn no_timer_runs_unless_the_app_starts_it(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let filed = scratch.file_outside("NOBODYLOOKED");
    cx.executor().advance_clock(Duration::from_secs(10));
    settle(cx);
    assert_eq!(last_reported(&shell, cx), None);
    assert!(!listed(&shell, filed, cx));
}

/// The timer started by `keep_watching` checks every second while the
/// window is active, and every five once it isn't (its loop, restarted when
/// the activation changes).
#[gpui_kit::test]
fn the_timer_checks_every_second_while_active_and_every_five_when_not(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.keep_watching(window, cx)));
    // The test window's own activation state is not what is under test.
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.activation_changed(true, cx)));
    settle(cx);
    assert!(cx.update(|_, cx| shell.read(cx).live.as_ref().unwrap().active));
    let advance = |cx: &mut VisualTestContext, by: Duration| {
        cx.executor().advance_clock(by);
        settle(cx);
    };

    let filed = scratch.file_outside("ONTHETIMER");
    advance(cx, Duration::from_millis(500));
    assert!(!listed(&shell, filed, cx), "not before the tick");
    advance(cx, Duration::from_millis(600));
    assert_eq!(last_reported(&shell, cx), Some(topics(&[Topic::Inbox])));
    assert!(listed(&shell, filed, cx));

    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.activation_changed(false, cx)));
    settle(cx);
    let second = scratch.file_outside("WHILEINACTIVE");
    advance(cx, Duration::from_secs(4));
    assert!(!listed(&shell, second, cx), "inactive: not every second");
    advance(cx, Duration::from_millis(1100));
    assert!(listed(&shell, second, cx), "inactive: every five seconds");

    let third = scratch.file_outside("ACTIVEAGAIN");
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.activation_changed(true, cx)));
    settle(cx);
    assert!(listed(&shell, third, cx), "active again: a check at once");
    let fourth = scratch.file_outside("BACKTOSECONDS");
    advance(cx, Duration::from_millis(1100));
    assert!(listed(&shell, fourth, cx), "active again: every second");
}

// What arrived (LU.5) ---------------------------------------------------------------

/// The arrival line's text, if there is one.
fn line(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| shell.read(cx).status(cx).arrival.map(|line| line.text()))
}

fn badge_unseen(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| {
        shell
            .read(cx)
            .arrived
            .arrivals()
            .inbox_badge_unseen()
            .is_some()
    })
}

fn marked_suggestions(
    shell: &Entity<ShellView>,
    cx: &mut VisualTestContext,
) -> BTreeSet<SuggestionId> {
    cx.update(|_, cx| shell.read(cx).inbox().read(cx).marked())
}

fn marked_rows(
    shell: &Entity<ShellView>,
    cx: &mut VisualTestContext,
) -> BTreeSet<orre_core::id::TransactionId> {
    cx.update(|_, cx| shell.read(cx).transactions().read(cx).marked())
}

/// Sets the arrivals' clock to `now` and moves them along to it.
fn at(shell: &Entity<ShellView>, now: std::time::Instant, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.freeze_time(now);
            shell.tick_arrivals(cx);
        })
    });
    settle(cx);
}

fn show(shell: &Entity<ShellView>, view: View, cx: &mut VisualTestContext) {
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.show(view, window, cx)));
    settle(cx);
}

fn add_row_outside(
    scratch: &Scratch,
    shell: &Entity<ShellView>,
    cx: &mut VisualTestContext,
) -> orre_core::id::TransactionId {
    let account = cx.update(|_, cx| {
        shell.read(cx).accounts().read(cx).data().tree.accounts[0]
            .account
            .id
    });
    let entry = ManualTransaction::new(
        account,
        NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(),
        Money::new(-1234, Currency::GBP),
        "OUTSIDE SHOP",
    );
    manual::create(&mut scratch.db(), &entry).unwrap()
}

/// Suggestions filed while the Inbox isn't on show: the line names the
/// client, the badge turns unseen and the new row is marked. Nothing times
/// out until the Inbox is shown; then one hold, a fade, and they're gone,
/// while the count stays.
#[gpui_kit::test]
fn suggestions_filed_out_of_sight_wait_until_the_inbox_is_shown(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let start = std::time::Instant::now();
    at(&shell, start, cx);
    // The Inbox shows its pending list, so the new row's id is known.
    show(&shell, View::Inbox, cx);
    show(&shell, View::Transactions, cx);

    let filed = scratch.file_outside("FROMOUTSIDE");
    check(&shell, cx);
    assert_eq!(
        line(&shell, cx).as_deref(),
        Some("1 suggestion from claude-code")
    );
    assert!(badge_unseen(&shell, cx));
    assert_eq!(marked_suggestions(&shell, cx), BTreeSet::from([filed]));

    // Unseen has no time limit.
    at(&shell, start + Duration::from_secs(3600), cx);
    assert!(badge_unseen(&shell, cx), "still unseen an hour later");
    assert!(line(&shell, cx).is_some());

    // More from the same client adds up.
    let second = scratch.file_outside("FROMOUTSIDEAGAIN");
    check(&shell, cx);
    assert_eq!(
        line(&shell, cx).as_deref(),
        Some("2 suggestions from claude-code")
    );

    let seen = start + Duration::from_secs(3601);
    cx.update(|_, cx| shell.update(cx, |shell, _| shell.freeze_time(seen)));
    show(&shell, View::Inbox, cx);
    assert_eq!(
        marked_suggestions(&shell, cx),
        BTreeSet::from([filed, second])
    );
    at(
        &shell,
        seen + crate::shell::arrivals::HOLD - Duration::from_millis(1),
        cx,
    );
    assert!(line(&shell, cx).is_some(), "held for ten seconds");
    at(
        &shell,
        seen + crate::shell::arrivals::HOLD + crate::shell::arrivals::FADE_OUT,
        cx,
    );
    assert_eq!(line(&shell, cx), None);
    assert!(!badge_unseen(&shell, cx));
    assert!(marked_suggestions(&shell, cx).is_empty());
    let pending = cx.update(|_, cx| shell.read(cx).inbox().read(cx).badge());
    assert!(pending > 0, "the count is a fact, not news");
}

/// A row added in a terminal while Transactions is on show: marked, said
/// in the line, seen at once, and the selection doesn't move.
#[gpui_kit::test]
fn a_row_added_outside_is_marked_and_said_once(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let start = std::time::Instant::now();
    at(&shell, start, cx);
    let transactions = cx.update(|_, cx| shell.read(cx).transactions().clone());
    let first = cx.update(|_, cx| transactions.read(cx).register().rows()[0].id);
    cx.update(|_, cx| transactions.update(cx, |view, cx| view.select([first], cx)));

    let added = add_row_outside(&scratch, &shell, cx);
    check(&shell, cx);
    assert_eq!(line(&shell, cx).as_deref(), Some("1 new transaction"));
    assert_eq!(marked_rows(&shell, cx), BTreeSet::from([added]));
    assert!(!badge_unseen(&shell, cx), "no suggestion arrived");
    let selected: Vec<_> = cx.update(|_, cx| {
        transactions
            .read(cx)
            .selected()
            .iter()
            .map(|row| row.id)
            .collect()
    });
    assert_eq!(selected, [first], "the mark takes nothing");

    // On show in a focused window, so it was seen on arrival.
    at(
        &shell,
        start + crate::shell::arrivals::HOLD + crate::shell::arrivals::FADE_OUT,
        cx,
    );
    assert_eq!(line(&shell, cx), None);
    assert!(marked_rows(&shell, cx).is_empty());
}

/// Deleted in a terminal: "removed", with no mark and nothing to open.
#[gpui_kit::test]
fn a_row_deleted_outside_is_said_removed(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let added = add_row_outside(&scratch, &shell, cx);
    check(&shell, cx);
    assert_eq!(line(&shell, cx).as_deref(), Some("1 new transaction"));

    manual::delete(&mut scratch.db(), added).unwrap();
    check(&shell, cx);
    assert_eq!(line(&shell, cx).as_deref(), Some("1 transaction removed"));
    let opens = cx.update(|_, cx| shell.read(cx).status(cx).arrival.unwrap().opens);
    assert_eq!(opens, None);
}

/// Clicking the line opens its view on what it's about, and sees it.
#[gpui_kit::test]
fn clicking_the_line_opens_the_inbox_on_its_pending_list(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let inbox = cx.update(|_, cx| shell.read(cx).inbox().clone());
    cx.update(|_, cx| {
        inbox.update(cx, |inbox, cx| {
            inbox.show_status(core_inbox::SuggestionStatus::Approved, cx)
        })
    });
    settle(cx);
    scratch.file_outside("FROMOUTSIDE");
    check(&shell, cx);
    assert!(badge_unseen(&shell, cx));

    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.arrival_clicked(window, cx)));
    settle(cx);
    assert_eq!(cx.update(|_, cx| shell.read(cx).view), View::Inbox);
    assert_eq!(
        cx.update(|_, cx| inbox.read(cx).status()),
        core_inbox::SuggestionStatus::Pending
    );
    let held = cx.update(|_, cx| shell.read(cx).arrived.arrivals().next_deadline());
    assert!(held.is_some(), "seen: the hold has started");
}

/// The app's own writes are never shown as arrivals: they have undo bars.
#[gpui_kit::test]
fn the_apps_own_writes_are_not_arrivals(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let inbox = cx.update(|_, cx| shell.read(cx).inbox().clone());
    let pending = scratch.pending();
    // Approving a rule changes the ledger too.
    cx.update(|_, cx| inbox.update(cx, |inbox, cx| inbox.approve(pending[1], cx)));
    settle(cx);
    check(&shell, cx);
    assert_eq!(line(&shell, cx), None);
    assert!(marked_rows(&shell, cx).is_empty());
    assert!(marked_suggestions(&shell, cx).is_empty());
}

/// What the window shows when an import lands mid-task: the line and the
/// mark, with the search, the selection and the open view untouched.
#[gpui_kit::test]
fn the_window_shows_what_arrived_and_keeps_what_the_user_was_doing(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let transactions = cx.update(|_, cx| shell.read(cx).transactions().clone());
    let first = cx.update(|_, cx| transactions.read(cx).register().rows()[0].id);
    cx.update(|window, cx| {
        transactions.update(cx, |view, cx| {
            view.select([first], cx);
            view.set_search("SHOP", window, cx);
        })
    });
    settle(cx);
    let describe =
        |cx: &mut VisualTestContext| cx.update(|window, cx| shell.read(cx).describe(window, cx));
    let before = describe(cx);
    assert_eq!(before.arrival, None);

    let added = add_row_outside(&scratch, &shell, cx);
    scratch.file_outside("FROMOUTSIDE");
    check(&shell, cx);
    settle(cx);

    let after = describe(cx);
    assert_eq!(after.view, before.view);
    assert_eq!(after.focus, before.focus);
    // The ledger and the Inbox both arrived; the line says the later.
    assert!(after.arrival.is_some());
    let (_, unseen) = after.inbox_badge.expect("pending suggestions");
    assert!(unseen, "filed while the Inbox wasn't on show");
    let crate::shell::describe::Screen::Transactions { transactions, .. } = after.screen else {
        panic!("still on Transactions");
    };
    assert_eq!(transactions.toolbar.search, "SHOP");
    let rows: Vec<_> = transactions
        .days
        .iter()
        .flat_map(|day| &day.rows)
        .chain(&transactions.flat)
        .collect();
    let new_row = rows
        .iter()
        .find(|row| row.id == added)
        .expect("listed: it matches");
    assert!(new_row.marked);
    assert!(
        rows.iter()
            .filter(|row| row.selected)
            .all(|row| row.id == first)
    );
}

/// The app's own import or undo writes on a background thread: no check
/// starts meanwhile, and when it's done the checks resume after the round
/// that reads it, so it's never shown as another process's (review of
/// LU.5).
#[gpui_kit::test]
fn the_apps_own_background_import_is_not_an_arrival(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let imports = cx.update(|_, cx| shell.read(cx).imports().clone());
    let reported = last_reported(&shell, cx);

    cx.update(|_, cx| imports.update(cx, |imports, cx| imports.writing(cx)));
    // What the import commits, on its background thread.
    add_row_outside(&scratch, &shell, cx);
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.check_changes(cx)));
    settle(cx);
    assert_eq!(
        last_reported(&shell, cx),
        reported,
        "no check while it writes"
    );

    // Done: the shell hears of it as the import does.
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.own_write_done(BTreeSet::from([Topic::Ledger]), cx)
        })
    });
    settle(cx);
    assert_eq!(check(&shell, cx), topics(&[]), "the round took it");
    assert_eq!(line(&shell, cx), None);
    assert!(marked_rows(&shell, cx).is_empty());
}

/// A failed import resumes the checks at once.
#[gpui_kit::test]
fn a_failed_own_import_resumes_the_checks(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let imports = cx.update(|_, cx| shell.read(cx).imports().clone());
    cx.update(|_, cx| imports.update(cx, |imports, cx| imports.writing(cx)));
    cx.update(|_, cx| imports.update(cx, |imports, cx| imports.write_failed(cx)));
    settle(cx);
    add_row_outside(&scratch, &shell, cx);
    assert_eq!(check(&shell, cx), topics(&[Topic::Ledger]));
    assert_eq!(line(&shell, cx).as_deref(), Some("1 new transaction"));
}

/// A check in flight when the app's own write lands may see that write:
/// what it reports is reloaded, but not shown as an arrival.
#[gpui_kit::test]
fn a_check_overlapping_an_own_write_shows_nothing(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    add_row_outside(&scratch, &shell, cx);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.check_changes(cx);
            // The app's own write, before the check's answer is in.
            shell.refresh(BTreeSet::from([Topic::Ledger]), cx);
        })
    });
    settle(cx);
    assert_eq!(line(&shell, cx), None);
    assert!(marked_rows(&shell, cx).is_empty());
}

/// A read for another process's change still in flight when the app's own
/// import starts may land with the import in it: nothing it brings is an
/// arrival.
#[gpui_kit::test]
fn an_outside_read_overlapping_an_own_import_shows_nothing(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let imports = cx.update(|_, cx| shell.read(cx).imports().clone());
    // What the import commits, seen by the outside read.
    add_row_outside(&scratch, &shell, cx);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.reload_views(&topics(&[Topic::Ledger]), Reader::Outside, cx)
        });
        imports.update(cx, |imports, cx| imports.writing(cx));
    });
    settle(cx);
    assert_eq!(line(&shell, cx), None);
    assert!(marked_rows(&shell, cx).is_empty());
}

/// An undo that removes the selected row while an outside read is in
/// flight: the inspector doesn't say it was removed outside the window.
#[gpui_kit::test]
fn an_outside_read_overlapping_an_own_undo_notes_no_removal(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let imports = cx.update(|_, cx| shell.read(cx).imports().clone());
    let transactions = cx.update(|_, cx| shell.read(cx).transactions().clone());
    let added = add_row_outside(&scratch, &shell, cx);
    check(&shell, cx);
    cx.update(|_, cx| transactions.update(cx, |view, cx| view.select([added], cx)));

    // What the undo removes, seen by the outside read.
    manual::delete(&mut scratch.db(), added).unwrap();
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.reload_views(&topics(&[Topic::Ledger]), Reader::Outside, cx)
        });
        imports.update(cx, |imports, cx| imports.writing(cx));
    });
    settle(cx);
    assert!(cx.update(|_, cx| transactions.read(cx).removed_note().is_none()));
    assert_ne!(line(&shell, cx).as_deref(), Some("1 transaction removed"));
}

/// Restores `backup` as the app's Settings does, and lets every view read
/// the restored data.
fn restore(
    shell: &Entity<ShellView>,
    store: &SettingsStore,
    backup: &std::path::Path,
    cx: &mut VisualTestContext,
) {
    let settings = cx.update(|_, cx| shell.read(cx).settings().clone());
    cx.update(|_, cx| settings.update(cx, |settings, cx| settings.restoring(cx)));
    settle(cx);
    let restored = store.restore(backup).unwrap();
    cx.update(|_, cx| settings.update(cx, |settings, cx| settings.restored(&restored, cx)));
    settle(cx);
}

/// A suggestion filed after the backup, still unseen (the Inbox not shown):
/// the restore takes it away, and the line, the badge's unseen fill and
/// the mark go with it (SLI-62).
#[gpui_kit::test]
fn a_restore_clears_a_suggestion_line_it_took_away(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    // The Inbox shows its pending list, so the new row's id is known.
    show(&shell, View::Inbox, cx);
    show(&shell, View::Transactions, cx);
    let badge = cx.update(|_, cx| shell.read(cx).inbox().read(cx).badge());
    let store = SettingsStore::new(scratch.path.clone());
    let (backup, _) = store.back_up().unwrap();

    let filed = scratch.file_outside("AFTERTHEBACKUP");
    check(&shell, cx);
    assert_eq!(
        line(&shell, cx).as_deref(),
        Some("1 suggestion from claude-code")
    );
    assert!(badge_unseen(&shell, cx));
    assert_eq!(marked_suggestions(&shell, cx), BTreeSet::from([filed]));

    restore(&shell, &store, &backup.path, cx);
    assert!(!listed(&shell, filed, cx));
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).inbox().read(cx).badge()),
        badge,
        "the badge counts the restored store"
    );
    assert_eq!(
        line(&shell, cx),
        None,
        "no line for a suggestion that's gone"
    );
    assert!(!badge_unseen(&shell, cx));
    assert!(marked_suggestions(&shell, cx).is_empty());
}

/// The same for a transaction added after the backup: its line and mark go
/// with it.
#[gpui_kit::test]
fn a_restore_clears_a_ledger_line_it_took_away(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let before = rows(&shell, cx);
    let store = SettingsStore::new(scratch.path.clone());
    let (backup, _) = store.back_up().unwrap();

    let added = add_row_outside(&scratch, &shell, cx);
    check(&shell, cx);
    assert_eq!(line(&shell, cx).as_deref(), Some("1 new transaction"));
    assert_eq!(marked_rows(&shell, cx), BTreeSet::from([added]));

    restore(&shell, &store, &backup.path, cx);
    assert_eq!(rows(&shell, cx), before);
    assert_eq!(line(&shell, cx), None, "no line for a row that's gone");
    assert!(marked_rows(&shell, cx).is_empty());
}
