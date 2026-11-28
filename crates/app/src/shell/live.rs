//! Live updates in the shell (ROADMAP LU.3; `docs/architecture/app.md` →
//! "Decision record: live updates (2026-11-21)" and "Decision record: live
//! updates in the shell (2026-11-22)").
//!
//! The shell keeps an [`orre_core::watch::Watcher`] on the database and
//! reloads the views whose [`Topic`] changed, whoever changed it: another
//! process (an MCP client, `orre import` in a terminal) or the app itself.
//!
//! - **The baseline** is taken before the window opens, off the UI thread
//!   ([`baseline`], in [`super::open`]), so it comes before any view's first
//!   read and a write landing between the two is reported. If it couldn't
//!   be taken, the first check that compares reloads every view.
//! - **Checks** run on a background thread, once a second while the window
//!   is active and every five seconds while it isn't, with one at once when
//!   it becomes active ([`poll_interval`]). Only [`super::open`] starts the
//!   timer: tests and the snapshot example never poll. A check's topics are
//!   reloaded with no rebase: the watcher has already taken them.
//! - **The app's own writes** reach the shell as [`Wrote`]: a view inside a
//!   shell ([`Reloads::ByShell`]) drops its read in flight and names the
//!   topics it changed, rather than reloading itself. The shell's `refresh`
//!   rebases those topics on a background thread, then reloads every view
//!   of them ([`views`]). That is the one
//!   place [`Watcher::rebase`] is called, so every rebase of a topic is
//!   followed by a read of every view of it started after the rebase, and a
//!   write another process made before the rebase is in those reads. Rounds
//!   are coalesced: topics named while one is in flight wait for the next,
//!   and a round is never cancelled once its rebase has begun. A view
//!   outside a shell (its own tests) reloads itself ([`Reloads::Itself`]).
//! - **The app's own restore** pauses the checks (the restored file would
//!   report every topic); when it's done, every topic is refreshed, which
//!   also takes the restored file's identity, and the checks resume when
//!   that round's rebase has finished. What had arrived (LU.5) is dropped
//!   with the file it came from. A failed restore resumes at once.
//!
//! Errors from a check or a rebase are logged at debug, once per run of
//! failures, and never with the database's contents: the next tick tries
//! again.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use gpui_kit::{AppContext as _, Context, Subscription, Task, Window};
use orre_core::inbox::SuggestionStatus;
use orre_core::watch::{Check, Topic, Watcher};

use super::arrivals::{Arrival, Arrivals, Channel, LedgerArrival, Phase, SuggestionArrival};
use super::{ShellView, View};
use crate::inbox::arrival::Arrived;
use crate::transactions::{MarkPhase, RegisterChanged};

/// How often the database is checked while the window is active.
const ACTIVE_TICK: Duration = Duration::from_secs(1);

/// How often it's checked while the window isn't.
const INACTIVE_TICK: Duration = Duration::from_secs(5);

/// A view's own write changed these topics: the shell rebases them and
/// reloads every view of them (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wrote(pub BTreeSet<Topic>);

impl Wrote {
    pub fn of(topics: impl IntoIterator<Item = Topic>) -> Self {
        Self(topics.into_iter().collect())
    }
}

/// Who reloads a view after its own write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reloads {
    /// The view, at once: outside a shell.
    #[default]
    Itself,
    /// The shell, after it rebased the topics the view names ([`Wrote`]).
    ByShell,
}

/// The views a change of some topics reloads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Views {
    /// The Inbox, and with it the tab's badge.
    pub inbox: bool,
    pub transactions: bool,
    /// The Overview: the figures, chart, categories and recurring payments.
    pub overview: bool,
    /// The accounts panel, and the status bar's balance and import line.
    pub accounts: bool,
    /// The review queues, and the status bar's failing checks.
    pub checks: bool,
    pub rules: bool,
    /// Settings' AI clients and month start, and the status bar's agent
    /// count.
    pub settings: bool,
}

