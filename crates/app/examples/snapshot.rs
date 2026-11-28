//! Renders Orre screens offscreen to PNG, for comparing them against the
//! Paper artboards (`docs/ROADMAP.md` D5.1).
//!
//! ```sh
//! cargo run --features dev-samples --example snapshot -- <out-dir>
//! ```
//!
//! Uses GPUI's headless platform with the real text system (cosmic-text on
//! Linux, so the bundled fonts shape as in the app) and the wgpu headless
//! renderer, so no window or display is needed. Each screen is written at
//! 1x and 2x:
//!
//! - `transactions`: Paper `18E-0`, one AI-suggested transaction selected;
//! - `transactions-multi`: Paper `1NP-0`, three uncategorised rows selected;
//! - `overview`: Paper `22V-0`, October 2026 on the sample data (M4.1), with
//!   no pending suggestions, so no agent card;
//! - `overview-empty`: the Overview with no transactions (the empty state);
//! - `overview-agent`: the Overview with the Inbox's sample suggestions
//!   pending, so the compact agent card shows (Paper `2LQ-0`);
//! - `transactions-category-menu`: the in-place category select open on the
//!   Blue Bottle row (composed for M3.3, no Paper drawing);
//! - `transactions-date-menu`: the Date chip's menu (composed for M3.3);
//! - `transactions-sorted`: sorted by amount, largest out first, with the
//!   Uncategorised chip on: one flat list with a Date column (M3.P13, Paper
//!   "Item · Sorted by a column (P13)" on `IS2-0`);
//! - `transactions-no-match`: a search that matches nothing;
//! - `transactions-split`: Paper `2Q3-0`, the Waitrose payment split, with
//!   tags, the bank's note and the user's;
//! - `transactions-split-short` and `-over`: the split editor not balanced
//!   (Paper `4XF-0`);
//! - `transactions-details`: an unsplit row's Details and Split button
//!   (Paper `3WT-0`, without the rule prompt of M3.13);
//! - `transactions-split-start`: just after "Split" (composed for M3.9);
//! - `transactions-tag-menu`: the tag menu with a new name typed (composed
//!   for M3.9);
//! - `transactions-edit-failed`: a name typed on Vinted, then another row
//!   clicked: the sample data refuses it and the failure bar says so
//!   (composed, M3.10 review follow-up);
//! - `rules`: the shell with the Rules tab active and its screen not built
//!   yet (title bar: Paper `B8Y-0`);
//! - `settings`: Settings · AI clients on sample data (Paper `AQ2-0`, title
//!   bar `B08-0`); `settings-off`: the same with AI access off (`GR9-0`);
//!   `settings-log` (`DC6-0`); `settings-backups` (`F63-0`);
//!   `settings-backed-up`: the notification after "Back up now" (`HF7-0`);
//!   `settings-enable` (`GYO-0`), `settings-restore` (`H6D-0`),
//!   `settings-restore-fails` (`H73-0`) and `settings-export` (`HAG-0`): the
//!   dialogs over Settings;
//! - `account`: Current account selected with no transaction selected, on
//!   the states board's accounts (panel and inspector: Paper `8D8-0`);
//! - `account-kind`: the same with the Kind menu open (Paper `FHZ-0`);
//! - `new-account`: the New account dialog (Paper `8IR-0`);
//! - `add-balance`: Add balance for Current account (Paper `I7C-0`);
//! - `add-balance-day-taken`: the same on 1 Sep, which has a balance:
//!   said before any click, Add balance disabled (Paper `IRD-0`, M3.P6);
//! - `account-context-menu`: a right-click on Current account (Paper
//!   "Context menu · account" on `IS2-0`, M3.P10); `edit-balance`: Edit for
//!   its 1 Sep balance ("Dialog · Edit balance", M3.P9);
//! - D6.9's accounts you keep yourself (M3.17): `accounts-asset`, House
//!   selected with its valuations (Paper `BSI-0`); `accounts-liability`,
//!   Mortgage (the inspector on `EPF-0`); `new-account-asset` (`EPI-0`);
//!   `add-valuation` (composed from `FS0-0`); `add-balance-liability`
//!   (`FS0-0`); `close-account` (`FT2-0`); `close-account-bank` and
//!   `delete-account` (composed); `account-closed`, the undo bar after
//!   closing (composed from D6.3's `53O-0`);
//! - the cash account (M3.10): `accounts-cash`, Cash selected (the inspector
//!   on `EPF-0`); `cash-count`, a count £5.00 short (`EPF-0`);
//!   `cash-count-first`, the first count (composed);
//! - `inbox-rule`: the Inbox with the UBER *EATS rule open (Paper `ABK-0`);
//! - `inbox-name`: the LIME*RIDE merchant name open (Paper `DL8-0`);
//! - `inbox-categorisation`, `inbox-transfer`, `inbox-refund`,
//!   `inbox-orre`: the other kinds' inspectors (Paper `564-0`, `EWZ-0`);
//! - `inbox-wide`: a wide rule summed by merchant (Paper `EWZ-0`);
//! - `inbox-approved`: after approving the DELIVEROO rule (`EWZ-0`'s bar,
//!   without Undo);
//! - `inbox-empty`: no suggestions;
//! - `import-file`: the Import dialog's File step (Paper `6J4-0`);
//! - `import-map`: the sample Monzo export on the Map step (`5ZQ-0`);
//! - `import-map-menu`: the same with the Merchant menu open, drawn over
//!   the dialog (composed);
//! - `import-map-own`: a CSV no preset recognises, mapped by hand (composed
//!   for M3.4, no Paper drawing);
//! - `import-map-no-accounts`: the same on a database with no accounts yet:
//!   the footer says to add one (M3.P1, composed);
//! - `import-map-none-importable`: the same where every account is closed:
//!   the footer says which kind to add (M3.P1, composed);
//! - `import-save-preset`: "Save as preset…" (composed for M3.5);
//! - `import-review`: the Review step on a scratch database where part of
//!   the file was imported before (`6J4-0`);
//! - `import-review-mismatch`: the same with entered balances that don't
//!   match and a lock date (`6NP-0`, `6NX-0`);
//! - `import-review-corrections`: the Review step where the earlier export
//!   had the Odeon row for another amount, so the sample is the bank's
//!   correction of it (R.4; composed, no Paper drawing);
//! - `import-done`: the notification after an import;
//! - `review-duplicates`: Possible duplicates with the Ocado pair open
//!   (Paper `8SE-0`); `review-balances`: Balance checks with the joint
//!   account's failing check selected (`98K-0`); `review-locked`: a pair in
//!   a locked period (`9MQ-0`); `review-merged`: the undo bar after a merge
//!   (`9MQ-0`); `review-lock` and `review-unlock`: the dialogs (`9MQ-0`);
//!   `review-empty`: no possible duplicates (`9MQ-0`);
//! - `rules`: Rules with rule 2 (UBER) selected (Paper `B1J-0`);
//!   `rules-never-reached`: rule 3, which UBER always catches (`HFH-0`);
//!   `rules-add-action`: the add-action menu open (`HFH-0`);
//!   `rules-pattern-error`: a pattern that can't be used (`HFH-0`);
//!   `rules-apply`: "Create rule…" from the VINTED edit (`EFT-0`);
//!   `rules-names`: Merchant names with Amazon selected (`G2O-0`);
//!   `rules-history`: Change history with the newest change (`GZT-0`);
//!   `rules-delete` and `rules-undo`: the dialogs (`HFH-0`); `rules-empty`:
//!   no rules yet (`HFH-0`); `transactions-rule-prompt`: "Create a rule
//!   from this edit?" after the VINTED edit (`3WT-0`); `rules-reorder`:
//!   UBER TRIP moved above UBER with ⌥↑, with the undo bar (`I8D-0`);
//!   `rules-reorder-filtered`: Add tag picked, JUSTGIVING moved above
//!   WAITROSE with ⌥↑ (`IIH-0`);
//! - `rules-categories`: Categories with Groceries selected;
//!   `rules-category-new`: New category, "Coffee beans" under Food;
//!   `rules-category-delete`: the dialog on the unused Coffee (composed,
//!   not drawn);
//! - `links-refund`: the ASOS.com refund with Orre's suggestion (Paper
//!   `9WS-0`); `links-shared`: Odeon shared with Sam (`BAT-0`);
//!   `links-dialog`: the link dialog on the refund (`EUY-0`);
//!   `links-linked`: the refund after "Link as refund", with the undo bar
//!   (`EC0-0`, `FGQ-0`); `links-purchase`: the purchase after linking
//!   (`FE4-0`); `links-claim`: Trainline claimed back, partly repaid
//!   (`FJ1-0`); `review-owed`: Owed to you with Sam selected (`CCF-0`);
//!   `review-owed-empty`: nothing owed (`GCJ-0`);
//! - `transactions-excluded`: the Excluded chip on, Odeon excluded as a
//!   bank error and selected (Paper `3FE-0`); `transactions-exclude-dialog`
//!   (`51L-0`); `transactions-manual`: Cash selected, a manual row in the
//!   inspector (`4D7-0`); `transactions-new-transaction` (`529-0`);
//!   `transactions-undo-import`: the undo-import confirmation (composed for
//!   M3.10);
//! - `transactions-transfer`, `transactions-transfer-confirmed`: the
//!   inspector's Transfer section, a suggested and a confirmed pair
//!   (composed for M3.8);
//! - `showcase`: the component showcase.
//!
//! The Inbox reads a scratch database seeded with `inbox::fixture`, the
//! review queues one seeded with `checks::fixture`, Rules one seeded with
//! `rules::fixture`, the links snapshots one seeded with
//! `transactions::link_fixture`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use gpui_kit::{AnyView, App, AppContext as _, Entity, HeadlessAppContext, Window, size};

