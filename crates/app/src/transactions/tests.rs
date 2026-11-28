//! UI integration tests for the Transactions view: selection and keys on the
//! Paper fixture, and filters, sorting and category edits against a scratch
//! database through [`TransactionStore`], as `orre` runs them.

use chrono::NaiveDate;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Modifiers, SharedString, TestAppContext, VisualTestContext};
use orre_core::account::{self, AccountKind, NewAccount};
use orre_core::db::Database;
use orre_core::id::{AccountId, TransactionId};
use orre_core::manual::{self, ManualTransaction};
use orre_core::money::{Currency, Money};
use orre_core::reporting::register::RowCategory;
use tempfile::TempDir;

use super::describe::{BarDescription, TransactionsDescription};
use super::editor::EditPlace;
use super::*;

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        init(cx);
    });
}

/// The view over the Paper fixture, in a window.
pub(super) fn fixture_view(cx: &mut TestAppContext) -> Entity<TransactionsView> {
    setup(cx);
    let (view, _) =
        cx.add_window_view(|window, cx| TransactionsView::new(Source::Fixture, window, cx));
    view
}

fn open_fixture(cx: &mut TestAppContext) -> (Entity<TransactionsView>, &mut VisualTestContext) {
    setup(cx);
    let (view, cx) =
        cx.add_window_view(|window, cx| TransactionsView::new(Source::Fixture, window, cx));
    redraw(cx);
    (view, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn click(id: impl Into<SharedString>, cx: &mut VisualTestContext) {
    let id = id.into();
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

fn shows(id: impl Into<SharedString>, cx: &mut VisualTestContext) -> bool {
    let id = id.into();
    cx.update(move |window, _| window.try_find(id).is_some())
}

fn type_text(text: &str, cx: &mut VisualTestContext) {
    let text = text.to_owned();
    cx.update(move |window, cx| window.input(&text, cx));
    redraw(cx);
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    redraw(cx);
}

fn row_id(id: TransactionId) -> String {
    format!("txn-{id}")
}

fn describe(
    view: &Entity<TransactionsView>,
    cx: &mut VisualTestContext,
) -> TransactionsDescription {
    cx.update(|_, cx| view.read(cx).describe(cx))
}

/// The selected rows, as described.
fn selected(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Vec<TransactionId> {
    describe(view, cx).selected()
}

/// The rows the table lists, as described.
fn listed(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Vec<TransactionId> {
    describe(view, cx).listed()
}

/// The menu that's open, as described.
fn open_menu(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Option<Menu> {
    describe(view, cx).menu.map(|menu| menu.menu)
}

/// Whether the bulk-action bar shows, as described.
fn bulk_bar(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> bool {
    matches!(describe(view, cx).bar, Some(BarDescription::Bulk { .. }))
}

// The fixture ----------------------------------------------------------------

#[gpui_kit::test]
fn clicks_replace_extend_and_toggle(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    let id = fixture::id;
    click(row_id(id("txn-pret-09")), cx);
    assert_eq!(selected(&view, cx), [id("txn-pret-09")]);

    cx.update(|window, cx| {
        window.click_with_modifiers(row_id(id("txn-tfl-09")), Modifiers::shift(), cx)
    });
    redraw(cx);
    assert_eq!(selected(&view, cx).len(), 5, "⇧-click selects the range");

    cx.update(|window, cx| {
        let toggle = Modifiers::secondary_key(cx);
        window.click_with_modifiers(row_id(id("txn-rainy-day-09")), toggle, cx)
    });
    redraw(cx);
    assert_eq!(
        selected(&view, cx),
        [
            id("txn-pret-09"),
            id("txn-bluebottle-09"),
            id("txn-amazon-09"),
            id("txn-tfl-09")
        ],
        "⌘-click toggles one row"
    );
}

#[gpui_kit::test]
fn keys_move_and_extend_the_selection(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    let id = fixture::id;
    // A click focuses the table, which the keys need.
    click(row_id(id("txn-amazon-09")), cx);
    press("down", cx);
    assert_eq!(selected(&view, cx), [id("txn-tfl-09")]);

    press("shift-up shift-up", cx);
    assert_eq!(
        selected(&view, cx),
        [
            id("txn-rainy-day-09"),
            id("txn-amazon-09"),
            id("txn-tfl-09")
        ]
    );
    assert!(bulk_bar(&view, cx), "several rows show the bulk-action bar");
    assert!(shows("bulk-clear", cx));

    press("escape", cx);
    assert!(selected(&view, cx).is_empty());
    assert!(!bulk_bar(&view, cx));

    press("end", cx);
    let last = fixture::TRANSACTIONS.last().expect("fixture rows").id;
    assert_eq!(selected(&view, cx), [id(last)]);
}

#[gpui_kit::test]
fn search_narrows_the_rows_and_escape_clears_it(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    click("search", cx);
    type_text("pret", cx);
    let rows = listed(&view, cx);
    assert_eq!(rows.len(), 3, "every Pret A Manger row");
    assert!(cx.update(|_, cx| {
        let register = view.read(cx).register();
        rows.iter().all(|id| {
            register
                .row(*id)
                .unwrap()
                .description
                .to_lowercase()
                .contains("pret")
        })
    }));

    // Nothing matches: the empty state offers to clear the filters.
    type_text("zzz", cx);
    let description = describe(&view, cx);
    assert!(description.listed().is_empty());
    assert_eq!(
        description.empty.map(|empty| (empty.title, empty.shortcut)),
        Some(("No transactions match", Some(("Esc", "Clear filters"))))
    );

    // The filter hid the selected row, so nothing is selected: Escape
    // clears the filters.
    assert!(selected(&view, cx).is_empty());
    press("escape", cx);
    assert_eq!(listed(&view, cx).len(), fixture::TRANSACTIONS.len());
    assert_eq!(describe(&view, cx).toolbar.search, "");
}

#[gpui_kit::test]
fn uncategorised_toggles_and_a_column_sort_lists_one_flat_order(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    click("filter-uncategorised", cx);
    let rows = listed(&view, cx);
    assert!(!rows.is_empty());
    assert!(cx.update(|_, cx| {
        let view = view.read(cx);
        rows.iter()
            .all(|id| view.register().row(*id).unwrap().is_uncategorised())
    }));
    click("filter-uncategorised", cx);
    assert_eq!(listed(&view, cx).len(), fixture::TRANSACTIONS.len());

    // Amount, smallest (most money out) first, as one list over every day.
    let amounts = |view: &Entity<TransactionsView>, cx: &mut VisualTestContext| {
        let ids = listed(view, cx);
        cx.update(|_, cx| {
            let register = view.read(cx).register();
            ids.iter()
                .map(|id| register.row(*id).unwrap().amount.minor())
                .collect::<Vec<_>>()
        })
    };
    let grouped = describe(&view, cx);
    assert!(!grouped.days.is_empty() && grouped.flat.is_empty());
    click("sort-amount", cx);
    let ascending = amounts(&view, cx);
    assert!(ascending.is_sorted(), "{ascending:?}");
    let flat = describe(&view, cx);
    assert!(flat.days.is_empty(), "no day headers while sorted");
    assert_eq!(flat.flat.len(), fixture::TRANSACTIONS.len());
    click("sort-amount", cx);
    let descending = amounts(&view, cx);
    assert!(descending.is_sorted_by(|a, b| a >= b), "{descending:?}");
    click("sort-amount", cx);
    let back = describe(&view, cx);
    assert!(back.flat.is_empty(), "back to the days");
    assert_eq!(
        listed(&view, cx)[0],
        fixture::id("txn-pret-09"),
        "back to newest first"
    );
}

#[gpui_kit::test]
fn a_flat_list_has_a_date_column_whose_title_goes_back_to_days(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    assert!(!shows("sort-date", cx), "days carry the date");
    click("sort-merchant", cx);
    assert!(shows("sort-date", cx), "the flat list's first column");
    assert!(describe(&view, cx).days.is_empty());

    click("sort-date", cx);
    let back = describe(&view, cx);
    assert_eq!(back.sort, orre_core::reporting::register::Sort::Default);
    assert!(back.flat.is_empty() && !back.days.is_empty());
    assert!(!shows("sort-date", cx));
}

#[gpui_kit::test]
fn the_date_chip_filters_by_period(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    click("filter-date", cx);
    assert_eq!(open_menu(&view, cx), Some(Menu::Date));
    click("option-last_month", cx);
    assert!(listed(&view, cx).is_empty(), "the fixture is all November");
    assert_eq!(open_menu(&view, cx), None);

    click("filter-date", cx);
    press("up", cx);
    press("enter", cx);
    assert_eq!(
        listed(&view, cx).len(),
        fixture::TRANSACTIONS.len(),
        "All time"
    );
}

#[gpui_kit::test]
fn the_fixture_cant_be_changed(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    click(row_id(fixture::id("txn-bluebottle-09")), cx);
    press("enter", cx);
    assert_eq!(
        open_menu(&view, cx),
        Some(Menu::Row(fixture::id("txn-bluebottle-09")))
    );
    // Nothing is highlighted until the user moves or types.
    press("down", cx);
    press("enter", cx);
    let description = describe(&view, cx);
    assert_eq!(
        description.one().unwrap().category_error.as_deref(),
        Some(super::write::SAMPLE_DATA)
    );
    assert_eq!(description.menu, None);
}

// The database ---------------------------------------------------------------

/// A scratch database: Current and Joint account, four rows on 9 Nov 2026.
struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    current: AccountId,
    rows: Vec<TransactionId>,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        let mut db = Database::open(&path).unwrap();
        let mut create = |name: &str| {
            account::create(
                &mut db,
                &NewAccount::new(name, AccountKind::Current, Currency::GBP),
            )
            .unwrap()
        };
        let current = create("Current account");
        let joint = create("Joint account");
        let day = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let rows = [
            (current, -420, "SQ *BLUE BOTTLE COFFEE"),
            (current, -1899, "STEAMGAMES"),
            (joint, -1675, "VINTED *KX8F2Q"),
            (joint, -2399, "AMAZON MARKETPLACE"),
        ]
        .into_iter()
        .map(|(account, minor, text)| {
            manual::create(
                &mut db,
                &ManualTransaction::new(account, day, Money::new(minor, Currency::GBP), text),
            )
            .unwrap()
        })
        .collect();
        Self {
            _dir: dir,
            path,
            current,
            rows,
        }
    }

    /// The stored category key of `id`, read afresh.
    fn category(&self, id: TransactionId) -> Option<String> {
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let db = TransactionStore::new(self.path.clone()).open().unwrap();
        let register = Register::load(&db.reader().unwrap(), today).unwrap();
        match register.row(id).unwrap().category {
            RowCategory::Category(category) => {
                Some(register.category(category).unwrap().key.to_string())
            }
            _ => None,
        }
    }
}

fn open_database<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<TransactionsView>, &'a mut VisualTestContext) {
    setup(cx);
    let store = TransactionStore::new(scratch.path.clone());
    let (view, cx) = cx.add_window_view(move |window, cx| {
        TransactionsView::new(Source::Database(store), window, cx)
    });
    redraw(cx);
    (view, cx)
}

#[gpui_kit::test]
fn the_table_reads_the_database(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let mut newest_first = scratch.rows.clone();
    newest_first.reverse();
    assert_eq!(listed(&view, cx), newest_first);
    assert!(shows(row_id(scratch.rows[0]), cx));
    let uncategorised = &describe(&view, cx).toolbar.chips[3];
    assert_eq!(
        (uncategorised.label.as_str(), uncategorised.count.as_deref()),
        ("Uncategorised", Some("4"))
    );
}

#[gpui_kit::test]
fn a_category_set_in_place_is_saved_through_core(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let coffee = scratch.rows[0];
    click(row_id(coffee), cx);
    press("enter", cx);
    assert_eq!(open_menu(&view, cx), Some(Menu::Row(coffee)));

    // The filter field narrows the 67 categories; Enter takes the first.
    type_text("eating", cx);
    let menu = describe(&view, cx).menu.unwrap();
    assert_eq!(menu.filter.as_deref(), Some("eating"));
    let first = &menu.groups[0].options[0];
    assert_eq!(
        (first.label.as_str(), first.highlighted),
        ("Eating out", true)
    );
    press("enter", cx);
    assert_eq!(open_menu(&view, cx), None);
    assert_eq!(scratch.category(coffee).as_deref(), Some("food.eating_out"));

    // The register is read again, and the row keeps its selection.
    let description = describe(&view, cx);
    let row = description.row(coffee).unwrap();
    assert_eq!(row.category, "Eating out");
    assert!(row.selected);
    assert_eq!(description.selected(), [coffee]);
    assert_eq!(description.bar, None, "one row: no undo bar");
}

#[gpui_kit::test]
fn a_bulk_categorise_can_be_undone(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let ids = scratch.rows[..3].to_vec();
    cx.update(|_, cx| view.update(cx, |view, cx| view.select(ids.clone(), cx)));
    redraw(cx);

    click("bulk-categorise", cx);
    assert_eq!(open_menu(&view, cx), Some(Menu::Bulk));
    type_text("clothing", cx);
    press("enter", cx);
    for id in &ids {
        assert_eq!(scratch.category(*id).as_deref(), Some("shopping.clothing"));
    }
    assert_eq!(
        describe(&view, cx).undo_message(),
        Some("Categorised 3 transactions as Clothing")
    );
    assert!(shows("undo-change", cx));

    click("undo-change", cx);
    for id in &ids {
        assert_eq!(scratch.category(*id), None);
    }
    assert!(!shows("undo-change", cx));
    assert!(
        bulk_bar(&view, cx),
        "the three are still selected: the bulk-action bar is back"
    );
}

#[gpui_kit::test]
fn the_category_chip_filters_by_a_group(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let coffee = scratch.rows[0];
    {
        let mut db = Database::open(&scratch.path).unwrap();
        orre_core::inbox::own::recategorize(
            &mut db,
            &[(
                coffee,
                orre_core::category::CategoryKey::new("food.eating_out").unwrap(),
            )],
        )
        .unwrap();
    }
    cx.update(|_, cx| view.update(cx, |view, cx| view.reload(cx)));
    redraw(cx);

    click("filter-category", cx);
    type_text("food", cx);
    // "Food" heads its group: the first option.
    press("enter", cx);
    assert_eq!(
        listed(&view, cx),
        [coffee],
        "a group includes its subcategories"
    );
}

#[gpui_kit::test]
fn the_panels_account_filters_the_table(cx: &mut TestAppContext) {
    use crate::accounts::Accounts;
    use crate::accounts::store::AccountStore;
    use crate::shell::{ShellView, View};

    let scratch = Scratch::new();
    setup(cx);
    cx.update(crate::shell::init);
    let (accounts_store, store) = (
        AccountStore::new(scratch.path.clone()),
        TransactionStore::new(scratch.path.clone()),
    );
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let accounts = cx.new(|_| Accounts::from_store(accounts_store));
        ShellView::with_sources(
            View::Transactions,
            accounts,
            Source::Database(store),
            window,
            cx,
        )
    });
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).transactions().clone());
    assert_eq!(listed(&view, cx).len(), 4);

    click(format!("account-{}", scratch.current), cx);
    let mut current: Vec<_> = scratch.rows[..2].to_vec();
    current.reverse();
    assert_eq!(listed(&view, cx), current);
    assert!(
        !shows("sort-account", cx),
        "the Account column hides on one account"
    );
    let description = describe(&view, cx);
    assert!(!description.account_column);
    assert_eq!(description.toolbar.chips[0].label, "Current account");
    assert!(description.rows().all(|row| row.account.is_none()));

    // The Account chip mirrors the panel: All accounts clears it.
    click("filter-account", cx);
    click("option-all", cx);
    assert_eq!(listed(&view, cx).len(), 4);
    let accounts = cx.update(|_, cx| shell.read(cx).accounts().clone());
    assert_eq!(cx.update(|_, cx| accounts.read(cx).selected()), None);
}

