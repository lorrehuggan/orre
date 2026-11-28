//! Settings (ROADMAP M3.15, M3.16; `docs/DESIGN.md` → "Settings"; Paper
//! `AQ2-0`, `DC6-0`, `F63-0`, states `GQA-0`): AI access and the data.
//!
//! The title bar's Settings button (⌘,) opens it. The left panel ([`panel`])
//! has AI ACCESS: Clients and Access log; DATA: Backups and Export…;
//! CALENDAR: Month start; and at its foot the data file's path.
//!
//! - [`clients`]: the kill switch, the enabled clients (level, last call,
//!   calls in 7 days, on/off), the clients that asked to connect (Ignore,
//!   Enable…), what each level shares, and the client inspector (level,
//!   the command, the last 7 days, Remove client…).
//! - [`log`]: the access log by day, filtered by client and period, and
//!   the call inspector (the redacted arguments, the purpose as the
//!   client's untrusted words). Never the result hash (decided with the
//!   user, as `orre ai-access log`).
//! - [`backups`]: Recent and One a month, Back up now, Restore from a
//!   file…, and the backup inspector (what it holds, what restoring would
//!   undo, Restore…, Show in folder, Save a copy…).
//! - [`month_start`] (M4.11, Paper `K57-0`), under CALENDAR in the panel:
//!   the day months start on and what a weekend does to it, a preview of
//!   last, this and next month, and what follows the rule. A choice
//!   applies at once.
//! - Dialogs, opened by the shell over everything: [`enable`], [`remove`],
//!   [`restore`] and [`export`].
//!
//! [`SettingsView`] holds what the screens show and makes every change
//! through [`store::SettingsStore`], which opens the database per call; the
//! rules are core's. Without a database (snapshots, tests) it shows
//! `fixture` data, which can't change.

pub mod backups;
pub mod clients;
pub mod data;
pub mod delete;
#[cfg(any(test, feature = "dev-driver"))]
pub(crate) mod describe;
pub mod enable;
pub mod export;
#[cfg(any(test, feature = "dev-samples"))]
pub mod fixture;
pub mod log;
pub mod month_start;
pub mod panel;
pub mod remove;
pub mod restore;
pub mod store;
pub(crate) mod table;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, SharedString, Styled as _, Task, Window, div,
};
use orre_core::access_log::activity::Cursor;
use orre_core::backup::detail::Since;
use orre_core::mcp_client::{AccessLevel, ClientName};
use orre_core::reporting::period::MonthStart;
use orre_core::watch::Topic;

use crate::accounts::store::StoreError;
use crate::shell::live::Wrote;
use crate::theme::tokens::{color, spacing};
use crate::ui::list::{self, Move};
use crate::ui::menu::SelectState;

use data::{AiData, BackupsData, Clock, LogFilter, LogRow, Period};
use store::SettingsStore;

/// Binds the arrow keys of Settings' panel and lists. Call once.
pub fn init(cx: &mut App) {
    panel::KEYS.bind(cx);
    table::LIST_KEYS.bind(cx);
}

/// Which screen Settings shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Section {
    #[default]
    Clients,
    Log,
    Backups,
    /// The month start (M4.11).
    MonthStart,
}

impl Section {
    /// In panel order.
    pub const ALL: [Section; 4] = [
        Section::Clients,
        Section::Log,
        Section::Backups,
        Section::MonthStart,
    ];

    /// What the panel calls it.
    pub const fn label(self) -> &'static str {
        match self {
            Section::Clients => "Clients",
            Section::Log => "Access log",
            Section::Backups => "Backups",
            Section::MonthStart => month_start::TITLE,
        }
    }
}

/// What a dialog restores: a listed backup, or a file the user chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreTarget {
    pub path: PathBuf,
    /// When it was taken, for a listed backup (or a file with a backup's
    /// name): the title names it, and "since then" is counted from it.
    pub taken_at: Option<DateTime<Utc>>,
    /// That time, local.
    pub local: Option<chrono::NaiveDateTime>,
    /// Chosen with "Restore from a file…": the dialog offers "Choose
    /// another file…" (SLI-71).
    pub from_file: bool,
}

/// The backup the Delete backup dialog would delete (R.12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteTarget {
    pub path: PathBuf,
    /// When it was taken, as the title names it.
    pub taken: String,
    pub reason: orre_core::backup::Reason,
}