use orre_app::accounts::{Accounts, fixture as accounts_fixture};
use orre_app::checks::store::ChecksStore;
use orre_app::checks::{Checks, Queue, fixture as checks_fixture};
use orre_app::import::store::ImportStore;
use orre_app::import::{ImportNotice, fixture as import_fixture};
use orre_app::inbox::store::{Entry, InboxStore};
use orre_app::inbox::{Inbox, fixture as inbox_fixture};
use orre_app::overview::{OverviewView, fixture as overview_fixture};
use orre_app::rules::store::RulesStore;
use orre_app::rules::view::{Input, Menu as RulesMenu};
use orre_app::rules::{Rules, Section as RulesSection, fixture as rules_fixture};
use orre_app::settings::restore::{RestoreDialog, Stage};
use orre_app::settings::{
    Notice, NoticeMessage, RestoreTarget, Section, SettingsView, fixture as settings_fixture,
};
use orre_app::shell::{ShellView, View};
use orre_app::showcase::Showcase;
use orre_app::transactions::{Menu, fixture};
use orre_core::db::Database;
use orre_core::inbox::{ORRE_SOURCE, Payload};
use orre_core::reporting::register::{Sort, SortColumn};

const WIDTH: f32 = 1440.;
const HEIGHT: f32 = 900.;