/// Which views reload when `topics` changed (decision record "live
/// updates", point 2): the Inbox on Inbox; Transactions on Ledger, and on
/// Rules for its merchant names; the Overview on Ledger, and on Rules for
/// the categories; Accounts and Checks on Ledger; Rules on
/// Rules, and on Ledger (its counts) only while it's the view on show
/// (`rules_on_show`), since showing it reloads it; Settings' AI clients and
/// month start on Settings. The views that cut months by the month start
/// (M4.11) reload on Settings too, which holds it: the Overview,
/// Transactions' month presets and the Inbox's month labels.
pub fn views(topics: &BTreeSet<Topic>, rules_on_show: bool) -> Views {
    let ledger = topics.contains(&Topic::Ledger);
    let rules = topics.contains(&Topic::Rules);
    let settings = topics.contains(&Topic::Settings);
    Views {
        inbox: topics.contains(&Topic::Inbox) || settings,
        transactions: ledger || rules || settings,
        overview: ledger || rules || settings,
        accounts: ledger,
        checks: ledger,
        rules: rules || (ledger && rules_on_show),
        settings,
    }
}

/// How long the timer waits between checks.
pub fn poll_interval(active: bool) -> Duration {
    if active { ACTIVE_TICK } else { INACTIVE_TICK }
}

/// A watcher whose baseline was taken, or tried: what [`super::open`]
/// hands the shell.
pub struct Watched {
    watcher: Watcher,
    /// Whether the baseline compared. If not (no database file yet, a
    /// restore held the lock, an error), the views may have read before it,
    /// so the first check that compares reloads every view.
    baselined: bool,
}

/// Takes the baseline of the database at `path`: blocking, so off the UI
/// thread, and before any view reads.
pub fn baseline(path: &Path) -> Watched {
    let mut watcher = Watcher::new(path);
    let baselined = match watcher.check() {
        Ok(check) => matches!(check, Check::Changed(_)),
        Err(error) => {
            tracing::debug!(%error, "couldn't take the live-updates baseline");
            false
        }
    };
    Watched { watcher, baselined }
}

/// The shell's live updates (module docs).
pub(super) struct Live {
    /// Shared with the background check or rebase in flight; they take
    /// turns.
    watcher: Arc<Mutex<Watcher>>,
    baselined: bool,
    /// Whether the window is active: the timer's interval.
    active: bool,
    /// While the app's own restore runs, nothing is checked.
    paused: bool,
    /// The next round that starts resumes the checks when its rebase is
    /// done: set by a restore, whose every topic that round holds.
    resume_with_next_round: bool,
    /// The app's own background writes in flight (an import, its undo):
    /// no check starts meanwhile, so the write isn't taken for another
    /// process's (LU.5).
    own_writes: usize,
    /// Own writes whose round hasn't started: released when the next
    /// round's rebase is done, like a restore's resume.
    release_with_next_round: usize,
    /// Bumped by every own write: a check that started before the bump
    /// overlapped one, so what it reports is reloaded as the app's own.
    epoch: u64,
    /// [`Self::epoch`] when the check in flight started.
    checking_epoch: u64,
    /// Topics named since the round in flight started.
    pending: BTreeSet<Topic>,
    /// The round in flight: a rebase, then the reloads. Never replaced
    /// while it runs.
    round: Option<Task<()>>,
    /// The check in flight.
    checking: Option<Task<()>>,
    timer: Option<Task<()>>,
    /// Whether the last check or rebase failed: a run of failures logs once.
    failing: bool,
    _activation: Option<Subscription>,
    /// What the last check reported, after the baseline rule (tests).
    #[cfg(test)]
    last_reported: Option<BTreeSet<Topic>>,
}