// The inspector's edits (M3.9) ----------------------------------------------

/// The split lines stored for `id`: (amount, category key).
fn stored_split(scratch: &Scratch, id: TransactionId) -> Vec<(i64, Option<String>)> {
    let db = Database::open(&scratch.path).unwrap();
    orre_core::split::lines(&db, id)
        .unwrap()
        .into_iter()
        .map(|line| {
            (
                line.amount.minor(),
                line.category.map(|key| key.to_string()),
            )
        })
        .collect()
}

/// Types `text` into split line `index`'s amount field, replacing it.
fn set_line_amount(
    view: &Entity<TransactionsView>,
    index: usize,
    text: &str,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        let input = view.read(cx).editor.split.as_ref().unwrap().lines[index]
            .amount
            .clone();
        input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
    });
    redraw(cx);
    type_text(text, cx);
}

fn line_key(view: &Entity<TransactionsView>, index: usize, cx: &mut VisualTestContext) -> u64 {
    cx.update(|_, cx| view.read(cx).editor.split.as_ref().unwrap().lines[index].key)
}

fn draft_status(
    view: &Entity<TransactionsView>,
    cx: &mut VisualTestContext,
) -> Option<orre_core::split::draft::DraftStatus> {
    cx.update(|_, cx| {
        view.read(cx)
            .editor
            .split
            .as_ref()
            .map(|split| split.draft.status())
    })
}