fn main() -> Result<()> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "snapshots".into()),
    );
    std::fs::create_dir_all(&out)?;

    let text_system = gpui_kit::platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(gpui_kit::assets::Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        orre_app::theme::install(cx).expect("theme installs");
        orre_app::showcase::init(cx);
        orre_app::shell::init(cx);
    });

    snapshot(&mut cx, &out, "transactions", |window, cx| {
        cx.new(|cx| {
            let shell = ShellView::new(View::Transactions, window, cx);
            // Paper's `JJY-0`: the sample's "3 new transactions" are marked.
            shell.transactions().update(cx, |view, cx| {
                view.show_marks_drawn(
                    ["txn-pret-09", "txn-bluebottle-09", "txn-tfl-09"]
                        .map(fixture::id)
                        .into(),
                    cx,
                )
            });
            shell
        })
        .into()
    })?;
    snapshot(&mut cx, &out, "transactions-multi", |window, cx| {
        cx.new(|cx| {
            let shell = ShellView::new(View::Transactions, window, cx);
            shell.transactions().update(cx, |view, cx| {
                view.select(
                    ["txn-bluebottle-09", "txn-steam-07", "txn-vinted-06"].map(fixture::id),
                    cx,
                )
            });
            shell
        })
        .into()
    })?;
    snapshot(&mut cx, &out, "overview", |window, cx| {
        cx.new(|cx| {
            let shell = ShellView::new(View::Overview, window, cx);
            shell.overview().update(cx, |view, cx| {
                view.select_month(overview_fixture::october(), cx)
            });
            shell
        })
        .into()
    })?;
    {
        let dir = tempfile::TempDir::new()?;
        let database = dir.path().join("orre.db");
        inbox_fixture::seed(&mut Database::open(&database)?);
        snapshot(&mut cx, &out, "overview-agent", move |window, cx| {
            let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
            let inbox = cx.new(|_| Inbox::from_store(InboxStore::new(database)));
            cx.new(|cx| {
                let shell = ShellView::with_data(View::Overview, accounts, inbox, window, cx);
                shell.overview().update(cx, |view, cx| {
                    view.select_month(overview_fixture::october(), cx)
                });
                shell
            })
            .into()
        })?;
        drop(dir);
    }
    snapshot(&mut cx, &out, "overview-empty", |window, cx| {
        cx.new(|cx| {
            let mut shell = ShellView::new(View::Overview, window, cx);
            let overview = cx.new(|_| OverviewView::sample(overview_fixture::empty));
            shell.install_overview(overview, window, cx);
            shell
        })
        .into()
    })?;
    for (name, step) in [
        ("transactions-category-menu", TableStep::CategoryMenu),
        ("transactions-date-menu", TableStep::DateMenu),
        ("transactions-sorted", TableStep::Sorted),
        ("transactions-no-match", TableStep::NoMatch),
        ("transactions-split", TableStep::Split),
        ("transactions-split-short", TableStep::SplitShort),
        ("transactions-split-over", TableStep::SplitOver),
        ("transactions-details", TableStep::Details),
        ("transactions-split-start", TableStep::SplitStart),
        ("transactions-tag-menu", TableStep::TagMenu),
        ("transactions-edit-failed", TableStep::EditFailed),
    ] {
        let built: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::default();
        let keep = built.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let shell = cx.new(|cx| ShellView::new(View::Transactions, window, cx));
            *keep.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        let then = move |window: &mut Window, cx: &mut App| {
            let shell = built.borrow().clone().expect("the shell was built");
            let view = shell.read(cx).transactions().clone();
            view.update(cx, |view, cx| match step {
                TableStep::CategoryMenu => {
                    let id = fixture::id("txn-bluebottle-09");
                    view.select([id], cx);
                    view.show_menu(Menu::Row(id), window, cx);
                }
                TableStep::DateMenu => view.show_menu(Menu::Date, window, cx),
                TableStep::Sorted => {
                    view.set_sort(Sort::Ascending(SortColumn::Amount), cx);
                    view.set_uncategorised(true, cx);
                }
                TableStep::NoMatch => view.set_search("zzz", window, cx),
                TableStep::Split | TableStep::SplitShort | TableStep::SplitOver => {
                    view.show_fixture(fixture::split_register(), cx);
                    view.select([fixture::id("txn-waitrose-07")], cx);
                    match step {
                        // Paper's states board: the third line removed.
                        TableStep::SplitShort => view.remove_split_line_at(2, window, cx),
                        TableStep::SplitOver => {
                            view.remove_split_line_at(2, window, cx);
                            view.type_split_amount(1, "17.10", window, cx);
                        }
                        _ => {}
                    }
                }
                TableStep::Details => view.select([fixture::id("txn-vinted-06")], cx),
                TableStep::SplitStart => {
                    view.select([fixture::id("txn-vinted-06")], cx);
                    view.split_selected(window, cx);
                }
                TableStep::TagMenu => {
                    view.show_fixture(fixture::split_register(), cx);
                    view.select([fixture::id("txn-waitrose-07")], cx);
                    view.show_menu(Menu::Tags, window, cx);
                    view.type_in_menu("Gift", window, cx);
                }
                TableStep::EditFailed => {
                    view.select([fixture::id("txn-vinted-06")], cx);
                    view.type_merchant("Vinted (jacket)", window, cx);
                    view.select([fixture::id("txn-bluebottle-09")], cx);
                }
            });
        };
        snapshot_then(&mut cx, &out, name, build, then)?;
    }
    snapshot(&mut cx, &out, "settings", |window, cx| {
        cx.new(|cx| ShellView::new(View::Settings, window, cx))
            .into()
    })?;
    for (name, section) in [
        ("settings-log", Section::Log),
        ("settings-backups", Section::Backups),
    ] {
        snapshot(&mut cx, &out, name, move |window, cx| {
            let shell = cx.new(|cx| ShellView::new(View::Settings, window, cx));
            let settings = shell.read(cx).settings().clone();
            settings.update(cx, |settings, cx| settings.show(section, cx));
            shell.into()
        })?;
    }
    // M4.11: the month start as Paper `K57-0` draws it (the 25th, Friday
    // before), and the default (calendar months, `KEZ-0`).
    for (name, start) in [
        ("settings-month-start", None),
        (
            "settings-month-start-default",
            Some(orre_core::reporting::period::MonthStart::CALENDAR),
        ),
    ] {
        snapshot(&mut cx, &out, name, move |window, cx| {
            let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
            let settings = cx.new(|cx| {
                let settings = SettingsView::fixture(cx);
                match start {
                    Some(start) => settings.with_month_start(start),
                    None => settings,
                }
            });
            settings.update(cx, |settings, cx| settings.show(Section::MonthStart, cx));
            cx.new(|cx| ShellView::with_settings(View::Settings, accounts, settings, window, cx))
                .into()
        })?;
    }
    snapshot(&mut cx, &out, "settings-off", |window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
        let settings = cx.new(SettingsView::fixture_off);
        cx.new(|cx| ShellView::with_settings(View::Settings, accounts, settings, window, cx))
            .into()
    })?;
    snapshot(&mut cx, &out, "settings-backed-up", |window, cx| {
        let shell = cx.new(|cx| ShellView::new(View::Settings, window, cx));
        let settings = shell.read(cx).settings().clone();
        settings.update(cx, |settings, cx| {
            settings.show(Section::Backups, cx);
            settings.set_notice(
                Some(Notice {
                    message: NoticeMessage::BackedUp {
                        transactions: Some(1_312),
                        size: 53 * 1024 * 1024 / 10,
                    },
                    reveal: Some(PathBuf::from("backup.db")),
                }),
                cx,
            );
        });
        shell.into()
    })?;
    for (name, dialog) in [
        ("settings-enable", SettingsDialog::Enable),
        ("settings-export", SettingsDialog::Export),
    ] {
        snapshot(&mut cx, &out, name, move |window, cx| {
            let shell = cx.new(|cx| ShellView::new(View::Settings, window, cx));
            let settings = shell.read(cx).settings().clone();
            settings.update(cx, |settings, cx| match dialog {
                SettingsDialog::Enable => settings.request_enable(
                    orre_core::mcp_client::ClientName::new("my-agent").unwrap(),
                    cx,
                ),
                SettingsDialog::Export => settings.request_export(cx),
            });
            shell.into()
        })?;
    }
    for (name, fails) in [
        ("settings-restore", false),
        ("settings-restore-fails", true),
    ] {
        snapshot(&mut cx, &out, name, move |_, cx| {
            let settings = cx.new(SettingsView::fixture);
            let supported = orre_core::db::Database::supported_schema_version();
            let (target, stage, since) = if fails {
                (
                    RestoreTarget {
                        path: PathBuf::from("orre-laptop.db"),
                        taken_at: None,
                        local: None,
                    },
                    Stage::Checked(orre_core::backup::detail::Assessment::TooNew {
                        found: supported + 2,
                        supported,
                    }),
                    None,
                )
            } else {
                let row = settings.read(cx).backups().rows[0].clone();
                (
                    RestoreTarget {
                        path: row.backup.path.clone(),
                        taken_at: Some(row.backup.taken_at),
                        local: Some(row.local),
                    },
                    Stage::Checked(orre_core::backup::detail::Assessment::Ready(
                        orre_core::backup::RestoreCheck {
                            path: row.backup.path.clone(),
                            named: Some((row.backup.reason, row.backup.taken_at)),
                            schema_version: supported,
                            supported_version: supported,
                            current: orre_core::backup::Current::At(supported),
                            interrupted: None,
                        },
                    )),
                    Some(settings_fixture::since()),
                )
            };
            cx.new(|cx| RestoreDialog::preview(settings, target, stage, since, cx))
                .into()
        })?;
    }
    for (name, step) in [
        ("account", AccountStep::Select),
        ("account-kind", AccountStep::KindMenu),
        ("new-account", AccountStep::NewAccount),
        ("add-balance", AccountStep::AddBalance),
        ("add-balance-day-taken", AccountStep::DayTaken),
        ("account-context-menu", AccountStep::ContextMenu),
        ("edit-balance", AccountStep::EditBalance),
    ] {
        let built: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::default();
        let keep = built.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::with_states()));
            let shell = cx.new(|cx| {
                ShellView::with_accounts(View::Transactions, accounts.clone(), window, cx)
            });
            accounts.update(cx, |accounts, cx| {
                accounts.select(Some(accounts_fixture::CURRENT), cx);
                match step {
                    AccountStep::Select | AccountStep::KindMenu | AccountStep::ContextMenu => {}
                    AccountStep::EditBalance => accounts.request_edit_balance(
                        accounts_fixture::CURRENT,
                        orre_core::id::BalanceId::new(1),
                        cx,
                    ),
                    AccountStep::NewAccount => accounts.request_new_account(cx),
                    AccountStep::AddBalance | AccountStep::DayTaken => {
                        accounts.request_add_balance(accounts_fixture::CURRENT, cx)
                    }
                }
            });
            *keep.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        // Opened once the selection has settled, as a click would.
        let then = move |window: &mut Window, cx: &mut App| {
            let shell = built.borrow().clone().expect("the shell was built");
            if step == AccountStep::DayTaken {
                let dialog = shell
                    .read(cx)
                    .dialog()
                    .cloned()
                    .and_then(|dialog| {
                        dialog
                            .downcast::<orre_app::accounts::add_balance::AddBalanceDialog>()
                            .ok()
                    })
                    .expect("Add balance is open");
                dialog.update(cx, |dialog, cx| {
                    dialog.type_amount("1,290.00", window, cx);
                    dialog.type_date("1 Sep 2026", window, cx);
                });
                return;
            }
            if step == AccountStep::ContextMenu {
                // A right-click on Current account, as in Paper's "Context
                // menu · account" (drawn on Savings).
                let accounts = shell.read(cx).accounts().clone();
                let focus = cx.focus_handle();
                accounts.update(cx, |accounts, cx| {
                    let groups = orre_app::accounts::context_menu::account_groups(
                        accounts.data(),
                        accounts_fixture::CURRENT,
                    );
                    accounts.open_context_menu(
                        orre_app::accounts::context_menu::ContextTarget::Account(
                            accounts_fixture::CURRENT,
                        ),
                        groups,
                        gpui_kit::point(gpui_kit::px(180.), gpui_kit::px(150.)),
                        focus,
                        window,
                        cx,
                    );
                });
                return;
            }
            if step != AccountStep::KindMenu {
                return;
            }
            let inspector = shell
                .read(cx)
                .transactions()
                .read(cx)
                .account_inspector(cx)
                .expect("an account is selected");
            inspector.update(cx, |inspector, cx| inspector.open_kind_menu(window, cx));
        };
        snapshot_then(&mut cx, &out, name, build, then)?;
    }
    valued_snapshots(&mut cx, &out)?;
    cash_snapshots(&mut cx, &out)?;
    inbox_snapshots(&mut cx, &out)?;
    import_snapshots(&mut cx, &out)?;
    review_snapshots(&mut cx, &out)?;
    rules_snapshots(&mut cx, &out)?;
    rules_category_snapshots(&mut cx, &out)?;
    links_snapshots(&mut cx, &out)?;
    exclusion_snapshots(&mut cx, &out)?;
    transfer_snapshots(&mut cx, &out)?;
    snapshot(&mut cx, &out, "showcase", |window, cx| {
        cx.new(|cx| Showcase::new(window, cx)).into()
    })?;
    Ok(())
}

