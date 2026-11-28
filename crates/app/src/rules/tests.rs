//! UI integration tests for the Rules view: the shell's Rules tab in a
//! headless window, against a scratch database seeded with [`fixture`]
//! through [`RulesStore`], as `orre` runs it. What the view shows is read
//! through its test-only `describe()` ([`super::describe`]).

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use orre_core::change::{self as core_change, BatchStatus};
use orre_core::db::Database;
use orre_core::id::TransactionId;
use orre_core::rule::{self as core_rule, MatchKind};
use tempfile::TempDir;

use super::describe::{
    Action, BarDescription, CanvasDescription, InspectorDescription, ItemDescription,
    PanelRowDescription, PreviewDescription, RulesDescription, ToolbarDescription, describe_window,
};
use super::view::{Input, Menu};
use super::*;
use crate::accounts::{Accounts, fixture as accounts_fixture};
use crate::inbox::Inbox;
use crate::shell::{ShellView, View};
use crate::transactions::{self, store::TransactionStore};

pub(super) struct Scratch {
    _dir: TempDir,
    pub(super) path: std::path::PathBuf,
    pub(super) seeded: fixture::Seeded,
}

impl Scratch {
    pub(super) fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        let mut db = Database::open(&path).unwrap();
        let seeded = fixture::seed(&mut db);
        Self {
            _dir: dir,
            path,
            seeded,
        }
    }

    pub(super) fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn category(&self, id: TransactionId) -> Option<String> {
        orre_core::reporting::register::Register::load(
            &self.db().reader().unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap(),
        )
        .unwrap()
        .rows()
        .iter()
        .find(|row| row.id == id)
        .and_then(|row| match row.category {
            orre_core::reporting::register::RowCategory::Category(id) => Some(id),
            _ => None,
        })
        .and_then(|category| {
            self.db()
                .categories()
                .unwrap()
                .iter()
                .find(|c| c.id() == category)
                .map(|c| c.key().as_str().to_owned())
        })
    }
}

pub(super) fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<ShellView>, Entity<Rules>, &'a mut VisualTestContext) {
    let (shell, rules, cx) = open_with(cx, scratch, Rules::from_store);
    redraw(cx);
    (shell, rules, cx)
}

/// The shell's Rules tab with the Rules `make` builds over the scratch
/// database, not yet redrawn.
fn open_with<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
    make: impl FnOnce(RulesStore) -> Rules + 'static,
) -> (Entity<ShellView>, Entity<Rules>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = RulesStore::new(scratch.path.clone());
    let transactions = TransactionStore::new(scratch.path.clone());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
        let inbox = cx.new(|_| Inbox::empty());
        let settings = cx.new(crate::settings::SettingsView::fixture);
        let mut shell = ShellView::with_all(
            View::Rules,
            accounts,
            transactions::Source::Database(transactions),
            inbox,
            settings,
            window,
            cx,
        );
        let rules = cx.new(|_| make(store));
        shell.install_rules(rules, window, cx);
        shell
    });
    let rules = cx.update(|_, cx| shell.read(cx).rules().clone());
    (shell, rules, cx)
}

pub(super) fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// What the Rules tab shows now ([`describe_window`]).
pub(super) fn describe(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> RulesDescription {
    describe_window(shell, cx)
}

/// The rows of the Rules table, as `describe()` lists them.
pub(super) fn rule_rows(shown: &RulesDescription) -> &[super::describe::RuleRowDescription] {
    match &shown.canvas {
        CanvasDescription::Rules(rows) => rows,
        other => panic!("not the rules table: {other:?}"),
    }
}

/// The preview on the canvas.
fn preview_of(shown: &RulesDescription) -> &PreviewDescription {
    match &shown.canvas {
        CanvasDescription::Preview(preview) => preview,
        other => panic!("not the preview: {other:?}"),
    }
}

/// The inspector's item.
pub(super) fn item(shown: &RulesDescription) -> &ItemDescription {
    match &shown.inspector {
        InspectorDescription::Item(item) => item,
        other => panic!("nothing in the inspector: {other:?}"),
    }
}

/// The labels of `actions`, with whether each is enabled.
fn labels(actions: &[Action]) -> Vec<(&str, bool)> {
    actions
        .iter()
        .map(|action| (action.label.as_str(), action.enabled))
        .collect()
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

fn shows(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).is_some())
}