/// Something Settings asks the shell for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsEvent {
    /// Open the Enable dialog for a client that asked to connect.
    EnableRequested(ClientName),
    /// Open the Remove client dialog.
    RemoveRequested(ClientName),
    /// Open the Restore dialog.
    RestoreRequested(RestoreTarget),
    /// Open the Delete backup dialog (R.12).
    DeleteRequested(DeleteTarget),
    /// Open the export dialog.
    ExportRequested,
    /// A restore started: the shell stops checking for changes until it's
    /// done ([`crate::shell::live`]).
    RestoreStarted,
    /// A backup was restored: everything else reads the database again.
    Restored,
    /// The restore failed and nothing was replaced: checking resumes.
    RestoreFailed,
    /// The kill switch or the clients changed: the status bar follows.
    AiAccessChanged,
}

/// What just happened, in the notification bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub message: NoticeMessage,
    /// "Show in folder" reveals this file.
    pub reveal: Option<PathBuf>,
}

/// A notification's message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoticeMessage {
    /// After "Back up now": the transactions it holds and its size.
    BackedUp {
        transactions: Option<u64>,
        size: u64,
    },
    /// After an export: rows written and the file's name.
    Exported { rows: usize, file: String },
    /// After a copy of a backup was saved.
    Copied { file: String },
    /// After a backup was deleted: the space it freed.
    Deleted { size: u64 },
    /// After a restore: when the backup was taken ("28 Nov 2026, 11:40"),
    /// when known. One fact: whether it was updated and what the access log
    /// kept are under "Since this backup" on the backup it took first
    /// ([`LastRestore`]).
    Restored { from: Option<String> },
    /// Something failed; the text says what, and that nothing changed.
    Failed(SharedString),
}

/// What a restore did beyond replacing the database, shown on the backup it
/// took of the database it replaced (`SettingsView::restore_lines`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastRestore {
    pub pre_restore: PathBuf,
    /// The backup was older and updated to this version.
    pub migrated: bool,
    /// Newer access-log rows kept from the replaced database; `None` when
    /// carrying them failed.
    pub log_kept: Option<usize>,
}

/// Where Settings' data comes from.
enum Source {
    Database(SettingsStore),
    /// Sample data that can't change (tests, `dev-samples`).
    #[cfg(any(test, feature = "dev-samples"))]
    Fixture,
    /// Nothing to read, and why (no data folder).
    Unavailable(SharedString),
}

/// The computer's clock; the UTC time and [`crate::today`] when it can't
/// be read. Never the sample clock (W1-3).
fn clock_now() -> Clock {
    store::now().unwrap_or_else(|_| Clock {
        now: chrono::Utc::now(),
        today: crate::today(),
    })
}

/// A read that failed, shown where its data would be.
type LoadError = Option<SharedString>;

/// The Settings screen.
pub struct SettingsView {
    source: Source,
    section: Section,
    clock: Clock,
    /// The data file, as the panel's foot shows it (`~` for home).
    data_file: SharedString,

    ai: AiData,
    ai_error: LoadError,
    selected_client: Option<ClientName>,
    level_select: SelectState,

    log_filter: LogFilter,
    log: Vec<LogRow>,
    log_more: bool,
    log_error: LoadError,
    selected_call: Option<i64>,
    client_filter: SelectState,
    period_filter: SelectState,
    log_scroll: ScrollHandle,

    backups: BackupsData,
    backups_error: LoadError,
    selected_backup: Option<PathBuf>,
    /// What restoring the selected backup would undo.
    since: Option<(PathBuf, Since)>,
    /// That count being read in the background for the backup it names
    /// (`load_since`): it reads the backup, which may be large or hostile.
    /// A newer read replaces it and reading the backups again drops it, so
    /// a read for another backup, or an older database, can't land.
    since_read: Option<(PathBuf, Task<()>)>,
    /// A backup is being taken.
    backing_up: bool,

    /// The rule that cuts months (M4.11).
    month_start: MonthStart,
    month_start_error: LoadError,
    day_select: SelectState,

    notice: Option<Notice>,
    /// What the last restore in this window did (session only).
    last_restore: Option<LastRestore>,
    panel_focus: FocusHandle,
    list_focus: FocusHandle,
    _task: Option<Task<()>>,
    /// The AI clients being read in the background (`reload_clients`). A
    /// newer read replaces it, and any read of the clients on this thread
    /// drops it, so a read that started before can't land after.
    clients_read: Option<Task<()>>,
}

