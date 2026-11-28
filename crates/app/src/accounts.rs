//! Accounts management (ROADMAP M3.2; `docs/DESIGN.md` → "Accounts panel";
//! `docs/ARCHITECTURE.md` → "Decision record: accounts management").
//!
//! - [`panel`]: the left panel of the money views: net worth, the accounts
//!   with their pots nested under them, and the categories.
//! - [`inspector`]: the account inspector, shown on Transactions while an
//!   account is selected and no transaction is: its details (name, bank,
//!   kind, currency, preset), entered balances, pots and imports.
//! - [`new_account`] and [`add_balance`]: the New account and Add balance
//!   dialogs; Add balance is also "Add valuation" for an asset and "Add a
//!   balance" (what's owed) for a liability (M3.17).
//! - [`valuations`]: what replaces the transaction table while an asset or
//!   liability account is selected: its values, as a step-line chart and a
//!   table (M3.17).
//! - [`context_menu`]: the right-click menus on the panel's rows (M3.P10).
//! - [`close`] and [`delete`]: the Close and Delete account dialogs (M3.17).
//!
//! After closing, the undo bar says so ([`notice_layer`]). The panel is as
//! of today (SLI-77): an account closing on a later day stays in it, and in
//! net worth, with "· closes 29 Nov" after its name, until that day; the
//! bar says "QA Loan closes on 29 Nov 2026" and its Undo reopens the
//! account. Closed today or earlier, the account leaves at once and the bar
//! says "QA Loan closed", with no Undo; a bar still showing when the day
//! comes turns into that ([`Accounts::follow_day`]). A closed account is reopened later with
//! `orre accounts reopen` (decided with the user, 2026-11-14). The shell's
//! clock tick calls [`Accounts::follow_day`], so on the closing day, from
//! local midnight, the accounts read again and the account leaves.
//!
//! [`Accounts`] holds what they show and which account is selected, and
//! makes every change through [`store::AccountStore`], which opens the
//! database for each call; core decides what's allowed. The accounts are
//! read on a background thread ([`Accounts::reload`]: at 100,000
//! transactions the account tree takes longer than a frame, decision record
//! "performance at 100,000 transactions", `docs/architecture/app.md`);
//! writes run on the UI thread and then ask for a read. Until the first read
//! is back the panel says nothing about accounts; later reads keep showing
//! the last one. Screens drawn
//! without a database (snapshots, tests) use `fixture` data, which can't
//! change.

pub mod add_balance;
pub mod close;
pub mod context_menu;
pub mod data;
pub mod delete;
#[cfg(any(test, feature = "dev-driver"))]
pub(crate) mod describe;
#[cfg(any(test, feature = "dev-samples"))]
pub mod fixture;
pub mod inspector;
pub mod new_account;
pub mod panel;
pub mod store;
pub mod valuations;

/// An account's or pot's name as it's shown: rows, titles, captions,
/// dialogs and `describe()` (untrusted text made plain, as every view shows
/// names: [`crate::inbox::text::plain`]). A name can come from an import or
/// a preset, so its control, bidi and other invisible characters are
/// stripped and it's cut to [`NAME_SHOWN`](crate::inbox::text::NAME_SHOWN).
/// The Name field holds the stored name as typed, so an edit round-trips.
pub fn name(raw: &str) -> String {
    crate::inbox::text::plain(raw, crate::inbox::text::NAME_SHOWN)
}

gpui_kit::actions!(
    orre_accounts,
    [
        /// ⌘N while an asset or liability account is selected: add a
        /// valuation (`DESIGN.md` → "Accounts you keep yourself"). On other
        /// accounts the key goes on to whoever binds it there (M3.10).
        AddValuation,
    ]
);

