//! The Rules view (ROADMAP M3.13 and the rules half of M3.7;
//! `docs/DESIGN.md` → "Rules", D6.7; `docs/ARCHITECTURE.md` → "Decision
//! record: rules" and "Decision record: the Rules view (built)").
//!
//! - **Rules** (`B1J-0`): every rule in priority order with what it decides
//!   today, the selected one edited in the inspector (When, Then, Past
//!   transactions). Edits save as they're made, once valid, and never re-run
//!   on history; only Apply changes past transactions.
//! - **Apply to past transactions** (`EFT-0`): a stored rule's or a new
//!   rule's changes previewed on the canvas, "Will change" and "Category you
//!   set" (left out unless "Change these too"), committed as one undoable
//!   change batch. A new rule (New rule, or "Create rule…" from
//!   Transactions) is made here and stored only on "Create rule…".
//! - **Merchant names** (`G2O-0`): the aliases, applied at once with no
//!   batch (the display name is derived).
//! - **Change history** (`GZT-0`): every change batch, each undoable.
//! - **Categories** (M3.7's categories editor; composed from the sections
//!   above, not drawn): every group with its subcategories and what uses
//!   each; renamed, hidden, shown, added and (when nothing uses it) deleted
//!   at once, with no batch, since no transaction changes.
//!
//! [`Rules`] holds what the view shows (from [`store::RulesStore`], which
//! opens the database per call), the open section, one pane per section
//! (`state`: its filter and its selection with the form that selection
//! edits, as one value, kept while another section is open) and the last
//! outcome. Leaving Rules drops a new rule not yet created and closes a
//! stored rule's preview. Every change is core's; this only picks IDs and
//! fields. Rule patterns, names and bank text are untrusted and shown plain
//! ([`text`]).
//!
//! What the view shows is read on a background thread ([`Rules::reload`]):
//! what each rule decides is worked out over the whole ledger, which at
//! 100,000 transactions takes far longer than a frame (decision record
//! "performance at 100,000 transactions", `docs/architecture/app.md`).
//! Until the first read lands the canvas says nothing, as the Inbox and
//! Transactions do. Writes stay on this thread and ask for a read after
//! them; until it lands the last read is shown, with what must show at once
//! done here (`write`): a moved rule takes its place, a deleted row goes, a
//! created rule or category is selected when the read lands, a form edited
//! meanwhile isn't refilled, and the same save isn't made twice.

#[cfg(any(test, feature = "dev-driver"))]
pub(crate) mod describe;
pub mod dialogs;
#[cfg(any(test, feature = "dev-samples"))]
pub mod fixture;
pub mod form;
pub mod inspector;
pub mod panel;
mod state;
pub mod store;
pub mod table;
pub mod text;
pub mod view;
mod write;

use std::collections::BTreeSet;

use gpui_kit::{Context, EventEmitter, SharedString, Task};
use orre_core::category::{self, CategoryKind, CategorySummary, GroupOverview};
use orre_core::change::{self, ChangeKind, HistoryEntry, HistoryItem};
use orre_core::id::{ChangeBatchId, MerchantAliasId, RuleId, TransactionId};
use orre_core::inbox::{Suggestion, SuggestionId, preview as inbox_preview};
use orre_core::merchant::{self, AliasDefinition, AliasKind, AliasSummary};
use orre_core::rule::{
    self, ActionKind, ApplyPreview, Draft, PreviewRow, RuleDefinition, RuleOrigin, RuleSummary,
};

use crate::accounts::store::StoreError;
use crate::shell::live::{Reloads, Wrote};
use crate::ui::list;
use form::{CategoryError, CategoryForm, FormError, NameForm, RuleForm};
use state::{CategoriesPane, CategoryEdit, HistoryPane, NameEdit, NamesPane, RuleEdit, RulesPane};
use store::{RulesData, RulesStore};
use text::Part;
use write::{Policy, Said};

gpui_kit::actions!(
    orre_rules,
    [
        /// ⌥↑ (Alt on Linux and Windows): move the selected rule one place
        /// up (`DESIGN.md` → "Rules" → "Keyboard reorder").
        MoveRuleUp,
        /// ⌥↓: move the selected rule one place down.
        MoveRuleDown,
    ]
);

/// Binds the view's keys ([`view::KEYS`]): ↑↓ move the selection, ⌥↑⌥↓
/// move the selected rule, Escape clears the selection or goes back from
/// the preview; in the panel ([`panel::KEYS`]), ↑↓ move its cursor and
/// Enter or Space act on it. Call once, before the first
/// [`view::RulesView`].
pub fn init(cx: &mut gpui_kit::App) {
    view::KEYS.bind(cx);
    panel::KEYS.bind(cx);
}

/// The lists under RULES in the panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Section {
    #[default]
    Rules,
    Names,
    History,
    Categories,
}

/// What the Categories section's panel filter keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CategoryFilter {
    /// The groups of this kind, with their subcategories.
    Kind(CategoryKind),
    /// The categories pickers leave out: hidden, or in a hidden group.
    Hidden,
}

/// The category selected in Categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CategorySelection {
    /// A stored category, by row ID.
    Stored(i64),
    /// "+ New category", not stored yet.
    New,
}

/// A row of the Categories table.
#[derive(Clone, Copy, Debug)]
pub struct CategoryRow<'a> {
    pub summary: &'a CategorySummary,
    /// Whether it's a subcategory (indented under its group).
    pub child: bool,
    /// Hidden itself, or with its group.
    pub hidden: bool,
}

/// What the view asks its owner (the shell) to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulesEvent {
    /// "Delete rule…": open the delete dialog.
    DeleteRuleRequested(RuleId),
    /// "Delete name…": open the delete dialog.
    DeleteNameRequested(MerchantAliasId),
    /// "Undo this change…": open the undo dialog.
    UndoRequested(ChangeBatchId),
    /// "Review in the Inbox": open the Inbox on the suggestion.
    ReviewSuggestion(SuggestionId),
    /// A change that Transactions, the accounts panel or the checks show
    /// (an apply, an undo, a deleted rule, a merchant name, a category):
    /// what reads them should read again. Editing or moving a rule changes
    /// no transaction and doesn't send it.
    Changed,
}

/// The apply-to-past-transactions preview on the canvas: of the stored
/// rule selected, or of the rule being made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Apply {
    /// Core's plan; `None` while the new rule's fields aren't valid.
    pub preview: Option<ApplyPreview>,
    /// Its rows, newest first.
    pub rows: Vec<PreviewRow>,
    /// A new rule: the transactions it matches, and the rules it overlaps.
    pub matches: usize,
    pub overlaps: Vec<RuleId>,
    /// "Change these too": include the rows the user set.
    pub include: bool,
}

impl Apply {
    /// The rows that change without being included.
    pub fn will_change(&self) -> impl Iterator<Item = &PreviewRow> {
        self.rows.iter().filter(|row| !row.user_set)
    }

    /// The rows the user set, changed only with "Change these too".
    pub fn user_set(&self) -> impl Iterator<Item = &PreviewRow> {
        self.rows.iter().filter(|row| row.user_set)
    }

    /// How many transactions the commit changes.
    pub fn changing(&self) -> usize {
        if self.include {
            self.rows.len()
        } else {
            self.will_change().count()
        }
    }

    fn included(&self) -> BTreeSet<TransactionId> {
        if !self.include {
            return BTreeSet::new();
        }
        self.user_set().map(|row| row.transaction).collect()
    }
}

/// What the last change did, shown until the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It went through: the bar's message, and the batch Undo reverts.
    Done {
        message: SharedString,
        undo: Option<ChangeBatchId>,
    },
    /// A rule moved with ⌥↑⌥↓: the bar names it and where it is now
    /// ([`Rules::moved_parts`]), and Undo moves it back.
    Moved(Moved),
    /// Core refused it; nothing changed.
    Refused(SharedString),
}