#[gpui_kit::test]
fn a_two_way_split_is_one_edit_and_add_line(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let amazon = scratch.rows[3];
    click(row_id(amazon), cx);
    assert!(shows("split", cx), "an unsplit row offers Split");
    let one = describe(&view, cx).one().cloned().unwrap();
    assert!(one.sections.contains(&"Split"));
    assert_eq!(one.split, None);

    click("split", cx);
    assert_eq!(
        draft_status(&view, cx),
        Some(orre_core::split::draft::DraftStatus::OneLine)
    );
    // The sign is never typed: 20.00 is −£20.00 on a payment.
    set_line_amount(&view, 0, "20.00", cx);
    assert!(stored_split(&scratch, amazon).is_empty(), "not saved yet");

    click("split-add-line", cx);
    assert_eq!(
        stored_split(&scratch, amazon),
        [(-2000, None), (-399, None)],
        "the new line took the remainder and the split saved"
    );
    // The table's row is split now, and the editor kept its fields.
    let description = describe(&view, cx);
    assert_eq!(
        description.row(amazon).unwrap().category,
        "Uncategorised, Uncategorised"
    );
    let split = description.one().unwrap().split.clone().unwrap();
    assert_eq!(
        split.lines,
        [
            // As typed; the line "Add line" filled in, formatted.
            ("Category".to_owned(), "20.00".to_owned()),
            ("Category".to_owned(), "−£3.99".to_owned()),
        ]
    );
    assert_eq!(split.remaining, "£0.00");

    // A line's category is chosen from the category menu.
    let key = line_key(&view, 0, cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.show_menu(Menu::SplitLine(key), window, cx)
        })
    });
    redraw(cx);
    type_text("clothing", cx);
    press("enter", cx);
    assert_eq!(
        stored_split(&scratch, amazon),
        [(-2000, Some("shopping.clothing".into())), (-399, None)]
    );
}