/// Binds the accounts panel's and the valuations canvas's keys. Call once,
/// before the first panel.
pub fn init(cx: &mut gpui_kit::App) {
    use std::rc::Rc;

    use gpui_kit::base::actions::{Cancel, Confirm};
    use gpui_kit::{KeyBinding, KeyBindingContextPredicate};
    panel::KEYS.bind(cx);
    cx.bind_keys([
        KeyBinding::new(panel::CLEAR_KEY, Cancel, Some(panel::CONTEXT)),
        // Enter on a flagged account opens its failing check, as its dot
        // does (`DESIGN.md` → "Keyboard paths").
        KeyBinding::new(
            panel::CHECK_KEY,
            Confirm { secondary: false },
            Some(panel::CONTEXT),
        ),
    ]);
    // ⌘N (Ctrl elsewhere): `KeyBinding::new` can't resolve `secondary-`,
    // so `load` asks the platform, as the shell's shortcuts do.
    let bindings = [panel::CONTEXT, valuations::CONTEXT].map(|context| {
        let context =
            Rc::new(KeyBindingContextPredicate::parse(context).expect("the key context parses"));
        KeyBinding::load(
            panel::ADD_VALUE_KEY,
            Box::new(AddValuation),
            Some(context),
            false,
            None,
            cx,
        )
        .expect("⌘N parses")
    });
    cx.bind_keys(bindings);
}

use chrono::NaiveDate;
use gpui_kit::{AppContext as _, Context, EventEmitter, SharedString, Task};
use orre_core::account::count::Expectation;
use orre_core::account::{AccountEdit, EnteredBalance, NewAccount};
use orre_core::id::{AccountId, BalanceId};
use orre_core::money::Money;
use orre_core::reporting::account_tree::AccountNode;

use crate::shell::live::{Reloads, Wrote};
use crate::ui::list;
use data::AccountsData;
use store::AccountStore;

/// Today, from the computer's clock ([`crate::today`]).
fn today() -> chrono::NaiveDate {
    crate::today()
}

/// Where the accounts come from.
enum Source {
    /// The database: what `orre` opens.
    Database(AccountStore),
    /// Sample data that can't change (tests, `dev-samples`).
    #[cfg(any(test, feature = "dev-samples"))]
    Fixture,
    /// Nothing to read, and why (no data folder).
    Unavailable(SharedString),
}

/// Something the panel or inspector asked for that the shell carries out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountsEvent {
    /// The selected account changed (`None`: all accounts).
    Selected(Option<AccountId>),
    /// The `+` on the Accounts section: open New account.
    NewAccountRequested,
    /// The `+` on the Assets and liabilities section: open New account
    /// with Kind set to Asset.
    NewValuedAccountRequested,
    /// "Add balance…" in the inspector (or "Add valuation…", ⌘N): open
    /// Add balance for the account.
    AddBalanceRequested(AccountId),
    /// A balance the user entered, clicked in the inspector or the
    /// valuations table: open Add balance's edit mode on it.
    EditBalanceRequested(AccountId, BalanceId),
    /// "Show transactions" in an account's context menu: select it and show
    /// Transactions.
    ShowTransactionsRequested(AccountId),
    /// "New transaction…" in an account's context menu: open New transaction
    /// on it.
    NewTransactionRequested(AccountId),
    /// "Import into this account…" in an account's context menu: open
    /// Import with it chosen.
    ImportRequested(AccountId),
    /// "Edit in the inspector" in an account's context menu: select it and
    /// show Transactions, where the inspector is.
    InspectRequested(AccountId),
    /// "Show transactions" in a category's context menu: filter
    /// Transactions to it, whatever the filter was.
    CategoryShowRequested(CategoryPick),
    /// A category group's context menu, other than Show transactions: open
    /// Rules on it.
    CategoryRulesRequested(i64),
    /// "Close account…" in the inspector.
    CloseRequested(AccountId),
    /// "Delete account…" in the inspector.
    DeleteRequested(AccountId),
    /// The flagged dot in the panel: open the account's failing balance
    /// check in the Inbox's Balance checks (`DESIGN.md` → "Review queues").
    ChecksRequested(AccountId),
    /// A row under CATEGORIES: show Transactions filtered to it.
    CategoryPicked(CategoryPick),
    /// The `+` on the Categories section: open Rules on a new category.
    NewCategoryRequested,
    /// A balance changed outside a dialog (Undo after removing one): the
    /// balance checks read again.
    BalancesChanged,
}

