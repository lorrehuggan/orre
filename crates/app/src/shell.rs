//! The app shell (ROADMAP M3.1; `docs/DESIGN.md` → "Finance screens"): the
//! one window `orre` opens, and the frame every view sits in.
//!
//! Top to bottom: the title bar, with the eight view tabs (Overview,
//! Transactions, Trends, Budgets, Net worth, Inbox, Reviews, Rules) and the
//! Settings button; the body, with the view's left panel and its canvas (and
//! inspector); the status bar. Navigation is the title bar's, as Paper draws
//! it (Rules `B1J-0`, Settings · AI clients `AQ2-0`), not a sidebar: the
//! left panel belongs to the view. The five views of the money share the
//! accounts panel ([`crate::accounts::panel`]); Inbox, Reviews, Rules and
//! Settings get their own lists with their screens.
//!
//! Keyboard: ⌘1–⌘8 open the views in tab order and ⌘, opens Settings
//! (Ctrl on Linux and Windows). Clicking the Settings button while Settings
//! is open goes back to the view it was opened from.
//!
//! Transactions (D5; on the database since M3.3), the Inbox (M3.6) and
//! Settings (M3.15, M3.16), both from the database, are built. The others
//! show the shell around an [`EmptyState`] that says so, until their roadmap
//! items land.
//!
//! The shell owns which view is open and the window's saved state
//! ([`window_state`]): [`open`] restores the bounds and the view from the
//! database, and the shell saves them again shortly after they change and
//! when the window closes.
//!
//! Importing (M3.4): ⌘I, the Transactions toolbar's Import, or a file
//! dropped anywhere on the window opens the Import dialog
//! ([`crate::import::dialog`]); after an import the accounts panel and
//! Transactions read the database again, and the notification offers Undo.
//!
//! Excluding and manual rows (M3.10): ⌘N on Transactions, or a cash
//! account's "New transaction", opens the New-transaction dialog with the
//! selected account preset; the Transactions inspector's Source section
//! asks for the Exclude dialog, the undo-import confirmation and Delete
//! transaction, which the shell opens like the others. The status bar
//! counts the transactions and, when there are some, the excluded ones.
//!
//! It also owns the accounts ([`Accounts`], M3.2), which the panel, the
//! Transactions inspector and the account dialogs share, and opens those
//! dialogs over everything when the panel or inspector asks; and Settings
//! ([`SettingsView`], M3.15, M3.16), whose dialogs it opens the same way,
//! and whose kill switch the status bar shows.
//!
//! Live updates (LU.3, [`live`]): the views read again when another process
//! changes the database, and after the app's own writes, which the views
//! name to the shell rather than reloading themselves.

pub mod arrivals;
#[cfg(any(test, feature = "dev-driver"))]
pub mod describe;
pub mod live;
mod status;
pub mod window_state;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui_kit::AnyView;
use gpui_kit::component::window_border;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, AppContext as _, Bounds, Context, Entity, ExternalPaths, FocusHandle,
    InteractiveElement as _, IntoElement, KeyBinding, KeyBindingContextPredicate,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, WeakEntity, Window,
    WindowOptions, div, px, size,
};
use orre_core::setting::WindowState;
use orre_core::watch::Topic;
use orre_core::{clock, paths};

use crate::accounts::add_balance::AddBalanceDialog;
use crate::accounts::close::CloseAccountDialog;
use crate::accounts::delete::DeleteAccountDialog;
use crate::accounts::new_account::NewAccountDialog;
use crate::accounts::panel::PanelCategories;
use crate::accounts::store::AccountStore;
use crate::accounts::{self, Accounts, AccountsEvent, DialogClosed};
use crate::checks::lock::{LockDialog, UnlockDialog};
use crate::checks::store::ChecksStore;
use crate::checks::view::ChecksView;
use crate::checks::{self, Checks, ChecksEvent, Queue};
use crate::import::dialog::ImportDialog;
use crate::import::store::ImportStore;
use crate::import::{self, ImportWrite, Imports, ImportsChanged};
use crate::inbox::arrival::Arrived;
use crate::inbox::store::InboxStore;
use crate::inbox::view::InboxView;
use crate::inbox::{self, Inbox};
use crate::overview::store::OverviewStore;
use crate::overview::{OverviewEvent, OverviewView};
use crate::rules::dialogs::{DeleteNameDialog, DeleteRuleDialog, UndoChangeDialog};
use crate::rules::store::RulesStore;
use crate::rules::view::RulesView;
use crate::rules::{self, Rules, RulesEvent};
use crate::settings::delete::DeleteBackupDialog;
use crate::settings::enable::EnableDialog;
use crate::settings::export::ExportDialog;
use crate::settings::remove::RemoveDialog;
use crate::settings::restore::RestoreDialog;
use crate::settings::store::SettingsStore;
use crate::settings::{self, Section, SettingsEvent, SettingsView};
use crate::theme::tokens::{color, size as token_size, typography};
use crate::transactions::confirm::{DeleteTransactionDialog, UndoImportDialog};
use crate::transactions::exclude_dialog::ExcludeDialog;
use crate::transactions::link_dialog::LinkDialog;
use crate::transactions::new_transaction::NewTransactionDialog;
use crate::transactions::store::TransactionStore;
use crate::transactions::{self, TransactionsView};
use crate::transactions::{Counts, RegisterChanged, TransactionsEvent};
use crate::ui::button::IconButton;
use crate::ui::empty_state::EmptyState;
use crate::ui::icon::Glyph;
use crate::ui::status_bar::{StatusBar, StatusItem};
use crate::ui::text::TextRoleExt as _;
use crate::ui::title_bar::{self, TitleBar, ViewTab};

use arrivals::{ArrivalLine, Phase};
use live::{Live, Shown, Wrote};
use status::{Ai, ChecksStatus, Status};
use window_state::WindowStore;

/// The key context of the whole window, where the view shortcuts work.
const SHELL_CONTEXT: &str = "OrreShell";

/// How long the window must stay put before its state is saved, so a drag
/// or a resize writes once, after it ends.
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// How often the status bar's clock is read.
const CLOCK_TICK: Duration = Duration::from_secs(30);

gpui_kit::actions!(
    orre_shell,
    [
        /// ⌘ and its tab's number ([`View::TABS`]; ⌘1): open Overview.
        ShowOverview,
        /// ⌘2: open Transactions.
        ShowTransactions,
        /// Open Trends: a tab, with a shortcut, only in tests and
        /// `dev-samples` builds (it isn't built).
        ShowTrends,
        /// Open Budgets: as Trends.
        ShowBudgets,
        /// Open Net worth: as Trends.
        ShowNetWorth,
        /// ⌘ and its tab's number: open the Inbox (⌘3; ⌘6 with every tab).
        ShowInbox,
        /// Open Reviews: as Trends.
        ShowReviews,
        /// ⌘ and its tab's number: open Rules (⌘4; ⌘8 with every tab).
        ShowRules,
        /// ⌘,: open Settings.
        OpenSettings,
        /// ⌘I: import a statement (M3.4).
        ImportStatement,
        /// ⌘N on Transactions: add a transaction by hand (M3.10).
        NewTransaction,
    ]
);

/// One of Orre's views: a tab in the title bar, or Settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum View {
    /// The view a first launch opens, the first tab: how the month is going.
    #[default]
    Overview,
    Transactions,
    Trends,
    Budgets,
    NetWorth,
    Inbox,
    Reviews,
    Rules,
    /// Not a tab: the title bar's Settings button opens it, and while it's
    /// open no tab is active (`DESIGN.md` → "Settings").
    Settings,
}