impl EventEmitter<SettingsEvent> for SettingsView {}
impl EventEmitter<Wrote> for SettingsView {}

/// `path` with the home directory written as `~`.
pub fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    match std::env::home_dir() {
        Some(home) if !home.as_os_str().is_empty() => match path.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => text,
        },
        _ => text,
    }
}

/// `message` as a sentence: a capital first letter and a full stop.
pub fn sentence(message: &str) -> SharedString {
    crate::accounts::new_account::sentence(message)
}

impl SettingsView {
    /// Settings over the database `store` opens, read now.
    pub fn from_store(store: SettingsStore, cx: &mut Context<Self>) -> Self {
        let clock = clock_now();
        let data_file = display_path(store.database()).into();
        let mut view = Self::empty(Source::Database(store), clock, data_file, cx);
        view.reload(cx);
        view
    }

    /// Settings showing sample data, which can't change.
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn fixture(cx: &mut Context<Self>) -> Self {
        let mut view = Self::empty(
            Source::Fixture,
            fixture::clock(),
            fixture::DATA_FILE.into(),
            cx,
        );
        view.ai = fixture::ai();
        view.log = fixture::log();
        view.backups = fixture::backups();
        view.month_start = fixture::month_start();
        view.since = view
            .backups
            .rows
            .first()
            .map(|row| (row.backup.path.clone(), fixture::since()));
        view.select_defaults(cx);
        view
    }

    /// Sample data whose month start is `start` (Paper `K57-0` and the
    /// states board `KEQ-0`), for snapshots and tests.
    pub fn with_month_start(mut self, start: MonthStart) -> Self {
        self.month_start = start;
        self
    }

    /// Sample data with AI access switched off (the states board, `GR9-0`).
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn fixture_off(cx: &mut Context<Self>) -> Self {
        let mut view = Self::fixture(cx);
        view.ai.on = false;
        view
    }

    /// Settings with nothing to show and why: no data folder.
    pub fn unavailable(reason: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let reason = reason.into();
        let mut view = Self::empty(
            Source::Unavailable(reason.clone()),
            clock_now(),
            "".into(),
            cx,
        );
        view.ai_error = Some(reason.clone());
        view.log_error = Some(reason.clone());
        view.month_start_error = Some(reason.clone());
        view.backups_error = Some(reason);
        view
    }

    fn empty(
        source: Source,
        clock: Clock,
        data_file: SharedString,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            source,
            section: Section::default(),
            clock,
            data_file,
            ai: AiData::default(),
            ai_error: None,
            selected_client: None,
            level_select: SelectState::new(cx),
            log_filter: LogFilter::default(),
            log: Vec::new(),
            log_more: false,
            log_error: None,
            selected_call: None,
            client_filter: SelectState::new(cx),
            period_filter: SelectState::new(cx),
            log_scroll: ScrollHandle::new(),
            backups: BackupsData::default(),
            backups_error: None,
            selected_backup: None,
            since: None,
            since_read: None,
            backing_up: false,
            month_start: MonthStart::CALENDAR,
            month_start_error: None,
            day_select: SelectState::new(cx),
            notice: None,
            last_restore: None,
            panel_focus: cx.focus_handle(),
            list_focus: cx.focus_handle(),
            _task: None,
            clients_read: None,
        }
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn ai(&self) -> &AiData {
        &self.ai
    }

    pub fn backups(&self) -> &BackupsData {
        &self.backups
    }

    /// The month start every report cuts months by.
    pub fn month_start(&self) -> MonthStart {
        self.month_start
    }

    pub fn log_rows(&self) -> &[LogRow] {
        &self.log
    }