/// A row under the panel's CATEGORIES.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CategoryPick {
    /// A top-level group, by category ID: its subcategories included.
    Category(i64),
    /// The rows that need a category.
    Uncategorised,
}

/// A dialog is done: the change was saved, or the user cancelled. The
/// shell closes it and puts focus back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogClosed;

/// An account just closed: the undo bar's message, and what Undo reopens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed {
    pub account: AccountId,
    /// Its name as shown, for the message on a later day.
    name: String,
    /// The day it closes on.
    on: NaiveDate,
    /// "House closes on 29 Nov 2026", or "House closed" once that day has
    /// come.
    pub message: SharedString,
    /// Whether the bar offers Undo: only while the closing day is still to
    /// come.
    pub undoable: bool,
}

impl Closed {
    /// What the bar says after `name` is closed on `on`, read on `today`.
    /// The day always has its year (SLI-77).
    pub fn new(account: AccountId, name: &str, on: NaiveDate, today: NaiveDate) -> Self {
        let undoable = on > today;
        let message = if undoable {
            format!("{name} closes on {}", crate::format::field_date(on))
        } else {
            format!("{name} closed")
        };
        Self {
            account,
            name: name.to_owned(),
            on,
            message: message.into(),
            undoable,
        }
    }

    /// The same bar read on `today`: "House closed", without Undo, once
    /// the closing day has come.
    pub fn on_day(&self, today: NaiveDate) -> Self {
        Self::new(self.account, &self.name, self.on, today)
    }
}

/// A balance just removed: the undo bar's message, and what Undo adds back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedBalance {
    pub account: AccountId,
    pub balance: EnteredBalance,
    /// "Removed the balance on 17 Nov 2026".
    pub message: SharedString,
}

/// The accounts the app shows, and which one is selected.
pub struct Accounts {
    source: Source,
    data: AccountsData,
    selected: Option<AccountId>,
    /// Why the accounts couldn't be read, shown in the panel.
    load_error: Option<SharedString>,
    /// Whether a read has come back, or there is nothing to read: until
    /// then an empty panel means "not yet", not "no accounts".
    loaded: bool,
    /// The background read in flight; a newer one replaces it.
    loading: Option<Task<()>>,
    /// Who reads the accounts again after their own write.
    reloads: Reloads,
    /// The account closed last, until the bar is dismissed.
    closed: Option<Closed>,
    /// Why Undo couldn't reopen it, said in the bar.
    closed_error: Option<SharedString>,
    /// The balance removed last, until the bar is dismissed.
    removed: Option<RemovedBalance>,
    /// Why Undo couldn't put it back, said in the bar.
    removed_error: Option<SharedString>,
    /// The context menu open on a panel row, if any.
    context_menu: Option<context_menu::ContextMenu>,
    /// Whether the panel last drew its focus ring, for `describe()`
    /// (test-only).
    #[cfg(any(test, feature = "dev-driver"))]
    panel_ring: std::cell::Cell<bool>,
}

impl EventEmitter<AccountsEvent> for Accounts {}
impl EventEmitter<Wrote> for Accounts {}

impl Accounts {
    /// The accounts in `store`, read now, on this thread: for tests and the
    /// snapshot. The app uses [`Accounts::unloaded`].
    pub fn from_store(store: AccountStore) -> Self {
        let mut accounts = Self {
            data: AccountsData::empty(Vec::new(), today()),
            source: Source::Database(store),
            selected: None,
            load_error: None,
            loaded: true,
            loading: None,
            reloads: Reloads::Itself,
            closed: None,
            closed_error: None,
            removed: None,
            removed_error: None,
            context_menu: None,
            #[cfg(any(test, feature = "dev-driver"))]
            panel_ring: Default::default(),
        };
        accounts.read_now();
        accounts
    }