/// What a Transactions snapshot does once the window has settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableStep {
    CategoryMenu,
    DateMenu,
    Sorted,
    NoMatch,
    Split,
    SplitShort,
    SplitOver,
    Details,
    SplitStart,
    TagMenu,
    EditFailed,
}

/// Which suggestion an Inbox snapshot opens.
type Pick = fn(&Entry) -> bool;

fn is_pattern(entry: &Entry, pattern: &str) -> bool {
    matches!(&entry.suggestion.payload, Payload::Rule(rule) if rule.pattern().as_str() == pattern)
}

/// The Inbox snapshots: each on a fresh scratch database.
fn inbox_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    let shots: [(&str, Option<Pick>, bool, bool); 9] = [
        (
            "inbox-rule",
            Some(|e| is_pattern(e, "UBER *EATS")),
            false,
            false,
        ),
        (
            "inbox-name",
            Some(|e| matches!(e.suggestion.payload, Payload::MerchantAlias(_))),
            false,
            false,
        ),
        (
            "inbox-categorisation",
            Some(|e| {
                matches!(&e.subject, orre_core::inbox::preview::Subject::Categorization(items)
                    if items.first().and_then(|i| i.transaction.as_ref()).is_some_and(|t| t.name() == "UBER *EATS"))
            }),
            false,
            false,
        ),
        (
            "inbox-transfer",
            Some(|e| matches!(e.suggestion.payload, Payload::TransferPair(_))),
            false,
            false,
        ),
        (
            "inbox-refund",
            Some(|e| matches!(e.suggestion.payload, Payload::RefundLink(_))),
            false,
            false,
        ),
        (
            "inbox-orre",
            Some(|e| e.suggestion.source_client == ORRE_SOURCE),
            false,
            false,
        ),
        ("inbox-wide", Some(|e| is_pattern(e, "PAY")), true, false),
        (
            "inbox-approved",
            Some(|e| is_pattern(e, "DELIVEROO")),
            false,
            true,
        ),
        ("inbox-empty", None, false, false),
    ];
    for (name, pick, wide, approve) in shots {
        let dir = tempfile::TempDir::new()?;
        let database = dir.path().join("orre.db");
        {
            let mut db = Database::open(&database)?;
            if name != "inbox-empty" {
                inbox_fixture::seed(&mut db);
            }
            if wide {
                inbox_fixture::seed_wide_rule(&mut db);
            }
        }
        snapshot(cx, out, name, move |window, cx| {
            let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
            let inbox = cx.new(|_| Inbox::from_store(InboxStore::new(database)));
            inbox.update(cx, |inbox, cx| {
                let id = pick.and_then(|pick| {
                    inbox
                        .data()
                        .entries
                        .iter()
                        .find(|entry| pick(entry))
                        .map(Entry::id)
                });
                if approve {
                    if let Some(id) = id {
                        inbox.approve(id, cx);
                    }
                } else {
                    inbox.select(id, cx);
                }
            });
            cx.new(|cx| ShellView::with_data(View::Inbox, accounts, inbox, window, cx))
                .into()
        })?;
        drop(dir);
    }
    Ok(())
}

/// What an Import snapshot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportShot {
    File,
    Map,
    MapMenu,
    MapOwn,
    NoAccounts,
    NoneImportable,
    SavePreset,
    Review,
    Mismatch,
    Corrections,
    Done,
}

/// A scratch database with Paper's accounts (Current account with Monzo's
/// preset, Joint account, Savings), and for the Review shots part of the
/// sample imported before, or balances and a lock date.
fn import_database(dir: &Path, shot: ImportShot) -> Result<PathBuf> {
    use chrono::NaiveDate;
    use orre_core::account::{self, AccountKind, EnteredBalance, EnteredSource, NewAccount};
    use orre_core::money::{Currency, Money};

    let database = dir.join("orre.db");
    let mut db = Database::open(&database)?;
    let mut current = NewAccount::new("Current account", AccountKind::Current, Currency::GBP);
    current.preset = Some("uk-monzo".to_owned());
    let current = account::create(&mut db, &current)?;
    account::create(
        &mut db,
        &NewAccount::new("Joint account", AccountKind::Current, Currency::GBP),
    )?;
    account::create(
        &mut db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )?;
    let day = |d| NaiveDate::from_ymd_opt(2026, if d > 20 { 10 } else { 11 }, d).expect("a date");
    match shot {
        ImportShot::Review | ImportShot::Corrections | ImportShot::Done => {
            // The file's last four rows (the oldest) came in an earlier export;
            // for the corrections shot, with the Odeon row at 12.80, so the
            // sample's 13.80 is the bank's correction of it.
            let lines: Vec<&str> = import_fixture::MONZO_NOVEMBER.lines().collect();
            let earlier = std::iter::once(lines[0].to_owned())
                .chain(lines[lines.len() - 4..].iter().map(|line| {
                    if shot == ImportShot::Corrections && line.contains("Odeon") {
                        line.replace("13.80", "12.80")
                    } else {
                        (*line).to_owned()
                    }
                }))
                .collect::<Vec<_>>()
                .join("\n");
            let presets = orre_import::preset::Presets::bundled()?;
            let statement = orre_import::AnyImporter::import(
                presets.get("uk-monzo").expect("bundled").importer(),
                &orre_import::SourceFile::from_bytes("monzo-early-november.csv", earlier),
            )?;
            let preview =
                orre_core::import::preview(&db, current, "monzo-early-november.csv", statement)?;
            let confirm = preview.needs_confirmation();
            orre_core::import::commit(&mut db, &preview, confirm)?;
        }
        ImportShot::Mismatch => {
            let balance = |date, minor| EnteredBalance {
                date,
                balance: Money::new(minor, Currency::GBP),
                source: EnteredSource::Statement,
                note: None,
            };
            account::add_balance(&mut db, current, &balance(day(31), 50_000))?;
            account::add_balance(&mut db, current, &balance(day(4), 50_000))?;
            account::add_balance(&mut db, current, &balance(day(9), 128_410))?;
            orre_core::reconciliation::lock(&mut db, current, day(4))?;
        }
        _ => {}
    }
    Ok(database)
}