impl View {
    /// The title bar's tabs, in order, each opened by ⌘ and its number:
    /// the shipped ones ([`View::SHIPPED_TABS`]) in a release or default
    /// build; all eight ([`View::ALL_TABS`]) in tests and `dev-samples`
    /// builds, so the shell tests and the snapshots keep Paper's title bar.
    #[cfg(any(test, feature = "dev-samples"))]
    pub const TABS: [View; 8] = View::ALL_TABS;
    /// The title bar's tabs, in order, each opened by ⌘ and its number:
    /// the shipped ones ([`View::SHIPPED_TABS`]).
    #[cfg(not(any(test, feature = "dev-samples")))]
    pub const TABS: [View; 4] = View::SHIPPED_TABS;

    /// The tabs v0.1 ships: the built screens. Trends, Budgets, Net worth
    /// and Reviews (M4.2–M4.7) are v0.2, so a release build has no tab or
    /// shortcut for them (W1-3).
    pub const SHIPPED_TABS: [View; 4] =
        [View::Overview, View::Transactions, View::Inbox, View::Rules];

    /// Every tab Paper draws, built or not.
    pub const ALL_TABS: [View; 8] = [
        View::Overview,
        View::Transactions,
        View::Trends,
        View::Budgets,
        View::NetWorth,
        View::Inbox,
        View::Reviews,
        View::Rules,
    ];

    /// Every view: the tabs, then Settings.
    pub const ALL: [View; 9] = [
        View::Overview,
        View::Transactions,
        View::Trends,
        View::Budgets,
        View::NetWorth,
        View::Inbox,
        View::Reviews,
        View::Rules,
        View::Settings,
    ];

    /// The key the window state saves the view under. Stable: changing one
    /// reopens saved windows on the default view.
    pub fn key(self) -> &'static str {
        match self {
            View::Overview => "overview",
            View::Transactions => "transactions",
            View::Trends => "trends",
            View::Budgets => "budgets",
            View::NetWorth => "net_worth",
            View::Inbox => "inbox",
            View::Reviews => "reviews",
            View::Rules => "rules",
            View::Settings => "settings",
        }
    }

    /// The view saved under `key`, if this build has it.
    pub fn from_key(key: &str) -> Option<View> {
        View::ALL.into_iter().find(|view| view.key() == key)
    }

    /// The name on its tab and in its title.
    pub fn label(self) -> &'static str {
        match self {
            View::Overview => "Overview",
            View::Transactions => "Transactions",
            View::Trends => "Trends",
            View::Budgets => "Budgets",
            View::NetWorth => "Net worth",
            View::Inbox => "Inbox",
            View::Reviews => "Reviews",
            View::Rules => "Rules",
            View::Settings => "Settings",
        }
    }

    /// The element id of its tab, or of the Settings button.
    pub fn element_id(self) -> &'static str {
        match self {
            View::Overview => "view-overview",
            View::Transactions => "view-transactions",
            View::Trends => "view-trends",
            View::Budgets => "view-budgets",
            View::NetWorth => "view-net-worth",
            View::Inbox => "view-inbox",
            View::Reviews => "view-reviews",
            View::Rules => "view-rules",
            View::Settings => "open-settings",
        }
    }

    /// Whether the view filters by account, and so has the accounts panel
    /// on its left (`DESIGN.md` → "Accounts panel").
    pub fn has_accounts_panel(self) -> bool {
        matches!(
            self,
            View::Overview | View::Transactions | View::Trends | View::Budgets | View::NetWorth
        )
    }

    /// Whether this build shows it: one of its [`View::TABS`], or Settings.
    /// A saved window on any other view opens on the default.
    pub fn is_shown(self) -> bool {
        self == View::Settings || View::TABS.contains(&self)
    }

    /// The action that opens it, as its shortcut does.
    fn show_action(self) -> Box<dyn Action> {
        match self {
            View::Overview => Box::new(ShowOverview),
            View::Transactions => Box::new(ShowTransactions),
            View::Trends => Box::new(ShowTrends),
            View::Budgets => Box::new(ShowBudgets),
            View::NetWorth => Box::new(ShowNetWorth),
            View::Inbox => Box::new(ShowInbox),
            View::Reviews => Box::new(ShowReviews),
            View::Rules => Box::new(ShowRules),
            View::Settings => Box::new(OpenSettings),
        }
    }

    /// Whether its screen exists yet.
    pub fn is_built(self) -> bool {
        matches!(
            self,
            View::Overview | View::Transactions | View::Inbox | View::Rules | View::Settings
        )
    }
}

/// Each of `tabs` with its shortcut: ⌘ and its number from 1, in order, so
/// the numbers have no holes whatever the tab set.
pub fn tab_shortcuts(tabs: &[View]) -> Vec<(String, View)> {
    tabs.iter()
        .enumerate()
        .map(|(index, view)| (format!("secondary-{}", index + 1), *view))
        .collect()
}

/// Binds the shell's shortcuts and the Transactions table's keys. Call once,
/// before [`open`] or the first [`ShellView`].
pub fn init(cx: &mut App) {
    let tabs = tab_shortcuts(&View::TABS)
        .into_iter()
        .map(|(keys, view)| (keys, view.show_action()));
    let others: [(String, Box<dyn Action>); 3] = [
        ("secondary-,".to_owned(), Box::new(OpenSettings)),
        ("secondary-i".to_owned(), Box::new(ImportStatement)),
        ("secondary-n".to_owned(), Box::new(NewTransaction)),
    ];
    let bindings: Vec<_> = tabs.chain(others).collect();
    let context = Rc::new(
        KeyBindingContextPredicate::parse(SHELL_CONTEXT).expect("the shell's key context parses"),
    );
    // `KeyBinding::new` can't resolve `secondary-` (⌘ on macOS, Ctrl
    // elsewhere); `load` asks the platform.
    let bindings: Vec<_> = bindings
        .into_iter()
        .map(|(keys, action)| {
            KeyBinding::load(&keys, action, Some(context.clone()), false, None, cx)
                .expect("the shell's shortcuts parse")
        })
        .collect();
    cx.bind_keys(bindings);
    transactions::init(cx);
    accounts::init(cx);
    inbox::init(cx);
    checks::init(cx);
    rules::init(cx);
    settings::init(cx);
}

/// The status `orre` exits with when its window can't open, so launchers
/// and scripts see the failure.
pub const WINDOW_FAILED_EXIT_CODE: i32 = 1;

/// Opens Orre's window as it was last left: the saved bounds and view
/// from the database in the data dir, or 1440×900 (the size it's designed
/// at, where the screen allows) on Transactions.
///
/// The window opens once the live-updates baseline is taken, on a
/// background thread, so it comes before any view's first read ([`live`]).
/// A window that can't open is logged and the process exits with
/// [`WINDOW_FAILED_EXIT_CODE`].
pub fn open(cx: &mut App) {
    let database = paths::database_path();
    let store = match &database {
        Ok(database) => Some(WindowStore::new(database.clone())),
        Err(error) => {
            tracing::warn!(%error, "no data dir; the window state won't be kept");
            None
        }
    };
    let saved = store.as_ref().and_then(WindowStore::load);
    let fallback = Bounds::centered(None, size(px(1440.), px(900.)), cx);
    let displays: Vec<_> = cx
        .displays()
        .iter()
        .map(|display| display.bounds())
        .collect();
    let options = WindowOptions {
        window_bounds: Some(window_state::restored_bounds(
            saved.as_ref(),
            &displays,
            fallback,
        )),
        window_min_size: Some(size(
            token_size::WINDOW_MIN_WIDTH,
            token_size::WINDOW_MIN_HEIGHT,
        )),
        ..title_bar::window_options()
    };
    let view = window_state::restored_view(saved.as_ref());
    let watched_path = database.as_ref().ok().cloned();
    cx.spawn(async move |cx| {
        let watched = match watched_path {
            Some(path) => Some(
                cx.background_spawn(async move { live::baseline(&path) })
                    .await,
            ),
            None => None,
        };
        let opened = cx.update(|cx| {
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| {
                    let database = database.map_err(|error| error.to_string());
                    let mut shell = ShellView::from_database(view, database, window, cx);
                    if let Some(store) = store {
                        shell.keep_window_state(store, window, cx);
                    }
                    shell.keep_time(cx);
                    if let Some(watched) = watched {
                        shell.watch(watched, cx);
                        shell.keep_watching(window, cx);
                    }
                    shell
                })
            })
        });
        match opened {
            // A dev build drives this window over a local socket (ROADMAP
            // U.5), against a scratch data dir only.
            #[cfg(feature = "dev-driver")]
            Ok((window, shell)) => cx.update(|cx| crate::dev_driver::start(window, shell, cx)),
            #[cfg(not(feature = "dev-driver"))]
            Ok(_) => {}
            Err(error) => {
                tracing::error!(%error, "couldn't open the window");
                // Exit directly rather than `cx.quit()`: on macOS GPUI's quit
                // terminates the process with status 0 and `run` never
                // returns. Logging writes to stderr synchronously and nothing
                // is open yet, so there is nothing to flush.
                std::process::exit(WINDOW_FAILED_EXIT_CODE);
            }
        }
    })
    .detach();
}