#[gpui_kit::test]
fn an_unbalanced_split_isnt_saved_and_leaving_the_row_drops_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let amazon = scratch.rows[3];
    {
        let mut db = Database::open(&scratch.path).unwrap();
        let gbp = |minor| Money::new(minor, Currency::GBP);
        orre_core::edit::set_split(
            &mut db,
            amazon,
            &[
                orre_core::split::SplitLine::new(gbp(-2000), None),
                orre_core::split::SplitLine::new(gbp(-399), None),
            ],
        )
        .unwrap();
    }
    let (view, cx) = open_database(cx, &scratch);
    click(row_id(amazon), cx);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).editor.split.as_ref().unwrap().lines.len()),
        2
    );

    set_line_amount(&view, 1, "5.00", cx);
    assert!(matches!(
        draft_status(&view, cx),
        Some(orre_core::split::draft::DraftStatus::Unbalanced { over: true, .. })
    ));
    assert_eq!(
        stored_split(&scratch, amazon),
        [(-2000, None), (-399, None)]
    );

    // Away and back: the stored split is shown again.
    click(row_id(scratch.rows[0]), cx);
    click(row_id(amazon), cx);
    assert_eq!(
        draft_status(&view, cx),
        Some(orre_core::split::draft::DraftStatus::Balanced)
    );
    let amount = cx.update(|_, cx| {
        view.read(cx).editor.split.as_ref().unwrap().lines[1]
            .amount
            .read(cx)
            .value()
    });
    assert_eq!(amount.as_ref(), "−£3.99");

    // Remove split leaves the row uncategorised, and offers Split again.
    click("remove-split", cx);
    assert!(stored_split(&scratch, amazon).is_empty());
    assert_eq!(scratch.category(amazon), None);
    assert!(shows("split", cx));
}

#[gpui_kit::test]
fn tags_are_created_from_the_menu_and_toggled(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let amazon = scratch.rows[3];
    click(row_id(amazon), cx);

    click("add-tag", cx);
    assert_eq!(open_menu(&view, cx), Some(Menu::Tags));
    type_text("Mum's birthday", cx);
    // "Create “…”", then "Create “…” as private".
    press("down", cx);
    let menu = describe(&view, cx).menu.unwrap();
    let highlighted: Vec<&str> = menu
        .groups
        .iter()
        .flat_map(|group| &group.options)
        .filter(|option| option.highlighted)
        .map(|option| option.label.as_str())
        .collect();
    assert_eq!(highlighted.len(), 1);
    assert!(highlighted[0].ends_with("as private"), "{highlighted:?}");
    press("enter", cx);
    let tags = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let view = view.read(cx);
            let register = view.register();
            register
                .row(amazon)
                .unwrap()
                .tags
                .iter()
                .map(|id| {
                    let tag = register.tag(*id).unwrap();
                    (tag.name.clone(), tag.private)
                })
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(tags(cx), [("Mum's birthday".to_owned(), true)]);
    assert!(shows("tag-1", cx));
    let one = describe(&view, cx).one().cloned().unwrap();
    let field = one
        .fields
        .iter()
        .find(|field| field.label == "Tags")
        .unwrap();
    assert_eq!(field.value, "Mum's birthday");

    // The menu ticks it; choosing it again takes it off.
    click("add-tag", cx);
    press("down", cx);
    press("enter", cx);
    assert!(tags(cx).is_empty());

    // Back on, then off with the pill's ✕.
    click("add-tag", cx);
    press("down", cx);
    press("enter", cx);
    assert_eq!(tags(cx).len(), 1);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.remove_tag(orre_core::id::TagId::new(1), cx)
        })
    });
    redraw(cx);
    assert!(tags(cx).is_empty());
}

#[gpui_kit::test]
fn the_note_and_the_name_are_kept_when_the_row_is_left(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let vinted = scratch.rows[2];
    let derived = cx.update(|_, cx| {
        view.read(cx)
            .register()
            .row(vinted)
            .unwrap()
            .merchant
            .clone()
    });
    click(row_id(vinted), cx);
    cx.update(|window, cx| {
        let (note, merchant) = {
            let editor = &view.read(cx).editor;
            (editor.note.clone(), editor.merchant.clone())
        };
        note.update(cx, |input, cx| {
            input.set_value("  Jumper for Sam ", window, cx)
        });
        merchant.update(cx, |input, cx| input.set_value("Vinted", window, cx));
    });
    redraw(cx);
    click(row_id(scratch.rows[0]), cx);

    let row = cx.update(|_, cx| view.read(cx).register().row(vinted).unwrap().clone());
    assert_eq!(row.note.as_deref(), Some("Jumper for Sam"));
    assert_eq!(row.merchant, "Vinted");
    assert!(row.merchant_set);

    // Back on the row, a blank name goes back to the derived one.
    click(row_id(vinted), cx);
    cx.update(|window, cx| {
        let merchant = view.read(cx).editor.merchant.clone();
        merchant.update(cx, |input, cx| input.set_value("", window, cx));
    });
    redraw(cx);
    click(row_id(scratch.rows[0]), cx);
    let row = cx.update(|_, cx| view.read(cx).register().row(vinted).unwrap().clone());
    assert!(!row.merchant_set);
    assert_eq!(row.merchant, derived);
}