    /// The accounts in `store`, not read yet: the shell starts the first
    /// read (`start_loading`), on a background thread.
    pub fn unloaded(store: AccountStore) -> Self {
        Self {
            loaded: false,
            data: AccountsData::empty(Vec::new(), today()),
            source: Source::Database(store),
            selected: None,
            load_error: None,
            loading: None,
            reloads: Reloads::Itself,
            closed: None,
            closed_error: None,
            removed: None,
            removed_error: None,
            context_menu: None,
            #[cfg(any(test, feature = "dev-driver"))]
            panel_ring: Default::default(),
        }
    }

    /// No accounts, because they can't be read: `reason` shows in the
    /// panel.
    pub fn unavailable(reason: impl Into<SharedString>) -> Self {
        let reason = reason.into();
        Self {
            source: Source::Unavailable(reason.clone()),
            data: AccountsData::empty(Vec::new(), today()),
            selected: None,
            load_error: Some(reason),
            loaded: true,
            loading: None,
            reloads: Reloads::Itself,
            closed: None,
            closed_error: None,
            removed: None,
            removed_error: None,
            context_menu: None,
            #[cfg(any(test, feature = "dev-driver"))]
            panel_ring: Default::default(),
        }
    }

    /// Sample data, which can't change.
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn fixture(data: AccountsData) -> Self {
        Self {
            source: Source::Fixture,
            data,
            selected: None,
            load_error: None,
            loaded: true,
            loading: None,
            reloads: Reloads::Itself,
            closed: None,
            closed_error: None,
            removed: None,
            removed_error: None,
            context_menu: None,
            #[cfg(any(test, feature = "dev-driver"))]
            panel_ring: Default::default(),
        }
    }

    /// Shows the undo bar for `closed`, as closing does (snapshots).
    pub fn with_closed(mut self, closed: Closed) -> Self {
        self.closed = Some(closed);
        self
    }

    /// The account closed last, while its undo bar shows.
    pub fn closed(&self) -> Option<&Closed> {
        self.closed.as_ref()
    }

    /// Why Undo couldn't reopen the account.
    pub fn closed_error(&self) -> Option<&SharedString> {
        self.closed_error.as_ref()
    }

    /// Whether the accounts have been read (or there are none to read):
    /// until then an empty panel says nothing about accounts.
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn data(&self) -> &AccountsData {
        &self.data
    }

    pub fn selected(&self) -> Option<AccountId> {
        self.selected
    }

    /// The selected account, if it's still open.
    pub fn selected_node(&self) -> Option<&AccountNode> {
        self.selected.and_then(|id| self.data.node(id))
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    /// Selects `account` (`None`: all accounts).
    pub fn select(&mut self, account: Option<AccountId>, cx: &mut Context<Self>) {
        if self.selected == account {
            return;
        }
        self.selected = account;
        cx.emit(AccountsEvent::Selected(account));
        cx.notify();
    }

    /// What a click on a panel row does: selects `account`, and keeps it
    /// selected if it's selected already (Escape goes back to all accounts).
    pub fn toggle(&mut self, account: AccountId, cx: &mut Context<Self>) {
        self.select(Some(account), cx);
    }

    /// Moves the selection through the panel's accounts in panel order
    /// ([`list::step`]: stopping at the ends; with nothing selected, down
    /// selects the first row and up the last). The arrow keys in the panel.
    pub fn select_next(&mut self, to: list::Move, cx: &mut Context<Self>) {
        let rows: Vec<AccountId> = panel::order(&self.data);
        if let Some(next) = list::step(&rows, self.selected.as_ref(), to) {
            self.select(Some(next), cx);
        }
    }

    /// The context menu open on a panel row.
    pub fn context_menu(&self) -> Option<&context_menu::ContextMenu> {
        self.context_menu.as_ref()
    }

    /// Opens a context menu of `groups` at `position`, on the row for
    /// `target`, and takes focus into it; closing it gives focus back to
    /// `trigger`. The selection doesn't change.
    pub fn open_context_menu(
        &mut self,
        target: context_menu::ContextTarget,
        groups: Vec<crate::ui::menu::MenuGroup>,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        trigger: gpui_kit::FocusHandle,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        if groups.is_empty() {
            return;
        }
        let mut select = crate::ui::menu::SelectState::with_trigger(trigger, cx);
        select.set_open(true, None, window, cx);
        self.context_menu = Some(context_menu::ContextMenu {
            target,
            position,
            groups,
            select,
        });
        cx.notify();
    }

    /// Closes the context menu (Escape, a click outside) and gives focus
    /// back to the panel.
    pub fn close_context_menu(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if let Some(mut menu) = self.context_menu.take() {
            menu.select.set_open(false, None, window, cx);
            cx.notify();
        }
    }

    /// The arrow keys in the context menu.
    pub fn move_context_highlight(&mut self, by: isize, cx: &mut Context<Self>) {
        if let Some(menu) = self.context_menu.as_mut() {
            menu.select.move_highlight(&menu.groups, by);
            cx.notify();
        }
    }

    /// An item of the context menu was chosen (click or Enter): closes the
    /// menu and asks for what the item does.
    pub fn choose_context_item(
        &mut self,
        key: &str,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.context_menu.as_ref().map(|menu| menu.target) else {
            return;
        };
        self.close_context_menu(window, cx);
        if let Some(event) = context_menu::event(target, key) {
            cx.emit(event);
        }
    }

    /// Asks the shell to open New account.
    pub fn request_new_account(&mut self, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::NewAccountRequested);
    }