/// A rule moved from the keyboard. Presses on the same rule, with nothing
/// else changed between them, are one move: Undo returns the rule to `from`,
/// where the first press found it. No transaction changes, so there's no
/// change batch (as for Delete rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    pub rule: RuleId,
    /// Its place (0 first) before the first press.
    pub from: usize,
}

/// Where the view reads from.
enum Source {
    Database(RulesStore),
    /// Nothing to read (no data folder, or a test without a database).
    None,
}

/// The name selected in Merchant names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameSelection {
    Stored(MerchantAliasId),
    /// "+ New name", not stored yet.
    New,
}

/// What the inspector's fields are filled from ([`Rules::form_key`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKey {
    None,
    Rule(RuleId),
    NewRule,
    Name(NameSelection),
    Category(CategorySelection),
}

/// The Rules view's state (module docs).
pub struct Rules {
    source: Source,
    data: RulesData,
    section: Section,
    rules: RulesPane,
    names: NamesPane,
    history: HistoryPane,
    categories: CategoriesPane,
    outcome: Option<Outcome>,
    /// The rules' order right after the last keyboard move, while its bar
    /// is up: a reload that finds another order (a rule added or moved
    /// elsewhere) drops the bar, so Undo can't put the rule back at a stale
    /// place.
    moved_order: Option<Vec<RuleId>>,
    load_error: Option<SharedString>,
    /// Whether a read has come back, or there is nothing to read.
    loaded: bool,
    /// The background read in flight; a newer one replaces it.
    loading: Option<Task<()>>,
    /// Who reads everything again after the view's own write.
    reloads: Reloads,
    /// Bumped on every change to an inspector form: a read refills a form
    /// only if this hasn't moved since the read was asked for.
    form_edits: u64,
    /// `form_edits` when the first read not yet landed was asked for: a
    /// read that replaces it stands in for it, so compares with this.
    asked_edits: Option<u64>,
    /// What was saved since the read in flight was asked for: a save of the
    /// same again is skipped until the read lands with it.
    written: Vec<Written>,
    /// A rule just created, selected when the read with it lands unless
    /// another rule is selected first.
    pending_rule: Option<RuleId>,
    /// A category just created, likewise.
    pending_category: Option<i64>,
}

/// An item as last saved, while the read that will show it is pending.
#[derive(Clone, Debug, PartialEq)]
enum Written {
    Rule(RuleId, RuleDefinition),
    Name(MerchantAliasId, AliasDefinition),
    Category(i64, String),
}

impl Written {
    fn same_item(&self, other: &Written) -> bool {
        match (self, other) {
            (Written::Rule(a, _), Written::Rule(b, _)) => a == b,
            (Written::Name(a, _), Written::Name(b, _)) => a == b,
            (Written::Category(a, _), Written::Category(b, _)) => a == b,
            _ => false,
        }
    }
}

impl EventEmitter<RulesEvent> for Rules {}
impl EventEmitter<Wrote> for Rules {}

fn message(error: impl std::fmt::Display) -> SharedString {
    SharedString::from(text::sentence(&error.to_string()))
}

impl Rules {
    /// The view over `store`, read now, on this thread, with the first row
    /// selected: for tests and the snapshot. The app uses
    /// [`Rules::unloaded`].
    pub fn from_store(store: RulesStore) -> Self {
        let mut rules = Self::unloaded(store);
        if let Source::Database(store) = &rules.source {
            let result = store.load();
            rules.land(result, rules.form_edits);
        }
        rules
    }

    /// The view over `store`, not read yet: [`view::RulesView::new`] starts
    /// the first read, on a background thread.
    pub fn unloaded(store: RulesStore) -> Self {
        Self {
            loaded: false,
            ..Self::with_source(Source::Database(store))
        }
    }

    /// Nothing to show, because nothing can be read: `reason` says why.
    pub fn unavailable(reason: impl Into<SharedString>) -> Self {
        Self {
            load_error: Some(reason.into()),
            ..Self::with_source(Source::None)
        }
    }

    /// An empty view that reads nothing (tests).
    pub fn empty() -> Self {
        Self::with_source(Source::None)
    }

    fn with_source(source: Source) -> Self {
        Self {
            source,
            data: RulesData::default(),
            section: Section::Rules,
            rules: RulesPane::default(),
            names: NamesPane::default(),
            history: HistoryPane::default(),
            categories: CategoriesPane::default(),
            outcome: None,
            moved_order: None,
            load_error: None,
            loaded: true,
            loading: None,
            reloads: Reloads::Itself,
            form_edits: 0,
            asked_edits: None,
            written: Vec::new(),
            pending_rule: None,
            pending_category: None,
        }
    }

    /// Whether the data has been read (or there is none to read): until
    /// then an empty table means "not yet", not "nothing here".
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn data(&self) -> &RulesData {
        &self.data
    }