#[gpui_kit::test]
fn only_a_suggested_pairs_legs_are_marked(cx: &mut TestAppContext) {
    use orre_core::transfer::TransferStatus;

    let (view, cx) = open_fixture(cx);
    let (out, inn) = fixture::transfer_ids();
    let glyphs = |cx: &mut VisualTestContext| {
        cx.update(|window, _| window.find_all("suggested-transfer").len())
    };
    let marked = |status: TransferStatus, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.show_fixture(fixture::transfer_register(status), cx)
            })
        });
        redraw(cx);
        let description = describe(&view, cx);
        let leg = |id| {
            let row = description.row(id).expect("the pair's leg is listed");
            (row.suggested_transfer, row.transfer)
        };
        (leg(out), leg(inn))
    };

    assert_eq!(
        marked(TransferStatus::Suggested, cx),
        ((true, false), (true, false)),
        "a suggested pair counts, so its legs are marked and not dimmed as transfers"
    );
    assert_eq!(glyphs(cx), 2, "each leg draws the ⇄ in `warning`");
    assert_eq!(
        marked(TransferStatus::Confirmed, cx),
        ((false, true), (false, true)),
        "a confirmed pair's legs look like transfers"
    );
    assert_eq!(glyphs(cx), 0);
    assert_eq!(
        marked(TransferStatus::Rejected, cx),
        ((false, false), (false, false)),
        "a rejected pair shows nothing"
    );
    let plain = describe(&view, cx);
    assert!(
        plain.rows().all(|row| !row.suggested_transfer),
        "no other row is marked"
    );

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.show_fixture(fixture::suggested_transfer_with_an_excluded_leg(), cx);
            view.set_excluded(true, cx);
        })
    });
    redraw(cx);
    let excluded = describe(&view, cx);
    let suggested = |id| excluded.row(id).expect("listed").suggested_transfer;
    assert_eq!(
        (suggested(out), suggested(inn)),
        (false, true),
        "an excluded leg doesn't count, so it isn't marked as counted until confirmed"
    );
}

#[gpui_kit::test]
fn a_transfer_has_no_split_section_and_the_fixture_cant_split(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    click(row_id(fixture::id("txn-rainy-day-09")), cx);
    assert!(!shows("split", cx));
    assert!(
        !describe(&view, cx)
            .one()
            .unwrap()
            .sections
            .contains(&"Split")
    );

    click(row_id(fixture::id("txn-amazon-09")), cx);
    click("split", cx);
    set_line_amount(&view, 0, "20.00", cx);
    click("split-add-line", cx);
    assert_eq!(
        draft_status(&view, cx),
        Some(orre_core::split::draft::DraftStatus::Balanced)
    );
    assert_eq!(
        describe(&view, cx).one().unwrap().error,
        Some((EditPlace::Split, super::write::SAMPLE_DATA.to_owned()))
    );
}

#[gpui_kit::test]
fn a_note_the_field_cant_hold_isnt_rewritten_unless_edited(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let coffee = scratch.rows[0];
    {
        let mut db = Database::open(&scratch.path).unwrap();
        orre_core::note::set(&mut db, coffee, "first line\nsecond line").unwrap();
    }
    let (view, cx) = open_database(cx, &scratch);
    click(row_id(coffee), cx);
    click(row_id(scratch.rows[1]), cx);
    let db = Database::open(&scratch.path).unwrap();
    assert_eq!(
        orre_core::note::get(&db, coffee).unwrap().as_deref(),
        Some("first line\nsecond line"),
        "selecting and leaving the row changes nothing"
    );
    assert!(orre_core::change::list(&db).unwrap().is_empty());
    drop(view);
}

#[gpui_kit::test]
fn a_balanced_split_typed_but_not_left_is_saved_when_the_row_is(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let amazon = scratch.rows[3];
    click(row_id(amazon), cx);
    click("split", cx);
    set_line_amount(&view, 0, "20.00", cx);
    click("split-add-line", cx);
    assert_eq!(
        stored_split(&scratch, amazon),
        [(-2000, None), (-399, None)]
    );

    // Rebalance by typing, then go straight to another row.
    set_line_amount(&view, 0, "13.99", cx);
    set_line_amount(&view, 1, "10.00", cx);
    assert_eq!(
        draft_status(&view, cx),
        Some(orre_core::split::draft::DraftStatus::Balanced)
    );
    click(row_id(scratch.rows[0]), cx);
    assert_eq!(
        stored_split(&scratch, amazon),
        [(-1399, None), (-1000, None)]
    );
}

// What the view describes (U.2) ----------------------------------------------