    /// Asks the shell to open New account with Kind set to Asset.
    pub fn request_new_valued_account(&mut self, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::NewValuedAccountRequested);
    }

    /// Asks the shell to open Add balance for `account`.
    pub fn request_add_balance(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::AddBalanceRequested(account));
    }

    /// ⌘N: asks for Add valuation when the selected account is an asset or
    /// liability. Returns whether it did, so the key can go on otherwise.
    pub fn request_add_valuation(&mut self, cx: &mut Context<Self>) -> bool {
        match self.selected_node() {
            Some(node) if !node.account.kind.holds_transactions() => {
                let id = node.account.id;
                self.request_add_balance(id, cx);
                true
            }
            _ => false,
        }
    }

    /// Asks the shell to open the edit dialog for one of `account`'s
    /// entered balances.
    pub fn request_edit_balance(
        &mut self,
        account: AccountId,
        balance: BalanceId,
        cx: &mut Context<Self>,
    ) {
        cx.emit(AccountsEvent::EditBalanceRequested(account, balance));
    }

    /// Asks the shell to open Close account for `account`.
    pub fn request_close(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::CloseRequested(account));
    }

    /// Asks the shell to open Delete account for `account`.
    pub fn request_delete(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::DeleteRequested(account));
    }

    /// The flagged dot: ask the shell to open the account's balance checks.
    pub fn request_checks(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::ChecksRequested(account));
    }

    /// Enter in the panel: what the selected account's flagged dot does,
    /// if it has one. Returns whether it did anything.
    pub fn request_selected_checks(&mut self, cx: &mut Context<Self>) -> bool {
        let flagged = self
            .selected_node()
            .filter(|node| node.status == orre_core::reconciliation::Status::Mismatch)
            .map(|node| node.account.id);
        if let Some(account) = flagged {
            self.request_checks(account, cx);
        }
        flagged.is_some()
    }

    /// A row under CATEGORIES: ask the shell to filter Transactions by it.
    pub fn request_category(&mut self, pick: CategoryPick, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::CategoryPicked(pick));
    }

    /// The Categories section's `+`: ask the shell to open a new category
    /// in Rules.
    pub fn request_new_category(&mut self, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::NewCategoryRequested);
    }

    /// Reads the accounts again, on a background thread. What is shown
    /// stays until the read is back; a read asked for meanwhile replaces
    /// this one. When it lands, a selected account that has gone (closed or
    /// deleted) is let go, and the views filtering by it are told.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.reload_on(today(), cx);
    }

    /// [`Accounts::reload`] as of `today`.
    fn reload_on(&mut self, today: NaiveDate, cx: &mut Context<Self>) {
        let Source::Database(store) = &self.source else {
            cx.notify();
            return;
        };
        let store = store.clone();
        self.loading = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { store.load(store.presets(), today) })
                .await
                .map_err(|error| SharedString::from(error.to_string()));
            this.update(cx, |this, cx| {
                this.loading = None;
                // A failed read keeps the last data, which may lack an
                // account selected since: only a read that worked lets go.
                if this.take(result) {
                    this.let_go_of_missing_selection(cx);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// The shell's clock tick: when `today` isn't the day the accounts were
    /// read on, reads them again, so an account whose closing day has come
    /// leaves the panel and net worth (from local midnight, within the
    /// tick). Sample data keeps its day. A close bar still showing for that
    /// account turns into "House closed", without Undo (SLI-77); a
    /// dismissed one stays dismissed, and a removed-balance bar is left as
    /// it is.
    pub fn follow_day(&mut self, today: NaiveDate, cx: &mut Context<Self>) {
        if let Some(closed) = self
            .closed
            .as_ref()
            .filter(|closed| closed.undoable && closed.on <= today)
        {
            self.closed = Some(closed.on_day(today));
            self.closed_error = None;
            cx.notify();
        }
        if self.loaded
            && self.loading.is_none()
            && today != self.data.today
            && matches!(self.source, Source::Database(_))
        {
            self.reload_on(today, cx);
        }
    }

    /// Starts the first read, if it hasn't been asked for.
    pub(crate) fn start_loading(&mut self, cx: &mut Context<Self>) {
        if !self.loaded && self.loading.is_none() {
            self.reload(cx);
        }
    }

    /// After their own write, the shell reads the accounts again, with
    /// every other view of the ledger ([`crate::shell::live`]).
    pub fn reload_through_shell(&mut self) {
        self.reloads = Reloads::ByShell;
    }

    /// Their own write changed the ledger: the accounts read again
    /// themselves, or inside a shell drop the read in flight (it started
    /// before the write, so it mustn't land after it) and leave the read to
    /// the shell, which starts it after the write.
    fn wrote_ledger(&mut self, cx: &mut Context<Self>) {
        match self.reloads {
            Reloads::Itself => self.reload(cx),
            Reloads::ByShell => {
                self.loading = None;
                cx.emit(Wrote::of([orre_core::watch::Topic::Ledger]));
            }
        }
    }

    /// Reads on this thread (tests and the snapshot).
    fn read_now(&mut self) {
        let Source::Database(store) = &self.source else {
            return;
        };
        let result = store
            .load(store.presets(), today())
            .map_err(|error| SharedString::from(error.to_string()));
        self.take(result);
        if self.selected.is_some_and(|id| !self.data.contains(id)) {
            self.selected = None;
        }
    }

    /// Takes a read; whether it worked.
    fn take(&mut self, result: Result<AccountsData, SharedString>) -> bool {
        self.loaded = true;
        match result {
            Ok(data) => {
                self.data = data;
                self.load_error = None;
                true
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the accounts");
                self.load_error = Some(error);
                false
            }
        }
    }

    /// The read that lands was started after every write that selected an
    /// account (a read asked for earlier was replaced), so a selected
    /// account that is missing from it has gone.
    fn let_go_of_missing_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_some_and(|id| !self.data.contains(id)) {
            self.selected = None;
            cx.emit(AccountsEvent::Selected(None));
        }
    }

    fn store(&self) -> Result<&AccountStore, SharedString> {
        match &self.source {
            Source::Database(store) => Ok(store),
            #[cfg(any(test, feature = "dev-samples"))]
            Source::Fixture => Err("This is sample data, which can't be changed.".into()),
            Source::Unavailable(reason) => Err(reason.clone()),
        }
    }

    /// Adds an account (and its first balance), then selects it. The error
    /// says why it wasn't added.
    pub fn create(
        &mut self,
        account: &NewAccount,
        balance: Option<&EnteredBalance>,
        cx: &mut Context<Self>,
    ) -> Result<AccountId, SharedString> {
        let id = self
            .store()?
            .create(account, balance)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.wrote_ledger(cx);
        // Selected at once, by ID; the read asked for after the write has
        // the account.
        self.select(Some(id), cx);
        Ok(id)
    }

    /// Changes an account's details. The error says why they weren't.
    pub fn update(
        &mut self,
        account: AccountId,
        edit: &AccountEdit,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .update(account, edit)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.wrote_ledger(cx);
        Ok(())
    }

    /// Closes `account` on `on`, with its open pots, and shows the undo
    /// bar. The error says why it wasn't closed.
    pub fn close(
        &mut self,
        account: AccountId,
        on: NaiveDate,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let name = self
            .data
            .node(account)
            .map(|node| name(&node.account.name))
            .unwrap_or_default();
        self.store()?
            .close(account, on)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.closed = Some(Closed::new(account, &name, on, today()));
        self.closed_error = None;
        self.removed = None;
        self.removed_error = None;
        self.wrote_ledger(cx);
        Ok(())
    }

    /// Undo on the bar: reopens the account just closed and selects it.
    pub fn undo_close(&mut self, cx: &mut Context<Self>) {
        let Some(closed) = self.closed.clone().filter(|closed| closed.undoable) else {
            return;
        };
        let reopened = self.store().and_then(|store| {
            store
                .reopen(closed.account)
                .map_err(|error| SharedString::from(error.to_string()))
        });
        match reopened {
            Ok(_) => {
                self.closed = None;
                self.closed_error = None;
                self.wrote_ledger(cx);
                self.select(Some(closed.account), cx);
            }
            Err(error) => {
                self.closed_error = Some(new_account::sentence(&error));
                cx.notify();
            }
        }
    }

    /// Closes the undo bar; the account stays closed.
    pub fn dismiss_closed(&mut self, cx: &mut Context<Self>) {
        self.closed = None;
        self.closed_error = None;
        cx.notify();
    }

    /// Deletes `account` and its values. The error says why it wasn't.
    pub fn delete(
        &mut self,
        account: AccountId,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .delete(account)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.wrote_ledger(cx);
        Ok(())
    }

    /// What Orre expects of a cash count of `counted` on `date` in
    /// `account`, and how far the count is from it. Sample data answers from
    /// [`data::AccountDetail::cash_expected`].
    pub fn cash_expectation(
        &self,
        account: AccountId,
        date: NaiveDate,
        counted: Money,
    ) -> Result<Expectation, SharedString> {
        match &self.source {
            Source::Database(store) => store
                .cash_expectation(account, date, counted)
                .map_err(|error| SharedString::from(error.to_string())),
            Source::Unavailable(reason) => Err(reason.clone()),
            #[cfg(any(test, feature = "dev-samples"))]
            Source::Fixture => {
                let detail = self.data.detail(account);
                let since = detail
                    .and_then(|detail| detail.balances.first())
                    .map(|count| count.date);
                let expected = detail.and_then(|detail| detail.cash_expected);
                Ok(Expectation {
                    since,
                    expected,
                    difference: expected
                        .map(|expected| counted.checked_sub(expected))
                        .transpose()
                        .map_err(|error| SharedString::from(error.to_string()))?,
                })
            }
        }
    }

    /// Records a cash count for `account`, with the "Cash count difference"
    /// transaction when `add_difference`. The error says why it wasn't.
    ///
    /// Reloads the accounts, so the panel's balance changes. A difference
    /// transaction also changes what Transactions shows: inside a shell the
    /// shell reads every view of the ledger again.
    pub fn count_cash(
        &mut self,
        account: AccountId,
        count: &EnteredBalance,
        add_difference: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .count_cash(account, count, add_difference)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.wrote_ledger(cx);
        Ok(())
    }

    /// Changes one balance the user entered. The error says why it
    /// wasn't.
    pub fn update_balance(
        &mut self,
        account: AccountId,
        balance: BalanceId,
        entered: &EnteredBalance,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .update_balance(account, balance, entered)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.wrote_ledger(cx);
        Ok(())
    }

    /// Removes one balance the user entered and shows the undo bar. The
    /// error says why it wasn't removed.
    pub fn remove_balance(
        &mut self,
        account: AccountId,
        balance: BalanceId,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let kind = self.data.node(account).map(|node| node.account.kind);
        let removed = self
            .store()?
            .remove_balance(account, balance)
            .map_err(|error| SharedString::from(error.to_string()))?;
        let noun = add_balance::noun(kind.unwrap_or(orre_core::account::AccountKind::Current));
        self.removed = Some(RemovedBalance {
            account,
            message: format!(
                "Removed the {noun} on {}",
                crate::format::field_date(removed.date)
            )
            .into(),
            balance: removed,
        });
        self.removed_error = None;
        self.closed = None;
        self.closed_error = None;
        self.wrote_ledger(cx);
        Ok(())
    }

    /// The balance removed last, while its undo bar shows.
    pub fn removed(&self) -> Option<&RemovedBalance> {
        self.removed.as_ref()
    }

    /// Why Undo couldn't put the removed balance back.
    pub fn removed_error(&self) -> Option<&SharedString> {
        self.removed_error.as_ref()
    }

    /// Undo on the bar after removing a balance: adds it back, as it was.
    pub fn undo_remove_balance(&mut self, cx: &mut Context<Self>) {
        let Some(removed) = self.removed.clone() else {
            return;
        };
        match self.add_balance(removed.account, &removed.balance, cx) {
            Ok(()) => {
                self.removed = None;
                self.removed_error = None;
                cx.emit(AccountsEvent::BalancesChanged);
            }
            Err(error) => {
                self.removed_error = Some(new_account::sentence(&add_balance::field_dates(&error)));
                cx.notify();
            }
        }
    }

    /// Undo on the bar: whatever it last did, a removed balance or a close.
    pub fn undo_notice(&mut self, cx: &mut Context<Self>) {
        if self.removed.is_some() {
            self.undo_remove_balance(cx);
        } else {
            self.undo_close(cx);
        }
    }

    /// Closes the undo bar, whichever it is; the change stays.
    pub fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.removed = None;
        self.removed_error = None;
        self.dismiss_closed(cx);
    }

    /// Records a balance for `account`. The error says why it wasn't.
    pub fn add_balance(
        &mut self,
        account: AccountId,
        balance: &EnteredBalance,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .add_balance(account, balance)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.wrote_ledger(cx);
        Ok(())
    }
}