/// Saving the window state: the latest state, the one last written, and the
/// pending delayed save.
struct Persistence {
    store: WindowStore,
    latest: WindowState,
    saved: Option<WindowState>,
    pending: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// The window's root view: the shell around the open view.
pub struct ShellView {
    view: View,
    /// The view to go back to when the Settings button is clicked again: the
    /// one open before Settings.
    before_settings: View,
    transactions: Entity<TransactionsView>,
    /// The Overview (M4.1): the month's figures, the chart, categories and
    /// recurring payments, filtered by the accounts panel's selection.
    overview: Entity<OverviewView>,
    overview_subscriptions: Vec<Subscription>,
    accounts: Entity<Accounts>,
    /// The suggestions (M3.6), which the Inbox's panel, canvas and the
    /// tab's badge share.
    inbox: Entity<Inbox>,
    inbox_view: Entity<InboxView>,
    /// The review queues (M3.12), under CHECKS in the Inbox's panel, and
    /// the status bar's failing balance checks.
    checks: Entity<Checks>,
    checks_view: Entity<ChecksView>,
    checks_subscriptions: Vec<Subscription>,
    /// The Rules view (M3.13): rules, merchant names and change history.
    rules: Entity<Rules>,
    rules_view: Entity<RulesView>,
    rules_subscriptions: Vec<Subscription>,
    settings: Entity<SettingsView>,
    /// The dialog open over the window, and its subscription.
    dialog: Option<(AnyView, Subscription)>,
    /// What the status bar counts, as Transactions last said.
    counts: Counts,
    /// What the accounts panel's CATEGORIES shows, as Transactions last
    /// said.
    panel_categories: PanelCategories,
    /// Where imports go, and the notification after one (M3.4).
    imports: Entity<Imports>,
    /// The Import dialog while it's the one open, for a file dropped on
    /// the window.
    import_dialog: Option<WeakEntity<ImportDialog>>,
    /// The accounts panel's focus: a Tab stop where the arrow keys move the
    /// selected account.
    panel_focus: FocusHandle,
    focus: FocusHandle,
    persistence: Option<Persistence>,
    /// The time the status bar's "Imported 2 min ago" counts from, and
    /// the task that keeps it current ([`Self::keep_time`]).
    now: DateTime<Utc>,
    clock: Option<Task<()>>,
    /// Live updates ([`live`]): `None` until [`Self::watch`], and in tests
    /// that don't watch, where the app's own writes reload the views at
    /// once.
    live: Option<Live>,
    /// What arrived from other processes (LU.5).
    arrived: Shown,
    _subscriptions: Vec<Subscription>,
}

/// What a shell is built from: each view's data, before the shell
/// subscribes to it.
struct Parts {
    accounts: Entity<Accounts>,
    source: transactions::Source,
    inbox: Entity<Inbox>,
    settings: Entity<SettingsView>,
    imports: Imports,
    overview: OverviewView,
}

impl ShellView {
    /// The shell showing `view` over the database at `database`, as `orre`
    /// opens it; with `Err` (no data folder, and why), every view says so,
    /// Transactions and the Overview included: no sample data (W1-3).
    pub fn from_database(
        view: View,
        database: Result<PathBuf, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let no_folder = |error: &String| format!("Orre has no data folder: {error}");
        let (accounts, source, inbox, checks, rules, imports, overview) = match &database {
            Ok(database) => (
                Accounts::unloaded(AccountStore::new(database.clone())),
                transactions::Source::Database(TransactionStore::new(database.clone())),
                Inbox::unloaded(InboxStore::new(database.clone())),
                Checks::unloaded(ChecksStore::new(database.clone())),
                Rules::unloaded(RulesStore::new(database.clone())),
                Imports::from_store(ImportStore::new(database.clone())),
                // Read in the background, as of today.
                OverviewView::unloaded(OverviewStore::new(database.clone()), crate::today()),
            ),
            // Without a data folder there's nothing to read: each view says
            // why.
            Err(error) => (
                Accounts::unavailable(no_folder(error)),
                transactions::Source::Unavailable(no_folder(error).into()),
                Inbox::unavailable(no_folder(error)),
                Checks::unavailable(no_folder(error)),
                Rules::unavailable(no_folder(error)),
                Imports::unavailable(no_folder(error)),
                OverviewView::unavailable(no_folder(error)),
            ),
        };
        let accounts = cx.new(|_| accounts);
        let inbox = cx.new(|_| inbox);
        let settings = cx.new(|cx| match &database {
            Ok(database) => SettingsView::from_store(SettingsStore::new(database.clone()), cx),
            Err(error) => SettingsView::unavailable(no_folder(error), cx),
        });
        let parts = Parts {
            accounts,
            source,
            inbox,
            settings,
            imports,
            overview,
        };
        let mut shell = ShellView::with_parts(view, parts, window, cx);
        let checks = cx.new(|_| checks);
        checks.update(cx, |checks, cx| checks.start_loading(cx));
        shell.install_checks(checks, window, cx);
        let rules = cx.new(|_| rules);
        shell.install_rules(rules, window, cx);
        let overview = shell.overview.clone();
        shell.install_overview(overview.clone(), window, cx);
        if database.is_ok() {
            overview.update(cx, |overview, cx| overview.reload(cx));
        }
        shell
    }

    /// The shell showing `view` with the sample accounts, keeping no window
    /// state (tests, snapshots).
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn new(view: View, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let accounts = cx.new(|_| Accounts::fixture(accounts::fixture::accounts()));
        Self::with_accounts(view, accounts, window, cx)
    }

    /// The shell showing `view` with `accounts` in its panel, the sample
    /// transactions, an empty Inbox and Settings on sample data.
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn with_accounts(
        view: View,
        accounts: Entity<Accounts>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_sources(view, accounts, transactions::Source::Fixture, window, cx)
    }

    /// The shell showing `view` with `accounts` in its panel, the
    /// transactions from `source`, an empty Inbox and Settings on sample
    /// data.
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn with_sources(
        view: View,
        accounts: Entity<Accounts>,
        source: transactions::Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let inbox = cx.new(|_| Inbox::empty());
        let settings = cx.new(SettingsView::fixture);
        Self::with_all(view, accounts, source, inbox, settings, window, cx)
    }

    /// The shell showing `view` with `accounts`, the sample transactions,
    /// `inbox` and Settings on sample data.
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn with_data(
        view: View,
        accounts: Entity<Accounts>,
        inbox: Entity<Inbox>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = cx.new(SettingsView::fixture);
        let source = transactions::Source::Fixture;
        Self::with_all(view, accounts, source, inbox, settings, window, cx)
    }