/// The Import snapshots.
fn import_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    let shots = [
        ("import-file", ImportShot::File),
        ("import-map", ImportShot::Map),
        ("import-map-menu", ImportShot::MapMenu),
        ("import-map-own", ImportShot::MapOwn),
        ("import-map-no-accounts", ImportShot::NoAccounts),
        ("import-map-none-importable", ImportShot::NoneImportable),
        ("import-save-preset", ImportShot::SavePreset),
        ("import-review", ImportShot::Review),
        ("import-review-mismatch", ImportShot::Mismatch),
        ("import-review-corrections", ImportShot::Corrections),
        ("import-done", ImportShot::Done),
    ];
    for (name, shot) in shots {
        let dir = tempfile::TempDir::new()?;
        let database = import_database(dir.path(), shot)?;
        let with_store = matches!(
            shot,
            ImportShot::Review | ImportShot::Mismatch | ImportShot::Corrections | ImportShot::Done
        );
        let built: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::default();
        let keep = built.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let shell = cx.new(|cx| {
                let mut shell = ShellView::new(View::Transactions, window, cx);
                if with_store {
                    shell.import_into(ImportStore::new(database.clone()), cx);
                }
                shell
            });
            *keep.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        let then = move |window: &mut Window, cx: &mut App| {
            let shell = built.borrow().clone().expect("the shell was built");
            let dialog = shell.update(cx, |shell, cx| shell.open_sample_import(window, cx));
            dialog.update(cx, |dialog, cx| {
                match shot {
                    ImportShot::File => return,
                    ImportShot::NoAccounts => {
                        dialog.forget_accounts(cx);
                        dialog.open_file(signed_amount_fixture(), cx);
                        return;
                    }
                    ImportShot::NoneImportable => {
                        dialog.close_accounts(cx);
                        dialog.open_file(signed_amount_fixture(), cx);
                        return;
                    }
                    ImportShot::MapOwn | ImportShot::SavePreset => {
                        dialog.open_file(signed_amount_fixture(), cx)
                    }
                    _ => dialog.open_file(import_fixture::file(), cx),
                }
                dialog.set_account(orre_core::id::AccountId::new(1), cx);
                match shot {
                    ImportShot::MapMenu => dialog.open_merchant_menu(window, cx),
                    ImportShot::SavePreset => {
                        dialog.open_save_form(window, cx);
                        dialog.set_preset_name("Example Bank", window, cx);
                    }
                    ImportShot::Review
                    | ImportShot::Mismatch
                    | ImportShot::Corrections
                    | ImportShot::Done => dialog.start_review(cx),
                    _ => {}
                }
            });
            if shot == ImportShot::Done {
                let imports = shell.read(cx).imports().clone();
                imports.update(cx, |imports, cx| {
                    imports.set_notice(
                        Some(ImportNotice::Imported {
                            batch: orre_core::id::BatchId::new(2),
                            message: orre_app::import::text::imported(
                                9,
                                import_fixture::FILE_NAME,
                                &[],
                                0,
                            ),
                        }),
                        cx,
                    )
                });
                shell.update(cx, |shell, cx| shell.close_dialog(window, cx));
            }
        };
        snapshot_then(cx, out, name, build, then)?;
        drop(dir);
    }
    Ok(())
}

/// What a review-queue snapshot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReviewShot {
    Duplicates,
    Balances,
    Locked,
    Merged,
    Lock,
    Unlock,
    Empty,
}

/// The review-queue snapshots: each on a fresh scratch database.
fn review_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    let shots = [
        ("review-duplicates", ReviewShot::Duplicates),
        ("review-balances", ReviewShot::Balances),
        ("review-locked", ReviewShot::Locked),
        ("review-merged", ReviewShot::Merged),
        ("review-lock", ReviewShot::Lock),
        ("review-unlock", ReviewShot::Unlock),
        ("review-empty", ReviewShot::Empty),
    ];
    for (name, shot) in shots {
        let dir = tempfile::TempDir::new()?;
        let database = dir.path().join("orre.db");
        let seeded = {
            let mut db = Database::open(&database)?;
            (shot != ReviewShot::Empty).then(|| {
                let seeded = checks_fixture::seed(&mut db);
                if shot == ReviewShot::Locked {
                    checks_fixture::seed_locked_pair(&mut db, &seeded);
                }
                seeded
            })
        };
        let accounts_ids = seeded.as_ref().map(|s| (s.joint, s.savings));
        let checks_slot: Rc<RefCell<Option<Entity<Checks>>>> = Rc::new(RefCell::new(None));
        let slot = checks_slot.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
            let inbox = cx.new(|_| Inbox::empty());
            let checks = cx.new(|_| Checks::from_store(ChecksStore::new(database)));
            *slot.borrow_mut() = Some(checks.clone());
            cx.new(|cx| {
                let mut shell = ShellView::with_data(View::Inbox, accounts, inbox, window, cx);
                shell.install_checks(checks.clone(), window, cx);
                checks.update(cx, |checks, cx| {
                    let queue = match shot {
                        ReviewShot::Balances | ReviewShot::Lock | ReviewShot::Unlock => {
                            Queue::Balances
                        }
                        _ => Queue::Duplicates,
                    };
                    checks.show(queue, cx);
                    if shot == ReviewShot::Locked
                        && let Some((_, savings)) = accounts_ids
                        && let Some(pair) = checks
                            .pairs()
                            .iter()
                            .find(|pair| pair.account == savings)
                            .map(|pair| pair.candidate)
                    {
                        checks.select_pair(Some(pair), cx);
                    }
                    if shot == ReviewShot::Merged {
                        checks.merge(cx);
                    }
                });
                shell
            })
            .into()
        };
        let then = move |_: &mut Window, cx: &mut App| {
            let Some(checks) = checks_slot.borrow().clone() else {
                return;
            };
            match (shot, accounts_ids) {
                (ReviewShot::Lock, Some((_, savings))) => {
                    checks.update(cx, |checks, cx| checks.request_lock(savings, cx))
                }
                (ReviewShot::Unlock, Some((joint, _))) => {
                    checks.update(cx, |checks, cx| checks.request_unlock(joint, cx))
                }
                _ => {}
            }
        };
        snapshot_then(cx, out, name, build, then)?;
        drop(dir);
    }
    Ok(())
}

/// Which links snapshot to take (M3.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinksShot {
    Refund,
    Shared,
    Dialog,
    Linked,
    Purchase,
    Claim,
    Owed,
    OwedEmpty,
}