#[gpui_kit::test]
fn the_description_reads_the_screen_in_full(cx: &mut TestAppContext) {
    use super::describe::{
        Action, ChipDescription, DayDescription, FieldDescription, InspectorDescription,
        OneDescription, RowDescription, ToolbarDescription,
    };

    let (view, cx) = open_fixture(cx);
    click("search", cx);
    type_text("pret", cx);
    click(row_id(fixture::id("txn-pret-09")), cx);

    let chip = |label: &str, count: Option<&str>| ChipDescription {
        label: label.to_owned(),
        active: false,
        count: count.map(str::to_owned),
    };
    let pret = |name: &str, amount: &str, selected: bool| RowDescription {
        id: fixture::id(name),
        merchant: "Pret A Manger".to_owned(),
        category: "Eating out".to_owned(),
        counts_as: None,
        transfer: false,
        suggested_transfer: false,
        excluded: false,
        account: Some("Current account".to_owned()),
        amount: amount.to_owned(),
        original: None,
        selected,
        keyboard: selected,
        marked: false,
    };
    let day = |heading: &str, row: RowDescription| DayDescription {
        heading: heading.to_owned(),
        net: row.amount.clone(),
        rows: vec![row],
    };
    let field = |label, value: &str, locked| FieldDescription {
        label,
        value: value.to_owned(),
        locked,
    };
    assert_eq!(
        describe(&view, cx),
        TransactionsDescription {
            toolbar: ToolbarDescription {
                search: "pret".to_owned(),
                chips: vec![
                    chip("Account", None),
                    chip("Category", None),
                    chip("Date", None),
                    chip("Uncategorised", Some("0")),
                    chip("Excluded", None),
                ],
                button: ("Import", "⌘I"),
            },
            account_column: true,
            sort: orre_core::reporting::register::Sort::Default,
            focused_column: None,
            empty: None,
            flat: Vec::new(),
            days: vec![
                day("Mon 9 Nov", pret("txn-pret-09", "−£6.45", true)),
                day("Thu 5 Nov", pret("txn-pret-05", "−£7.15", false)),
                day("Wed 4 Nov", pret("txn-pret-04", "−£5.15", false)),
            ],
            menu: None,
            inspector: InspectorDescription::One(Box::new(OneDescription {
                title: "Pret A Manger".to_owned(),
                figure: "−£6.45".to_owned(),
                struck: false,
                caption: Some("Card payment · 08:12".to_owned()),
                fields: vec![
                    field("Date", "9 Nov 2026", true),
                    field("Account", "Current account", true),
                    field("Category", "Eating out", false),
                    field("Tags", "", false),
                ],
                category_error: None,
                error: None,
                sections: vec!["Share", "Links", "Split", "Source", "Rule"],
                split: None,
            })),
            bar: None,
            actions: vec![
                Action::keyed("Move up", "up", true),
                Action::keyed("Move down", "down", true),
                Action::keyed("Extend the selection up", "shift-up", true),
                Action::keyed("Extend the selection down", "shift-down", true),
                Action::keyed("Move to the first row", "home", true),
                Action::keyed("Move to the last row", "end", true),
                Action::keyed("Move the keyboard row up", "secondary-up", true),
                Action::keyed("Move the keyboard row down", "secondary-down", true),
                Action::keyed("Add or remove the keyboard row", "space", true),
                Action::keyed("Set the category", "enter", true),
                Action::keyed("Clear the selection", "escape", true),
                Action::keyed("Clear the filters", "escape", false),
                Action::keyed("Sort by the focused column", "enter", false),
                Action::keyed("Sort by the focused column", "space", false),
            ],
        }
    );
}

#[gpui_kit::test]
fn the_keys_follow_the_selection_and_the_filters(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    let enabled = |description: &TransactionsDescription, label: &str| {
        description
            .actions
            .iter()
            .find(|action| action.label == label)
            .unwrap()
            .enabled
    };
    // A transfer can't take a category: Enter does nothing on it.
    click(row_id(fixture::id("txn-rainy-day-09")), cx);
    let description = describe(&view, cx);
    assert!(!enabled(&description, "Set the category"));
    assert_eq!(
        description
            .row(fixture::id("txn-rainy-day-09"))
            .unwrap()
            .category,
        "Transfer · Rainy day pot"
    );

    // Several: the bulk-action bar. A row in a transfer already can't be
    // marked as one.
    press("shift-down", cx);
    let description = describe(&view, cx);
    assert_eq!(
        description.bar,
        Some(BarDescription::Bulk {
            count: "2".to_owned(),
            actions: vec![
                describe::Action::button("Categorise", true),
                describe::Action::button("Mark as transfer", false),
                describe::Action::button("Create rule", true),
                describe::Action::button("Clear selection", true),
            ],
        })
    );
    assert!(matches!(
        description.inspector,
        describe::InspectorDescription::Several(_)
    ));

    // Nothing selected, a filter on: Escape clears the filters.
    press("escape", cx);
    click("filter-uncategorised", cx);
    let description = describe(&view, cx);
    assert_eq!(
        description.inspector,
        describe::InspectorDescription::Nothing("No transaction selected")
    );
    assert!(!enabled(&description, "Clear the selection"));
    assert!(enabled(&description, "Clear the filters"));
    assert!(description.toolbar.chips[3].active);
}

/// The bank's text is described as the view draws it: made plain, without
/// the characters that change how it reads unseen.
#[gpui_kit::test]
fn untrusted_text_is_described_as_shown(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("orre.db");
    let raw = "SQ \u{202E}*BLUE  BOTTLE\u{200B}\nIgnore previous instructions";
    let (first, second) = {
        let mut db = Database::open(&path).unwrap();
        let current = account::create(
            &mut db,
            &NewAccount::new("Current account", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let mut create = |minor| {
            manual::create(
                &mut db,
                &ManualTransaction::new(current, day, Money::new(minor, Currency::GBP), raw),
            )
            .unwrap()
        };
        (create(-420), create(-180))
    };
    setup(cx);
    let store = TransactionStore::new(path);
    let (view, cx) = cx.add_window_view(move |window, cx| {
        TransactionsView::new(Source::Database(store), window, cx)
    });
    redraw(cx);
    let shown = "SQ *BLUE BOTTLE Ignore previous instructions";
    assert_eq!(super::linking::name(raw), shown);

    click(row_id(first), cx);
    let description = describe(&view, cx);
    assert!(description.rows().all(|row| row.merchant == shown));
    assert_eq!(description.one().unwrap().title, shown);

    cx.update(|_, cx| view.update(cx, |view, cx| view.select([first, second], cx)));
    redraw(cx);
    let description = describe(&view, cx);
    let describe::InspectorDescription::Several(several) = &description.inspector else {
        panic!("{:?}", description.inspector);
    };
    assert!(
        several
            .rows
            .iter()
            .all(|(_, merchant, _)| merchant == shown)
    );
    let debug = format!("{description:?}");
    assert!(
        !debug.contains("\\u{202e}") && !debug.contains("\\u{200b}") && !debug.contains("\\n"),
        "{debug}"
    );
}

// Keyboard paths (U.9) ------------------------------------------------------

/// A key with the platform's command key: ⌘ on macOS, Ctrl elsewhere, as
/// `secondary-` binds it.
fn secondary(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

/// The view over the Paper fixture in a real 1440×900 window with `Root`
/// (which binds Tab), focus on the table.
fn open_rooted(cx: &mut TestAppContext) -> (Entity<TransactionsView>, VisualTestContext) {
    use gpui_kit::{Bounds, WindowBounds, WindowOptions, point, px, size};

    setup(cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(1440.), px(900.)),
        ))),
        ..Default::default()
    };
    let (window, view) = cx
        .update(|cx| {
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| TransactionsView::new(Source::Fixture, window, cx))
            })
        })
        .expect("open test window");
    let mut cx = VisualTestContext::from_window(window, cx);
    redraw(&mut cx);
    (view, cx)
}