#[gpui_kit::test]
fn rules_list_in_order_with_what_they_decide(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let ids: Vec<RuleId> =
        cx.update(|_, cx| rules.read(cx).rules().iter().map(|s| s.rule.id()).collect());
    assert_eq!(ids, scratch.seeded.rules);
    let shown = describe(&shell, cx);
    let rows = rule_rows(&shown);
    assert_eq!(rows.len(), 14);
    assert_eq!(
        rows.iter().map(|row| row.number).collect::<Vec<_>>(),
        (1..=14).collect::<Vec<_>>()
    );
    let never: Vec<&str> = rows
        .iter()
        .filter(|row| row.never_reached)
        .map(|row| row.when.as_str())
        .collect();
    assert_eq!(
        never,
        ["Merchant contains UBER TRIP"],
        "UBER TRIP loses to UBER"
    );
    assert_eq!(rows[2].matches, "0");
    assert_eq!(
        (rows[5].account.as_str(), rows[5].then.as_str()),
        ("Joint account", "Groceries + tag"),
        "WAITROSE: one account, a category and a tag"
    );
    assert_eq!(rows[11].then, "Tag Gift Aid", "no category: the tag leads");
    assert_eq!(
        rows[13].when,
        "Bank text contains FLAT 2B RENT · over £1,000"
    );
    assert_eq!(
        shown.selected.as_deref(),
        Some("Merchant contains TFL TRAVEL"),
        "the first rule opens"
    );
    assert_eq!(item(&shown).title, "Merchant contains TFL TRAVEL");
    assert!(shows(&format!("rule-{}", scratch.seeded.rules[1]), cx));

    // The ACTION filter narrows the table; a second click clears it.
    click("rules-action-add_tag", cx);
    let shown = describe(&shell, cx);
    let tagged: Vec<&str> = rule_rows(&shown)
        .iter()
        .map(|row| row.when.as_str())
        .collect();
    assert_eq!(
        tagged,
        ["Merchant contains WAITROSE", "Merchant contains JUSTGIVING"],
        "WAITROSE and JUSTGIVING add tags"
    );
    let on: Vec<&PanelRowDescription> = shown.panel.filters.iter().filter(|f| f.selected).collect();
    assert_eq!(on.len(), 1);
    assert_eq!((on[0].label, on[0].count), ("Add tag", 2));
    click("rules-action-add_tag", cx);
    let shown = describe(&shell, cx);
    assert_eq!(rule_rows(&shown).len(), 14);
    assert!(shown.panel.filters.iter().all(|filter| !filter.selected));

    // Selecting the never-reached rule offers to move it above UBER.
    let trip = scratch.seeded.rules[2];
    click(&format!("rule-{trip}"), cx);
    let shown = describe(&shell, cx);
    let card = item(&shown);
    assert_eq!(
        card.never_reached.as_deref(),
        Some(
            "Rule 2, Merchant contains UBER, comes first and catches all 2 transactions this \
             rule would match."
        )
    );
    assert_eq!(
        card.caption.as_deref(),
        Some("transactions decided by this rule · rule 3 of 14")
    );
    assert_eq!(
        labels(&card.actions)[..2],
        [("Move above rule 2", true), ("Show rule 2", true)]
    );
    click("rules-move-above", cx);
    let shown = describe(&shell, cx);
    let moved = &rule_rows(&shown)[1];
    assert_eq!(
        moved.when, "Merchant contains UBER TRIP",
        "moved above rule 2"
    );
    assert_eq!(
        (moved.matches.as_str(), moved.never_reached),
        ("2", false),
        "it decides its UBER TRIP rows now"
    );
    assert_eq!(item(&shown).never_reached, None);
}

#[gpui_kit::test]
fn editing_a_rule_saves_without_touching_history(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let pret = scratch.seeded.rules[3];
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(pret), cx)));
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).rules_view().clone());

    // A keystroke changes the form; leaving the field (Enter) saves.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::Pattern, "PRET", window, cx)
        })
    });
    let stored = core_rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(
        stored.definition().pattern().as_str(),
        "PRET A MANGER",
        "not yet"
    );
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.save_rule(cx)));
    let stored = core_rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(stored.definition().pattern().as_str(), "PRET");

    // A pattern that can't be used isn't saved, and says why.
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit(
                true,
                |form| {
                    form.kind = MatchKind::Regex;
                    form.text = "^(PRET".into();
                },
                cx,
            )
        })
    });
    redraw(cx);
    let error = cx.update(|_, cx| rules.read(cx).form_error());
    assert!(matches!(error, Some(form::FormError::Pattern(_))));
    let stored = core_rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(stored.definition().pattern().kind(), MatchKind::Contains);

    // Adding an action takes the menu; Split isn't offered with Category.
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit(false, |form| form.kind = MatchKind::Contains, cx)
        })
    });
    cx.update(|window, cx| view.update(cx, |view, cx| view.show_menu(Menu::AddAction, window, cx)));
    redraw(cx);
    let available = cx.update(|_, cx| {
        rules
            .read(cx)
            .form()
            .unwrap()
            .availability(orre_core::rule::ActionKind::SplitTemplate)
    });
    assert_eq!(
        available,
        Some(form::Unavailable::NotWith(
            orre_core::rule::ActionKind::SetCategory
        ))
    );
}

#[gpui_kit::test]
fn applying_a_rule_previews_then_changes_and_undoes(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let dishoom = scratch.seeded.rules[12];
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(dishoom), cx)));
    redraw(cx);
    click("rules-apply", cx);
    let shown = describe(&shell, cx);
    assert_eq!(shown.toolbar.title, "Apply to past transactions");
    assert_eq!(
        shown.toolbar.caption,
        "Rule 13: Merchant starts with DISHOOM → Eating out · nothing changes until you apply"
    );
    assert_eq!(labels(&shown.toolbar.buttons), [("Back to rules", true)]);
    assert!(
        shown.toolbar.keys.is_empty(),
        "no key hints over the preview"
    );
    let preview = preview_of(&shown);
    assert_eq!(preview.will_change.len(), 1, "the one DISHOOM row");
    let row = &preview.will_change[0];
    assert_eq!(
        (row.merchant.as_str(), row.now.as_str(), row.after.as_str()),
        ("DISHOOM", "Uncategorised", "Eating out")
    );
    assert_eq!(preview.user_set, None);
    assert_eq!(labels(&preview.actions), [("Apply to 1 transaction", true)]);
    assert!(
        labels(&item(&shown).actions).contains(&("Apply to past transactions…", false)),
        "Apply is off while its preview is open"
    );

    click("rules-commit", cx);
    let shown = describe(&shell, cx);
    assert_eq!(
        shown.bar,
        Some(BarDescription {
            message: "Changed 1 transaction to Eating out".into(),
            undo: true,
        })
    );
    assert!(
        rule_rows(&shown).iter().any(|row| row.selected),
        "back to the table"
    );
    let Some(Outcome::Done {
        undo: Some(batch), ..
    }) = cx.update(|_, cx| rules.read(cx).outcome().cloned())
    else {
        panic!("the bar's Undo reverts a batch");
    };
    let batch_info = core_change::get(&scratch.db(), batch).unwrap();
    assert_eq!(batch_info.rule, Some(dishoom));

    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.undo(cx)));
    let batch_info = core_change::get(&scratch.db(), batch).unwrap();
    assert_eq!(batch_info.status, BatchStatus::Undone);
    assert_eq!(describe(&shell, cx).bar, None, "the bar goes");
}

