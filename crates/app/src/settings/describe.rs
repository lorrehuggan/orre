//! What Settings shows, as plain data, for tests (ROADMAP U.3): the left
//! panel and its foot, the open screen's toolbar and table, the selection,
//! the inspector, the notification bar, a dialog, and the keys of the
//! panel and the list.
//!
//! Compiled for tests and the dev driver only (the `dev-driver` feature,
//! debug builds): it isn't in a release build, and nothing but the tests and
//! the driver can reach it, `orre mcp`
//! included (`docs/architecture/app.md` → "Decision record: views that
//! describe themselves stay out of the release binary").
//!
//! Every string comes from the code the screens draw with ([`super::panel`],
//! [`super::clients`], [`super::log`], [`super::backups`], [`super::table`]),
//! so a purpose (the client's untrusted words) is described as shown:
//! through [`crate::cli::ai_access::one_line`], cut as the row cuts it.
//! Client names are [`ClientName`]s, which can't hold anything to clean.
//! Styling (tones, mono runs, icons) is left out, but for a call's kind
//! icon; runs are joined into one string.
//!
//! What it leaves out: the lanes' headings, "What each level shares" (fixed
//! text), the inspector's fixed explanatory captions, an open select's
//! menu, and the pointer's hover.
//!
//! [`ClientName`]: orre_core::mcp_client::ClientName

use gpui_kit::{AnyView, App};

pub use crate::ui::describe::{Action, Dialog, Header, Inspector, PanelRow, Section};

use super::data::{BackupRow, LogRow};
use super::delete::DeleteBackupDialog;
use super::enable::EnableDialog;
use super::export::ExportDialog;
use super::remove::RemoveDialog;
use super::restore::RestoreDialog;
use super::table::{self, NoticeText, Run, count, file_size};
use super::{Section as Screen, SettingsView, backups, clients, log, month_start, panel};

/// What Settings shows ([`SettingsView::describe`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsDescription {
    /// The panel's rows: Clients, Access log, Backups, Export…, then Month
    /// start, the open screen selected.
    pub panel: Vec<PanelRow>,
    /// The panel's foot: where the data lives.
    pub data_file: String,
    /// The toolbar's title and caption.
    pub title: &'static str,
    pub caption: &'static str,
    /// Why the screen's data couldn't be read, under the toolbar.
    pub error: Option<String>,
    pub body: Body,
    /// The selected row's inspector; none with nothing selected.
    pub inspector: Option<Inspector>,
    pub notice: Option<NoticeDescription>,
    /// The dialog open over the window, when it's one of Settings'
    /// (`SettingsDescription::with_dialog`).
    pub dialog: Option<Dialog>,
    /// The panel's keys (↑↓ between the screens).
    pub panel_keys: Vec<Action>,
    /// The open list's keys, and whether it has rows to move through.
    pub list_keys: Vec<Action>,
}

/// The open screen's canvas under the toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Clients(ClientsDescription),
    Log(LogDescription),
    Backups(BackupsDescription),
    MonthStart(MonthStartDescription),
}

/// The month start (M4.11): the rule's two fields as shown, and the
/// preview's months. The helper's fixed sentences are left out; its first
/// paragraph, with this month in it, is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonthStartDescription {
    /// Starts on, as the field shows it: "25th of the month".
    pub starts_on: String,
    /// The Weekends segment chosen: "Leave it" or "Use the Friday before".
    pub weekends: &'static str,
    /// "as of today, Tue 10 Nov".
    pub as_of: String,
    pub months: Vec<month_start::MonthRow>,
    pub helper: String,
}

/// AI clients.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientsDescription {
    /// The kill switch.
    pub ai_access: bool,
    /// The banner while AI access is off: its title, and Turn on.
    pub off_banner: Option<(&'static str, Action)>,
    pub clients: Vec<ClientDescription>,
    /// What the Clients group says with none.
    pub empty: Option<String>,
    pub requests: Vec<RequestDescription>,
}