/// The undo bar after closing an account, centred over the foot of the
/// window's body like the import notification: "House closes on 29 Nov 2026"
/// and Undo, which reopens it, or "House closed" alone for a day that has
/// come. `None` when no account was just closed.
pub fn notice_layer(
    accounts: &gpui_kit::Entity<Accounts>,
    cx: &gpui_kit::App,
) -> Option<gpui_kit::AnyElement> {
    use gpui_kit::{IntoElement as _, ParentElement as _, Styled as _, div};

    use crate::theme::tokens::spacing;
    use crate::ui::undo_bar::UndoBar;

    let state = accounts.read(cx);
    let message = match (state.removed(), state.closed()) {
        (Some(removed), _) => state
            .removed_error()
            .cloned()
            .unwrap_or_else(|| removed.message.clone()),
        (None, Some(closed)) => state
            .closed_error()
            .cloned()
            .unwrap_or_else(|| closed.message.clone()),
        (None, None) => return None,
    };
    let undoable =
        state.removed().is_some() || state.closed().is_some_and(|closed| closed.undoable);
    let (undo, dismiss) = (accounts.downgrade(), accounts.downgrade());
    let bar = UndoBar::new(message);
    let bar = if undoable {
        bar.on_undo(move |_, _, cx| {
            let _ = undo.update(cx, |accounts, cx| accounts.undo_notice(cx));
        })
    } else {
        bar.without_undo()
    };
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(spacing::S6)
            .flex()
            .justify_center()
            .child(bar.on_dismiss(move |_, _, cx| {
                let _ = dismiss.update(cx, |accounts, cx| accounts.dismiss_notice(cx));
            }))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests;