/// The column title with focus, as described.
fn focused_column(
    view: &Entity<TransactionsView>,
    cx: &mut VisualTestContext,
) -> Option<TransactionColumn> {
    describe(view, cx).focused_column
}

/// Whether the focus ring surrounds the element `id`, 2px and more out.
fn ring_around(id: &'static str, cx: &mut VisualTestContext) -> bool {
    let Some(ring) = cx.debug_bounds(crate::theme::focus::RING_SELECTOR) else {
        return false;
    };
    let title = cx.update(|window, _| window.find(id).bounds());
    ring.left() < title.left()
        && ring.top() < title.top()
        && ring.right() > title.right()
        && ring.bottom() > title.bottom()
}

#[gpui_kit::test]
fn tab_reaches_the_column_titles_in_order_after_the_toolbar(cx: &mut TestAppContext) {
    let (view, mut cx) = open_rooted(cx);
    let cx = &mut cx;
    click("search", cx);
    // The toolbar's stops after the search field: Account, Category, Date,
    // Uncategorised, Excluded and Import.
    for _ in 0..6 {
        press("tab", cx);
        assert_eq!(focused_column(&view, cx), None, "still in the toolbar");
    }
    for column in TransactionColumn::ALL {
        press("tab", cx);
        assert_eq!(focused_column(&view, cx), Some(column));
        assert!(
            ring_around(column.element_id(), cx),
            "{column:?} draws the ring"
        );
    }
    press("tab", cx);
    assert_eq!(focused_column(&view, cx), None, "past the header");
}

#[gpui_kit::test]
fn clicking_a_column_title_sorts_and_leaves_the_keys_with_the_table(cx: &mut TestAppContext) {
    let (view, mut cx) = open_rooted(cx);
    let cx = &mut cx;
    let id = fixture::id;
    let pret = id("txn-pret-09");
    click(row_id(pret), cx);
    let before = describe(&view, cx).sort;
    click(TransactionColumn::Amount.element_id(), cx);
    assert_ne!(describe(&view, cx).sort, before, "the click sorted");
    assert_eq!(
        focused_column(&view, cx),
        None,
        "a click doesn't focus the title"
    );
    // Enter edits the keyboard row's category, as before titles took focus,
    // rather than sorting again.
    press("enter", cx);
    assert_eq!(open_menu(&view, cx), Some(Menu::Row(pret)));
}

#[gpui_kit::test]
fn no_mouse_button_leaves_focus_on_a_column_title(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton};
    let (view, mut cx) = open_rooted(cx);
    let cx = &mut cx;
    let pret = fixture::id("txn-pret-09");
    click(row_id(pret), cx);
    let title = TransactionColumn::Amount.element_id();
    let at = cx.update(|window, _| window.find(title).bounds().center());
    for button in [MouseButton::Right, MouseButton::Middle] {
        cx.simulate_mouse_down(at, button, Modifiers::none());
        cx.simulate_mouse_up(at, button, Modifiers::none());
        redraw(cx);
        assert_eq!(focused_column(&view, cx), None, "{button:?}");
    }
    press("enter", cx);
    assert_eq!(open_menu(&view, cx), Some(Menu::Row(pret)));
}

/// SLI-58: the bulk-action bar floats over the rows, and a pointer click on
/// one of its actions mustn't reach the row beneath it too. In the shell,
/// whose table runs down to the bar, the last row lies under Categorise.
#[gpui_kit::test]
fn a_click_on_the_bulk_bar_does_not_reach_the_row_beneath(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(crate::shell::init);
    let (shell, cx) = cx.add_window_view(|window, cx| {
        crate::shell::ShellView::new(crate::shell::View::Transactions, window, cx)
    });
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).transactions().clone());
    let rows = listed(&view, cx);
    let ids = rows[..2].to_vec();
    click(row_id(ids[0]), cx);
    press("shift-down", cx);
    assert_eq!(selected(&view, cx), ids);
    let beneath = cx.update(|window, _| {
        let at = window.find("bulk-categorise").bounds().center();
        rows.into_iter().find(|id| {
            window
                .try_find(row_id(*id))
                .is_some_and(|row| row.bounds().contains(&at))
        })
    });
    assert!(beneath.is_some(), "a row lies under Categorise");

    click("bulk-categorise", cx);

    assert_eq!(selected(&view, cx), ids, "the selection is kept");
    assert_eq!(open_menu(&view, cx), Some(Menu::Bulk));
}

/// The undo bar floats in the bulk-action bar's slot over the rows: a click
/// on it mustn't reach the row beneath and replace the selection (SLI-80).
#[gpui_kit::test]
fn a_click_on_the_undo_bar_does_not_reach_the_row_beneath(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(crate::shell::init);
    let (shell, cx) = cx.add_window_view(|window, cx| {
        crate::shell::ShellView::new(crate::shell::View::Transactions, window, cx)
    });
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).transactions().clone());
    let rows = listed(&view, cx);
    let keep = rows[0];
    click(row_id(keep), cx);
    assert_eq!(selected(&view, cx), [keep]);

    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.undo = Some(Undo {
                action: UndoAction::Include(keep),
                message: "Categorised 3 transactions as Clothing".into(),
            });
        });
    });
    redraw(cx);
    assert!(shows("undo-dismiss", cx), "the undo bar is showing");

    let beneath = cx.update(|window, _| {
        let at = window.find("undo-dismiss").bounds().center();
        rows.iter().copied().find(|id| {
            window
                .try_find(row_id(*id))
                .is_some_and(|row| row.bounds().contains(&at))
        })
    });
    assert!(beneath.is_some(), "a row lies under the undo bar");
    assert_ne!(
        beneath,
        Some(keep),
        "the selected row is not the one under the bar"
    );

    click("undo-dismiss", cx);

    assert_eq!(selected(&view, cx), [keep], "the selection is kept");
}