/// The links snapshots: each on a fresh scratch database seeded with D6.5's
/// fixture, Transactions and the review queues reading it.
fn links_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    use orre_app::accounts::store::AccountStore;
    use orre_app::transactions::link_fixture::{self, Variant};
    use orre_app::transactions::links::LinkFrom;
    use orre_app::transactions::{Source, store::TransactionStore};

    let shots = [
        ("links-refund", LinksShot::Refund),
        ("links-shared", LinksShot::Shared),
        ("links-dialog", LinksShot::Dialog),
        ("links-linked", LinksShot::Linked),
        ("links-purchase", LinksShot::Purchase),
        ("links-claim", LinksShot::Claim),
        ("review-owed", LinksShot::Owed),
        ("review-owed-empty", LinksShot::OwedEmpty),
    ];
    for (name, shot) in shots {
        let dir = tempfile::TempDir::new()?;
        let database = dir.path().join("orre.db");
        let seeded = {
            let mut db = Database::open(&database)?;
            let variant = if shot == LinksShot::Claim {
                Variant::PartlyRepaid
            } else {
                Variant::Suggested
            };
            (shot != LinksShot::OwedEmpty).then(|| link_fixture::seed(&mut db, variant))
        };
        let pick = seeded.as_ref().map(|seeded| match shot {
            LinksShot::Shared => seeded.odeon,
            LinksShot::Purchase => seeded.asos_purchase,
            LinksShot::Claim => seeded.trainline,
            _ => seeded.asos_refund,
        });
        let refund = seeded.as_ref().map(|seeded| seeded.asos_refund);
        let shell_slot: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::new(RefCell::new(None));
        let slot = shell_slot.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let accounts = cx.new(|_| Accounts::from_store(AccountStore::new(database.clone())));
            let source = Source::Database(TransactionStore::new(database.clone()));
            let owed = matches!(shot, LinksShot::Owed | LinksShot::OwedEmpty);
            let view = if owed {
                View::Inbox
            } else {
                View::Transactions
            };
            let shell = cx.new(|cx| {
                let mut shell = ShellView::with_sources(view, accounts, source, window, cx);
                let checks = cx.new(|_| Checks::from_store(ChecksStore::new(database)));
                shell.install_checks(checks.clone(), window, cx);
                if owed {
                    checks.update(cx, |checks, cx| checks.show(Queue::Owed, cx));
                }
                shell
            });
            *slot.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        // Once the register has been read, as a click would.
        let then = move |window: &mut Window, cx: &mut App| {
            let shell = shell_slot.borrow().clone().expect("the shell was built");
            let transactions = shell.read(cx).transactions().clone();
            // Owed to you opens on the person who owes most: Sam.
            if shot == LinksShot::Owed {
                return;
            }
            if matches!(shot, LinksShot::Linked | LinksShot::Purchase)
                && let Some(refund) = refund
            {
                transactions.update(cx, |view, cx| view.select([refund], cx));
                transactions.update(cx, |view, cx| view.confirm_all_suggestions(cx));
            }
            if let Some(pick) = pick {
                transactions.update(cx, |view, cx| view.select([pick], cx));
            }
            if shot == LinksShot::Dialog
                && let Some(refund) = refund
            {
                shell.update(cx, |shell, cx| {
                    shell.open_link_dialog(LinkFrom::Transaction(refund), window, cx);
                });
            }
        };
        snapshot_then(cx, out, name, build, then)?;
        drop(dir);
    }
    Ok(())
}

/// What a Rules snapshot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RulesShot {
    Rules,
    NeverReached,
    AddAction,
    PatternError,
    Apply,
    Names,
    History,
    Delete,
    Undo,
    Empty,
    Prompt,
    /// ⌥↑ on rule 3 (UBER TRIP).
    Reorder,
    /// ⌥↑ on rule 12 (JUSTGIVING) with Add tag picked.
    ReorderFiltered,
}

/// The Rules snapshots: each on a fresh scratch database.
fn rules_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    let shots = [
        ("rules", RulesShot::Rules),
        ("rules-never-reached", RulesShot::NeverReached),
        ("rules-add-action", RulesShot::AddAction),
        ("rules-pattern-error", RulesShot::PatternError),
        ("rules-apply", RulesShot::Apply),
        ("rules-names", RulesShot::Names),
        ("rules-history", RulesShot::History),
        ("rules-delete", RulesShot::Delete),
        ("rules-undo", RulesShot::Undo),
        ("rules-empty", RulesShot::Empty),
        ("transactions-rule-prompt", RulesShot::Prompt),
        ("rules-reorder", RulesShot::Reorder),
        ("rules-reorder-filtered", RulesShot::ReorderFiltered),
    ];
    for (name, shot) in shots {
        let dir = tempfile::TempDir::new()?;
        let database = dir.path().join("orre.db");
        let seeded = {
            let mut db = Database::open(&database)?;
            (shot != RulesShot::Empty).then(|| rules_fixture::seed(&mut db))
        };
        let rule_ids = seeded.as_ref().map(|s| s.rules.clone()).unwrap_or_default();
        let vinted = seeded.as_ref().map(|s| s.vinted);
        let database_path = database.clone();
        let shell_slot: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::new(RefCell::new(None));
        let slot = shell_slot.clone();
        let ids = rule_ids.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
            let inbox = cx.new(|_| Inbox::empty());
            let settings = cx.new(SettingsView::fixture);
            let view = if shot == RulesShot::Prompt {
                View::Transactions
            } else {
                View::Rules
            };
            let source = orre_app::transactions::Source::Database(
                orre_app::transactions::store::TransactionStore::new(database.clone()),
            );
            let rules = cx.new(|_| Rules::from_store(RulesStore::new(database)));
            let shell = cx.new(|cx| {
                let mut shell =
                    ShellView::with_all(view, accounts, source, inbox, settings, window, cx);
                shell.install_rules(rules.clone(), window, cx);
                rules.update(cx, |rules, cx| match shot {
                    RulesShot::Rules | RulesShot::AddAction | RulesShot::Delete => {
                        rules.select_rule(ids.get(1).copied(), cx)
                    }
                    RulesShot::NeverReached | RulesShot::Reorder => {
                        rules.select_rule(ids.get(2).copied(), cx)
                    }
                    RulesShot::ReorderFiltered => {
                        rules.toggle_action_filter(orre_core::rule::ActionKind::AddTag, cx);
                        rules.select_rule(ids.get(11).copied(), cx)
                    }
                    RulesShot::PatternError => rules.select_rule(ids.get(9).copied(), cx),
                    RulesShot::Names => rules.show(RulesSection::Names, cx),
                    RulesShot::History | RulesShot::Undo => rules.show(RulesSection::History, cx),
                    _ => {}
                });
                shell
            });
            *slot.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        let then = move |window: &mut Window, cx: &mut App| {
            let Some(shell) = shell_slot.borrow().clone() else {
                return;
            };
            let rules = shell.read(cx).rules().clone();
            let view = shell.read(cx).rules_view().clone();
            match shot {
                RulesShot::AddAction => view.update(cx, |view, cx| {
                    view.show_menu(RulesMenu::AddAction, window, cx)
                }),
                RulesShot::PatternError => view.update(cx, |view, cx| {
                    view.type_into(Input::Pattern, "^(ASOS|ZARA", window, cx)
                }),
                RulesShot::Apply => {
                    let proposal = vinted.and_then(|vinted| {
                        let db = orre_app::transactions::store::TransactionStore::new(
                            database_path.clone(),
                        )
                        .open()
                        .ok()?;
                        orre_core::rule::propose(&db, vinted).ok().flatten()
                    });
                    if let Some(proposal) = proposal {
                        rules.update(cx, |rules, cx| {
                            rules.new_rule_from(
                                &proposal.definition,
                                orre_core::rule::RuleOrigin::Edit,
                                cx,
                            )
                        });
                    }
                }
                RulesShot::Delete => rules.update(cx, |rules, cx| rules.request_delete_rule(cx)),
                RulesShot::Undo => rules.update(cx, |rules, cx| rules.request_undo(cx)),
                RulesShot::Reorder | RulesShot::ReorderFiltered => {
                    rules.update(cx, |rules, cx| rules.reorder_selected(false, cx))
                }
                RulesShot::Prompt => {
                    if let Some(vinted) = vinted {
                        let transactions = shell.read(cx).transactions().clone();
                        transactions.update(cx, |view, cx| {
                            view.select([vinted], cx);
                            view.propose_rule(vinted, cx);
                        });
                    }
                }
                _ => {}
            }
        };
        snapshot_then(cx, out, name, build, then)?;
        drop(dir);
    }
    Ok(())
}

/// What a Categories snapshot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CategoryShot {
    List,
    New,
    Delete,
}