    pub fn log_filter(&self) -> &LogFilter {
        &self.log_filter
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn selected_client(&self) -> Option<&ClientName> {
        self.selected_client.as_ref()
    }

    pub fn selected_backup(&self) -> Option<&PathBuf> {
        self.selected_backup.as_ref()
    }

    pub fn selected_call(&self) -> Option<i64> {
        self.selected_call
    }

    /// Whether AI access is on (the kill switch), for the status bar.
    pub fn ai_on(&self) -> bool {
        self.ai.on
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// The store, or why nothing can change.
    pub fn store(&self) -> Result<&SettingsStore, SharedString> {
        match &self.source {
            Source::Database(store) => Ok(store),
            #[cfg(any(test, feature = "dev-samples"))]
            Source::Fixture => Err("This is sample data, which can't be changed.".into()),
            Source::Unavailable(reason) => Err(reason.clone()),
        }
    }

    /// Whether it shows sample data (tests, `dev-samples`).
    #[cfg(any(test, feature = "dev-samples"))]
    pub fn is_sample(&self) -> bool {
        matches!(self.source, Source::Fixture)
    }

    /// Shows `section`, reading its data again.
    pub fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        self.notice = None;
        self.day_select.dismiss();
        self.reload_section(section);
        self.select_defaults(cx);
        cx.notify();
    }

    /// Reads everything again: after a restore, when Settings opens, and
    /// when another process changed the database. The section, the
    /// selections, the filters, the notice and the log's scroll position
    /// stay (a selection only moves when its row is gone), and the log
    /// keeps as many pages as were loaded so a call further down stays
    /// listed. Dialogs belong to the shell and are not touched.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let loaded = self.log.len();
        for section in Section::ALL {
            self.reload_section(section);
        }
        self.reload_log_pages(loaded);
        self.select_defaults(cx);
        cx.notify();
    }