#[gpui_kit::test]
fn a_rule_from_an_edit_is_previewed_then_created_and_applied(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    // Transactions proposes the rule after the VINTED edit (`3WT-0`).
    let transactions = cx.update(|_, cx| shell.read(cx).transactions().clone());
    cx.update(|_, cx| {
        transactions.update(cx, |view, cx| view.propose_rule(scratch.seeded.vinted, cx))
    });
    let db = TransactionStore::new(scratch.path.clone()).open().unwrap();
    let proposal = core_rule::propose(&db, scratch.seeded.vinted)
        .unwrap()
        .expect("VINTED proposes a rule");
    drop(db);
    assert_eq!(proposal.definition.pattern().as_str(), "VINTED");
    assert_eq!(proposal.matches, 6);
    assert_eq!(proposal.uncategorized, 4);
    assert_eq!(proposal.user_set, 2);

    // "Create rule…" opens Rules on the preview, nothing saved.
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.new_rule_from(&proposal.definition, core_rule::RuleOrigin::Edit, cx)
        })
    });
    redraw(cx);
    let shown = describe(&shell, cx);
    assert!(
        shown
            .toolbar
            .caption
            .starts_with("New rule: Merchant contains VINTED → "),
        "{}",
        shown.toolbar.caption
    );
    let preview = preview_of(&shown);
    let (user_set, include) = preview.user_set.clone().expect("the two the user set");
    assert_eq!(
        (preview.will_change.len(), user_set.len(), include),
        (4, 2, false)
    );
    assert!(
        user_set.iter().all(|row| row.after == "No change"),
        "left as they are unless included"
    );
    assert_eq!(
        labels(&preview.actions),
        [
            ("Create rule and change 4", true),
            ("Create rule only", true)
        ]
    );
    let new = item(&shown);
    assert_eq!(new.title, "Merchant contains VINTED");
    assert_eq!(
        (new.figure.as_deref(), new.caption.as_deref()),
        (
            Some("6"),
            Some("earlier transactions match · new rule, not saved yet")
        )
    );
    assert_eq!(shown.panel.sections[0].count, 14, "nothing stored yet");
    assert!(shows("rules-create-only", cx));

    // "Change these too" includes the two the user set.
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.set_include(true, cx)));
    let shown = describe(&shell, cx);
    let preview = preview_of(&shown);
    assert_eq!(
        labels(&preview.actions)[0],
        ("Create rule and change 6", true)
    );
    assert!(
        preview
            .user_set
            .as_ref()
            .is_some_and(|(rows, on)| *on && rows.iter().all(|row| row.after != "No change"))
    );
    click("rules-commit", cx);
    let shown = describe(&shell, cx);
    assert_eq!(shown.panel.sections[0].count, 15);
    let first = &rule_rows(&shown)[0];
    assert_eq!(
        (first.when.as_str(), first.selected),
        ("Merchant contains VINTED", true),
        "a new rule goes first, selected"
    );
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).data().rules[0].rule.origin()),
        Some(core_rule::RuleOrigin::Edit)
    );
    assert_eq!(
        scratch.category(scratch.seeded.vinted).as_deref(),
        Some("shopping")
    );
}

#[gpui_kit::test]
fn names_and_history_open_with_their_inspectors(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    click("rules-section-names", cx);
    let shown = describe(&shell, cx);
    assert!(shown.panel.sections[1].selected, "Merchant names is open");
    assert_eq!(shown.panel.filter_title, "Match");
    assert_eq!(
        shown.toolbar.caption,
        "8 names · how a bank’s merchant text is shown"
    );
    let CanvasDescription::Names(names) = &shown.canvas else {
        panic!("the names table: {:?}", shown.canvas);
    };
    assert_eq!(names.len(), 8);
    let amazon = &names[0];
    assert_eq!(
        (
            amazon.bank_text.as_str(),
            amazon.shows_as.as_str(),
            amazon.matches.as_str(),
            amazon.selected
        ),
        ("Starts with AMZN MKTP", "Amazon Marketplace", "3", true)
    );
    assert_eq!(names[7].bank_text, "Pattern ^UBER \\*?(TRIP|BV)");
    let inspector = item(&shown);
    assert_eq!(
        (inspector.title.as_str(), inspector.figure.as_deref()),
        ("Amazon Marketplace", Some("3"))
    );
    assert_eq!(labels(&inspector.actions), [("Delete name…", true)]);

    // A new name applies at once once both fields are filled.
    click("rules-new-name", cx);
    assert_eq!(
        item(&describe(&shell, cx)).caption.as_deref(),
        Some("new name, saved once both fields are filled in")
    );
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit_name(
                true,
                |form| {
                    form.pattern = "VINTED".into();
                    form.name = "Vinted".into();
                },
                cx,
            )
        })
    });
    redraw(cx);
    let shown = describe(&shell, cx);
    assert_eq!(shown.panel.sections[1].count, 9);
    assert_eq!(item(&shown).title, "Vinted");

    click("rules-section-history", cx);
    let shown = describe(&shell, cx);
    let CanvasDescription::History(changes) = &shown.canvas else {
        panic!("the history: {:?}", shown.canvas);
    };
    assert!(
        changes.len() >= 5,
        "the applies, the undone one and the edits"
    );
    let undone: Vec<&str> = changes
        .iter()
        .filter(|change| change.undone)
        .map(|change| change.change.as_str())
        .collect();
    assert_eq!(undone, ["Applied rule PRET A MANGER → Eating out"]);
    assert!(
        changes
            .iter()
            .any(|change| change.kind.starts_with("Undone "))
    );
    let newest = &changes[0];
    assert!(newest.selected, "the newest change opens");
    let inspector = item(&shown);
    assert_eq!(inspector.title, newest.change);
    assert_eq!(labels(&inspector.actions), [("Undo this change…", true)]);
    let items = cx.update(|_, cx| rules.read(cx).batch_items().len());
    assert!(items > 0, "with what it changed");

    // Undo this change… opens the dialog, which says what goes back.
    click("rules-undo-change", cx);
    let dialog = describe(&shell, cx).dialog.expect("the undo dialog");
    assert_eq!(dialog.title, format!("Undo “{}”?", newest.change));
    assert!(
        dialog.body.contains("back to the values"),
        "{}",
        dialog.body
    );
    assert_eq!(
        labels(&dialog.actions),
        [("Cancel", true), ("Undo change", true)]
    );
}