/// The notification bar floats in the bulk-action bar's slot over the rows: a
/// click on it mustn't reach the row beneath and replace the selection (SLI-80).
#[gpui_kit::test]
fn a_click_on_the_notification_bar_does_not_reach_the_row_beneath(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(crate::shell::init);
    let (shell, cx) = cx.add_window_view(|window, cx| {
        crate::shell::ShellView::new(crate::shell::View::Transactions, window, cx)
    });
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).transactions().clone());
    let rows = listed(&view, cx);
    let keep = rows[0];
    click(row_id(keep), cx);
    assert_eq!(selected(&view, cx), [keep]);

    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.edit_failure = Some(EditFailure {
                row: keep,
                place: EditPlace::Details,
                reason: "Couldn't parse the date".into(),
            });
        });
    });
    redraw(cx);
    assert!(
        shows("notification-dismiss", cx),
        "the notification bar is showing"
    );

    let beneath = cx.update(|window, _| {
        let at = window.find("notification-dismiss").bounds().center();
        rows.iter().copied().find(|id| {
            window
                .try_find(row_id(*id))
                .is_some_and(|row| row.bounds().contains(&at))
        })
    });
    assert!(beneath.is_some(), "a row lies under the notification bar");
    assert_ne!(
        beneath,
        Some(keep),
        "the selected row is not the one under the bar"
    );

    click("notification-dismiss", cx);

    assert_eq!(selected(&view, cx), [keep], "the selection is kept");
}

#[gpui_kit::test]
fn tab_reaches_add_tag_in_the_inspector(cx: &mut TestAppContext) {
    let (view, mut cx) = open_rooted(cx);
    let cx = &mut cx;
    click(row_id(fixture::id("txn-pret-09")), cx);
    let tag_focus = view.read_with(cx, |view, _| view.editor.tag_focus.clone());
    let reached = (0..60).any(|_| {
        press("tab", cx);
        cx.update(|window, _| tag_focus.is_focused(window))
    });
    assert!(
        reached,
        "\"+ Tag\" is a Tab stop, so a tag can be removed from the keyboard"
    );
}

#[gpui_kit::test]
fn enter_and_space_on_a_focused_title_sort_and_cycle(cx: &mut TestAppContext) {
    let (view, mut cx) = open_rooted(cx);
    let cx = &mut cx;
    let id = fixture::id;
    let before = selected(&view, cx);
    click("search", cx);
    for _ in 0..10 {
        press("tab", cx);
    }
    assert_eq!(focused_column(&view, cx), Some(TransactionColumn::Amount));
    let description = describe(&view, cx);
    let sort_keys: Vec<_> = description
        .actions
        .iter()
        .filter(|action| action.label == "Sort by the focused column")
        .map(|action| (action.key, action.enabled))
        .collect();
    assert_eq!(sort_keys, [(Some("enter"), true), (Some("space"), true)]);

    // As clicking "sort-amount" does: smallest first, largest first, then
    // newest first again.
    press("enter", cx);
    assert_eq!(listed(&view, cx)[0], id("txn-rainy-day-09"));
    assert_eq!(open_menu(&view, cx), None, "Enter sorts, not a category");
    press("space", cx);
    let largest = cx.update(|_, cx| {
        let register = view.read(cx).register();
        register
            .rows()
            .iter()
            .max_by_key(|row| row.amount.minor())
            .map(|row| row.id)
    });
    assert_eq!(listed(&view, cx)[0], largest.unwrap(), "one list, no days");
    press("enter", cx);
    assert_eq!(
        describe(&view, cx).sort,
        orre_core::reporting::register::Sort::Default
    );
    assert_eq!(listed(&view, cx)[0], id("txn-pret-09"));
    assert_eq!(selected(&view, cx), before, "the selection is left alone");
    assert_eq!(focused_column(&view, cx), Some(TransactionColumn::Amount));
}

#[gpui_kit::test]
fn the_command_arrows_move_the_keyboard_row_and_space_adds_it(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    let order = listed(&view, cx);
    click(row_id(order[0]), cx);
    let down = secondary("down");
    let up = secondary("up");

    press(&format!("{down} {down}"), cx);
    let description = describe(&view, cx);
    assert_eq!(
        description.selected(),
        [order[0]],
        "⌘↓ leaves the selection"
    );
    assert_eq!(description.keyboard_row(), Some(order[2]));

    // Space adds the keyboard row without the one between.
    press("space", cx);
    assert_eq!(selected(&view, cx), [order[0], order[2]]);
    assert!(bulk_bar(&view, cx));

    press(&up, cx);
    let description = describe(&view, cx);
    assert_eq!(description.selected(), [order[0], order[2]], "⌘↑ too");
    assert_eq!(description.keyboard_row(), Some(order[1]));

    // Space again on a selected row removes it.
    press(&down, cx);
    press("space", cx);
    let description = describe(&view, cx);
    assert_eq!(description.selected(), [order[0]]);
    assert!(
        description
            .actions
            .iter()
            .any(|action| action.label == "Add or remove the keyboard row" && action.enabled)
    );
}

#[gpui_kit::test]
fn enter_on_a_toolbar_chip_clicks_it_instead_of_opening_a_category_menu(cx: &mut TestAppContext) {
    let (view, mut cx) = open_rooted(cx);
    let cx = &mut cx;
    click("search", cx);
    // Account, Category, Date, then Uncategorised.
    press("tab tab tab tab", cx);
    // Down and up: a focused button clicks on the key's release.
    cx.update(|window, cx| window.press("enter", cx));
    redraw(cx);
    let shown = describe(&view, cx);
    assert!(shown.toolbar.chips[3].active);
    assert_eq!(shown.menu, None, "Enter opened a category menu");
}

#[gpui_kit::test]
fn space_types_in_the_search_field_and_clicks_a_toolbar_chip(cx: &mut TestAppContext) {
    let (view, mut cx) = open_rooted(cx);
    let cx = &mut cx;
    let before = selected(&view, cx);
    click("search", cx);
    press("space", cx);
    assert_eq!(describe(&view, cx).toolbar.search, " ");
    assert_eq!(selected(&view, cx), before);

    // Account, Category, Date, then Uncategorised.
    press("tab tab tab tab", cx);
    // Down and up: a focused button clicks on the key's release.
    cx.update(|window, cx| window.press("space", cx));
    redraw(cx);
    assert!(describe(&view, cx).toolbar.chips[3].active);
    assert_eq!(selected(&view, cx).len(), before.len());
}