fn lock(watcher: &Mutex<Watcher>) -> MutexGuard<'_, Watcher> {
    // A panic in a check leaves the watcher as it was: its state is only
    // replaced at the end of a check or rebase that worked.
    watcher
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl ShellView {
    /// Keeps `watched` from now on: checks and the app's own writes reload
    /// the views (module docs). Starts no timer ([`Self::keep_watching`]).
    pub fn watch(&mut self, watched: Watched, cx: &mut Context<Self>) {
        self.live = Some(Live {
            watcher: Arc::new(Mutex::new(watched.watcher)),
            baselined: watched.baselined,
            active: true,
            paused: false,
            resume_with_next_round: false,
            own_writes: 0,
            release_with_next_round: 0,
            epoch: 0,
            checking_epoch: 0,
            pending: BTreeSet::new(),
            round: None,
            checking: None,
            timer: None,
            failing: false,
            _activation: None,
            #[cfg(test)]
            last_reported: None,
        });
        cx.notify();
    }

    /// Checks the database on a timer from now on, faster while the window
    /// is active, and at once when it becomes active (only [`super::open`]).
    pub fn keep_watching(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.activation_changed(window.is_window_active(), cx)
        });
        let Some(live) = self.live.as_mut() else {
            return;
        };
        live.active = window.is_window_active();
        live._activation = Some(activation);
        self.restart_timer(cx);
        self.keep_arrivals_timed(cx);
    }

    /// The window became active or stopped being: the timer follows, and
    /// becoming active checks at once.
    pub(super) fn activation_changed(&mut self, active: bool, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if live.active == active {
            return;
        }
        live.active = active;
        if live.timer.is_some() {
            self.restart_timer(cx);
        }
        if active {
            self.check_changes(cx);
            self.see_on_show(cx);
        }
    }

    fn restart_timer(&mut self, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let interval = poll_interval(live.active);
        live.timer = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                if this.update(cx, |this, cx| this.check_changes(cx)).is_err() {
                    return;
                }
            }
        }));
    }

    /// Asks the watcher what changed, on a background thread, and reloads
    /// the views of what did. Nothing while paused or while a check is in
    /// flight.
    pub(super) fn check_changes(&mut self, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if live.paused || live.own_writes > 0 || live.checking.is_some() {
            return;
        }
        live.checking_epoch = live.epoch;
        let watcher = live.watcher.clone();
        live.checking = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { lock(&watcher).check() })
                .await;
            this.update(cx, |this, cx| this.checked(result, cx)).ok();
        }));
    }

    /// Takes a check's answer. Whatever it reports is reloaded, even if a
    /// restore started meanwhile: the watcher has taken those topics, so
    /// dropping the reload would lose them.
    fn checked(&mut self, result: Result<Check, orre_core::watch::Error>, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        live.checking = None;
        let topics = match result {
            Ok(Check::Changed(_)) if !live.baselined => {
                live.baselined = true;
                Topic::ALL.into_iter().collect()
            }
            Ok(check) => check.topics(),
            Err(error) => {
                if !live.failing {
                    tracing::debug!(%error, "couldn't check the database for changes");
                }
                live.failing = true;
                return;
            }
        };
        live.failing = false;
        // An own write landed while the check ran: it may be what the check
        // saw, so nothing it reports is shown as an arrival.
        let reader = if live.checking_epoch == live.epoch {
            Reader::Outside
        } else {
            Reader::Own
        };
        #[cfg(test)]
        {
            live.last_reported = Some(topics.clone());
        }
        if !topics.is_empty() {
            self.reload_views(&topics, reader, cx);
        }
    }

    /// The app's own write changed `topics`: rebase them, then reload every
    /// view of them (module docs). Without a watcher (tests), the views
    /// reload at once.
    pub(super) fn refresh(&mut self, topics: BTreeSet<Topic>, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            self.reload_views(&topics, Reader::Own, cx);
            return;
        };
        live.epoch += 1;
        live.pending.extend(topics.iter().copied());
        if views(&topics, false).transactions {
            // A read already in flight may land with this write in it.
            self.transactions
                .update(cx, |transactions, _| transactions.read_follows_own_write());
        }
        self.start_round(cx);
    }

    /// Starts a round for the pending topics, unless one is in flight (it
    /// starts the next when it ends).
    fn start_round(&mut self, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if live.round.is_some() || live.pending.is_empty() {
            return;
        }
        let topics = std::mem::take(&mut live.pending);
        let resumes = std::mem::take(&mut live.resume_with_next_round);
        let releases = std::mem::take(&mut live.release_with_next_round);
        let watcher = live.watcher.clone();
        let rebased: Vec<Topic> = topics.iter().copied().collect();
        live.round = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { lock(&watcher).rebase(&rebased) })
                .await;
            this.update(cx, |this, cx| {
                this.end_round(&topics, Round { resumes, releases }, result, cx)
            })
            .ok();
        }));
    }

    /// After a round's rebase: every view of its topics reads again, and
    /// the next round starts.
    fn end_round(
        &mut self,
        topics: &BTreeSet<Topic>,
        round: Round,
        result: Result<(), orre_core::watch::Error>,
        cx: &mut Context<Self>,
    ) {
        if let Some(live) = self.live.as_mut() {
            live.round = None;
            match result {
                Ok(()) => live.failing = false,
                Err(error) => {
                    // The watcher kept what it had: the next check reports
                    // the topics again, which is a reload too many, not one
                    // missed.
                    if !live.failing {
                        tracing::debug!(%error, "couldn't rebase the live-updates watcher");
                    }
                    live.failing = true;
                }
            }
            if round.resumes {
                live.paused = false;
            }
            live.own_writes = live.own_writes.saturating_sub(round.releases);
        }
        self.reload_views(topics, Reader::Own, cx);
        self.start_round(cx);
    }

    /// Reloads the views of `topics` ([`views`]), for `reader`: what a
    /// read for another process's change brings is shown as an arrival
    /// (LU.5, [`Arrived`]); what one after the app's own write brings isn't.
    fn reload_views(&mut self, topics: &BTreeSet<Topic>, reader: Reader, cx: &mut Context<Self>) {
        let views = views(topics, self.view == View::Rules);
        let outside = reader == Reader::Outside;
        if views.inbox {
            self.arrived.outside_inbox = outside;
            self.inbox.update(cx, |inbox, cx| inbox.reload(cx));
        }
        if views.transactions {
            self.arrived.outside_transactions = outside;
            self.transactions.update(cx, |transactions, cx| {
                if !outside {
                    // A write in another view: a row it removed wasn't
                    // removed "outside this window".
                    transactions.read_follows_own_write();
                }
                transactions.reload(cx)
            });
        }
        if views.overview {
            self.overview.update(cx, |overview, cx| overview.reload(cx));
        }
        if views.accounts {
            self.accounts.update(cx, |accounts, cx| accounts.reload(cx));
        }
        if views.checks {
            self.checks.update(cx, |checks, cx| checks.reload(cx));
        }
        if views.rules {
            self.rules.update(cx, |rules, cx| rules.reload(cx));
        }
        if views.settings {
            self.settings.update(cx, |settings, cx| {
                settings.reload_clients(cx);
                settings.reload_month_start(cx);
            });
        }
        cx.notify();
    }

    /// The app's own restore started: no checks until it's done.
    pub(super) fn pause_watching(&mut self) {
        if let Some(live) = self.live.as_mut() {
            live.paused = true;
        }
    }

    /// The app's own restore is done: every topic is refreshed, and the
    /// checks resume when that round's rebase has finished. What had
    /// arrived is dropped: the restored file replaced the rows it named,
    /// and the refresh is the app's own, so it would never clear them.
    pub(super) fn restored(&mut self, cx: &mut Context<Self>) {
        if let Some(live) = self.live.as_mut() {
            live.resume_with_next_round = true;
        }
        self.arrived.arrivals = Arrivals::new();
        self.sync_arrivals(cx);
        self.refresh(Topic::ALL.into_iter().collect(), cx);
    }

    /// The app's own import or undo started writing in the background: no
    /// check starts until it's done ([`Live::own_writes`]). A Transactions
    /// read already in flight may land with the write in it: it's no longer
    /// another process's, and a row it finds gone went by the user's hand.
    /// Only a read in flight is marked, so a write that fails leaves the
    /// next read as it was.
    pub(super) fn own_write_started(&mut self, cx: &mut Context<Self>) {
        if let Some(live) = self.live.as_mut() {
            live.own_writes += 1;
            live.epoch += 1;
        }
        self.arrived.outside_transactions = false;
        self.transactions.update(cx, |transactions, _| {
            if transactions.reading() {
                transactions.read_follows_own_write();
            }
        });
    }

    /// That write is done and changed `topics`: they're refreshed, and the
    /// checks resume when that round's rebase has finished.
    pub(super) fn own_write_done(&mut self, topics: BTreeSet<Topic>, cx: &mut Context<Self>) {
        if let Some(live) = self.live.as_mut()
            && live.own_writes > live.release_with_next_round
        {
            live.release_with_next_round += 1;
        }
        self.refresh(topics, cx);
    }

    /// That write failed: the checks resume, with one at once.
    pub(super) fn own_write_failed(&mut self, cx: &mut Context<Self>) {
        if let Some(live) = self.live.as_mut() {
            live.own_writes = live.own_writes.saturating_sub(1);
        }
        self.check_changes(cx);
    }

    /// The app's own restore failed: the checks resume, with one at once.
    pub(super) fn restore_failed(&mut self, cx: &mut Context<Self>) {
        if let Some(live) = self.live.as_mut() {
            live.paused = false;
        }
        self.check_changes(cx);
    }
}