#[gpui_kit::test]
fn typing_then_selecting_another_rule_keeps_each_rules_text(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let (pret, uber) = (scratch.seeded.rules[3], scratch.seeded.rules[1]);
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(pret), cx)));
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).rules_view().clone());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::Pattern, "PRET", window, cx)
        })
    });
    // Another row is chosen before the field is left.
    click(&format!("rule-{uber}"), cx);
    let stored = |id| {
        core_rule::get(&scratch.db(), id)
            .unwrap()
            .definition()
            .pattern()
            .as_str()
            .to_owned()
    };
    assert_eq!(stored(pret), "PRET", "what was typed is kept");
    assert_eq!(stored(uber), "UBER", "and never lands on the next rule");
    let shown = cx.update(|_, cx| rules.read(cx).form().unwrap().text.clone());
    assert_eq!(shown, "UBER");
}

// Categories -------------------------------------------------------------------------

fn category_key(text: &str) -> orre_core::category::CategoryKey {
    orre_core::category::CategoryKey::new(text).unwrap()
}

/// The stored category with `key` in `scratch`'s database.
fn stored_category(scratch: &Scratch, key: &str) -> Option<orre_core::category::Category> {
    scratch
        .db()
        .categories()
        .unwrap()
        .get(&category_key(key))
        .cloned()
}

#[gpui_kit::test]
fn the_categories_section_lists_and_filters_every_category(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let seeded = fixture::seed_categories(&mut scratch.db());
    let (shell, rules, cx) = open(cx, &scratch);
    click("rules-section-categories", cx);
    let categories = |shown: &RulesDescription| match &shown.canvas {
        CanvasDescription::Categories(rows) => rows.clone(),
        other => panic!("the categories table: {other:?}"),
    };
    let shown = describe(&shell, cx);
    let all = cx.update(|_, cx| rules.read(cx).data().categories.len());
    assert_eq!(shown.panel.sections[3].count, all);
    assert_eq!(
        categories(&shown).len(),
        all,
        "every category, hidden ones too"
    );
    assert_eq!(
        shown.selected.as_deref(),
        Some(categories(&shown)[0].label.as_str()),
        "the first row opens"
    );
    assert!(shows(&format!("category-{}", seeded.coffee), cx));
    assert!(shows(&format!("category-{}", seeded.side_projects), cx));
    assert!(labels(&item(&shown).actions).contains(&("Hide category", true)));
    assert_eq!(
        shown
            .panel
            .filters
            .iter()
            .map(|f| f.label)
            .collect::<Vec<_>>(),
        ["Spending", "Income", "Transfers", "Hidden"]
    );

    // KIND keeps a kind's groups and subcategories; a second click clears.
    click("rules-category-kind-income", cx);
    let shown = describe(&shell, cx);
    let income = categories(&shown);
    assert!(!income.is_empty());
    assert!(income.iter().all(|row| row.kind == "Income"));
    assert_eq!(shown.panel.filters[1].count, income.len());
    assert!(shown.panel.filters[1].selected);
    click("rules-category-kind-income", cx);
    assert_eq!(categories(&describe(&shell, cx)).len(), all);

    // Hidden: Mortgage, and Travel with its subcategories.
    click("rules-category-hidden", cx);
    let hidden: Vec<(String, bool, bool)> = categories(&describe(&shell, cx))
        .into_iter()
        .map(|row| (row.label, row.subcategory, row.hidden))
        .collect();
    assert_eq!(
        hidden,
        [
            ("Mortgage".to_owned(), true, true),
            ("Travel".to_owned(), false, true),
            ("Flights".to_owned(), true, true),
            ("Accommodation".to_owned(), true, true),
        ]
    );

    // ↓ moves the selection through the listed rows.
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_category(None, cx)));
    let shown = describe(&shell, cx);
    assert_eq!(
        shown.inspector,
        InspectorDescription::Nothing(inspector::NO_CATEGORY)
    );
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.select_next(crate::ui::list::Move::DOWN, cx)
        })
    });
    let first = cx.update(|_, cx| rules.read(cx).selected_category());
    let mortgage = stored_category(&scratch, "home.mortgage").unwrap().id();
    assert_eq!(first, Some(CategorySelection::Stored(mortgage)));
    assert_eq!(describe(&shell, cx).selected.as_deref(), Some("Mortgage"));

    // Another section and back keeps the Categories selection.
    click("rules-section-rules", cx);
    assert!(describe(&shell, cx).panel.sections[0].selected);
    click("rules-section-categories", cx);
    assert_eq!(describe(&shell, cx).selected.as_deref(), Some("Mortgage"));
}