    pub fn today(&self) -> chrono::NaiveDate {
        self.data.today.unwrap_or_else(crate::today)
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    pub fn action_filter(&self) -> Option<ActionKind> {
        self.rules.filter
    }

    pub fn match_filter(&self) -> Option<AliasKind> {
        self.names.filter
    }

    pub fn kind_filter(&self) -> Option<ChangeKind> {
        self.history.filter
    }

    pub fn category_filter(&self) -> Option<CategoryFilter> {
        self.categories.filter
    }

    /// The preview on the canvas, of the selected rule or the new one.
    pub fn apply(&self) -> Option<&Apply> {
        self.rules.editing.as_ref()?.apply()
    }

    /// Pending agent rules over the selected rule's rows.
    pub fn touching(&self) -> &[Suggestion] {
        match &self.rules.editing {
            Some(RuleEdit::Stored { touching, .. }) => touching,
            _ => &[],
        }
    }

    // Reading ------------------------------------------------------------------

    /// Reads everything again, on a background thread, keeping the
    /// selection where it still is: the one way the view reads again, after
    /// every save and when the shell asks. What is shown stays until the
    /// read lands; a read asked for meanwhile replaces this one, so only the
    /// newest is applied. Until then a moved or deleted row has already
    /// moved or gone (the local steps after those writes), a save of what
    /// was just saved is skipped, and a form changed since the read was
    /// asked for keeps what was typed when it lands.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        use gpui_kit::AppContext as _;

        let Source::Database(store) = &self.source else {
            cx.notify();
            return;
        };
        let store = store.clone();
        let edits = *self.asked_edits.get_or_insert(self.form_edits);
        self.loading = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { store.load() }).await;
            this.update(cx, |this, cx| {
                this.loading = None;
                this.land(result, edits);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Starts the first read, if it hasn't been asked for.
    pub(crate) fn start_loading(&mut self, cx: &mut Context<Self>) {
        if !self.loaded && self.loading.is_none() {
            self.reload(cx);
        }
    }

    /// After the view's own write, the shell reads everything again, with
    /// every other view of what it changed ([`crate::shell::live`]).
    pub fn reload_through_shell(&mut self) {
        self.reloads = Reloads::ByShell;
    }

    /// Takes a read: its data, or the old data kept with the error. A rule
    /// or category just created is selected, unless another was selected
    /// meanwhile; the first read selects the open section's first row; the
    /// move bar goes if the order moved elsewhere; the selections are
    /// brought up to date. `edits` is [`Rules::form_edits`] when the read
    /// (or the first one it replaced) was asked for.
    fn land(&mut self, result: Result<RulesData, StoreError>, edits: u64) {
        let first = !self.loaded;
        // Typed and not saved, as of the data the form was filled from: a
        // read nobody asked for (another process changed the database)
        // doesn't overwrite it, whenever the typing was.
        let unsaved_rule = self.rule_unsaved();
        let unsaved_category = self.category_unsaved();
        self.loaded = true;
        self.asked_edits = None;
        self.written.clear();
        match result {
            Ok(data) => {
                self.data = data;
                self.load_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the rules");
                self.load_error = Some(message(error));
            }
        }
        if let Some(id) = self.pending_rule.take()
            && self.rules.editing.is_none()
        {
            self.open_rule(Some(id));
        }
        if let Some(id) = self.pending_category.take()
            && self.categories.editing.is_none()
        {
            self.open_category(Some(CategorySelection::Stored(id)));
        }
        if first {
            self.select_default();
        }
        if let Some(order) = self.moved_order.take()
            && matches!(self.outcome, Some(Outcome::Moved(_)))
        {
            if order == self.rule_order() {
                self.moved_order = Some(order);
            } else {
                self.outcome = None;
            }
        }
        let untouched = self.form_edits == edits;
        self.reload_rule(untouched && !unsaved_rule);
        self.reload_name();
        self.reload_batch();
        self.reload_category(untouched && !unsaved_category);
    }

    /// Whether the selected stored rule's form holds valid fields that
    /// aren't what is stored (as far as the view knows).
    fn rule_unsaved(&self) -> bool {
        let Some(RuleEdit::Stored { id, form, .. }) = &self.rules.editing else {
            return false;
        };
        form.definition(self.data.currency(form.account))
            .is_ok_and(|definition| !self.is_stored(&Written::Rule(*id, definition)))
    }

    /// Whether the selected stored category's form holds a valid name that
    /// isn't the stored one (as far as the view knows).
    fn category_unsaved(&self) -> bool {
        let Some(CategoryEdit::Stored { id, form }) = &self.categories.editing else {
            return false;
        };
        let label = form.label(self.category_siblings(), Some(*id));
        label.is_ok_and(|label| !self.is_stored(&Written::Category(*id, label)))
    }

    /// Records that `item` was saved, for the read in flight: a save of the
    /// same again is skipped until it lands.
    fn wrote(&mut self, item: Written) {
        self.written.retain(|written| !written.same_item(&item));
        self.written.push(item);
    }

    /// Whether `item` is what is stored, as far as the view knows: as last
    /// saved while a read is pending, else as last read.
    fn is_stored(&self, item: &Written) -> bool {
        if let Some(written) = self.written.iter().find(|w| w.same_item(item)) {
            return written == item;
        }
        match item {
            Written::Rule(id, definition) => self
                .data
                .rule(*id)
                .is_some_and(|summary| summary.rule.definition() == definition),
            Written::Name(id, definition) => self
                .data
                .name(*id)
                .is_some_and(|summary| summary.alias.definition() == definition),
            Written::Category(id, label) => self
                .data
                .category_entry(*id)
                .is_some_and(|(summary, _)| summary.category.label() == label),
        }
    }

    /// The selected rule after a read: gone with the rule, and filled again
    /// from the stored rule unless what was typed is invalid or already the
    /// stored one, or the form changed since the read was asked for
    /// (`untouched` false), so an edit in progress isn't overwritten.
    fn reload_rule(&mut self, untouched: bool) {
        let Some(RuleEdit::Stored { id, form, .. }) = &self.rules.editing else {
            return;
        };
        let Some(summary) = self.data.rule(*id) else {
            self.rules.editing = None;
            return;
        };
        let typed = form.definition(self.data.currency(form.account));
        if untouched && matches!(&typed, Ok(definition) if definition != summary.rule.definition())
        {
            let fresh = RuleForm::of(summary.rule.definition());
            if let Some(RuleEdit::Stored { form, .. }) = &mut self.rules.editing {
                *form = fresh;
            }
        }
    }

    fn reload_name(&mut self) {
        if let Some(NameEdit::Stored { id, .. }) = &self.names.editing
            && self.data.name(*id).is_none()
        {
            self.names.editing = None;
        }
    }

    fn reload_batch(&mut self) {
        let Some(batch) = self.history.selected_id() else {
            return;
        };
        self.history.selected = self
            .data
            .entry(batch)
            .is_some()
            .then(|| (batch, self.load_items(batch)));
    }

    /// The selected category after a read: gone with the category, and
    /// filled again unless a rename that isn't valid yet is in the field or
    /// the form changed since the read was asked for.
    fn reload_category(&mut self, untouched: bool) {
        let Some(CategoryEdit::Stored { id, .. }) = &self.categories.editing else {
            return;
        };
        let id = *id;
        match self.data.category_entry(id) {
            Some((summary, _)) if untouched && self.category_error().is_none() => {
                self.categories.editing = Some(CategoryEdit::Stored {
                    id,
                    form: CategoryForm::of(&summary.category),
                });
            }
            Some(_) => {}
            None => self.categories.editing = None,
        }
    }

    /// Selects the first row of the open section when nothing is.
    fn select_default(&mut self) {
        match self.section {
            Section::Rules if self.rules.editing.is_none() => {
                let first = self.rules().first().map(|summary| summary.rule.id());
                self.open_rule(first);
            }
            Section::Names if self.names.editing.is_none() => {
                let first = self.names().first().map(|name| name.alias.id());
                self.open_name(first.map(NameSelection::Stored));
            }
            Section::History if self.history.selected.is_none() => {
                let first = self.history().first().map(|entry| entry.batch.id);
                self.open_batch(first);
            }
            Section::Categories if self.categories.editing.is_none() => {
                let first = self
                    .category_rows()
                    .first()
                    .map(|row| row.summary.category.id());
                self.open_category(first.map(CategorySelection::Stored));
            }
            _ => {}
        }
    }

    // Sections and filters -----------------------------------------------------

    /// Opens `section`: the one way sections change. Leaving Rules drops a
    /// new rule and closes a preview (`RulesPane::leave`); an actual switch
    /// clears the outcome and selects the section's first row if it has none.
    pub fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section != section {
            self.enter(section);
            self.outcome = None;
            self.select_default();
        }
        cx.notify();
    }

    /// Makes `section` the open one, leaving Rules as `RulesPane::leave`
    /// says. Selecting or making a row in another section goes through it too.
    fn enter(&mut self, section: Section) {
        if self.section == Section::Rules && section != Section::Rules {
            self.rules.leave();
        }
        self.section = section;
    }

    /// Toggles the ACTION filter and opens Rules as [`Rules::show`] does,
    /// after filtering: a section with nothing selected selects the first
    /// row the filter keeps.
    pub fn toggle_action_filter(&mut self, kind: ActionKind, cx: &mut Context<Self>) {
        let filter = &mut self.rules.filter;
        *filter = (*filter != Some(kind)).then_some(kind);
        self.show(Section::Rules, cx);
    }

    pub fn toggle_match_filter(&mut self, kind: AliasKind, cx: &mut Context<Self>) {
        let filter = &mut self.names.filter;
        *filter = (*filter != Some(kind)).then_some(kind);
        self.show(Section::Names, cx);
    }

    pub fn toggle_kind_filter(&mut self, kind: ChangeKind, cx: &mut Context<Self>) {
        let filter = &mut self.history.filter;
        *filter = (*filter != Some(kind)).then_some(kind);
        self.show(Section::History, cx);
    }

    pub fn toggle_category_filter(&mut self, filter: CategoryFilter, cx: &mut Context<Self>) {
        let open = &mut self.categories.filter;
        *open = (*open != Some(filter)).then_some(filter);
        self.show(Section::Categories, cx);
    }

    /// Every category as the table lists it, each group followed by its
    /// subcategories, before the filter.
    fn all_category_rows(&self) -> Vec<CategoryRow<'_>> {
        let mut rows = Vec::new();
        for group in &self.data.category_overview {
            let group_hidden = group.group.category.is_hidden();
            rows.push(CategoryRow {
                summary: &group.group,
                child: false,
                hidden: group_hidden,
            });
            rows.extend(group.children.iter().map(|child| CategoryRow {
                summary: child,
                child: true,
                hidden: group_hidden || child.category.is_hidden(),
            }));
        }
        rows
    }