/// An enabled (or turned-off) client's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientDescription {
    pub name: String,
    pub level: &'static str,
    pub last_connected: String,
    pub calls: String,
    /// Its ON toggle.
    pub on: bool,
    pub selected: bool,
}

/// A client that asked to connect: its row, the line under it, and Ignore
/// and Enable….
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestDescription {
    pub name: String,
    pub access: &'static str,
    pub last_tried: String,
    pub refused: String,
    pub line: String,
    pub actions: Vec<Action>,
}

/// The access log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogDescription {
    /// The filters' labels: the client, then the period.
    pub filters: (String, &'static str),
    pub days: Vec<DayDescription>,
    /// What it says when the filter matches no call.
    pub empty: Option<String>,
    /// Earlier calls load as the list is scrolled.
    pub more: bool,
}

/// A day's group of calls, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayDescription {
    pub title: String,
    pub count: String,
    pub calls: Vec<CallDescription>,
}

/// A call's row, lane by lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallDescription {
    pub time: String,
    pub client: String,
    /// The kind lane: "refused", "writes", or nothing for a read.
    pub kind: Option<&'static str>,
    pub tool: String,
    pub purpose: String,
    pub rows: String,
    pub selected: bool,
}

/// Backups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupsDescription {
    /// "Restore from a file…" and "Back up now" (off while one is taken).
    pub actions: Vec<Action>,
    /// Recent, then One a month, each with its backups.
    pub groups: Vec<(&'static str, Vec<BackupDescription>)>,
    /// What it says with no backups.
    pub empty: Option<&'static str>,
    /// The count and total size of every backup under the table.
    pub total: Option<String>,
}

/// A backup's row, lane by lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupDescription {
    pub taken: String,
    pub reason: String,
    pub transactions: String,
    pub size: String,
    pub selected: bool,
}

/// The notification bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticeDescription {
    pub message: String,
    /// Shown as a failure (`danger`).
    pub failed: bool,
    /// "Show in folder", when it names a file.
    pub actions: Vec<Action>,
}

fn joined(runs: &[Run]) -> String {
    Run::joined(runs)
}