#[gpui_kit::test]
fn a_new_category_is_checked_then_created(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    click("rules-section-categories", cx);
    click("rules-new-category", cx);
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_category()),
        Some(CategorySelection::New)
    );
    // Nothing typed: Create waits, and says nothing yet.
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).category_error()),
        Some(form::CategoryError::Blank)
    );
    let shown = describe(&shell, cx);
    assert_eq!(item(&shown).title, "New category");
    assert_eq!(labels(&item(&shown).actions), [("Create category", false)]);
    assert_eq!(shown.selected, None, "no row for it yet");
    let view = cx.update(|_, cx| shell.read(cx).rules_view().clone());

    // A name a sibling has is refused, ignoring case.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::CategoryName, "groceries", window, cx)
        })
    });
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit_category(false, |form| form.group = Some(category_key("food")), cx)
        })
    });
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).category_error()),
        Some(form::CategoryError::Taken)
    );
    click("rules-create-category", cx);
    assert!(stored_category(&scratch, "food.groceries_2").is_none());

    // Another name under Food is created with Food's kind, and selected.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::CategoryName, "Coffee beans", window, cx)
        })
    });
    assert_eq!(cx.update(|_, cx| rules.read(cx).category_error()), None);
    assert_eq!(
        labels(&item(&describe(&shell, cx)).actions),
        [("Create category", true)]
    );
    click("rules-create-category", cx);
    let created = stored_category(&scratch, "food.u_coffee_beans").expect("created");
    assert_eq!(created.label(), "Coffee beans");
    assert_eq!(created.kind(), orre_core::category::CategoryKind::Spending);
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_category()),
        Some(CategorySelection::Stored(created.id()))
    );
    assert_eq!(
        describe(&shell, cx).selected.as_deref(),
        Some("Coffee beans")
    );

    // A new group takes the kind chosen for it.
    click("rules-new-category", cx);
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit_category(
                false,
                |form| {
                    form.name = "Side income".into();
                    form.kind = orre_core::category::CategoryKind::Income;
                },
                cx,
            )
        })
    });
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.create_category(cx)));
    let group = stored_category(&scratch, "u_side_income").expect("a new group");
    assert!(group.is_group());
    assert_eq!(group.kind(), orre_core::category::CategoryKind::Income);
}

#[gpui_kit::test]
fn renaming_hiding_and_deleting_a_category_go_to_the_database(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let seeded = fixture::seed_categories(&mut scratch.db());
    let (shell, rules, cx) = open(cx, &scratch);
    let view = cx.update(|_, cx| shell.read(cx).rules_view().clone());
    let groceries = stored_category(&scratch, "food.groceries").unwrap().id();
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.select_category(Some(CategorySelection::Stored(groceries)), cx)
        })
    });
    redraw(cx);

    // A rename saves on leaving the field; the key stays.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::CategoryName, "Food shop", window, cx)
        })
    });
    assert_eq!(
        stored_category(&scratch, "food.groceries").unwrap().label(),
        "Groceries",
        "not yet"
    );
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.save_category_name(cx)));
    assert_eq!(
        stored_category(&scratch, "food.groceries").unwrap().label(),
        "Food shop"
    );
    // A name its group already has isn't saved.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::CategoryName, "Takeaways", window, cx)
        })
    });
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.save_category_name(cx)));
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).category_error()),
        Some(form::CategoryError::Taken)
    );
    assert_eq!(
        stored_category(&scratch, "food.groceries").unwrap().label(),
        "Food shop"
    );

    // Groceries is used, so it can be hidden but not deleted.
    redraw(cx);
    let shown = describe(&shell, cx);
    assert_eq!(item(&shown).title, "Food shop");
    assert_eq!(item(&shown).footer.as_deref(), Some("Key food.groceries"));
    assert_eq!(labels(&item(&shown).actions), [("Hide category", true)]);
    assert!(!shows("rules-delete-category", cx));
    click("rules-category-hide", cx);
    assert!(
        stored_category(&scratch, "food.groceries")
            .unwrap()
            .is_hidden()
    );
    let shown = describe(&shell, cx);
    assert_eq!(labels(&item(&shown).actions), [("Show category", true)]);
    assert!(
        item(&shown)
            .caption
            .as_deref()
            .is_some_and(|caption| caption.ends_with(" · hidden"))
    );
    // The rule editor's category menu leaves it out, unless it's chosen.
    let data = cx.update(|_, cx| rules.read(cx).data().clone());
    let offered = |chosen: Option<&orre_core::category::CategoryKey>| {
        view::category_groups(&data, chosen)
            .iter()
            .flat_map(|group| group.options.iter())
            .any(|option| option.key.as_ref() == "food.groceries")
    };
    assert!(!offered(None));
    assert!(offered(Some(&category_key("food.groceries"))));
    // Travel is hidden as a group: none of it is offered.
    let travel = view::category_groups(&data, None)
        .iter()
        .flat_map(|group| group.options.iter())
        .any(|option| option.key.as_ref().starts_with("travel"));
    assert!(!travel);
    click("rules-category-hide", cx);
    assert!(
        !stored_category(&scratch, "food.groceries")
            .unwrap()
            .is_hidden()
    );

    // Coffee is unused: Delete category… opens the dialog, which deletes it.
    click(&format!("category-{}", seeded.coffee), cx);
    let shown = describe(&shell, cx);
    assert_eq!(
        (item(&shown).figure.as_deref(), shown.selected.as_deref()),
        (Some("0"), Some("Coffee"))
    );
    assert_eq!(
        labels(&item(&shown).actions),
        [("Hide category", true), ("Delete category…", true)]
    );
    click("rules-delete-category", cx);
    let described = describe(&shell, cx).dialog.expect("the dialog opens");
    assert_eq!(described.title, "Delete the category “Coffee”?");
    assert!(
        described
            .body
            .contains("under a new key: food.u_coffee is never used again")
    );
    assert_eq!(
        labels(&described.actions),
        [("Cancel", true), ("Delete category", true)]
    );
    let dialog = cx.update(|_, cx| view.read(cx).delete_category_dialog().cloned());
    let dialog = dialog.expect("the dialog opens");
    cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.submit(cx)));
    redraw(cx);
    assert!(stored_category(&scratch, "food.u_coffee").is_none());
    assert_eq!(describe(&shell, cx).dialog, None, "the dialog closes");
    assert!(matches!(
        cx.update(|_, cx| rules.read(cx).selected_category()),
        Some(CategorySelection::Stored(_))
    ));
}