/// The Categories snapshots (M3.7): each on a fresh scratch database seeded
/// with `rules::fixture::seed` and `seed_categories`.
fn rules_category_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    use orre_app::rules::view::Input as RulesInput;
    use orre_app::rules::{CategorySelection, Section as RulesSection};
    for (name, shot) in [
        ("rules-categories", CategoryShot::List),
        ("rules-category-new", CategoryShot::New),
        ("rules-category-delete", CategoryShot::Delete),
    ] {
        let dir = tempfile::TempDir::new()?;
        let database = dir.path().join("orre.db");
        let (groceries, coffee) = {
            let mut db = Database::open(&database)?;
            rules_fixture::seed(&mut db);
            let seeded = rules_fixture::seed_categories(&mut db);
            let groceries = db
                .categories()?
                .get(&orre_core::category::CategoryKey::new("food.groceries")?)
                .map(|category| category.id());
            (groceries, seeded.coffee)
        };
        let shell_slot: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::new(RefCell::new(None));
        let slot = shell_slot.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
            let inbox = cx.new(|_| Inbox::empty());
            let settings = cx.new(SettingsView::fixture);
            let source = orre_app::transactions::Source::Database(
                orre_app::transactions::store::TransactionStore::new(database.clone()),
            );
            let rules = cx.new(|_| Rules::from_store(RulesStore::new(database)));
            let shell = cx.new(|cx| {
                let mut shell =
                    ShellView::with_all(View::Rules, accounts, source, inbox, settings, window, cx);
                shell.install_rules(rules.clone(), window, cx);
                rules.update(cx, |rules, cx| {
                    rules.show(RulesSection::Categories, cx);
                    let selected = match shot {
                        CategoryShot::List => groceries.map(CategorySelection::Stored),
                        CategoryShot::New => Some(CategorySelection::New),
                        CategoryShot::Delete => Some(CategorySelection::Stored(coffee)),
                    };
                    rules.select_category(selected, cx);
                });
                shell
            });
            *slot.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        let then = move |window: &mut Window, cx: &mut App| {
            let Some(shell) = shell_slot.borrow().clone() else {
                return;
            };
            let rules = shell.read(cx).rules().clone();
            let view = shell.read(cx).rules_view().clone();
            match shot {
                CategoryShot::List => {}
                CategoryShot::New => {
                    view.update(cx, |view, cx| {
                        view.type_into(RulesInput::CategoryName, "Coffee beans", window, cx)
                    });
                    rules.update(cx, |rules, cx| {
                        rules.edit_category(
                            false,
                            |form| {
                                form.group = orre_core::category::CategoryKey::new("food").ok();
                            },
                            cx,
                        )
                    });
                }
                CategoryShot::Delete => {
                    view.update(cx, |view, cx| view.request_delete_category(window, cx))
                }
            }
        };
        snapshot_then(cx, out, name, build, then)?;
        drop(dir);
    }
    Ok(())
}

/// `fixtures/csv/signed-amount.csv`: a layout no preset has.
fn signed_amount_fixture() -> orre_import::SourceFile {
    orre_import::SourceFile::from_bytes(
        "example-bank-november.csv",
        include_bytes!("../../../fixtures/csv/signed-amount.csv").to_vec(),
    )
}

/// Which Settings dialog a snapshot opens over the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsDialog {
    Enable,
    Export,
}

/// D6.9's screens: an asset and a liability selected, and the dialogs.
fn valued_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    use accounts_fixture::{CURRENT, HOUSE, MORTGAGE};
    for (name, step) in [
        ("accounts-asset", ValuedStep::Select(HOUSE)),
        ("accounts-liability", ValuedStep::Select(MORTGAGE)),
        ("new-account-asset", ValuedStep::NewAccount),
        ("add-valuation", ValuedStep::AddValue(HOUSE)),
        ("add-balance-liability", ValuedStep::AddValue(MORTGAGE)),
        ("close-account", ValuedStep::Close(HOUSE)),
        ("close-account-bank", ValuedStep::Close(CURRENT)),
        ("delete-account", ValuedStep::Delete(HOUSE)),
        ("account-closed", ValuedStep::Closed),
    ] {
        snapshot(cx, out, name, move |window, cx| {
            let mut data = accounts_fixture::valued();
            if step == ValuedStep::Closed {
                // House has gone from the panel and net worth.
                data.tree.accounts.retain(|node| node.account.id != HOUSE);
                data.tree
                    .net_worth
                    .add(orre_core::money::Money::new(
                        -38_500_000,
                        orre_core::money::Currency::GBP,
                    ))
                    .expect("the fixture's sums fit");
            }
            let mut fixture = Accounts::fixture(data);
            if step == ValuedStep::Closed {
                let today = accounts_fixture::today();
                fixture = fixture.with_closed(orre_app::accounts::Closed::new(
                    HOUSE, "House", today, today,
                ));
            }
            let accounts = cx.new(|_| fixture);
            let shell = cx.new(|cx| {
                ShellView::with_accounts(View::Transactions, accounts.clone(), window, cx)
            });
            accounts.update(cx, |accounts, cx| match step {
                ValuedStep::Select(id) => accounts.select(Some(id), cx),
                ValuedStep::NewAccount => accounts.request_new_valued_account(cx),
                ValuedStep::AddValue(id) => {
                    accounts.select(Some(id), cx);
                    accounts.request_add_balance(id, cx);
                }
                ValuedStep::Close(id) => {
                    accounts.select(Some(id), cx);
                    accounts.request_close(id, cx);
                }
                ValuedStep::Delete(id) => {
                    accounts.select(Some(id), cx);
                    accounts.request_delete(id, cx);
                }
                ValuedStep::Closed => {}
            });
            shell.into()
        })?;
    }
    Ok(())
}

/// The cash account's screens (M3.10): its inspector and the Count cash
/// dialog, short and for the first count.
fn cash_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    use accounts_fixture::CASH;
    for (name, counted, first) in [
        ("accounts-cash", None, false),
        ("cash-count", Some("18.00"), false),
        ("cash-count-first", Some("23.00"), true),
    ] {
        let built: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::default();
        let keep = built.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let mut data = accounts_fixture::valued();
            if first {
                let detail = data.details.get_mut(&CASH).expect("Cash");
                detail.balances.clear();
                detail.cash_expected = None;
            }
            let accounts = cx.new(|_| Accounts::fixture(data));
            let shell = cx.new(|cx| {
                ShellView::with_accounts(View::Transactions, accounts.clone(), window, cx)
            });
            accounts.update(cx, |accounts, cx| {
                accounts.select(Some(CASH), cx);
                if counted.is_some() {
                    accounts.request_add_balance(CASH, cx);
                }
            });
            *keep.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        // The count is typed once the dialog is open, as a person would.
        let then = move |window: &mut Window, cx: &mut App| {
            let Some(counted) = counted else {
                return;
            };
            let shell = built.borrow().clone().expect("the shell was built");
            let dialog = shell.read(cx).dialog().expect("the dialog is open").clone();
            let dialog = dialog
                .downcast::<orre_app::accounts::add_balance::AddBalanceDialog>()
                .expect("it is the count dialog");
            dialog.update(cx, |dialog, cx| dialog.type_amount(counted, window, cx));
        };
        snapshot_then(cx, out, name, build, then)?;
    }
    Ok(())
}

/// What an M3.10 snapshot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExclusionShot {
    Excluded,
    ExcludeDialog,
    Manual,
    NewTransaction,
    UndoImport,
}