    /// The shell showing `view` with `accounts`, the sample transactions, an
    /// empty Inbox and `settings` as Settings.
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn with_settings(
        view: View,
        accounts: Entity<Accounts>,
        settings: Entity<SettingsView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let inbox = cx.new(|_| Inbox::empty());
        let source = transactions::Source::Fixture;
        Self::with_all(view, accounts, source, inbox, settings, window, cx)
    }

    /// The shell showing `view` with `accounts`, the transactions from
    /// `source`, `inbox` and `settings`; the Overview and the Import dialog
    /// on sample data until the shell is given the database's (tests,
    /// snapshots).
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn with_all(
        view: View,
        accounts: Entity<Accounts>,
        source: transactions::Source,
        inbox: Entity<Inbox>,
        settings: Entity<SettingsView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let parts = Parts {
            accounts,
            source,
            inbox,
            settings,
            imports: Imports::sample(),
            overview: OverviewView::fixture(),
        };
        Self::with_parts(view, parts, window, cx)
    }

    fn with_parts(view: View, parts: Parts, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let Parts {
            accounts,
            source,
            inbox,
            settings,
            imports,
            overview,
        } = parts;
        // A view this build doesn't show opens as the default.
        let view = if view.is_shown() {
            view
        } else {
            View::default()
        };
        let focus = cx.focus_handle();
        // Focused from the start, so the shortcuts work before any click.
        focus.focus(window, cx);
        let transactions = cx.new(|cx| {
            let mut transactions = TransactionsView::new(source, window, cx);
            transactions.show_accounts(accounts.clone(), window, cx);
            transactions.reload_through_shell();
            transactions
        });
        let panel_categories = transactions.read(cx).panel_categories();
        accounts.update(cx, |accounts, cx| {
            accounts.reload_through_shell();
            accounts.start_loading(cx);
        });
        inbox.update(cx, |inbox, _| inbox.reload_through_shell());
        // The app's own writes reload the views through the shell ([`live`]).
        let subscriptions = vec![
            cx.subscribe(&transactions, |this, _, wrote: &Wrote, cx| {
                this.refresh(wrote.0.clone(), cx)
            }),
            cx.subscribe(&accounts, |this, _, wrote: &Wrote, cx| {
                this.refresh(wrote.0.clone(), cx)
            }),
            cx.subscribe(&inbox, |this, _, wrote: &Wrote, cx| {
                this.refresh(wrote.0.clone(), cx)
            }),
            cx.subscribe(&settings, |this, _, wrote: &Wrote, cx| {
                this.refresh(wrote.0.clone(), cx)
            }),
            // What another process changed, shown where it lands (LU.5).
            cx.subscribe(&transactions, |this, _, event: &RegisterChanged, cx| {
                this.register_changed(event, cx)
            }),
            cx.subscribe(&inbox, |this, _, event: &Arrived, cx| {
                this.suggestions_arrived(event, cx)
            }),
            cx.subscribe_in(
                &accounts,
                window,
                |this, _, event: &AccountsEvent, window, cx| {
                    this.on_accounts_event(*event, window, cx)
                },
            ),
            cx.observe(&accounts, |_, _, cx| cx.notify()),
            cx.observe(&inbox, |_, _, cx| cx.notify()),
            // The status bar counts the enabled AI clients.
            cx.observe(&settings, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &settings,
                window,
                |this, _, event: &SettingsEvent, window, cx| {
                    this.on_settings_event(event.clone(), window, cx)
                },
            ),
            cx.subscribe_in(
                &transactions,
                window,
                |this, _, event: &TransactionsEvent, window, cx| {
                    this.on_transactions_event(event.clone(), window, cx)
                },
            ),
            // The status bar's counts and the panel's categories follow
            // the register and the table's filter.
            cx.observe(&transactions, |this, transactions, cx| {
                let counts = transactions.read(cx).counts();
                let categories = transactions.read(cx).panel_categories();
                if counts != this.counts || categories != this.panel_categories {
                    this.counts = counts;
                    this.panel_categories = categories;
                    cx.notify();
                }
            }),
        ];
        let imports = cx.new(|_| imports);
        let mut subscriptions = subscriptions;
        subscriptions.push(Self::watch_imports(&imports, cx));
        subscriptions.push(Self::watch_import_writes(&imports, cx));
        let inbox_view = cx.new(|cx| InboxView::new(inbox.clone(), cx));
        if view == View::Inbox {
            inbox_view.read(cx).focus_handle().clone().focus(window, cx);
        }
        let checks = cx.new(|_| Checks::empty());
        let checks_view = cx.new(|cx| ChecksView::new(checks.clone(), cx));
        let rules = cx.new(|_| Rules::empty());
        let rules_view = cx.new(|cx| RulesView::new(rules.clone(), window, cx));
        let overview = cx.new(|_| overview);
        let mut shell = Self {
            view,
            before_settings: View::default(),
            transactions,
            overview: overview.clone(),
            overview_subscriptions: Vec::new(),
            accounts,
            inbox,
            inbox_view,
            checks,
            checks_view,
            checks_subscriptions: Vec::new(),
            rules,
            rules_view,
            rules_subscriptions: Vec::new(),
            settings,
            dialog: None,
            counts: Counts::default(),
            panel_categories,
            imports,
            import_dialog: None,
            panel_focus: cx.focus_handle(),
            focus,
            persistence: None,
            now: clock::now().unwrap_or_default(),
            clock: None,
            live: None,
            arrived: Shown::default(),
            _subscriptions: subscriptions,
        };
        shell.install_overview(overview, window, cx);
        shell
    }

    /// Imports go into the database `store` opens from now on (without
    /// one, the Import dialog shows the sample export and can't import).
    pub fn import_into(&mut self, store: ImportStore, cx: &mut Context<Self>) {
        self.imports = cx.new(|_| Imports::from_store(store));
        let subscription = Self::watch_imports(&self.imports, cx);
        self._subscriptions.push(subscription);
        let subscription = Self::watch_import_writes(&self.imports, cx);
        self._subscriptions.push(subscription);
    }

    /// After an import or its undo, the views of the ledger read the
    /// database again: the panel, Transactions, and the checks (possible
    /// duplicates added or cleared, the balance checks changed).
    fn watch_imports(imports: &Entity<Imports>, cx: &mut Context<Self>) -> Subscription {
        cx.subscribe(imports, |this, _, _: &ImportsChanged, cx| {
            this.own_write_done(BTreeSet::from([Topic::Ledger]), cx);
            // "Imported just now", not the time last read.
            if let Ok(now) = clock::now() {
                this.now = now;
            }
            cx.notify();
        })
    }

    /// The checks wait while the app's own import or undo writes
    /// ([`ImportWrite`]).
    fn watch_import_writes(imports: &Entity<Imports>, cx: &mut Context<Self>) -> Subscription {
        cx.subscribe(imports, |this, _, write: &ImportWrite, cx| match write {
            ImportWrite::Started => this.own_write_started(cx),
            ImportWrite::Failed => this.own_write_failed(cx),
        })
    }