// describe() --------------------------------------------------------------------------

/// The whole description of a small case, field by field: a new rule with
/// nothing typed, in a view with no database.
#[gpui_kit::test]
fn a_new_rule_in_an_empty_view_is_described_in_full(cx: &mut TestAppContext) {
    let rules = cx.new(|_| Rules::empty());
    rules.update(cx, |rules, cx| rules.new_rule(cx));
    let shown = rules.read_with(cx, |rules, _| rules.describe());
    let row = |label, selected| PanelRowDescription {
        label,
        count: 0,
        selected,
    };
    let key = |label: &str, key, enabled| Action::keyed(label, key, enabled);
    assert_eq!(
        shown,
        RulesDescription {
            panel: super::describe::PanelDescription {
                sections: vec![
                    row("Rules", true),
                    row("Merchant names", false),
                    row("Change history", false),
                    row("Categories", false),
                ],
                filter_title: "Action",
                filters: vec![
                    row("Set category", false),
                    row("Set merchant", false),
                    row("Add tag", false),
                    row("Set note", false),
                    row("Exclude", false),
                    row("Split", false),
                    row("My share", false),
                ],
                cursor: None,
                load_error: None,
            },
            toolbar: ToolbarDescription {
                title: "Apply to past transactions",
                caption: "New rule: not finished · nothing changes until you apply".into(),
                keys: vec![],
                buttons: vec![Action::button("Back to rules", true)],
            },
            refused: None,
            canvas: CanvasDescription::Preview(PreviewDescription {
                will_change: vec![],
                user_set: None,
                message: Some("Finish the rule in the inspector to see what it would change."),
                actions: vec![
                    Action::button("Create rule and change 0", false),
                    Action::button("Create rule only", false),
                ],
            }),
            selected: None,
            inspector: InspectorDescription::Item(Box::new(ItemDescription {
                title: "New rule".into(),
                figure: Some("0".into()),
                caption: Some("transactions match · new rule, not saved yet".into()),
                never_reached: None,
                suggestion: None,
                footer: None,
                actions: vec![],
            })),
            bar: None,
            dialog: None,
            actions: vec![
                key("Reorder", "alt-up", false),
                key("Reorder", "alt-down", false),
                key("Back, or clear the selection", "escape", true),
                key("Move up", "up", false),
                key("Move down", "down", false),
            ],
        }
    );

    // Escape goes back: an empty table, and nothing selected.
    rules.update(cx, |rules, cx| rules.cancel(cx));
    let shown = rules.read_with(cx, |rules, _| rules.describe());
    assert_eq!(
        shown.canvas,
        CanvasDescription::Empty {
            title: "No rules yet",
            caption: "Set a category on a transaction and Orre offers to turn it into a rule \
                      once the merchant comes back. Or start one with New rule.",
        }
    );
    assert_eq!(
        shown.inspector,
        InspectorDescription::Nothing(inspector::NO_RULE)
    );
    assert_eq!(
        shown.toolbar.caption,
        "0 rules · checked top to bottom on every import"
    );
    assert!(shown.toolbar.keys.is_empty(), "no hints over no rules");
    assert!(shown.actions.iter().all(|action| !action.enabled));
}

/// U.7: the keys bound in the Rules canvas's context are exactly
/// [`view::KEYS`], the toolbar hints ↑↓ then ⌥↑↓ (one hint for both), and
/// every key `describe()` lists is bound, so the toolbar, the keymap and
/// `describe()` can't drift.
#[gpui_kit::test]
fn the_toolbar_hints_every_bound_key(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, _, cx) = open(cx, &scratch);
    cx.update(|_, cx| view::KEYS.assert_bound(cx));
    let shown = describe(&shell, cx);
    assert_eq!(shown.toolbar.keys, [("↑↓", "Select"), ("⌥↑↓", "Reorder")]);
    for key in view::KEYS.keys() {
        for stroke in key.keys().iter().chain(key.and_keys()) {
            assert!(
                shown
                    .actions
                    .iter()
                    .any(|action| action.key == Some(*stroke)),
                "{stroke} isn't listed"
            );
        }
        assert_eq!(key.hint().is_none(), key.keys() == ["escape"], "{key:?}");
    }
    let listed: Vec<Option<&str>> = shown.actions.iter().map(|action| action.key).collect();
    assert_eq!(
        listed,
        [
            Some("alt-up"),
            Some("alt-down"),
            Some("escape"),
            Some("up"),
            Some("down")
        ]
    );
}