/// M3.10's snapshots, on the fixture: `transactions-excluded` (Paper
/// `3FE-0`: the Excluded chip on, the Odeon row excluded and selected),
/// `transactions-exclude-dialog` (`51L-0`, Bank error picked),
/// `transactions-manual` (`4D7-0`: Cash selected, a manual row in the
/// inspector), `transactions-new-transaction` (`529-0`) and
/// `transactions-undo-import` (the undo-import confirmation, composed).
fn exclusion_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    use orre_app::transactions::confirm::{UndoImportDialog, undo_lines};
    use orre_core::exclusion::ExclusionReason;

    for (name, shot) in [
        ("transactions-excluded", ExclusionShot::Excluded),
        ("transactions-exclude-dialog", ExclusionShot::ExcludeDialog),
        ("transactions-manual", ExclusionShot::Manual),
        (
            "transactions-new-transaction",
            ExclusionShot::NewTransaction,
        ),
        ("transactions-undo-import", ExclusionShot::UndoImport),
    ] {
        let built: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::default();
        let keep = built.clone();
        let manual = matches!(shot, ExclusionShot::Manual | ExclusionShot::NewTransaction);
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let data = if manual {
                accounts_fixture::valued()
            } else {
                accounts_fixture::accounts()
            };
            let accounts = cx.new(|_| Accounts::fixture(data));
            let shell = cx.new(|cx| {
                ShellView::with_accounts(View::Transactions, accounts.clone(), window, cx)
            });
            if manual {
                // Cash selected first: selecting an account clears the
                // transaction selection, which `then` makes.
                let view = shell.read(cx).transactions().clone();
                view.update(cx, |view, cx| {
                    view.show_fixture(fixture::manual_register(), cx)
                });
                accounts.update(cx, |accounts, cx| accounts.select(Some(fixture::CASH), cx));
            }
            *keep.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        let then = move |window: &mut Window, cx: &mut App| {
            let shell = built.borrow().clone().expect("the shell was built");
            let view = shell.read(cx).transactions().clone();
            let odeon = fixture::id("txn-odeon-07");
            match shot {
                ExclusionShot::Excluded => view.update(cx, |view, cx| {
                    view.show_fixture(fixture::excluded_register(), cx);
                    view.set_excluded(true, cx);
                    view.select([odeon], cx);
                }),
                ExclusionShot::ExcludeDialog => {
                    view.update(cx, |view, cx| view.select([odeon], cx));
                    let dialog =
                        shell.update(cx, |shell, cx| shell.open_exclude_dialog(odeon, window, cx));
                    dialog.update(cx, |dialog, cx| {
                        dialog.choose_reason(ExclusionReason::BankError, window, cx);
                        dialog.set_details(
                            "Charged twice for one booking; the refund is on its way.",
                            window,
                            cx,
                        );
                    });
                }
                ExclusionShot::Manual | ExclusionShot::NewTransaction => {
                    view.update(cx, |view, cx| view.select([fixture::manual_id(2)], cx));
                    if shot == ExclusionShot::NewTransaction {
                        let dialog = shell
                            .update(cx, |shell, cx| shell.open_new_transaction(window, cx))
                            .expect("Transactions is open");
                        dialog.update(cx, |dialog, cx| {
                            dialog.fill("12.80", "Farmers' market", "", window, cx)
                        });
                    }
                }
                ExclusionShot::UndoImport => {
                    view.update(cx, |view, cx| view.select([odeon], cx));
                    let preview = orre_core::import::UndoPreview {
                        batch: orre_core::import::BatchInfo {
                            id: orre_core::id::BatchId::new(2),
                            account: accounts_fixture::CURRENT,
                            source_file: "monzo-november.csv".to_owned(),
                            format: orre_core::statement::SourceFormat::Csv,
                            preset_id: Some("uk-monzo".to_owned()),
                            imported_at: "2026-11-09T08:12:00.000Z".to_owned(),
                            status: orre_core::import::BatchStatus::Committed,
                            transactions: 296,
                        },
                        transactions: 296,
                        mirrored: 4,
                        balances: 1,
                        flags: 0,
                        edited: 3,
                        transfers: 0,
                        merged_into: 0,
                        later_batches: Vec::new(),
                        restored: Vec::new(),
                        superseded: 0,
                    };
                    let lines = undo_lines(&preview, "Current account");
                    let dialog = cx.new(|cx| {
                        let focus = cx.focus_handle();
                        UndoImportDialog::preview(view.clone(), &preview, lines, focus)
                    });
                    shell.update(cx, |shell, cx| shell.show_dialog(dialog, window, cx));
                }
            }
        };
        snapshot_then(cx, out, name, build, then)?;
    }
    Ok(())
}

/// M3.8's Transfer section, on the fixture with a £250.00 move from
/// Current account to Savings: `transactions-transfer` (the pair
/// suggested, Savings' side selected) and `transactions-transfer-confirmed`
/// (confirmed, Current account's side selected). Composed, not drawn.
fn transfer_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    use orre_core::transfer::TransferStatus;

    for (name, status) in [
        ("transactions-transfer", TransferStatus::Suggested),
        ("transactions-transfer-confirmed", TransferStatus::Confirmed),
    ] {
        let built: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::default();
        let keep = built.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
            let shell = cx.new(|cx| {
                ShellView::with_accounts(View::Transactions, accounts.clone(), window, cx)
            });
            *keep.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        let then = move |_: &mut Window, cx: &mut App| {
            let shell = built.borrow().clone().expect("the shell was built");
            let view = shell.read(cx).transactions().clone();
            let (money_out, money_in) = fixture::transfer_ids();
            let selected = if status == TransferStatus::Suggested {
                money_in
            } else {
                money_out
            };
            view.update(cx, |view, cx| {
                view.show_fixture(fixture::transfer_register(status), cx);
                view.select([selected], cx);
            });
        };
        snapshot_then(cx, out, name, build, then)?;
    }
    Ok(())
}

/// What a D6.9 snapshot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValuedStep {
    Select(orre_core::id::AccountId),
    NewAccount,
    AddValue(orre_core::id::AccountId),
    Close(orre_core::id::AccountId),
    Delete(orre_core::id::AccountId),
    Closed,
}

/// What the account snapshots do after selecting Current account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccountStep {
    Select,
    KindMenu,
    NewAccount,
    AddBalance,
    DayTaken,
    ContextMenu,
    EditBalance,
}

/// Opens `build`'s view in a 1440×900 window and writes it as
/// `<name>@1x.png` and `<name>@2x.png`.
fn snapshot(
    cx: &mut HeadlessAppContext,
    out: &Path,
    name: &str,
    build: impl FnOnce(&mut Window, &mut App) -> AnyView,
) -> Result<()> {
    snapshot_then(cx, out, name, build, |_, _| {})
}

/// [`snapshot`], running `then` once the window has settled, before the
/// capture: for a state reached by a click (an open menu).
fn snapshot_then(
    cx: &mut HeadlessAppContext,
    out: &Path,
    name: &str,
    build: impl FnOnce(&mut Window, &mut App) -> AnyView,
    then: impl FnOnce(&mut Window, &mut App),
) -> Result<()> {
    let window = cx.open_window(
        size(gpui_kit::px(WIDTH), gpui_kit::px(HEIGHT)),
        |window, cx| {
            let view = build(window, cx);
            cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
        },
    )?;
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| then(window, cx))?;
    cx.run_until_parked();

    for scale in [1.0_f32, 2.0] {
        // The headless window's backing is always 2x. A 1x scene fills its
        // top-left quarter, so crop to the logical size times the scale.
        cx.update_window(window.into(), |_, window, _| {
            window.set_scale_factor(scale);
        })?;
        cx.run_until_parked();
        let full = cx
            .capture_screenshot(window.into())
            .context("capture the window")?;
        let (w, h) = ((WIDTH * scale) as u32, (HEIGHT * scale) as u32);
        let image = image::imageops::crop_imm(&full, 0, 0, w, h).to_image();
        let path = out.join(format!("{name}@{scale}x.png"));
        image.save(&path)?;
        println!("{} {}x{}", path.display(), image.width(), image.height());
    }
    Ok(())
}