/// What a round does when its rebase is done, besides the reloads.
#[derive(Clone, Copy, Debug)]
struct Round {
    /// Resumes the checks a restore paused.
    resumes: bool,
    /// Releases this many own writes ([`Live::own_writes`]).
    releases: usize,
}

/// Who a reload is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reader {
    /// A check found another process's change.
    Outside,
    /// The app's own write.
    Own,
}

/// What arrived from other processes, as the shell shows it (LU.5,
/// [`super::arrivals`]): the arrival line, the Inbox badge's unseen fill
/// and the row marks.
///
/// Only a read the shell started for a check counts: the views report
/// every read that changed something ([`RegisterChanged`], [`Arrived`]),
/// and the shell takes the one it marked as outside. A read for the app's
/// own write clears the mark, so when the two overlap the change is left
/// unshown rather than shown as another process's.
#[derive(Default)]
pub(super) struct Shown {
    arrivals: Arrivals,
    /// Bumped when a line appears where there was none: the key of its
    /// fade-in. A line replacing another changes at once.
    line_generation: u64,
    /// Whether Transactions' read in flight is for another process.
    outside_transactions: bool,
    /// Whether the Inbox's read in flight is for another process.
    outside_inbox: bool,
    /// The one timer, to the next phase change ([`Arrivals::next_deadline`]);
    /// only in the app ([`ShellView::keep_watching`]), like the checks.
    timer: Option<Task<()>>,
    timed: bool,
    /// The time tests set.
    #[cfg(test)]
    frozen: Option<Instant>,
}