    /// Reads the AI clients again, for the shell after they changed
    /// ([`crate::shell::live`]), on a background thread: it flushes refused
    /// calls into the access log (a write) and aggregates `mcp_access_log`,
    /// once with no time bound. LU.6 measured that at 100,000 transactions:
    /// 14 ms with 5,000 log rows but 129 ms with 100,000 (a year of heavy
    /// use), over the 50 ms budget. What is shown stays until the read lands;
    /// a newer read replaces one in flight, and the view's own writes drop
    /// it (`reload_section`). The selections stay where their rows still are.
    pub fn reload_clients(&mut self, cx: &mut Context<Self>) {
        let Source::Database(store) = &self.source else {
            return;
        };
        let store = store.clone();
        if let Ok(clock) = store::now() {
            self.clock = clock;
        }
        let clock = self.clock;
        let work = cx
            .background_executor()
            .spawn(async move { store.load_ai(&clock) });
        self.clients_read = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                this.clients_read = None;
                this.land_clients(result);
                this.select_defaults(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Reads the month start again, for the shell after the settings
    /// changed ([`crate::shell::live`]): a restore, or another process.
    pub fn reload_month_start(&mut self, cx: &mut Context<Self>) {
        self.reload_section(Section::MonthStart);
        cx.notify();
    }

    fn land_clients(&mut self, result: Result<AiData, StoreError>) {
        match result {
            Ok(ai) => {
                self.ai = ai;
                self.ai_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the AI clients");
                self.ai_error = Some(sentence(&error.to_string()));
            }
        }
    }

    /// The app's own write to the clients or the kill switch, after the
    /// clients were read again here: the shell rebases the Settings topic and
    /// reads them once more ([`crate::shell::live`]).
    fn wrote_clients(cx: &mut Context<Self>) {
        cx.emit(Wrote::of([Topic::Settings]));
    }

    /// Reads further pages of the log until it lists `rows` calls again, or
    /// runs out. Calls logged since push older ones down, so the count alone
    /// can leave the selected call out: then at most one page more is read
    /// for it, which covers the calls that arrive between two reloads. Past
    /// that (Settings opened again after thousands of calls, a call pruned
    /// or filtered out) the selection falls back to the newest call
    /// (`select_defaults`), rather than reading page after page on the UI
    /// thread, and a chase page that didn't find it is dropped again so
    /// failed chases don't each keep a page more. The log is newest first
    /// and IDs grow with time, so a last row older than the selected call
    /// means it has gone.
    fn reload_log_pages(&mut self, rows: usize) {
        let Source::Database(store) = &self.source else {
            return;
        };
        let store = store.clone();
        let selected_unlisted = |view: &Self| {
            view.selected_call.is_some_and(|id| {
                !view.log.iter().any(|row| row.call.id == id)
                    && view.log.last().is_some_and(|row| row.call.id > id)
            })
        };
        let mut before_chase = None;
        while self.log_more
            && self.log_error.is_none()
            && (self.log.len() < rows
                || (self.log.len() < rows + store::LOG_PAGE as usize && selected_unlisted(self)))
        {
            if self.log.len() >= rows && before_chase.is_none() {
                before_chase = Some((self.log.len(), self.log_more));
            }
            let after = self.log.last().map(|row| Cursor::after(&row.call));
            self.load_log(&store, after);
        }
        if let Some((len, more)) = before_chase
            && !self.selected_call_listed()
        {
            self.log.truncate(len);
            self.log_more = more;
        }
    }

    fn selected_call_listed(&self) -> bool {
        self.selected_call
            .is_some_and(|id| self.log.iter().any(|row| row.call.id == id))
    }

    fn reload_section(&mut self, section: Section) {
        let Source::Database(store) = &self.source else {
            return;
        };
        if let Ok(clock) = store::now() {
            self.clock = clock;
        }
        let store = store.clone();
        match section {
            Section::Clients => {
                self.clients_read = None;
                self.land_clients(store.load_ai(&self.clock));
            }
            Section::Log => self.load_log(&store, None),
            Section::Backups => {
                match store.load_backups() {
                    Ok(backups) => {
                        self.backups = backups;
                        self.backups_error = None;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "couldn't read the backups");
                        self.backups_error = Some(sentence(&error.to_string()));
                    }
                }
                self.since = None;
                self.since_read = None;
            }
            Section::MonthStart => match store.load_month_start() {
                Ok(start) => {
                    self.month_start = start;
                    self.month_start_error = None;
                }
                Err(error) => {
                    tracing::warn!(%error, "couldn't read the month start");
                    self.month_start_error = Some(sentence(&error.to_string()));
                }
            },
        }
    }

    /// Reads the first page of the log for the filter, or the next page
    /// after `after`.
    fn load_log(&mut self, store: &SettingsStore, after: Option<Cursor>) {
        let next = after.is_some();
        match store.load_log(&self.log_filter, &self.clock, after) {
            Ok(page) => {
                if next {
                    self.log.extend(page.rows);
                } else {
                    self.log = page.rows;
                }
                self.log_more = page.more;
                self.log_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the access log");
                self.log_error = Some(sentence(&error.to_string()));
            }
        }
    }

    /// Reads the next page of the log, when there is one: earlier days load
    /// as the list is scrolled to its end.
    pub fn load_more_log(&mut self, cx: &mut Context<Self>) {
        if !self.log_more {
            return;
        }
        let Source::Database(store) = &self.source else {
            return;
        };
        let store = store.clone();
        let after = self.log.last().map(|row| Cursor::after(&row.call));
        self.load_log(&store, after);
        cx.notify();
    }

    /// Selects the first row of each list whose selection is gone.
    fn select_defaults(&mut self, cx: &mut Context<Self>) {
        if self
            .selected_client
            .as_ref()
            .is_none_or(|name| self.ai.client(name).is_none())
        {
            self.selected_client = self.ai.clients.first().map(|client| client.name.clone());
        }
        if self
            .selected_call
            .is_none_or(|id| !self.log.iter().any(|row| row.call.id == id))
        {
            self.selected_call = self.log.first().map(|row| row.call.id);
        }
        if self
            .selected_backup
            .as_ref()
            .is_none_or(|path| self.backups.row(path).is_none())
        {
            self.selected_backup = self.backups.rows.first().map(|row| row.backup.path.clone());
        }
        self.load_since(cx);
    }

    /// Reads what restoring the selected backup would undo, unless known or
    /// being read, on a background thread: until it lands, "Since this
    /// backup" lists nothing.
    fn load_since(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected_backup.clone() else {
            self.since = None;
            self.since_read = None;
            return;
        };
        if self.since.as_ref().is_some_and(|(known, _)| *known == path)
            || self
                .since_read
                .as_ref()
                .is_some_and(|(reading, _)| *reading == path)
        {
            return;
        }
        let Source::Database(store) = &self.source else {
            return;
        };
        let Some(row) = self.backups.row(&path) else {
            return;
        };
        let store = store.clone();
        let (backup, taken_at) = (row.backup.path.clone(), row.backup.taken_at);
        let work = cx
            .background_executor()
            .spawn(async move { store.since(&backup, taken_at) });
        let reading = path.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                if this
                    .since_read
                    .as_ref()
                    .is_none_or(|(current, _)| *current != reading)
                {
                    return;
                }
                this.since_read = None;
                if this.selected_backup.as_ref() != Some(&reading) {
                    return;
                }
                this.since = match result {
                    Ok(since) => Some((reading, since)),
                    Err(error) => {
                        tracing::warn!(%error, "couldn't count what restoring would undo");
                        None
                    }
                };
                cx.notify();
            })
            .ok();
        });
        self.since_read = Some((path, task));
    }

    pub fn select_client(&mut self, name: ClientName, cx: &mut Context<Self>) {
        self.level_select.dismiss();
        self.selected_client = Some(name);
        cx.notify();
    }

    pub fn select_call(&mut self, id: i64, cx: &mut Context<Self>) {
        self.selected_call = Some(id);
        cx.notify();
    }

    pub fn select_backup(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.selected_backup = Some(path);
        self.load_since(cx);
        cx.notify();
    }

    /// Moves the selection in the open list (the arrow keys,
    /// [`list::step`]).
    pub fn select_next(&mut self, to: Move, cx: &mut Context<Self>) {
        match self.section {
            Section::Clients => {
                let names: Vec<ClientName> = self
                    .ai
                    .clients
                    .iter()
                    .map(|client| client.name.clone())
                    .collect();
                if let Some(name) = list::step(&names, self.selected_client.as_ref(), to) {
                    self.select_client(name, cx);
                }
            }
            Section::Log => {
                let ids: Vec<i64> = self.log.iter().map(|row| row.call.id).collect();
                if let Some(id) = list::step(&ids, self.selected_call.as_ref(), to) {
                    self.select_call(id, cx);
                }
            }
            Section::Backups => {
                let paths: Vec<PathBuf> = table::backup_order(&self.backups);
                if let Some(path) = list::step(&paths, self.selected_backup.as_ref(), to) {
                    self.select_backup(path, cx);
                }
            }
            // A form, not a list: nothing to move through.
            Section::MonthStart => {}
        }
    }

    /// Sets the month start, at once (no Save: decided by the user,
    /// 2026-11-25). Every view that cuts months re-reads: the shell reloads
    /// them on the Settings topic ([`crate::shell::live::views`]). A refused
    /// change is shown in the notification bar and leaves the rule as it was.
    pub fn set_month_start(&mut self, start: MonthStart, cx: &mut Context<Self>) {
        if start == self.month_start {
            return;
        }
        let result = match self.store() {
            Ok(store) => store
                .set_month_start(start)
                .map_err(|error| sentence(&error.to_string())),
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => {
                self.notice = None;
                self.reload_section(Section::MonthStart);
                cx.emit(Wrote::of([Topic::Settings]));
            }
            Err(error) => self.fail(error, cx),
        }
        cx.notify();
    }

    /// Moves to the next or previous section in the panel (the arrow keys
    /// there).
    pub fn show_next(&mut self, to: Move, cx: &mut Context<Self>) {
        if let Some(section) = list::step(&Section::ALL, Some(&self.section), to) {
            self.show(section, cx);
        }
    }

    /// Runs a change through the store, then reads the clients again and
    /// tells the shell. A refused change is shown in the notification bar.
    fn change_ai(
        &mut self,
        change: impl FnOnce(&SettingsStore) -> Result<(), StoreError>,
        cx: &mut Context<Self>,
    ) {
        let result = match self.store() {
            Ok(store) => change(store).map_err(|error| sentence(&error.to_string())),
            Err(error) => Err(error),
        };
        let wrote = result.is_ok();
        if let Err(error) = result {
            self.notice = Some(Notice {
                message: NoticeMessage::Failed(error),
                reveal: None,
            });
        }
        self.reload_section(Section::Clients);
        self.select_defaults(cx);
        if wrote {
            Self::wrote_clients(cx);
        }
        cx.emit(SettingsEvent::AiAccessChanged);
        cx.notify();
    }

    /// The kill switch.
    pub fn set_ai_on(&mut self, on: bool, cx: &mut Context<Self>) {
        self.change_ai(|store| store.set_ai_on(on), cx);
    }

    /// A client's ON switch: off keeps its level.
    pub fn turn_client(&mut self, name: ClientName, on: bool, cx: &mut Context<Self>) {
        self.change_ai(move |store| store.turn(&name, on), cx);
    }

    pub fn set_level(&mut self, name: ClientName, level: AccessLevel, cx: &mut Context<Self>) {
        self.change_ai(move |store| store.set_level(&name, level), cx);
    }

    /// "Ignore" on a request: hidden, not enabled.
    pub fn ignore(&mut self, name: ClientName, cx: &mut Context<Self>) {
        self.change_ai(move |store| store.ignore(&name), cx);
    }

    /// Enables a client at `level` (the Enable dialog). The error says why
    /// it wasn't.
    pub fn enable(
        &mut self,
        name: &ClientName,
        level: AccessLevel,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .enable(name, level)
            .map_err(|error| sentence(&error.to_string()))?;
        self.reload_section(Section::Clients);
        self.selected_client = Some(name.clone());
        self.select_defaults(cx);
        Self::wrote_clients(cx);
        cx.emit(SettingsEvent::AiAccessChanged);
        cx.notify();
        Ok(())
    }

    /// Removes a client (the Remove client dialog).
    pub fn remove(
        &mut self,
        name: &ClientName,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .remove(name)
            .map_err(|error| sentence(&error.to_string()))?;
        self.reload_section(Section::Clients);
        self.select_defaults(cx);
        Self::wrote_clients(cx);
        cx.emit(SettingsEvent::AiAccessChanged);
        cx.notify();
        Ok(())
    }

    /// "Open access log" in the client inspector: the log, that client's
    /// calls only.
    pub fn open_log_for(&mut self, client: Option<String>, cx: &mut Context<Self>) {
        self.log_filter.client = client;
        self.selected_call = None;
        self.show(Section::Log, cx);
    }

    /// Changes the log's filter and reads it again.
    pub fn set_log_filter(&mut self, filter: LogFilter, cx: &mut Context<Self>) {
        self.log_filter = filter;
        self.selected_call = None;
        self.reload_section(Section::Log);
        self.select_defaults(cx);
        self.log_scroll.set_offset(Default::default());
        cx.notify();
    }

    pub fn request_enable(&mut self, name: ClientName, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::EnableRequested(name));
    }

    pub fn request_remove(&mut self, name: ClientName, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::RemoveRequested(name));
    }

    pub fn request_export(&mut self, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::ExportRequested);
    }

    /// "Restore…" on the selected backup.
    pub fn request_restore_selected(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self
            .selected_backup
            .as_ref()
            .and_then(|path| self.backups.row(path))
        else {
            return;
        };
        cx.emit(SettingsEvent::RestoreRequested(RestoreTarget {
            path: row.backup.path.clone(),
            taken_at: Some(row.backup.taken_at),
            local: Some(row.local),
            from_file: false,
        }));
    }

    /// "Restore from a file…": the system's open dialog, then the Restore
    /// dialog for the file chosen.
    pub fn restore_from_file(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.store() {
            self.fail(error, cx);
            return;
        }
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Restore".into()),
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |_, cx| {
                cx.emit(SettingsEvent::RestoreRequested(RestoreTarget::chosen(path)))
            });
        }));
    }

    /// "Back up now": a manual backup on a background thread, then the
    /// notification.
    pub fn back_up_now(&mut self, cx: &mut Context<Self>) {
        if self.backing_up {
            return;
        }
        let store = match self.store() {
            Ok(store) => store.clone(),
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        self.backing_up = true;
        self.notice = None;
        cx.notify();
        let work = cx
            .background_executor()
            .spawn(async move { store.back_up() });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.backing_up = false;
                this.notice = Some(match result {
                    Ok((backup, contents)) => {
                        this.selected_backup = Some(backup.path.clone());
                        Notice {
                            message: NoticeMessage::BackedUp {
                                transactions: contents.map(|contents| contents.transactions),
                                size: backup.size,
                            },
                            reveal: Some(backup.path),
                        }
                    }
                    Err(error) => Notice {
                        message: NoticeMessage::Failed(sentence(&format!(
                            "Couldn't back up: {error}. Nothing changed"
                        ))),
                        reveal: None,
                    },
                });
                this.reload_section(Section::Backups);
                this.select_defaults(cx);
                cx.notify();
            });
        }));
    }

    /// "Save a copy…" on the selected backup: the system's save dialog,
    /// then a private copy there.
    pub fn save_copy(&mut self, cx: &mut Context<Self>) {
        let store = match self.store() {
            Ok(store) => store.clone(),
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        let Some(from) = self.selected_backup.clone() else {
            return;
        };
        let name = from
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = std::env::home_dir().unwrap_or_default();
        let target = cx.prompt_for_new_path(&dir, Some(&name));
        self._task = Some(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(to))) = target.await else {
                return;
            };
            let result = store.save_copy(&from, &to);
            let _ = this.update(cx, |this, cx| {
                this.notice = Some(match result {
                    Ok(()) => Notice {
                        message: NoticeMessage::Copied {
                            file: to
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        },
                        reveal: Some(to),
                    },
                    Err(error) => Notice {
                        message: NoticeMessage::Failed(sentence(&format!(
                            "Couldn't save a copy: {error}"
                        ))),
                        reveal: None,
                    },
                });
                cx.notify();
            });
        }));
    }

    /// "Delete backup…" on the selected backup: the shell opens the Delete
    /// backup dialog, which asks first.
    pub fn request_delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self
            .selected_backup
            .as_ref()
            .and_then(|path| self.backups.row(path))
        else {
            return;
        };
        cx.emit(SettingsEvent::DeleteRequested(DeleteTarget {
            path: row.backup.path.clone(),
            taken: backups::taken(row, self.clock.today),
            reason: row.backup.reason,
        }));
    }

    /// Deletes the backup at `path` for good (the Delete backup dialog).
    /// The error says why it couldn't.
    pub fn delete_backup(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let deleted = self
            .store()?
            .delete_backup(path)
            .map_err(|error| sentence(&format!("Couldn't delete it: {error}")))?;
        self.notice = Some(Notice {
            message: NoticeMessage::Deleted { size: deleted.size },
            reveal: None,
        });
        self.reload_section(Section::Backups);
        self.select_defaults(cx);
        cx.notify();
        Ok(())
    }

    /// Shows the file at `path` in the system's file manager.
    pub fn reveal(&self, path: &Path, cx: &mut Context<Self>) {
        cx.reveal_path(path);
    }

    /// A restore is starting (the Restore dialog): tells the shell.
    pub fn restoring(&mut self, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::RestoreStarted);
    }

    /// The restore failed (the Restore dialog says why): tells the shell.
    pub fn restore_failed(&mut self, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::RestoreFailed);
    }

    /// After a restore (the Restore dialog) of the backup taken at
    /// `taken` (local), when known: reads everything again, says so, and
    /// tells the shell.
    pub fn restored(
        &mut self,
        restored: &orre_core::backup::Restored,
        taken: Option<chrono::NaiveDateTime>,
        cx: &mut Context<Self>,
    ) {
        self.last_restore = restored.pre_restore.as_ref().map(|backup| LastRestore {
            pre_restore: backup.path.clone(),
            migrated: restored.migrated_to.is_some(),
            log_kept: restored.log_rows_carried,
        });
        self.notice = Some(Notice {
            message: NoticeMessage::Restored {
                from: taken.map(restore::refusal::when),
            },
            reveal: restored
                .pre_restore
                .as_ref()
                .map(|backup| backup.path.clone()),
        });
        self.selected_backup = None;
        self.since = None;
        self.reload(cx);
        cx.emit(SettingsEvent::Restored);
        cx.emit(SettingsEvent::AiAccessChanged);
    }

    /// After an export (the export dialog).
    pub fn exported(&mut self, rows: usize, path: PathBuf, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            message: NoticeMessage::Exported {
                rows,
                file: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            },
            reveal: Some(path),
        });
        cx.notify();
    }

    fn fail(&mut self, error: SharedString, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            message: NoticeMessage::Failed(error),
            reveal: None,
        });
        cx.notify();
    }

    /// Shows `notice` in the notification bar (snapshots of the states
    /// board).
    pub fn set_notice(&mut self, notice: Option<Notice>, cx: &mut Context<Self>) {
        self.notice = notice;
        cx.notify();
    }

    pub fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        cx.notify();
    }

    /// The notification bar, centred 24px above the canvas's foot.
    fn render_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.notice.as_ref()?;
        let bar = table::notice_bar(notice, cx);
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(spacing::S6)
                .flex()
                .justify_center()
                .child(bar)
                .into_any_element(),
        )
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let canvas = match self.section {
            Section::Clients => self.render_clients(window, cx),
            Section::Log => self.render_log(window, cx),
            Section::Backups => self.render_backups(window, cx),
            Section::MonthStart => self.render_month_start(window, cx),
        };
        let inspector = match self.section {
            Section::Clients => self.render_client_inspector(window, cx),
            Section::Log => self.render_call_inspector(cx),
            Section::Backups => self.render_backup_inspector(cx),
            Section::MonthStart => Some(self.render_month_start_inspector()),
        };
        let notice = self.render_notice(cx);
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(self.render_panel(window, cx))
            .child(
                div()
                    .id("settings-canvas")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .bg(color::CANVAS)
                    .child(canvas)
                    .when_some(notice, |this, notice| this.child(notice)),
            )
            .children(inspector)
    }
}

impl Period {
    /// The period's key as a menu option key.
    pub fn option_key(self) -> SharedString {
        self.key().into()
    }
}

#[cfg(test)]
mod tests;