    /// Reads the time every half minute from now on, so the status bar's
    /// "Imported 2 min ago" keeps up (tests and snapshots keep the time
    /// the shell opened at).
    pub fn keep_time(&mut self, cx: &mut Context<Self>) {
        self.clock = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CLOCK_TICK).await;
                let Ok(now) = clock::now() else { continue };
                if this.update(cx, |this, cx| this.set_now(now, cx)).is_err() {
                    return;
                }
            }
        }));
    }

    /// Sets the time, redrawing only when the status bar could change: on
    /// a new minute. Each new minute also tells the accounts today's date,
    /// so on a new local day they read again and an account whose closing
    /// day has come leaves the panel (SLI-77).
    fn set_now(&mut self, now: DateTime<Utc>, cx: &mut Context<Self>) {
        let minute = |time: DateTime<Utc>| time.timestamp().div_euclid(60);
        let changed = minute(now) != minute(self.now);
        self.now = now;
        if changed {
            let today = crate::today();
            self.accounts
                .update(cx, |accounts, cx| accounts.follow_day(today, cx));
            cx.notify();
        }
    }

    /// What the status bar says now.
    fn status(&self, cx: &App) -> Status {
        let settings = self.settings.read(cx);
        let checks = self.checks.read(cx);
        Status::read(&status::Inputs {
            ai_on: settings.ai_on(),
            enabled_clients: settings.ai().enabled(),
            failing: checks.failing(),
            has_checks: checks.has_checks(),
            accounts: self.accounts.read(cx).data(),
            counts: self.transactions.read(cx).counts(),
            now: self.now,
            arrival: self.arrived.arrivals().line(),
        })
    }

    /// Where imports go, and the notification after one.
    pub fn imports(&self) -> &Entity<Imports> {
        &self.imports
    }

    /// Opens the Import dialog, reading `path` at once when given (a file
    /// dropped on the window). A file dropped while it's open goes to it.
    pub fn open_import(
        &mut self,
        path: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.import_dialog.as_ref().and_then(WeakEntity::upgrade) {
            if let Some(path) = path {
                open.update(cx, |dialog, cx| dialog.open_dropped(&[path], cx));
            }
            return;
        }
        if self.dialog.is_some() {
            return;
        }
        let imports = self.imports.clone();
        let selected = self.accounts.read(cx).selected();
        let accounts = self.accounts.clone();
        let dialog = cx.new(|cx| {
            ImportDialog::new(imports, selected, path, window, cx).with_accounts(accounts)
        });
        self.import_dialog = Some(dialog.downgrade());
        self.open_dialog(dialog, window, cx);
    }

    /// Opens the Import dialog on the first of `paths`, dropped together on
    /// the window; the dialog says so when it left others.
    pub fn open_import_dropped(
        &mut self,
        paths: &[std::path::PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.import_dialog.as_ref().and_then(WeakEntity::upgrade) {
            open.update(cx, |dialog, cx| dialog.open_dropped(paths, cx));
            return;
        }
        let Some(first) = paths.first().cloned() else {
            return;
        };
        self.open_import(Some(first), window, cx);
        // A new dialog reads the first once its setup has loaded.
        if paths.len() > 1
            && let Some(open) = self.import_dialog.as_ref().and_then(WeakEntity::upgrade)
        {
            open.update(cx, |dialog, cx| dialog.note_dropped(paths.len(), cx));
        }
    }

    /// Shows the sample Import dialog (snapshots): the sample export on
    /// its Map step.
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn open_sample_import(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ImportDialog> {
        let imports = self.imports.clone();
        let dialog = cx.new(|cx| ImportDialog::sample(imports, window, cx));
        self.import_dialog = Some(dialog.downgrade());
        self.open_dialog(dialog.clone(), window, cx);
        dialog
    }

    /// Closes the open dialog, as its own Cancel would.
    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = None;
        self.import_dialog = None;
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// The accounts the panel shows.
    pub fn accounts(&self) -> &Entity<Accounts> {
        &self.accounts
    }

    /// The suggestions the Inbox shows.
    pub fn inbox(&self) -> &Entity<Inbox> {
        &self.inbox
    }

    /// Uses `checks` for the review queues (the shell starts with empty
    /// ones that read nothing).
    pub fn install_checks(
        &mut self,
        checks: Entity<Checks>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        checks.update(cx, |checks, _| checks.reload_through_shell());
        self.checks_subscriptions = vec![
            cx.subscribe_in(
                &checks,
                window,
                |this, _, event: &ChecksEvent, window, cx| {
                    this.on_checks_event(event.clone(), window, cx)
                },
            ),
            cx.subscribe(&checks, |this, _, wrote: &Wrote, cx| {
                this.refresh(wrote.0.clone(), cx)
            }),
            cx.observe(&checks, |_, _, cx| cx.notify()),
        ];
        self.checks_view = cx.new(|cx| ChecksView::new(checks.clone(), cx));
        self.checks = checks;
        cx.notify();
    }

    /// Shows `overview` as the Overview: filtered by the accounts panel's
    /// selection, counting the Inbox's pending suggestions, its Review
    /// opening the Inbox (the shell starts with the sample data).
    pub fn install_overview(
        &mut self,
        overview: Entity<OverviewView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (accounts, inbox) = (self.accounts.clone(), self.inbox.clone());
        overview.update(cx, |overview, cx| {
            overview.show_accounts(accounts, cx);
            overview.show_inbox(inbox, cx);
        });
        self.overview_subscriptions = vec![
            cx.subscribe_in(
                &overview,
                window,
                |this, _, event: &OverviewEvent, window, cx| match event {
                    OverviewEvent::ReviewRequested => this.show(View::Inbox, window, cx),
                },
            ),
            cx.observe(&overview, |_, _, cx| cx.notify()),
        ];
        self.overview = overview;
        cx.notify();
    }

    /// The Overview's state.
    pub fn overview(&self) -> &Entity<OverviewView> {
        &self.overview
    }

    /// Uses `rules` for the Rules view (the shell starts with an empty one
    /// that reads nothing).
    pub fn install_rules(
        &mut self,
        rules: Entity<Rules>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        rules.update(cx, |rules, _| rules.reload_through_shell());
        self.rules_subscriptions = vec![
            cx.subscribe_in(&rules, window, |this, _, event: &RulesEvent, window, cx| {
                this.on_rules_event(*event, window, cx)
            }),
            cx.subscribe(&rules, |this, _, wrote: &Wrote, cx| {
                this.refresh(wrote.0.clone(), cx)
            }),
            cx.observe(&rules, |_, _, cx| cx.notify()),
        ];
        self.rules_view = cx.new(|cx| RulesView::new(rules.clone(), window, cx));
        self.rules = rules;
        cx.notify();
    }

    /// The Rules view's state.
    pub fn rules(&self) -> &Entity<Rules> {
        &self.rules
    }

    /// The Rules view.
    pub fn rules_view(&self) -> &Entity<RulesView> {
        &self.rules_view
    }

    fn on_rules_event(&mut self, event: RulesEvent, window: &mut Window, cx: &mut Context<Self>) {
        let rules = self.rules.clone();
        match event {
            RulesEvent::DeleteRuleRequested(rule) => {
                let dialog = cx.new(|cx| DeleteRuleDialog::new(rules, rule, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            RulesEvent::DeleteNameRequested(name) => {
                let dialog = cx.new(|cx| DeleteNameDialog::new(rules, name, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            RulesEvent::UndoRequested(batch) => {
                let dialog = cx.new(|cx| UndoChangeDialog::new(rules, batch, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            RulesEvent::ReviewSuggestion(id) => {
                self.show(View::Inbox, window, cx);
                self.checks.update(cx, |checks, cx| checks.hide(cx));
                self.inbox.update(cx, |inbox, cx| {
                    inbox.show_status(orre_core::inbox::SuggestionStatus::Pending, cx);
                    inbox.select(Some(id), cx);
                });
            }
            // What the other views read changed: the `Wrote` that came with
            // it names the ledger, and the shell's round reloads them.
            RulesEvent::Changed => {}
        }
    }

    /// The review queues.
    pub fn checks(&self) -> &Entity<Checks> {
        &self.checks
    }

    fn on_checks_event(&mut self, event: ChecksEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            ChecksEvent::Shown(queue) => {
                if self.view != View::Inbox {
                    self.show(View::Inbox, window, cx);
                }
                let focus = match queue {
                    Some(_) => self.checks_view.read(cx).focus_handle().clone(),
                    None => self.inbox_view.read(cx).focus_handle().clone(),
                };
                focus.focus(window, cx);
                cx.notify();
            }
            ChecksEvent::LockRequested(account) => {
                let checks = self.checks.clone();
                let dialog = cx.new(|cx| LockDialog::new(checks, account, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            ChecksEvent::UnlockRequested(account) => {
                let checks = self.checks.clone();
                let dialog = cx.new(|cx| UnlockDialog::new(checks, account, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            ChecksEvent::LinkPaymentRequested(label, currency) => {
                self.open_link_dialog(
                    crate::transactions::links::LinkFrom::Person(label, currency),
                    window,
                    cx,
                );
            }
            ChecksEvent::AddBalanceRequested(account) => {
                self.on_accounts_event(AccountsEvent::AddBalanceRequested(account), window, cx)
            }
            // A merge, an undo or a lock: the `Wrote` that came with it
            // reloads the accounts panel and Transactions.
            ChecksEvent::Changed => {}
        }
    }

    /// What the Transactions screen asks for: Rules on a rule to create or
    /// open (M3.13), the link dialog, and Owed to you reading again after a
    /// link or share changed (M3.11).
    fn on_transactions_event(
        &mut self,
        event: TransactionsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TransactionsEvent::CreateRule(rule, origin) => {
                self.show(View::Rules, window, cx);
                self.rules
                    .update(cx, |rules, cx| rules.new_rule_from(&rule, origin, cx));
            }
            TransactionsEvent::OpenRule(rule) => {
                self.show(View::Rules, window, cx);
                self.rules
                    .update(cx, |rules, cx| rules.select_rule(Some(rule), cx));
            }
            TransactionsEvent::LinkRequested(from) => {
                self.open_link_dialog(from, window, cx);
            }
            // Links, shares or the ledger changed: the `Wrote` that came
            // with it reloads Owed to you, the accounts and the checks.
            TransactionsEvent::Changed | TransactionsEvent::LedgerChanged => {}
            TransactionsEvent::ExcludeRequested(transaction) => {
                self.open_exclude_dialog(transaction, window, cx);
            }
            TransactionsEvent::UndoImportRequested(batch) => {
                let transactions = self.transactions.clone();
                let dialog = cx.new(|cx| UndoImportDialog::new(transactions, batch, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            TransactionsEvent::DeleteRequested(transaction) => {
                let transactions = self.transactions.clone();
                let dialog = cx
                    .new(|cx| DeleteTransactionDialog::new(transactions, transaction, window, cx));
                self.open_dialog(dialog, window, cx);
            }
        }
    }

    /// Opens the Exclude dialog on `transaction` ("Exclude…", or "Edit
    /// reason…" on an excluded row).
    pub fn open_exclude_dialog(
        &mut self,
        transaction: orre_core::id::TransactionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ExcludeDialog> {
        let transactions = self.transactions.clone();
        let dialog = cx.new(|cx| ExcludeDialog::new(transactions, transaction, window, cx));
        self.open_dialog(dialog.clone(), window, cx);
        dialog
    }

    /// ⌘N: opens the New-transaction dialog with the selected account
    /// preset, on Transactions only. Another dialog keeps the window.
    pub fn open_new_transaction(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<NewTransactionDialog>> {
        if self.view != View::Transactions || self.dialog.is_some() {
            return None;
        }
        let transactions = self.transactions.clone();
        let account = transactions.read(cx).selected_account();
        let dialog = cx.new(|cx| NewTransactionDialog::new(transactions, account, window, cx));
        self.open_dialog(dialog.clone(), window, cx);
        Some(dialog)
    }

    /// Shows `dialog` over the window (snapshots: a dialog built over
    /// sample data).
    pub fn show_dialog<D>(&mut self, dialog: Entity<D>, window: &mut Window, cx: &mut Context<Self>)
    where
        D: Render + gpui_kit::EventEmitter<DialogClosed>,
    {
        self.open_dialog(dialog, window, cx);
    }

    /// Opens the link dialog from `from` (a transaction's "Link…", a
    /// person's "Link a payment…").
    pub fn open_link_dialog(
        &mut self,
        from: crate::transactions::links::LinkFrom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<LinkDialog> {
        let transactions = self.transactions.clone();
        let dialog = cx.new(|cx| LinkDialog::new(transactions, from, window, cx));
        self.open_dialog(dialog.clone(), window, cx);
        dialog
    }

    /// Opens Balance checks on `account`'s failing check: the flagged dot
    /// in the Accounts panel, and the status bar's failing checks.
    pub fn show_balance_checks(
        &mut self,
        account: Option<orre_core::id::AccountId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show(View::Inbox, window, cx);
        self.checks.update(cx, |checks, cx| match account {
            Some(account) => checks.show_account(account, cx),
            None => checks.show(Queue::Balances, cx),
        });
    }

    /// Settings: AI access, backups and export.
    pub fn settings(&self) -> &Entity<SettingsView> {
        &self.settings
    }

    /// The dialog open over the window, if any.
    pub fn dialog(&self) -> Option<&AnyView> {
        self.dialog.as_ref().map(|(view, _)| view)
    }

    fn on_accounts_event(
        &mut self,
        event: AccountsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            // Selecting an account shows it in the inspector, which a
            // selected transaction would cover.
            AccountsEvent::Selected(Some(_)) => self
                .transactions
                .update(cx, |transactions, cx| transactions.clear_selection(cx)),
            AccountsEvent::Selected(None) => self.transactions.update(cx, |_, cx| cx.notify()),
            AccountsEvent::NewAccountRequested => {
                let accounts = self.accounts.clone();
                let dialog = cx.new(|cx| NewAccountDialog::new(accounts, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            AccountsEvent::NewValuedAccountRequested => {
                let accounts = self.accounts.clone();
                let dialog = cx.new(|cx| {
                    NewAccountDialog::with_kind(
                        accounts,
                        orre_core::account::AccountKind::Asset,
                        window,
                        cx,
                    )
                });
                self.open_dialog(dialog, window, cx);
            }
            AccountsEvent::AddBalanceRequested(account) => {
                let accounts = self.accounts.clone();
                let dialog = cx.new(|cx| AddBalanceDialog::new(accounts, account, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            AccountsEvent::EditBalanceRequested(account, balance) => {
                let accounts = self.accounts.clone();
                let dialog =
                    cx.new(|cx| AddBalanceDialog::edit(accounts, account, balance, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            // The accounts panel's context menus (M3.P10).
            AccountsEvent::ShowTransactionsRequested(account)
            | AccountsEvent::InspectRequested(account) => self.show_account(account, window, cx),
            AccountsEvent::NewTransactionRequested(account) => {
                self.show_account(account, window, cx);
                if self.dialog.is_none() {
                    let transactions = self.transactions.clone();
                    let dialog = cx.new(|cx| {
                        NewTransactionDialog::new(transactions, Some(account), window, cx)
                    });
                    self.open_dialog(dialog, window, cx);
                }
            }
            // `open_import` reads the selected account.
            AccountsEvent::ImportRequested(account) => {
                self.accounts
                    .update(cx, |accounts, cx| accounts.select(Some(account), cx));
                self.open_import(None, window, cx);
            }
            AccountsEvent::CategoryShowRequested(pick) => {
                self.show(View::Transactions, window, cx);
                self.transactions.update(cx, |transactions, cx| {
                    transactions.pick_category(pick, false, cx)
                });
            }
            AccountsEvent::CategoryRulesRequested(category) => {
                self.show(View::Rules, window, cx);
                self.rules.update(cx, |rules, cx| {
                    rules.select_category(
                        Some(crate::rules::CategorySelection::Stored(category)),
                        cx,
                    )
                });
            }
            AccountsEvent::CloseRequested(account) => {
                let accounts = self.accounts.clone();
                let dialog = cx.new(|cx| CloseAccountDialog::new(accounts, account, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            AccountsEvent::DeleteRequested(account) => {
                let accounts = self.accounts.clone();
                let dialog = cx.new(|cx| DeleteAccountDialog::new(accounts, account, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            AccountsEvent::ChecksRequested(account) => {
                self.show_balance_checks(Some(account), window, cx)
            }
            // From another view a row shows its transactions; on
            // Transactions, the picked row again clears the filter.
            AccountsEvent::CategoryPicked(pick) => {
                let toggle = self.view == View::Transactions;
                self.show(View::Transactions, window, cx);
                self.transactions.update(cx, |transactions, cx| {
                    transactions.pick_category(pick, toggle, cx)
                });
            }
            AccountsEvent::NewCategoryRequested => {
                self.show(View::Rules, window, cx);
                self.rules.update(cx, |rules, cx| rules.new_category(cx));
            }
            // The balance written came with a `Wrote`, which reloads the
            // checks.
            AccountsEvent::BalancesChanged => {}
        }
    }

    /// Selects `account` and shows Transactions, where its inspector is
    /// (no transaction stays selected over it).
    fn show_account(
        &mut self,
        account: orre_core::id::AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.accounts
            .update(cx, |accounts, cx| accounts.select(Some(account), cx));
        self.show(View::Transactions, window, cx);
        self.transactions
            .update(cx, |transactions, cx| transactions.clear_selection(cx));
    }

    fn on_settings_event(
        &mut self,
        event: SettingsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.settings.clone();
        match event {
            SettingsEvent::EnableRequested(name) => {
                let dialog = cx.new(|cx| EnableDialog::new(settings, name, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            SettingsEvent::RemoveRequested(name) => {
                let dialog = cx.new(|cx| RemoveDialog::new(settings, name, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            SettingsEvent::RestoreRequested(target) => {
                let dialog = cx.new(|cx| RestoreDialog::new(settings, target, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            SettingsEvent::DeleteRequested(target) => {
                let dialog = cx.new(|cx| DeleteBackupDialog::new(settings, target, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            SettingsEvent::ExportRequested => {
                let dialog = cx.new(|cx| ExportDialog::new(settings, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            // No checks while the app's own restore runs ([`live`]).
            SettingsEvent::RestoreStarted => self.pause_watching(),
            // The database was replaced: every view reads it again.
            SettingsEvent::Restored => self.restored(cx),
            SettingsEvent::RestoreFailed => self.restore_failed(cx),
            SettingsEvent::AiAccessChanged => cx.notify(),
        }
    }

    /// Opens Settings on AI clients: the status bar's AI item.
    pub fn show_ai_clients(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show(View::Settings, window, cx);
        self.settings
            .update(cx, |settings, cx| settings.show(Section::Clients, cx));
    }

    /// Shows `dialog` over the window until it closes, then puts focus back
    /// on the shell. What a dialog wrote reloads the views through the
    /// view it wrote with ([`live`]), not here.
    fn open_dialog<D>(&mut self, dialog: Entity<D>, window: &mut Window, cx: &mut Context<Self>)
    where
        D: Render + gpui_kit::EventEmitter<DialogClosed>,
    {
        let subscription =
            cx.subscribe_in(&dialog, window, |this, _, _: &DialogClosed, window, cx| {
                this.dialog = None;
                this.import_dialog = None;
                this.focus.focus(window, cx);
                cx.notify();
            });
        self.dialog = Some((dialog.into(), subscription));
        cx.notify();
    }

    /// The open view.
    pub fn view(&self) -> View {
        self.view
    }

    /// The Transactions screen, which stays alive while other views are
    /// open, so its selection and scroll position survive a switch.
    pub fn transactions(&self) -> &Entity<TransactionsView> {
        &self.transactions
    }

    /// Opens `view`, and takes focus back to the shell: whatever had it (the
    /// Transactions table) may have left the window, and a focus outside the
    /// window would leave the shortcuts nowhere to go.
    pub fn show(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        // An unbuilt view has no tab in a release build, so nothing shows it.
        if self.view == view || !view.is_shown() {
            return;
        }
        if view == View::Inbox {
            // What approving would change is computed when shown (decision
            // record "Inbox impact preview", point 6), and A, R and ↑↓ work
            // at once.
            self.inbox.update(cx, |inbox, cx| inbox.reload(cx));
            // The CHECKS counts, and the open queue, are read again too.
            self.checks.update(cx, |checks, cx| checks.reload(cx));
            let focus = if self.checks.read(cx).queue().is_some() {
                self.checks_view.read(cx).focus_handle().clone()
            } else {
                self.inbox_view.read(cx).focus_handle().clone()
            };
            focus.focus(window, cx);
        } else {
            self.focus.focus(window, cx);
        }
        if view == View::Overview {
            // The day may have moved on since the last read (a new month).
            self.overview.update(cx, |overview, cx| overview.reload(cx));
        }
        if view == View::Rules {
            // What each rule decides and the history change while it's
            // closed.
            self.rules.update(cx, |rules, cx| rules.reload(cx));
            self.rules_view
                .read(cx)
                .focus_handle()
                .clone()
                .focus(window, cx);
        }
        if view == View::Settings {
            self.before_settings = self.view;
            // What clients did and which backups exist change while it's
            // closed.
            self.settings.update(cx, |settings, cx| settings.reload(cx));
        }
        self.view = view;
        self.see_on_show(cx);
        self.remember(window, cx);
        cx.notify();
    }

    /// The Settings button: opens Settings, or while it's open goes back to
    /// the view open before it (Transactions when the window opened on
    /// Settings). ⌘, only opens it.
    pub fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = if self.view == View::Settings {
            self.before_settings
        } else {
            View::Settings
        };
        self.show(view, window, cx);
    }

    /// Saves the window state to `store` from now on: shortly after the
    /// window moves, resizes or changes view, and when it closes or the app
    /// quits.
    pub fn keep_window_state(
        &mut self,
        store: WindowStore,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let subscriptions = vec![
            cx.observe_window_bounds(window, |this, window, cx| this.remember(window, cx)),
            cx.on_release(|this, _| this.save_now()),
            cx.on_app_quit(|this, _| {
                this.save_now();
                async {}
            }),
        ];
        self.persistence = Some(Persistence {
            store,
            latest: window_state::snapshot(window.window_bounds(), self.view),
            saved: None,
            pending: None,
            _subscriptions: subscriptions,
        });
    }

    /// Notes the window's current state and saves it after [`SAVE_DELAY`],
    /// unless it changes again first.
    fn remember(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(persistence) = self.persistence.as_mut() else {
            return;
        };
        let state = window_state::snapshot(window.window_bounds(), self.view);
        if state == persistence.latest {
            return;
        }
        persistence.latest = state;
        let store = persistence.store.clone();
        // Replacing the task cancels the save it was waiting to make.
        persistence.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let Ok(Some(state)) = this.update(cx, |this, _| this.take_unsaved()) else {
                return;
            };
            cx.background_executor()
                .spawn(async move { store.save(&state) })
                .await;
        }));
    }

    /// The latest state if it hasn't been saved, marked as saved.
    fn take_unsaved(&mut self) -> Option<WindowState> {
        let persistence = self.persistence.as_mut()?;
        if persistence.saved.as_ref() == Some(&persistence.latest) {
            return None;
        }
        persistence.saved = Some(persistence.latest.clone());
        persistence.saved.clone()
    }

    /// Saves the latest state now, on this thread: the window is closing or
    /// the app is quitting, and a background save might not finish.
    fn save_now(&mut self) {
        if let Some(persistence) = self.persistence.as_mut() {
            persistence.pending = None;
        }
        if let Some(state) = self.take_unsaved()
            && let Some(persistence) = &self.persistence
        {
            persistence.store.save(&state);
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> TitleBar {
        let bar = View::TABS.into_iter().fold(TitleBar::new(), |bar, view| {
            let mut tab = ViewTab::new(view.element_id(), view.label())
                .active(self.view == view)
                .on_click(cx.listener(move |this, _, window, cx| this.show(view, window, cx)));
            if view == View::Inbox {
                // Pending suggestions from AI clients; Orre's own
                // don't count (`DESIGN.md` → "Inbox impact preview").
                let pending = self.inbox.read(cx).badge();
                if pending > 0 {
                    // Unseen while it went up out of sight (LU.5).
                    let unseen = self.arrived.arrivals().inbox_badge_unseen().is_some();
                    tab = tab
                        .badge(SharedString::from(pending.to_string()))
                        .badge_unseen(unseen);
                }
            }
            bar.tab(tab)
        });
        bar.settings(
            IconButton::new(
                View::Settings.element_id(),
                Glyph::Settings,
                View::Settings.label(),
            )
            .active(self.view == View::Settings)
            .on_click(cx.listener(|this, _, window, cx| this.toggle_settings(window, cx))),
        )
    }

    /// The canvas of a view whose screen isn't built yet: the empty state,
    /// centred (`DESIGN.md` → "Review queues": an empty canvas centres its
    /// message).
    fn render_unbuilt(&self) -> impl IntoElement {
        let view = self.view;
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .justify_center()
            .bg(color::CANVAS)
            .child(
                EmptyState::new(format!("{} isn't built yet", view.label()))
                    .body("It comes in a later version of Orre. Transactions works now.")
                    .shortcut("⌘2", "Open Transactions"),
            )
    }
}

/// The status bar (`status`): from the database, items with nothing to
/// say left out; the sample data keeps Paper's. The AI item ("N agents
/// connected" or "AI access off") opens Settings → AI clients, and failing
/// balance checks open Balance checks.
fn status_bar(status: &Status, generation: u64, cx: &mut Context<ShellView>) -> StatusBar {
    let mut bar = StatusBar::new();
    let ai = match status.ai {
        Ai::Off => Some(StatusItem::text("AI access off").muted_icon(Glyph::Refused)),
        Ai::Connected(count) => Some(StatusItem::text(status::agents_text(count)).agent_dot()),
        Ai::None => None,
    };
    if let Some(ai) = ai {
        let shell = cx.entity().downgrade();
        bar = bar.start(ai.on_click(
            "status-ai-access",
            move |window: &mut Window, cx: &mut gpui_kit::App| {
                let _ = shell.update(cx, |shell, cx| shell.show_ai_clients(window, cx));
            },
        ));
    }
    // A failing balance check replaces "Reconciled" (`DESIGN.md` → "Review
    // queues"), and opens Balance checks.
    match status.checks {
        ChecksStatus::Failing(failing) => {
            let shell = cx.entity().downgrade();
            bar = bar.start(
                StatusItem::text(status::failing_text(failing))
                    .danger_dot()
                    .on_click("status-balance-checks", move |window, cx| {
                        let _ = shell
                            .update(cx, |shell, cx| shell.show_balance_checks(None, window, cx));
                    }),
            );
        }
        ChecksStatus::Reconciled => {
            bar = bar.start(StatusItem::text("Reconciled").icon(Glyph::Check));
        }
        ChecksStatus::None => {}
    }
    if let Some(import) = &status.import {
        bar = bar.start(StatusItem::text(SharedString::from(import.clone())));
    }
    bar = bar.start(StatusItem::mono(SharedString::from(
        status.transactions.clone(),
    )));
    if let Some(line) = &status.arrival {
        bar = bar.end(arrival_item(line, generation, cx));
    }
    if let Some(currency) = &status.currency {
        bar = bar.end(StatusItem::mono(SharedString::from(currency.clone())));
    }
    bar
}

/// The arrival line (LU.5, `DESIGN.md` → "Live updates: showing what
/// arrived"): its lead, the copy with a client's name in `moldavite-text`,
/// and a click that opens its view, except for removed rows.
fn arrival_item(line: &ArrivalLine, generation: u64, cx: &mut Context<ShellView>) -> StatusItem {
    let mut item = line
        .spans
        .iter()
        .fold(StatusItem::text(""), |item, span| {
            item.span(SharedString::from(span.text.clone()), span.emphasis)
        })
        .secondary();
    item = match line.lead {
        arrivals::Lead::Sparkle => item.sparkle(),
        arrivals::Lead::Dot => item.new_dot(),
        arrivals::Lead::None => item,
    };
    // Generation 0 is the sample's line, which was always there.
    item = match line.phase {
        Phase::Shown if generation == 0 => item,
        Phase::Shown => item.fade(format!("status-arrival-in-{generation}"), false),
        Phase::Fading { .. } => item.fade(format!("status-arrival-out-{generation}"), true),
    };
    if line.opens.is_some() {
        let shell = cx.entity().downgrade();
        item = item.on_click("status-arrival", move |window, cx| {
            let _ = shell.update(cx, |shell, cx| shell.arrival_clicked(window, cx));
        });
    }
    item
}

impl Render for ShellView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view;
        let notice = import::notice_layer(&self.imports, cx);
        // After closing an account: its undo bar (M3.17).
        let closed = view
            .has_accounts_panel()
            .then(|| accounts::notice_layer(&self.accounts, cx))
            .flatten();
        let body = div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .when(view.has_accounts_panel(), |this| {
                this.child(accounts::panel::render(
                    &self.accounts,
                    &self.panel_categories,
                    &self.panel_focus,
                    window,
                    cx,
                ))
            })
            .when(view == View::Inbox, |this| {
                this.child(inbox::panel::render(&self.inbox, &self.checks, cx))
            })
            .when(view == View::Rules, |this| {
                this.child(rules::panel::render(&self.rules, cx))
            })
            .map(|this| match view {
                View::Overview => this.child(self.overview.clone()),
                View::Transactions => this.child(self.transactions.clone()),
                View::Inbox if self.checks.read(cx).queue().is_some() => {
                    this.child(self.checks_view.clone())
                }
                View::Inbox => this.child(self.inbox_view.clone()),
                View::Rules => this.child(self.rules_view.clone()),
                View::Settings => this.child(self.settings.clone()),
                _ => this.child(self.render_unbuilt()),
            })
            .children(notice)
            .children(closed);
        window_border().child(
            div()
                .key_context(SHELL_CONTEXT)
                .track_focus(&self.focus)
                .on_action(cx.listener(|this, _: &ShowOverview, window, cx| {
                    this.show(View::Overview, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ShowTransactions, window, cx| {
                    this.show(View::Transactions, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ShowTrends, window, cx| {
                    this.show(View::Trends, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ShowBudgets, window, cx| {
                    this.show(View::Budgets, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ShowNetWorth, window, cx| {
                    this.show(View::NetWorth, window, cx)
                }))
                .on_action(
                    cx.listener(|this, _: &ShowInbox, window, cx| {
                        this.show(View::Inbox, window, cx)
                    }),
                )
                .on_action(cx.listener(|this, _: &ShowReviews, window, cx| {
                    this.show(View::Reviews, window, cx)
                }))
                .on_action(
                    cx.listener(|this, _: &ShowRules, window, cx| {
                        this.show(View::Rules, window, cx)
                    }),
                )
                .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                    this.show(View::Settings, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ImportStatement, window, cx| {
                    this.open_import(None, window, cx)
                }))
                .on_action(cx.listener(|this, _: &NewTransaction, window, cx| {
                    this.open_new_transaction(window, cx);
                }))
                // A statement dropped anywhere on the window (`6J4-0`).
                .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                    this.open_import_dropped(paths.paths(), window, cx);
                }))
                .size_full()
                .flex()
                .flex_col()
                .bg(color::CHROME)
                .text_role(typography::BODY)
                .text_color(color::TEXT)
                .child(self.render_title_bar(cx))
                .child(body)
                .child(status_bar(
                    &self.status(cx),
                    self.arrived.line_generation(),
                    cx,
                ))
                .when_some(
                    self.dialog.as_ref().map(|(view, _)| view.clone()),
                    |this, dialog| this.child(dialog),
                ),
        )
    }
}

#[cfg(test)]
mod tests;