impl Shown {
    pub(super) fn arrivals(&self) -> &Arrivals {
        &self.arrivals
    }

    pub(super) fn line_generation(&self) -> u64 {
        self.line_generation
    }
}

/// The view an arrival is seen through.
fn view_of(channel: Channel) -> View {
    match channel {
        Channel::Inbox => View::Inbox,
        Channel::Transactions => View::Transactions,
    }
}

impl ShellView {
    /// The time arrivals are measured in.
    fn instant(&self) -> Instant {
        #[cfg(test)]
        if let Some(frozen) = self.arrived.frozen {
            return frozen;
        }
        Instant::now()
    }

    /// Sets the time arrivals are measured in (tests).
    #[cfg(test)]
    pub(super) fn freeze_time(&mut self, now: Instant) {
        self.arrived.frozen = Some(now);
    }

    /// Whether the window is focused; without a watcher (tests), it is.
    fn window_active(&self) -> bool {
        self.live.as_ref().is_none_or(|live| live.active)
    }

    /// Whether `channel`'s view is on show in a focused window.
    fn on_show(&self, channel: Channel) -> bool {
        self.window_active() && self.view == view_of(channel)
    }

    /// Transactions read the register again: what an outside read brought
    /// arrives.
    pub(super) fn register_changed(&mut self, event: &RegisterChanged, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.arrived.outside_transactions) || event.after_own_write {
            return;
        }
        let change = &event.change;
        self.arrive(
            Arrival::Ledger(LedgerArrival {
                new: change.new.iter().copied().collect(),
                changed: change.changed.iter().copied().collect(),
                removed: change.removed.len(),
                import_undone: change.import_undone,
            }),
            cx,
        );
    }

    /// The Inbox read suggestions that weren't there: from an outside read,
    /// they arrive.
    pub(super) fn suggestions_arrived(&mut self, event: &Arrived, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.arrived.outside_inbox) {
            return;
        }
        self.arrive(
            Arrival::Suggestions(SuggestionArrival {
                per_client: event.0.by_client.clone(),
                new_ids: event.0.ids.clone(),
            }),
            cx,
        );
    }

    fn arrive(&mut self, arrival: Arrival, cx: &mut Context<Self>) {
        let now = self.instant();
        let seen_now = self.on_show(arrival.channel());
        let had_line = self.arrived.arrivals.line().is_some();
        self.arrived.arrivals.arrive(arrival, seen_now, now);
        if !had_line && self.arrived.arrivals.line().is_some() {
            self.arrived.line_generation += 1;
        }
        self.sync_arrivals(cx);
    }

    /// The view on show, in a focused window, has seen what arrived through
    /// it: called when a view is shown and when the window becomes active.
    pub(super) fn see_on_show(&mut self, cx: &mut Context<Self>) {
        if !self.window_active() {
            return;
        }
        let channel = match self.view {
            View::Inbox => Channel::Inbox,
            View::Transactions => Channel::Transactions,
            _ => return,
        };
        let now = self.instant();
        self.arrived.arrivals.seen(channel, now);
        self.sync_arrivals(cx);
    }

    /// The arrival line was clicked: it's seen, and its view opens with
    /// nothing in it changed (the Inbox on its pending suggestions).
    pub(super) fn arrival_clicked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.arrived.arrivals.line().and_then(|line| line.opens) else {
            return;
        };
        let now = self.instant();
        self.arrived.arrivals.clicked(now);
        if channel == Channel::Inbox && self.inbox.read(cx).status() != SuggestionStatus::Pending {
            self.inbox.update(cx, |inbox, cx| {
                inbox.show_status(SuggestionStatus::Pending, cx)
            });
        }
        self.show(view_of(channel), window, cx);
        self.sync_arrivals(cx);
    }

    /// Moves what arrived along to now: a hold ends, a fade ends.
    pub(super) fn tick_arrivals(&mut self, cx: &mut Context<Self>) {
        let now = self.instant();
        self.arrived.arrivals.tick(now);
        // Re-armed even when nothing changed: a timer that fired a little
        // early has the same deadline still to reach.
        self.sync_arrivals(cx);
    }

    /// Starts the arrivals' timer from now on (only the app).
    pub(super) fn keep_arrivals_timed(&mut self, cx: &mut Context<Self>) {
        self.arrived.timed = true;
        self.sync_arrivals(cx);
    }

    /// Hands the views their marks, and sets the timer to the next phase
    /// change.
    fn sync_arrivals(&mut self, cx: &mut Context<Self>) {
        let (ids, phase) = match self.arrived.arrivals.transaction_marks() {
            Some(marked) => (marked.ids.clone(), marked.phase),
            None => (BTreeSet::new(), Phase::Shown),
        };
        let phase = match phase {
            Phase::Shown => MarkPhase::Shown,
            Phase::Fading { .. } => MarkPhase::FadingOut,
        };
        let transactions = self.transactions.read(cx);
        if transactions.marked() != ids || transactions.mark_phase() != phase {
            self.transactions.update(cx, |transactions, cx| {
                transactions.set_marks(ids, phase, cx)
            });
        }
        let (ids, fading) = match self.arrived.arrivals.suggestion_marks() {
            Some(marked) => (
                marked.ids.clone(),
                matches!(marked.phase, Phase::Fading { .. }),
            ),
            None => (BTreeSet::new(), false),
        };
        let inbox = self.inbox.read(cx);
        if inbox.marked() != ids || inbox.marks_fading() != fading {
            self.inbox
                .update(cx, |inbox, cx| inbox.set_marks(ids, fading, cx));
        }
        self.arrived.timer = None;
        if self.arrived.timed
            && let Some(deadline) = self.arrived.arrivals.next_deadline()
        {
            let wait = deadline.saturating_duration_since(self.instant());
            self.arrived.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                this.update(cx, |this, cx| this.tick_arrivals(cx)).ok();
            }));
        }
        cx.notify();
    }
}