#[gpui_kit::test]
fn delete_rule_asks_first_and_names_what_stays(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, _, cx) = open(cx, &scratch);
    click("rules-delete", cx);
    let dialog = describe(&shell, cx).dialog.expect("the dialog opens");
    assert_eq!(
        dialog.title,
        "Delete the rule “Merchant contains TFL TRAVEL”?"
    );
    assert!(
        dialog
            .body
            .starts_with("The 2 transactions it categorised keep Transport. "),
        "{}",
        dialog.body
    );
    assert_eq!(
        labels(&dialog.actions),
        [("Cancel", true), ("Delete rule", true)]
    );
    assert_eq!(dialog.error, None);
}

#[gpui_kit::test]
fn an_unloaded_view_says_nothing_until_its_first_read_then_selects_the_first_rule(
    cx: &mut TestAppContext,
) {
    let scratch = Scratch::new();
    // Made, not yet read: no rows, no "No rules yet", nothing selected.
    let unread = Rules::unloaded(RulesStore::new(scratch.path.clone()));
    assert!(!unread.loaded());
    let shown = unread.describe();
    assert_eq!(shown.canvas, CanvasDescription::Rules(Vec::new()));
    assert_eq!(shown.selected, None);

    // The view asks for the read when it is made, and it lands later.
    let (shell, rules, cx) = open_with(cx, &scratch, move |_| unread);
    redraw(cx);
    assert!(cx.update(|_, cx| rules.read(cx).loaded()));
    let shown = describe(&shell, cx);
    assert_eq!(rule_rows(&shown).len(), 14);
    assert_eq!(
        shown.selected.as_deref(),
        Some("Merchant contains TFL TRAVEL"),
        "the first read selects the first rule"
    );
    assert_eq!(item(&shown).title, "Merchant contains TFL TRAVEL");
}

#[gpui_kit::test]
fn a_rule_opened_before_the_first_read_is_selected_when_it_lands(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let third = scratch.seeded.rules[2];
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = RulesStore::new(scratch.path.clone());
    let transactions = TransactionStore::new(scratch.path.clone());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
        let inbox = cx.new(|_| Inbox::empty());
        let settings = cx.new(crate::settings::SettingsView::fixture);
        let mut shell = ShellView::with_all(
            View::Rules,
            accounts,
            transactions::Source::Database(transactions),
            inbox,
            settings,
            window,
            cx,
        );
        let rules = cx.new(|_| Rules::unloaded(store));
        shell.install_rules(rules.clone(), window, cx);
        // "Open rule" from Transactions, before the first read is back.
        rules.update(cx, |rules, cx| {
            assert!(!rules.loaded(), "asked for before the read landed");
            rules.select_rule(Some(third), cx);
        });
        shell
    });
    let rules = cx.update(|_, cx| shell.read(cx).rules().clone());
    redraw(cx);
    assert!(cx.update(|_, cx| rules.read(cx).loaded()));
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_rule_id()),
        Some(third),
        "the rule asked for, not the first row"
    );
}

#[gpui_kit::test]
fn a_created_rule_is_selected_when_its_read_lands(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let db = TransactionStore::new(scratch.path.clone()).open().unwrap();
    let proposal = core_rule::propose(&db, scratch.seeded.vinted)
        .unwrap()
        .expect("VINTED proposes a rule");
    drop(db);
    let start = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            rules.update(cx, |rules, cx| {
                rules.new_rule_from(&proposal.definition, core_rule::RuleOrigin::Edit, cx)
            })
        });
        redraw(cx);
    };

    // "Create rule and change 4": the preview closes at once, so it can't
    // be clicked twice, and nothing is selected until the read is back.
    start(cx);
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.commit(cx)));
    let (preview, new, selected) = cx.update(|_, cx| {
        let rules = rules.read(cx);
        (
            rules.apply().is_some(),
            rules.is_new(),
            rules.selected_rule_id(),
        )
    });
    assert_eq!((preview, new, selected), (false, false, None));
    redraw(cx);
    let shown = describe(&shell, cx);
    assert_eq!(shown.panel.sections[0].count, 15);
    assert_eq!(
        shown.selected.as_deref(),
        Some("Merchant contains VINTED"),
        "selected once the read lands"
    );
    let vinted = cx.update(|_, cx| rules.read(cx).data().rules[0].rule.id());
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_rule_id()),
        Some(vinted)
    );

    // "Create rule only", then another rule chosen before the read is back:
    // that choice stands.
    start(cx);
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit(false, |form| form.text = "DEPOP".into(), cx);
            rules.create_only(cx);
        })
    });
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_rule_id()),
        None,
        "nothing new selected yet"
    );
    let uber = scratch.seeded.rules[1];
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(uber), cx)));
    redraw(cx);
    let shown = describe(&shell, cx);
    assert_eq!(shown.panel.sections[0].count, 16, "DEPOP is stored");
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_rule_id()),
        Some(uber)
    );
}