/// A section of label and runs rows.
fn rows_section(
    title: Option<&str>,
    rows: impl IntoIterator<Item = (&'static str, Vec<Run>)>,
) -> Section {
    let section = Section {
        title: title.map(str::to_owned),
        ..Section::default()
    };
    rows.into_iter().fold(section, |section, (label, runs)| {
        section.row(label, joined(&runs))
    })
}

impl SettingsView {
    // AI clients -----------------------------------------------------------------

    fn describe_clients(&self) -> Body {
        let today = self.clock.today;
        Body::Clients(ClientsDescription {
            ai_access: self.ai.on,
            off_banner: (!self.ai.on)
                .then(|| (clients::OFF_TITLE, Action::button("Turn on", true))),
            clients: self
                .ai
                .clients
                .iter()
                .map(|client| ClientDescription {
                    name: client.name.to_string(),
                    level: clients::level_label(client.level),
                    last_connected: client.last_seen.map_or_else(
                        || clients::NEVER.to_owned(),
                        |time| clients::when(time, today),
                    ),
                    calls: count(client.week.calls),
                    on: client.enabled,
                    selected: self.selected_client.as_ref() == Some(&client.name),
                })
                .collect(),
            empty: (self.ai.clients.is_empty() && self.ai_error.is_none())
                .then(|| joined(&clients::no_clients())),
            requests: self
                .ai
                .requests
                .iter()
                .map(|request| RequestDescription {
                    name: request.name.to_string(),
                    access: clients::NOT_ENABLED,
                    last_tried: request
                        .last_tried
                        .map_or_else(|| "—".to_owned(), |time| clients::when(time, today)),
                    refused: count(request.refused),
                    line: joined(&clients::request_line(request)),
                    actions: vec![
                        Action::button("Ignore", true),
                        Action::button("Enable…", true),
                    ],
                })
                .collect(),
        })
    }

    fn describe_client(&self) -> Option<Inspector> {
        let client = self
            .selected_client
            .as_ref()
            .and_then(|name| self.ai.client(name))?;
        let access = Section::titled("Access").row("Level", clients::level_label(client.level));
        let connect = Section::titled("Connect")
            .row("", clients::command(&client.name))
            .action(Action::button("Copy the command", true));
        let week = rows_section(Some("Last 7 days"), clients::week_rows(client))
            .action(Action::button("Open access log", true));
        let remove = Section::default().action(Action::button("Remove client…", true));
        Some(Inspector {
            header: Header {
                title: client.name.to_string(),
                figure: Some(count(client.week.calls)),
                caption: Some(joined(&clients::client_caption(client))),
            },
            sections: vec![access, connect, week, remove],
        })
    }

    // The access log ---------------------------------------------------------------

    fn describe_call(&self, row: &LogRow) -> CallDescription {
        let call = &row.call;
        CallDescription {
            time: row.local.format("%H:%M").to_string(),
            client: call.client.clone(),
            kind: log::kind(row).map(|glyph| match glyph {
                crate::ui::icon::Glyph::Refused => "refused",
                _ => "writes",
            }),
            tool: call.tool.clone(),
            purpose: log::purpose(call).0,
            rows: log::rows(call),
            selected: self.selected_call == Some(call.id),
        }
    }

    fn describe_log(&self) -> Body {
        let empty = self.log.is_empty() && self.log_error.is_none();
        Body::Log(LogDescription {
            filters: (self.client_filter_label(), self.log_filter.period.label()),
            days: self
                .log_days()
                .into_iter()
                .map(|(date, rows)| DayDescription {
                    title: log::day_title(date, self.clock.today),
                    count: count(rows.len() as u64),
                    calls: rows.iter().map(|row| self.describe_call(row)).collect(),
                })
                .collect(),
            empty: empty.then(|| self.empty_log()),
            more: self.log_more,
        })
    }

    fn describe_call_inspector(&self) -> Option<Inspector> {
        let row = self
            .selected_call
            .and_then(|id| self.log.iter().find(|row| row.call.id == id))?;
        let call = &row.call;
        let (figure, caption) = log::call_header(row, self.clock.today);
        let mut details = rows_section(None, log::call_rows(row));
        if let Some(refusal) = log::refusal(call) {
            details = details.row("", format!("{refusal}."));
        }
        let arguments = match log::argument_lines(call) {
            None => Section::titled("Arguments").row("", "None."),
            Some(lines) => lines
                .into_iter()
                .fold(Section::titled("Arguments"), |section, (key, value, _)| {
                    section.row(key.trim_end(), value)
                }),
        };
        let purpose = Section::titled("Purpose").row(
            "",
            log::full_purpose(call).unwrap_or_else(|| log::NO_PURPOSE.to_owned()),
        );
        Some(Inspector {
            header: Header {
                title: call.tool.clone(),
                figure: Some(figure),
                caption: Some(joined(&caption)),
            },
            sections: vec![details, arguments, purpose],
        })
    }

    // Backups --------------------------------------------------------------------

    fn describe_backup(&self, row: &BackupRow) -> BackupDescription {
        BackupDescription {
            taken: backups::taken(row, self.clock.today),
            reason: joined(&backups::reason(&row.detail)),
            transactions: row
                .contents
                .map_or_else(|| "—".to_owned(), |contents| count(contents.transactions)),
            size: file_size(row.backup.size),
            selected: self.selected_backup.as_ref() == Some(&row.backup.path),
        }
    }

    fn describe_backups(&self) -> Body {
        let (monthly, recent): (Vec<&BackupRow>, Vec<&BackupRow>) =
            self.backups.rows.iter().partition(|row| row.monthly);
        let groups = [(backups::RECENT, recent), (backups::MONTHLY, monthly)]
            .into_iter()
            .filter(|(_, members)| !members.is_empty())
            .map(|(title, members)| {
                (
                    title,
                    members
                        .into_iter()
                        .map(|row| self.describe_backup(row))
                        .collect(),
                )
            })
            .collect();
        Body::Backups(BackupsDescription {
            actions: vec![
                Action::button("Restore from a file…", true),
                Action::button("Back up now", !self.backing_up),
            ],
            groups,
            empty: (self.backups.rows.is_empty() && self.backups_error.is_none())
                .then_some(backups::NO_BACKUPS),
            total: backups::total(&self.backups.rows),
        })
    }

    fn describe_backup_inspector(&self) -> Option<Inspector> {
        let row = self
            .selected_backup
            .as_ref()
            .and_then(|path| self.backups.row(path))?;
        let supported = self.backups.supported;
        let details = rows_section(None, backups::backup_rows(row, supported))
            .row("Size", file_size(row.backup.size));
        let since = match self.since_for(row) {
            Some(since) if since.is_empty() => {
                Section::titled("Since this backup").row("", backups::NOTHING_SINCE)
            }
            Some(since) => backups::since_lines(since).into_iter().fold(
                Section::titled("Since this backup"),
                |section, (label, value)| section.row(joined(&label), value),
            ),
            None => Section::titled("Since this backup"),
        };
        let since = self
            .restore_lines(row)
            .into_iter()
            .fold(since, |section, (label, value)| {
                section.row(joined(&label), value)
            })
            .action(Action::button(
                "Restore…",
                backups::readable(row, supported),
            ));
        let (dir, name) = backups::file_lines(row);
        let file = Section::titled("File")
            .row("", format!("{dir}{name}"))
            .action(Action::button("Show in folder", true))
            .action(Action::button("Save a copy…", true));
        Some(Inspector {
            header: Header {
                title: joined(&backups::backup_title(row, self.clock.today)),
                figure: row.contents.map(|contents| count(contents.transactions)),
                caption: Some(backups::backup_caption(row).to_owned()),
            },
            sections: vec![
                details,
                since,
                file,
                Section::default().action(Action::button("Delete backup…", true)),
            ],
        })
    }

    // The month start ---------------------------------------------------------------

    fn describe_month_start(&self) -> Body {
        let start = self.month_start;
        let today = self.clock.today;
        Body::MonthStart(MonthStartDescription {
            starts_on: format!("{} of the month", month_start::field_value(start.day())),
            weekends: match start.weekends() {
                orre_core::reporting::period::Weekends::Keep => month_start::LEAVE_IT,
                orre_core::reporting::period::Weekends::FridayBefore => month_start::FRIDAY_BEFORE,
            },
            as_of: month_start::as_of(today),
            months: month_start::preview(today, start),
            helper: joined(&month_start::helper(today, start)),
        })
    }

    fn describe_month_start_inspector(&self) -> Inspector {
        let listed = |title: &str, rows: &[(&str, &str)]| {
            rows.iter()
                .fold(Section::titled(title), |section, (name, what)| {
                    section.row(*name, *what)
                })
        };
        Inspector {
            header: Header {
                title: month_start::STARTS_ON_THE.to_owned(),
                figure: Some(month_start::day_label(self.month_start.day())),
                caption: Some(month_start::rule_caption(self.month_start).to_owned()),
            },
            sections: vec![
                listed(month_start::FOLLOWS_IT, &month_start::FOLLOWS),
                listed(month_start::KEEPS_ITS_OWN, &month_start::KEEPS),
            ],
        }
    }

    // The whole -------------------------------------------------------------------

    /// What Settings shows now, as plain data (module docs). A dialog is
    /// the shell's: add it with `SettingsDescription::with_dialog`.
    pub fn describe(&self) -> SettingsDescription {
        let row = |section: Screen| PanelRow {
            label: section.label(),
            count: Some(self.panel_count(section)),
            flagged: false,
            selected: self.section == section,
        };
        // As the panel draws them: Export… ends DATA, before CALENDAR.
        let panel = vec![
            row(Screen::Clients),
            row(Screen::Log),
            row(Screen::Backups),
            PanelRow {
                label: panel::EXPORT,
                count: None,
                flagged: false,
                selected: false,
            },
            row(Screen::MonthStart),
        ];
        let (title, caption, error, body, inspector, rows) = match self.section {
            Screen::Clients => (
                clients::TITLE,
                clients::CAPTION,
                &self.ai_error,
                self.describe_clients(),
                self.describe_client(),
                !self.ai.clients.is_empty(),
            ),
            Screen::Log => (
                log::TITLE,
                log::CAPTION,
                &self.log_error,
                self.describe_log(),
                self.describe_call_inspector(),
                !self.log.is_empty(),
            ),
            Screen::Backups => (
                backups::TITLE,
                backups::CAPTION,
                &self.backups_error,
                self.describe_backups(),
                self.describe_backup_inspector(),
                !self.backups.rows.is_empty(),
            ),
            Screen::MonthStart => (
                month_start::TITLE,
                month_start::CAPTION,
                &self.month_start_error,
                self.describe_month_start(),
                Some(self.describe_month_start_inspector()),
                false,
            ),
        };
        SettingsDescription {
            panel,
            data_file: self.data_file.to_string(),
            title,
            caption,
            error: error.as_ref().map(ToString::to_string),
            body,
            inspector,
            notice: self.notice.as_ref().map(|notice| NoticeDescription {
                message: match table::notice_text(&notice.message) {
                    NoticeText::Rich(runs) => joined(&runs),
                    NoticeText::Plain(text) => text,
                },
                failed: matches!(notice.message, super::NoticeMessage::Failed(_)),
                actions: matches!(notice.message, super::NoticeMessage::Restored { .. })
                    .then(|| Action::button(super::clients::TITLE, true))
                    .into_iter()
                    .chain(
                        notice
                            .reveal
                            .iter()
                            .map(|_| Action::button("Show in folder", true)),
                    )
                    .collect(),
            }),
            dialog: None,
            panel_keys: Action::moves(&panel::KEYS, true),
            list_keys: Action::moves(&table::LIST_KEYS, rows),
        }
    }
}

impl SettingsDescription {
    /// With `dialog`, the shell's open dialog, when it's one of Settings'.
    pub fn with_dialog(mut self, dialog: Option<&AnyView>, cx: &App) -> Self {
        self.dialog = Self::dialog_of(dialog, cx);
        self
    }

    /// `dialog` described, when it's one of this view's.
    pub fn dialog_of(dialog: Option<&AnyView>, cx: &App) -> Option<crate::ui::describe::Dialog> {
        dialog.and_then(|dialog| {
            let dialog = dialog.clone();
            let dialog = match dialog.downcast::<EnableDialog>() {
                Ok(enable) => return Some(enable.read(cx).describe()),
                Err(dialog) => dialog,
            };
            let dialog = match dialog.downcast::<RemoveDialog>() {
                Ok(remove) => return Some(remove.read(cx).describe()),
                Err(dialog) => dialog,
            };
            let dialog = match dialog.downcast::<DeleteBackupDialog>() {
                Ok(delete) => return Some(delete.read(cx).describe()),
                Err(dialog) => dialog,
            };
            let dialog = match dialog.downcast::<RestoreDialog>() {
                Ok(restore) => return Some(restore.read(cx).describe()),
                Err(dialog) => dialog,
            };
            dialog
                .downcast::<ExportDialog>()
                .ok()
                .map(|export| export.read(cx).describe())
        })
    }
}