    fn keeps(filter: CategoryFilter, row: &CategoryRow<'_>) -> bool {
        match filter {
            CategoryFilter::Kind(kind) => row.summary.category.kind() == kind,
            CategoryFilter::Hidden => row.hidden,
        }
    }

    /// The categories the table lists, filtered by KIND or Hidden.
    pub fn category_rows(&self) -> Vec<CategoryRow<'_>> {
        let filter = self.categories.filter;
        self.all_category_rows()
            .into_iter()
            .filter(|row| filter.is_none_or(|filter| Self::keeps(filter, row)))
            .collect()
    }

    /// How many categories `filter` keeps (the panel's counts).
    pub fn count_categories(&self, filter: CategoryFilter) -> usize {
        self.all_category_rows()
            .iter()
            .filter(|row| Self::keeps(filter, row))
            .count()
    }

    /// The rules the table lists, filtered by ACTION.
    pub fn rules(&self) -> Vec<&RuleSummary> {
        self.data
            .rules
            .iter()
            .filter(|summary| {
                self.rules
                    .filter
                    .is_none_or(|kind| has_action(summary, kind))
            })
            .collect()
    }

    /// The names the table lists, filtered by MATCH.
    pub fn names(&self) -> Vec<&AliasSummary> {
        self.data
            .names
            .iter()
            .filter(|name| self.names.filter.is_none_or(|kind| is_match(name, kind)))
            .collect()
    }

    /// The changes the table lists, filtered by KIND.
    pub fn history(&self) -> Vec<&HistoryEntry> {
        self.data
            .history
            .iter()
            .filter(|entry| self.history.filter.is_none_or(|kind| is_kind(entry, kind)))
            .collect()
    }

    /// Rules with an action of `kind` (the ACTION counts).
    pub fn count_with_action(&self, kind: ActionKind) -> usize {
        self.data
            .rules
            .iter()
            .filter(|summary| has_action(summary, kind))
            .count()
    }

    /// Names that match as `kind` (the MATCH counts).
    pub fn count_names(&self, kind: AliasKind) -> usize {
        self.data
            .names
            .iter()
            .filter(|name| is_match(name, kind))
            .count()
    }

    /// Changes of `kind` (the KIND counts).
    pub fn count_history(&self, kind: ChangeKind) -> usize {
        self.data
            .history
            .iter()
            .filter(|entry| is_kind(entry, kind))
            .count()
    }

    // Rules ----------------------------------------------------------------------

    pub fn selected_rule(&self) -> Option<&RuleSummary> {
        self.data.rule(self.selected_rule_id()?)
    }

    /// The selected stored rule; `None` for the new rule.
    pub fn selected_rule_id(&self) -> Option<RuleId> {
        self.rules.editing.as_ref()?.stored_id()
    }

    /// The fields in the inspector: the selected rule's or the new rule's.
    pub fn form(&self) -> Option<&RuleForm> {
        self.rules.editing.as_ref().map(RuleEdit::form)
    }

    /// Why the inspector's rule can't be saved yet, if it can't.
    pub fn form_error(&self) -> Option<FormError> {
        let form = self.form()?;
        form.definition(self.data.currency(form.account)).err()
    }

    /// What the inspector's fields are for: the view fills them again when
    /// it changes, and ignores a late event from the fields of another.
    pub fn form_key(&self) -> FormKey {
        match self.section {
            Section::Rules => match &self.rules.editing {
                Some(RuleEdit::Stored { id, .. }) => FormKey::Rule(*id),
                Some(RuleEdit::New { .. }) => FormKey::NewRule,
                None => FormKey::None,
            },
            Section::Names => self.names.selection().map_or(FormKey::None, FormKey::Name),
            Section::History => FormKey::None,
            Section::Categories => self
                .categories
                .selection()
                .map_or(FormKey::None, FormKey::Category),
        }
    }

    /// Whether the inspector shows the new rule.
    pub fn is_new(&self) -> bool {
        self.rules.editing.as_ref().is_some_and(RuleEdit::is_new)
    }

    /// The new rule's place in the list; 0 when no new rule is being made.
    pub fn new_position(&self) -> usize {
        match &self.rules.editing {
            Some(RuleEdit::New { position, .. }) => *position,
            _ => 0,
        }
    }

    pub fn new_origin(&self) -> RuleOrigin {
        match &self.rules.editing {
            Some(RuleEdit::New { origin, .. }) => *origin,
            _ => RuleOrigin::New,
        }
    }

    /// Opens rule `id` for editing, or nothing.
    fn open_rule(&mut self, id: Option<RuleId>) {
        self.rules.editing = id.and_then(|id| {
            let summary = self.data.rule(id)?;
            // As last saved while its read is pending, else as read: a form
            // filled from the older read would save the old rule back.
            let definition = self
                .written
                .iter()
                .find_map(|written| match written {
                    Written::Rule(rule, definition) if *rule == id => Some(definition),
                    _ => None,
                })
                .unwrap_or(summary.rule.definition());
            Some(RuleEdit::Stored {
                id,
                form: RuleForm::of(definition),
                touching: self.touching_rule(id),
                apply: None,
            })
        });
    }

    /// Pending agent rule suggestions that would take over the rows `rule`
    /// filed (the inspector's sparkle line).
    fn touching_rule(&self, rule: RuleId) -> Vec<Suggestion> {
        if matches!(self.source, Source::None) {
            return Vec::new();
        }
        self.query(|db| inbox_preview::touching_rule(db, rule))
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "couldn't read the suggestions over a rule");
                Vec::new()
            })
    }

    /// Selects rule `id` in the table (from the preview too: back to it).
    pub fn select_rule(&mut self, id: Option<RuleId>, cx: &mut Context<Self>) {
        // What was typed for the rule being left is kept, as leaving its
        // field would: the field's blur arrives after the selection moved.
        if self.selected_rule_id() != id && self.section == Section::Rules {
            self.save_rule(cx);
        }
        // Chosen over a rule just created that the read hasn't brought yet.
        self.pending_rule = None;
        self.enter(Section::Rules);
        match &mut self.rules.editing {
            Some(RuleEdit::Stored {
                id: open, apply, ..
            }) if Some(*open) == id => *apply = None,
            _ => self.open_rule(id),
        }
        // A rule the view hasn't read yet ("Open rule" from Transactions just
        // after start-up, or on a rule an Inbox approval made, while the read
        // the shell asked for is in flight or, after a save, still to start)
        // is selected when the read lands, instead of nothing or the first
        // row.
        let read_owed = self.loading.is_some() || self.asked_edits.is_some();
        if self.rules.editing.is_none() && (!self.loaded || read_owed) {
            self.pending_rule = id;
        }
        cx.notify();
    }

    /// What a click on a rule's row does: selects `id`, and keeps it
    /// selected if it's selected already (Escape clears the selection).
    pub fn toggle_rule(&mut self, id: RuleId, cx: &mut Context<Self>) {
        self.select_rule(Some(id), cx);
    }

    /// Moves the selection in the open section (↑↓, [`list::step`]).
    pub fn select_next(&mut self, to: list::Move, cx: &mut Context<Self>) {
        match self.section {
            Section::Rules if self.apply().is_none() => {
                let order: Vec<RuleId> = self.rules().iter().map(|s| s.rule.id()).collect();
                if let Some(next) = list::step(&order, self.selected_rule_id().as_ref(), to) {
                    self.select_rule(Some(next), cx);
                }
            }
            Section::Rules => {}
            Section::Names => {
                let order: Vec<MerchantAliasId> =
                    self.names().iter().map(|n| n.alias.id()).collect();
                let current = match self.names.selection() {
                    Some(NameSelection::Stored(id)) => Some(id),
                    _ => None,
                };
                if let Some(next) = list::step(&order, current.as_ref(), to) {
                    self.select_name(Some(NameSelection::Stored(next)), cx);
                }
            }
            Section::History => {
                let order: Vec<ChangeBatchId> = self.history().iter().map(|e| e.batch.id).collect();
                if let Some(next) = list::step(&order, self.history.selected_id().as_ref(), to) {
                    self.select_batch(Some(next), cx);
                }
            }
            Section::Categories => {
                let order: Vec<i64> = self
                    .category_rows()
                    .iter()
                    .map(|row| row.summary.category.id())
                    .collect();
                let current = match self.categories.selection() {
                    Some(CategorySelection::Stored(id)) => Some(id),
                    _ => None,
                };
                if let Some(next) = list::step(&order, current.as_ref(), to) {
                    self.select_category(Some(CategorySelection::Stored(next)), cx);
                }
            }
        }
    }

    /// Escape: back from the preview, else clears the selection.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.apply().is_some() {
            self.back(cx);
            return;
        }
        match self.section {
            Section::Rules => self.select_rule(None, cx),
            Section::Names => self.select_name(None, cx),
            Section::History => self.select_batch(None, cx),
            Section::Categories => self.select_category(None, cx),
        }
    }

    /// Changes the inspector's fields. A new rule's preview follows at
    /// once; a stored rule is saved when `save` and the fields are valid
    /// (text fields save on blur and Enter, selects at once).
    pub fn edit(&mut self, save: bool, change: impl FnOnce(&mut RuleForm), cx: &mut Context<Self>) {
        let Some(edit) = self.rules.editing.as_mut() else {
            return;
        };
        change(edit.form_mut());
        self.form_edits += 1;
        if edit.is_new() {
            self.refresh_preview();
        } else if save {
            self.save_rule(cx);
        }
        cx.notify();
    }

    /// Saves the selected rule's fields when they're valid and changed.
    pub fn save_rule(&mut self, cx: &mut Context<Self>) {
        let Some(RuleEdit::Stored { id, form, .. }) = &self.rules.editing else {
            return;
        };
        let id = *id;
        let Ok(definition) = form.definition(self.data.currency(form.account)) else {
            return;
        };
        let saved = Written::Rule(id, definition.clone());
        if self.is_stored(&saved) {
            return;
        }
        let policy = Policy::new(Said::Toolbar);
        if self
            .write(policy, cx, |db| rule::update(db, id, &definition))
            .is_ok()
        {
            self.wrote(saved);
            self.refresh_preview();
            let fresh = self.touching_rule(id);
            if let Some(RuleEdit::Stored {
                id: open, touching, ..
            }) = &mut self.rules.editing
                && *open == id
            {
                *touching = fresh;
            }
        }
    }

    /// Moves rule `id` to `position` (dragging a row, Position, "Move
    /// above rule 2").
    pub fn move_rule(&mut self, id: RuleId, position: usize, cx: &mut Context<Self>) {
        let _ = self.move_to(id, position, cx);
    }

    /// [`Rules::move_rule`], for ⌥↑⌥↓ and their Undo too: a refusal is said
    /// under the toolbar, and `Err` says only that nothing moved.
    fn move_to(
        &mut self,
        id: RuleId,
        position: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        if self.data.position(id) == Some(position) {
            return Ok(());
        }
        // What was typed for the selected rule is saved first, as leaving
        // its field would: reading again after the move would replace it.
        self.save_rule(cx);
        // The move takes the place of the rule shown at `position`, found in
        // the database at the write: until a read lands the shown order can
        // lack a rule made elsewhere (an Inbox approval, a create), so the
        // shown index alone could be one off.
        let target = self
            .data
            .rules
            .get(position)
            .map(|summary| summary.rule.id())
            .filter(|target| *target != id);
        let policy = Policy::new(Said::Toolbar);
        self.write(policy, cx, |db| {
            let to = match target {
                Some(target) => rule::list(db)?
                    .iter()
                    .position(|stored| stored.id() == target)
                    .unwrap_or(position),
                None => position,
            };
            rule::move_to(db, id, to)
        })?;
        // The table shows the new order at once, as core made it; what each
        // rule decides follows with the read.
        if let Some(from) = self.data.position(id) {
            let summary = self.data.rules.remove(from);
            let to = position.min(self.data.rules.len());
            self.data.rules.insert(to, summary);
        }
        Ok(())
    }

    /// ⌥↑ (`down` false) or ⌥↓ (`down` true): moves the selected stored
    /// rule one place among the rows the table shows. With a filter on, it
    /// moves past the rows the filter hides to just above (or below) the
    /// next row shown. At either end nothing happens. Through the drop's
    /// [`Rules::move_rule`] path, so a refusal is said as for a drop; once it
    /// moves, the undo bar says where it went ([`Outcome::Moved`]).
    pub fn reorder_selected(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.section != Section::Rules || self.apply().is_some() {
            return;
        }
        let Some(id) = self.selected_rule_id() else {
            return;
        };
        let shown: Vec<RuleId> = self.rules().iter().map(|s| s.rule.id()).collect();
        let Some(at) = shown.iter().position(|rule| *rule == id) else {
            return;
        };
        let next = if down {
            shown.get(at + 1)
        } else {
            at.checked_sub(1).and_then(|before| shown.get(before))
        };
        // Taking the next shown row's place puts the rule just past it,
        // whichever way it moves.
        let (Some(from), Some(to)) = (
            self.data.position(id),
            next.and_then(|next| self.data.position(*next)),
        ) else {
            return;
        };
        // Presses on the same rule add up to one move, until anything else
        // changes (which takes the bar away).
        let from = match self.outcome {
            Some(Outcome::Moved(moved)) if moved.rule == id => moved.from,
            _ => from,
        };
        if self.move_to(id, to, cx).is_ok() {
            self.outcome = Some(Outcome::Moved(Moved { rule: id, from }));
            self.moved_order = Some(self.rule_order());
            cx.notify();
        }
    }

    /// Every stored rule, in order.
    fn rule_order(&self) -> Vec<RuleId> {
        self.data
            .rules
            .iter()
            .map(|summary| summary.rule.id())
            .collect()
    }

    /// The undo bar's line for a keyboard move: "Moved “Merchant contains
    /// `UBER TRIP`” to rule 2 of 14", the rule as its inspector title names
    /// it, the text to match in mono. `None` unless a move is the outcome
    /// and the rule is still there.
    pub fn moved_parts(&self) -> Option<Vec<Part>> {
        let Some(Outcome::Moved(moved)) = self.outcome else {
            return None;
        };
        let summary = self.data.rule(moved.rule)?;
        let position = self.data.position(moved.rule)? + 1;
        let rule = summary.rule.definition();
        Some(vec![
            Part::text(format!("Moved “{} ", text::condition(rule))),
            Part::mono(text::pattern(rule)),
            Part::text(format!("” to rule {position} of {}", self.data.rules.len())),
        ])
    }

    /// The new rule's Position.
    pub fn set_new_position(&mut self, position: usize, cx: &mut Context<Self>) {
        let last = self.data.rules.len();
        if let Some(RuleEdit::New { position: open, .. }) = &mut self.rules.editing {
            *open = position.min(last);
        }
        self.refresh_preview();
        cx.notify();
    }

    /// "+ New rule": the preview with an empty rule (composed: `DESIGN.md`
    /// → "Rules" → "Built in M3.13").
    pub fn new_rule(&mut self, cx: &mut Context<Self>) {
        self.start_new(RuleForm::default(), RuleOrigin::New, cx);
    }

    /// "Create rule…" from Transactions: the preview with `rule` filled in.
    pub fn new_rule_from(
        &mut self,
        rule: &RuleDefinition,
        origin: RuleOrigin,
        cx: &mut Context<Self>,
    ) {
        self.reload(cx);
        self.start_new(RuleForm::of(rule), origin, cx);
    }

    fn start_new(&mut self, form: RuleForm, origin: RuleOrigin, cx: &mut Context<Self>) {
        self.enter(Section::Rules);
        self.outcome = None;
        self.rules.editing = Some(RuleEdit::New {
            form,
            position: 0,
            origin,
            apply: Apply::default(),
        });
        self.refresh_preview();
        cx.notify();
    }

    /// "Apply to past transactions…": the selected rule's preview.
    pub fn apply_selected(&mut self, cx: &mut Context<Self>) {
        let Some(RuleEdit::Stored { apply, .. }) = &mut self.rules.editing else {
            return;
        };
        *apply = Some(Apply::default());
        self.outcome = None;
        self.refresh_preview();
        cx.notify();
    }

    /// The back button: from the preview to the table. A new rule is
    /// dropped and the first row selected.
    pub fn back(&mut self, cx: &mut Context<Self>) {
        match &mut self.rules.editing {
            Some(RuleEdit::New { .. }) => {
                self.rules.editing = None;
                self.select_default();
            }
            Some(RuleEdit::Stored { apply, .. }) => *apply = None,
            None => {}
        }
        cx.notify();
    }

    /// "Change these too".
    pub fn set_include(&mut self, include: bool, cx: &mut Context<Self>) {
        if let Some(apply) = self.rules.editing.as_mut().and_then(RuleEdit::apply_mut) {
            apply.include = include;
        }
        cx.notify();
    }

    /// Plans the open preview again for the current fields.
    fn refresh_preview(&mut self) {
        let Some(edit) = &self.rules.editing else {
            return;
        };
        let Some(include) = edit.apply().map(|apply| apply.include) else {
            return;
        };
        // With no database there is nothing to preview, and nothing to say.
        if matches!(self.source, Source::None) {
            return;
        }
        let mut next = Apply {
            include,
            ..Apply::default()
        };
        let result = match edit {
            RuleEdit::Stored { id, .. } => {
                let id = *id;
                self.query(|db| {
                    let preview = rule::preview_apply(db, id)?;
                    let rows = rule::preview_rows(db, &preview)?;
                    Ok::<_, rule::ApplyError>((preview, rows))
                })
                .map(|(preview, rows)| {
                    next.preview = Some(preview);
                    next.rows = rows;
                })
            }
            RuleEdit::New { .. } => match edit.draft(&self.data) {
                Some(draft) => self
                    .query(|db| {
                        let preview = rule::preview_draft(db, &draft)?;
                        let rows = rule::preview_rows(db, &preview.preview)?;
                        Ok::<_, rule::ApplyError>((preview, rows))
                    })
                    .map(|(preview, rows)| {
                        next.matches = preview.matches;
                        next.overlaps = preview.overlaps;
                        next.preview = Some(preview.preview);
                        next.rows = rows;
                    }),
                // Not valid yet: nothing to preview.
                None => Ok(()),
            },
        };
        if let Err(error) = result {
            self.outcome = Some(Outcome::Refused(error));
        }
        if let Some(apply) = self.rules.editing.as_mut().and_then(RuleEdit::apply_mut) {
            *apply = next;
        }
    }

    /// "Create rule and change 4" or "Apply to 4 transactions": applies the
    /// preview as one change batch (storing the new rule first).
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        self.finish(true, cx);
    }

    /// "Create rule only": stores the new rule without touching history.
    pub fn create_only(&mut self, cx: &mut Context<Self>) {
        self.finish(false, cx);
    }

    fn finish(&mut self, apply_it: bool, cx: &mut Context<Self>) {
        /// What the preview applies.
        enum Target {
            Stored(RuleId),
            New(Draft),
        }
        let Some(edit) = &self.rules.editing else {
            return;
        };
        let Some(apply) = edit.apply().cloned() else {
            return;
        };
        let target = match edit {
            RuleEdit::Stored { id, .. } => Target::Stored(*id),
            RuleEdit::New { .. } => {
                let Some(draft) = edit.draft(&self.data) else {
                    return;
                };
                Target::New(draft)
            }
        };
        let changed_message = |count: usize, category: Option<String>| -> SharedString {
            let rows = text::count(count, "transaction", "transactions");
            match category {
                Some(label) => format!("Changed {rows} to {label}"),
                None => format!("Changed {rows}"),
            }
            .into()
        };
        let category =
            edit.form()
                .action(ActionKind::SetCategory)
                .and_then(|action| match action {
                    form::ActionForm::Category(Some(key)) => {
                        self.data.category(key).map(|option| option.label.clone())
                    }
                    _ => None,
                });
        let included = apply.included();
        let creating = matches!(target, Target::New(_));
        let result = match target {
            Target::Stored(id) => {
                let Some(preview) = apply.preview.as_ref() else {
                    return;
                };
                let policy = Policy::new(Said::Toolbar).tells_shell_when(changed_transactions);
                self.write(policy, cx, |db| {
                    rule::commit_apply(db, preview, &included)
                        .map(|applied| (Some(id), Some(applied)))
                })
            }
            Target::New(draft) => {
                let preview = apply.preview.as_ref().filter(|_| apply_it);
                let policy = Policy::new(Said::Toolbar).tells_shell_when(changed_transactions);
                self.write(policy, cx, |db| {
                    rule::save_draft(db, &draft, preview.map(|preview| (preview, &included)))
                        .map(|saved| (Some(saved.rule), saved.applied))
                })
            }
        };
        if let Ok((rule, applied)) = result {
            if creating {
                // The new rule is in the read the save asked for: the
                // preview closes now, so it can't be created twice, and the
                // rule is selected when the read lands.
                self.rules.editing = None;
                self.pending_rule = rule;
            } else {
                self.open_rule(rule);
            }
            self.outcome = match applied {
                Some(applied) if applied.transactions > 0 => Some(Outcome::Done {
                    message: changed_message(applied.transactions, category),
                    undo: applied.batch,
                }),
                _ => None,
            };
            cx.notify();
        }
    }

    /// The bar's Undo: undoes the last apply, or moves a rule moved from
    /// the keyboard back to where the presses began. When an apply's undo
    /// fails, the bar offers Undo again with the reason; a move's is said
    /// under the toolbar, as for a drop.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        match self.outcome.clone() {
            Some(Outcome::Done {
                undo: Some(batch), ..
            }) => {
                let policy = Policy::new(Said::UndoBar(batch)).tells_shell();
                let _ = self.write(policy, cx, |db| change::undo(db, batch));
            }
            Some(Outcome::Moved(moved)) => {
                // Back where it began (pressed one way, then the other):
                // nothing to move.
                if self.data.position(moved.rule) == Some(moved.from) {
                    self.dismiss_outcome(cx);
                    return;
                }
                let _ = self.move_to(moved.rule, moved.from, cx);
            }
            _ => {}
        }
    }

    /// Undoes `batch` for the undo dialog: the error is only returned, for
    /// the dialog to say.
    pub fn undo_batch(
        &mut self,
        batch: ChangeBatchId,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let policy = Policy::new(Said::Dialog).tells_shell();
        self.write(policy, cx, |db| change::undo(db, batch))
            .map(|_| ())
    }

    /// What undoing `batch` would do, for the dialog.
    pub fn undo_preview(
        &self,
        batch: ChangeBatchId,
    ) -> Result<orre_core::change::UndoSummary, SharedString> {
        self.query(|db| change::undo_preview(db, batch))
    }

    pub fn dismiss_outcome(&mut self, cx: &mut Context<Self>) {
        if self.outcome.take().is_some() {
            cx.notify();
        }
    }

    pub fn request_delete_rule(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_rule_id() {
            cx.emit(RulesEvent::DeleteRuleRequested(id));
        }
    }

    /// Deletes rule `id` (the dialog) and selects the row after it. Its
    /// transactions keep their values.
    pub fn delete_rule(&mut self, id: RuleId, cx: &mut Context<Self>) -> Result<(), SharedString> {
        let order: Vec<RuleId> = self.rules().iter().map(|s| s.rule.id()).collect();
        let next = row_after(&order, id);
        let policy = Policy::new(Said::Dialog).tells_shell();
        self.write(policy, cx, |db| rule::delete(db, id))?;
        // Gone at once, so it can't be selected before the read lands.
        self.data.rules.retain(|summary| summary.rule.id() != id);
        self.open_rule(next);
        cx.notify();
        Ok(())
    }

    /// "Review in the Inbox".
    pub fn review_suggestion(&mut self, id: SuggestionId, cx: &mut Context<Self>) {
        cx.emit(RulesEvent::ReviewSuggestion(id));
    }

    // Merchant names -------------------------------------------------------------

    pub fn selected_name(&self) -> Option<NameSelection> {
        self.names.selection()
    }

    pub fn name_form(&self) -> Option<&NameForm> {
        self.names.editing.as_ref().map(NameEdit::form)
    }

    pub fn name_error(&self) -> Option<form::NameError> {
        self.name_form()?.definition().err()
    }

    /// Opens `selection` for editing, or nothing.
    fn open_name(&mut self, selection: Option<NameSelection>) {
        self.names.editing = match selection {
            Some(NameSelection::Stored(id)) => self.data.name(id).map(|summary| {
                // As last saved while its read is pending (see `open_rule`).
                let definition = self
                    .written
                    .iter()
                    .find_map(|written| match written {
                        Written::Name(name, definition) if *name == id => Some(definition),
                        _ => None,
                    })
                    .unwrap_or(summary.alias.definition());
                NameEdit::Stored {
                    id,
                    form: NameForm::of(definition),
                }
            }),
            Some(NameSelection::New) => Some(NameEdit::New {
                form: NameForm::default(),
            }),
            None => None,
        };
    }

    pub fn select_name(&mut self, name: Option<NameSelection>, cx: &mut Context<Self>) {
        if self.names.selection() != name && self.section == Section::Names {
            self.save_name(cx);
        }
        self.enter(Section::Names);
        if self.names.selection() != name {
            self.open_name(name);
        }
        cx.notify();
    }

    /// What a click on a name's row does: selects `id`, and keeps it
    /// selected if it's selected already (Escape clears the selection).
    pub fn toggle_name(&mut self, id: MerchantAliasId, cx: &mut Context<Self>) {
        self.select_name(Some(NameSelection::Stored(id)), cx);
    }

    /// "+ New name".
    pub fn new_name(&mut self, cx: &mut Context<Self>) {
        self.outcome = None;
        self.select_name(Some(NameSelection::New), cx);
    }

    /// Changes the name's fields; saves when `save` and they're valid. A
    /// name applies everywhere at once, with no preview or batch.
    pub fn edit_name(
        &mut self,
        save: bool,
        change: impl FnOnce(&mut NameForm),
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.names.editing.as_mut() else {
            return;
        };
        change(edit.form_mut());
        self.form_edits += 1;
        if save {
            self.save_name(cx);
        }
        cx.notify();
    }

    fn save_name(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &self.names.editing else {
            return;
        };
        let Ok(definition) = edit.form().definition() else {
            return;
        };
        let policy = Policy::new(Said::Toolbar).tells_shell();
        let saved = match edit {
            NameEdit::Stored { id, .. } => {
                let id = *id;
                if self.is_stored(&Written::Name(id, definition.clone())) {
                    return;
                }
                self.write(policy, cx, |db| {
                    merchant::edit(db, id, &definition).map(|_| id)
                })
            }
            NameEdit::New { .. } => self.write(policy, cx, |db| {
                merchant::add(db, &definition).map(|(id, _)| id)
            }),
        };
        // A name just created stays open, with the fields as typed.
        if let Ok(id) = saved {
            self.wrote(Written::Name(id, definition));
            self.names.editing = match self.names.editing.take() {
                Some(NameEdit::New { form }) => Some(NameEdit::Stored { id, form }),
                other => other,
            };
            cx.notify();
        }
    }

    pub fn request_delete_name(&mut self, cx: &mut Context<Self>) {
        if let Some(NameSelection::Stored(id)) = self.names.selection() {
            cx.emit(RulesEvent::DeleteNameRequested(id));
        }
    }

    /// Deletes name `id` (the dialog) and selects the row after it: its
    /// rows show their cleaned text again, or another name.
    pub fn delete_name(
        &mut self,
        id: MerchantAliasId,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let order: Vec<MerchantAliasId> = self.names().iter().map(|n| n.alias.id()).collect();
        let next = row_after(&order, id);
        let policy = Policy::new(Said::Dialog).tells_shell();
        self.write(policy, cx, |db| merchant::delete(db, id))?;
        // Gone at once, so it can't be selected before the read lands.
        self.data.names.retain(|summary| summary.alias.id() != id);
        self.open_name(next.map(NameSelection::Stored));
        cx.notify();
        Ok(())
    }

    // Change history ---------------------------------------------------------------

    pub fn selected_batch(&self) -> Option<&HistoryEntry> {
        self.data.entry(self.history.selected_id()?)
    }

    pub fn batch_items(&self) -> &[HistoryItem] {
        self.history
            .selected
            .as_ref()
            .map_or(&[], |(_, items)| items)
    }

    fn load_items(&self, batch: ChangeBatchId) -> Vec<HistoryItem> {
        if matches!(self.source, Source::None) {
            return Vec::new();
        }
        self.query(|db| change::history_items(db, batch))
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "couldn't read a change's items");
                Vec::new()
            })
    }

    /// Opens `batch` with its items, or nothing.
    fn open_batch(&mut self, batch: Option<ChangeBatchId>) {
        self.history.selected = batch.map(|batch| (batch, self.load_items(batch)));
    }

    pub fn select_batch(&mut self, batch: Option<ChangeBatchId>, cx: &mut Context<Self>) {
        self.enter(Section::History);
        if self.history.selected_id() != batch {
            self.open_batch(batch);
        }
        cx.notify();
    }

    /// What a click on a change's row does: selects `batch`, and keeps it
    /// selected if it's selected already (Escape clears the selection).
    pub fn toggle_batch(&mut self, batch: ChangeBatchId, cx: &mut Context<Self>) {
        self.select_batch(Some(batch), cx);
    }

    pub fn request_undo(&mut self, cx: &mut Context<Self>) {
        if let Some(batch) = self.history.selected_id() {
            cx.emit(RulesEvent::UndoRequested(batch));
        }
    }

    // Categories -------------------------------------------------------------------

    pub fn selected_category(&self) -> Option<CategorySelection> {
        self.categories.selection()
    }

    /// The selected stored category and its group (its own, for a group).
    pub fn selected_category_entry(&self) -> Option<(&CategorySummary, &GroupOverview)> {
        match self.categories.selection()? {
            CategorySelection::Stored(id) => self.data.category_entry(id),
            CategorySelection::New => None,
        }
    }

    pub fn category_form(&self) -> Option<&CategoryForm> {
        self.categories.editing.as_ref().map(CategoryEdit::form)
    }

    /// The categories beside the selected one, or beside where the new one
    /// goes: row ID and label.
    fn category_siblings(&self) -> Vec<(i64, &str)> {
        let parent = match &self.categories.editing {
            Some(CategoryEdit::Stored { id, .. }) => match self.data.category_entry(*id) {
                Some((summary, group)) if !summary.category.is_group() => {
                    Some(group.group.category.key().clone())
                }
                Some(_) => None,
                None => return Vec::new(),
            },
            Some(CategoryEdit::New { form }) => form.group.clone(),
            None => return Vec::new(),
        };
        let summaries: Vec<&CategorySummary> = match parent {
            Some(parent) => self
                .data
                .category_group(&parent)
                .map(|group| group.children.iter().collect())
                .unwrap_or_default(),
            None => self
                .data
                .category_overview
                .iter()
                .map(|group| &group.group)
                .collect(),
        };
        summaries
            .into_iter()
            .map(|summary| (summary.category.id(), summary.category.label()))
            .collect()
    }

    /// Why the category's name can't be saved, if it can't.
    pub fn category_error(&self) -> Option<CategoryError> {
        let edit = self.categories.editing.as_ref()?;
        let except = match edit {
            CategoryEdit::Stored { id, .. } => Some(*id),
            CategoryEdit::New { .. } => None,
        };
        edit.form().label(self.category_siblings(), except).err()
    }

    /// Opens `selection` for editing, or nothing.
    fn open_category(&mut self, selection: Option<CategorySelection>) {
        self.categories.editing = match selection {
            Some(CategorySelection::Stored(id)) => {
                self.data.category_entry(id).map(|(summary, _)| {
                    let mut form = CategoryForm::of(&summary.category);
                    // As last renamed while its read is pending (see
                    // `open_rule`).
                    if let Some(label) = self.written.iter().find_map(|written| match written {
                        Written::Category(category, label) if *category == id => Some(label),
                        _ => None,
                    }) {
                        form.name = label.clone();
                    }
                    CategoryEdit::Stored { id, form }
                })
            }
            Some(CategorySelection::New) => Some(CategoryEdit::New {
                form: CategoryForm::default(),
            }),
            None => None,
        };
    }

    pub fn select_category(
        &mut self,
        selection: Option<CategorySelection>,
        cx: &mut Context<Self>,
    ) {
        // A rename typed before another row is chosen is kept, as leaving
        // the field would: the field's blur arrives after the selection.
        if self.categories.selection() != selection && self.section == Section::Categories {
            self.save_category_name(cx);
        }
        // Chosen over a category just created that the read hasn't brought
        // yet.
        self.pending_category = None;
        self.enter(Section::Categories);
        if self.categories.selection() != selection {
            self.open_category(selection);
        }
        cx.notify();
    }

    /// What a click on a category's row does: selects `id`, and keeps it
    /// selected if it's selected already (Escape clears the selection).
    pub fn toggle_category(&mut self, id: i64, cx: &mut Context<Self>) {
        self.select_category(Some(CategorySelection::Stored(id)), cx);
    }

    /// "+ New category": the inspector's form, empty.
    pub fn new_category(&mut self, cx: &mut Context<Self>) {
        self.outcome = None;
        self.select_category(Some(CategorySelection::New), cx);
    }

    /// Changes the category's fields. A stored category is renamed when
    /// `save` (blur, Enter) and the name is valid; a new one waits for
    /// "Create category".
    pub fn edit_category(
        &mut self,
        save: bool,
        change: impl FnOnce(&mut CategoryForm),
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.categories.editing.as_mut() else {
            return;
        };
        change(edit.form_mut());
        self.form_edits += 1;
        if save {
            self.save_category_name(cx);
        }
        cx.notify();
    }

    /// Renames the selected stored category when its name is valid and
    /// changed.
    pub fn save_category_name(&mut self, cx: &mut Context<Self>) {
        let Some((summary, _)) = self.selected_category_entry() else {
            return;
        };
        let (id, key) = (summary.category.id(), summary.category.key().clone());
        let Some(form) = self.category_form() else {
            return;
        };
        let Ok(label) = form.label(self.category_siblings(), Some(id)) else {
            return;
        };
        let saved = Written::Category(id, label.clone());
        if self.is_stored(&saved) {
            return;
        }
        let policy = Policy::new(Said::Toolbar).tells_shell();
        if self
            .write(policy, cx, |db| category::rename(db, &key, &label))
            .is_ok()
        {
            self.wrote(saved);
        }
    }

    /// "Create category": adds the new category and selects it.
    pub fn create_category(&mut self, cx: &mut Context<Self>) {
        let Some(CategoryEdit::New { form }) = &self.categories.editing else {
            return;
        };
        if self.category_error().is_some() {
            return;
        }
        let new = form.new_category();
        let policy = Policy::new(Said::Toolbar).tells_shell();
        if let Ok(category) = self.write(policy, cx, |db| category::add(db, &new)) {
            // The new category is in the read the save asked for: the form
            // closes now, so it can't be created twice, and the category is
            // selected when the read lands.
            self.categories.filter = None;
            self.categories.editing = None;
            self.pending_category = Some(category.id());
            cx.notify();
        }
    }

    /// Hides the selected category from pickers, or shows it again.
    pub fn set_category_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        let Some((summary, _)) = self.selected_category_entry() else {
            return;
        };
        let key = summary.category.key().clone();
        let policy = Policy::new(Said::Toolbar).tells_shell();
        let _ = self.write(policy, cx, |db| category::set_hidden(db, &key, hidden));
    }

    /// Deletes category `id` (the dialog) and selects the row after it.
    pub fn delete_category(&mut self, id: i64, cx: &mut Context<Self>) -> Result<(), SharedString> {
        let key = self
            .data
            .category_entry(id)
            .map(|(summary, _)| summary.category.key().clone())
            .ok_or_else(|| SharedString::from("The category is gone."))?;
        let order: Vec<i64> = self
            .category_rows()
            .iter()
            .map(|row| row.summary.category.id())
            .collect();
        let next = row_after(&order, id);
        let policy = Policy::new(Said::Dialog).tells_shell();
        self.write(policy, cx, |db| category::delete(db, &key))?;
        // Gone at once, so it can't be selected before the read lands. Core
        // refuses to delete a group with subcategories, so only its row goes.
        self.data
            .category_overview
            .retain(|group| group.group.category.id() != id);
        for group in &mut self.data.category_overview {
            group.children.retain(|child| child.category.id() != id);
        }
        self.data.categories.retain(|option| option.key != key);
        self.open_category(next.map(CategorySelection::Stored));
        cx.notify();
        Ok(())
    }
}

/// Whether `summary`'s rule has an action of `kind`.
fn has_action(summary: &RuleSummary, kind: ActionKind) -> bool {
    summary.rule.definition().action(kind).is_some()
}

/// Whether `name` matches as `kind`.
fn is_match(name: &AliasSummary, kind: AliasKind) -> bool {
    name.alias.definition().kind() == kind
}

/// Whether `entry` is a change of `kind`.
fn is_kind(entry: &HistoryEntry, kind: ChangeKind) -> bool {
    entry.batch.kind == kind
}

/// Whether an apply or a save with an apply changed any transaction.
fn changed_transactions(done: &(Option<RuleId>, Option<rule::Applied>)) -> bool {
    done.1
        .as_ref()
        .is_some_and(|applied| applied.transactions > 0)
}

/// The row after `id` in `order`, else the row before it, else nothing: what
/// is selected once `id` is deleted, in every section.
fn row_after<T: Copy + PartialEq>(order: &[T], id: T) -> Option<T> {
    let at = order.iter().position(|row| *row == id)?;
    order
        .get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|before| order.get(before)))
        .copied()
}

#[cfg(test)]
mod panel_tests;
#[cfg(test)]
mod reorder_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod write_tests;