#[gpui_kit::test]
fn a_created_category_is_selected_when_its_read_lands(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    click("rules-section-categories", cx);
    click("rules-new-category", cx);
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit_category(
                false,
                |form| {
                    form.name = "Coffee beans".into();
                    form.group = Some(category_key("food"));
                },
                cx,
            );
            rules.create_category(cx);
        })
    });
    // Stored, and the form closed at once so it can't be created twice; not
    // selected until the read with it is back.
    let created = stored_category(&scratch, "food.u_coffee_beans").expect("created");
    assert_eq!(cx.update(|_, cx| rules.read(cx).selected_category()), None);
    assert!(cx.update(|_, cx| rules.read(cx).category_form().is_none()));

    redraw(cx);
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_category()),
        Some(CategorySelection::Stored(created.id()))
    );
    assert_eq!(
        describe(&shell, cx).selected.as_deref(),
        Some("Coffee beans")
    );
}

/// A rule added from another process, as `orre categorize` or an approved
/// suggestion would.
fn add_rule_outside(scratch: &Scratch, pattern: &str) {
    use orre_core::rule::{AmountBounds, MatchField, Pattern, RuleAction, RuleDefinition};
    let definition = RuleDefinition::new(
        MatchField::Merchant,
        Pattern::new(MatchKind::Contains, pattern).unwrap(),
        AmountBounds::default(),
        None,
        vec![RuleAction::SetCategory(category_key("transport"))],
    )
    .unwrap();
    core_rule::create(&mut scratch.db(), &definition).unwrap();
}

/// LU.4: a reload nobody asked for, with a rule open and its pattern typed
/// but not yet saved, keeps the section, the selected rule and what was
/// typed, and lists the rule added elsewhere.
#[gpui_kit::test]
fn an_unasked_reload_keeps_the_open_rule_and_its_unsaved_text(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let pret = scratch.seeded.rules[3];
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(pret), cx)));
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).rules_view().clone());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::Pattern, "PRET", window, cx)
        })
    });
    redraw(cx);
    let before = describe(&shell, cx);
    let rows_before = rule_rows(&before).len();

    add_rule_outside(&scratch, "OUTSIDE");
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.reload(cx)));
    redraw(cx);

    let after = describe(&shell, cx);
    assert_eq!(rule_rows(&after).len(), rows_before + 1);
    assert_eq!(after.selected, before.selected);
    assert_eq!(item(&after).title, item(&before).title);
    assert_eq!(item(&after).title, "Merchant contains PRET");
    let text = cx.update(|_, cx| rules.read(cx).form().map(|form| form.text.clone()));
    assert_eq!(text.as_deref(), Some("PRET"), "the reload didn't refill it");
    let stored = core_rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(stored.definition().pattern().as_str(), "PRET A MANGER");
}

/// LU.4: the same for a category being renamed, in its section.
#[gpui_kit::test]
fn an_unasked_reload_keeps_the_section_and_an_unsaved_category_name(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.show(Section::Categories, cx);
            rules.edit_category(false, |form| form.name = "Typed name".into(), cx);
        })
    });
    redraw(cx);
    let before = describe(&shell, cx);

    add_rule_outside(&scratch, "OUTSIDE");
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.reload(cx)));
    redraw(cx);

    assert_eq!(
        cx.update(|_, cx| rules.read(cx).section()),
        Section::Categories
    );
    let name = cx.update(|_, cx| rules.read(cx).category_form().map(|form| form.name.clone()));
    assert_eq!(name.as_deref(), Some("Typed name"));
    assert_eq!(describe(&shell, cx).selected, before.selected);
}

// A plain click on the row already selected keeps it (SLI-70) -----------------

#[gpui_kit::test]
fn clicking_the_preselected_rule_keeps_it_selected(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let first = cx
        .update(|_, cx| rules.read(cx).selected_rule_id())
        .expect("opening Rules selects the first rule");
    let title = item(&describe(&shell, cx)).title.clone();

    click(&format!("rule-{first}"), cx);
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_rule_id()),
        Some(first)
    );
    let shown = describe(&shell, cx);
    assert_eq!(shown.selected.as_deref(), Some(title.as_str()));
    assert_eq!(item(&shown).title, title, "the inspector stays filled");
}

#[gpui_kit::test]
fn clicking_the_preselected_name_keeps_it_selected(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    click("rules-section-names", cx);
    let Some(NameSelection::Stored(first)) = cx.update(|_, cx| rules.read(cx).names.selection())
    else {
        panic!("opening Merchant names selects the first name");
    };

    click(&format!("name-{first}"), cx);
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).names.selection()),
        Some(NameSelection::Stored(first))
    );
    assert_eq!(
        item(&describe(&shell, cx)).title,
        "Amazon Marketplace",
        "the inspector stays filled"
    );
}

#[gpui_kit::test]
fn clicking_the_preselected_change_keeps_it_selected(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    click("rules-section-history", cx);
    let first = cx
        .update(|_, cx| rules.read(cx).history.selected_id())
        .expect("opening History selects the newest change");
    let title = item(&describe(&shell, cx)).title.clone();

    click(&format!("change-{first}"), cx);
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).history.selected_id()),
        Some(first)
    );
    assert_eq!(
        item(&describe(&shell, cx)).title,
        title,
        "the inspector stays filled"
    );
}

#[gpui_kit::test]
fn clicking_the_preselected_category_keeps_it_selected(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    fixture::seed_categories(&mut scratch.db());
    let (shell, rules, cx) = open(cx, &scratch);
    click("rules-section-categories", cx);
    let Some(CategorySelection::Stored(first)) =
        cx.update(|_, cx| rules.read(cx).categories.selection())
    else {
        panic!("opening Categories selects the first category");
    };
    let title = item(&describe(&shell, cx)).title.clone();

    click(&format!("category-{first}"), cx);
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).categories.selection()),
        Some(CategorySelection::Stored(first))
    );
    assert_eq!(
        item(&describe(&shell, cx)).title,
        title,
        "the inspector stays filled"
    );
}
