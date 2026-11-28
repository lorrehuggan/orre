# Orre — Code map

Where things are in the code. Milestone numbers (M1.3, D6.6, …) point to
[`ROADMAP.md`](ROADMAP.md), the reasons to [`ARCHITECTURE.md`](ARCHITECTURE.md). This
map lags the code: check the tree before assuming a module exists, and update the map
when you add or move one.

The Cargo workspace (M1.1) is `crates/core`, `crates/import`, `crates/mcp` and
`crates/app`. Shared files stay at the root: `assets/` (fonts, brand) and `design/`
(tokens).

## `crates/core`

### Database, schema and basics

- `db`, `paths`: data dir, WAL, permissions, migrations (M1.3); `paths::presets_dir`.
  `db::migrations::run` turns foreign keys off for a migration whose first line is
  `-- orre: foreign_keys=off` and checks them before it commits (R.4);
  `db/superseded_tests.rs` covers V21; `db/schema_snapshot_tests.rs` pins the schema
  text SQLite stores against `db/schema_snapshot.txt`.
- Migrations in `crates/core/migrations/`: `V1__initial_schema.sql` (M1.4) to
  `V24__transaction_text.sql`. Each feature below names the migration it added.
- `money`: `Currency`, `Money`, exact decimal parsing (M1.5).
- `statement`: the normalized import output, `Statement`, `StatementLine`, flagged rows
  (M1.6).
- `id`: row IDs. `clock`: now and local time via SQLite.

### Accounts

- `account`: accounts and entered balances (M1.11–M1.13, `V3__import_flags.sql`);
  `update_balance` and `remove_balance` for a balance the user entered (M3.P9);
  editing (`update`, `usage`), `create_with_balance`, `entered_balances` and the preset
  (`V15__account_preset.sql`, M3.2); valuations as entered balances
  (`EnteredSource::Valuation`) and `account::close` (closing with the lock-date guard,
  reopening, deleting; M3.17); `account::count`, a cash count plus an optional "Cash
  count difference" row in one DB transaction, via `manual::create_in` (M3.10).
- `balance`: balance checks.
- `sub_account`: pots found by key, auto-created, rows mirrored and paired as confirmed
  transfers (M1.17, `V4__sub_account_key.sql`).

### Import commit

- `import`: preview → backup → commit → undo with dedupe and reconciliation, then preset
  categories, the pot mirror, merchant names, rules and transfer pairing in the same DB
  transaction. The commit also detects possible duplicates and suggests links.
  `import::rehearse`, `CommitSummary::uncategorized` (M3.4).
- `import/supersede.rs`: the bank's corrections (R.4, `V21__superseded.sql`): which stored
  row a file row corrects or why it can't, superseding it in the commit and carrying the
  user's fields across; `import/correction_tests.rs`.
- `import/source_identity.rs`: a file's account number as a keyed hash (R.13,
  `V23__account_source_identity.sql`, `setting::IMPORT_ACCOUNT_KEY`): the wrong-account
  warning (`ImportPreview::source_account_mismatch`), learnt on commit, forgotten on undo;
  `import/source_identity_tests.rs`. The number is `statement::SourceAccountNumber`,
  in memory only, `Debug` redacted.
- `removal` (crate-private): undo and manual deletes go through it. It dissolves transfer
  groups, clears every row that refers to a transaction (checked against the schema's
  foreign keys by a test), then deletes. A row a removed row superseded is restored
  rather than deleted.

### Categories

- `category`: `CategoryKey` (M1.8); `CategoryKind`, `DEFAULT_CATEGORIES` and
  `CategoryTree` via `Database::categories`, seeded by `V2__default_categories.sql`
  (M1.14). `CategoryTree::visible`/`is_visible` for pickers.
- `category::assign`: whether a transaction takes a category of its own (refused by
  `inbox::own::recategorize`, skipped at approval, read by `RegisterRow::takes_category`)
  and whether it still needs one, `needs_category` (read by
  `RegisterRow::is_uncategorised` and the import summary's count).
- `category/edit.rs`: `category::{add, rename, set_hidden, delete, usage, overview,
  derive_key, retired_keys}`, the categories editor's core (M3.7,
  `V18__category_hidden.sql`). New user keys get a `u_` part, and a deleted key is
  retired for good (R.11, `V22__category_key_retired.sql`).

### Rules and merchant names

- `rule`: validated `RuleDefinition`, `Pattern`, `RuleAction`; the store and the
  `RuleSet` engine (M1.15, every action kind since M1.24). Applying a rule to history:
  `rule::preview_apply`/`commit_apply` (M1.24).
- The Rules view's read models: `rule::overview`, `rule::preview_rows`; a new rule is
  `rule::preview_draft` then `save_draft`; `rule::update` edits without touching history;
  `rule::propose` drafts a rule from an edit. `V16__rules_editor.sql` (`rule.origin`,
  `change_batch.rule_id`, `rule_prompt_dismissed`).
- `merchant`: deterministic clean-up, aliases, `merchant_display_source`; runs in the
  import commit before rules (M1.23, `V7__merchant_alias.sql`). `merchant::overview`.

### Transactions and their edits

- `split`: lines summing exactly to the parent, the `ledger_line` view (M1.20,
  `V5__transaction_split.sql`); `split::draft::SplitDraft`, the split editor's model
  (M3.9).
- `tag`, `note`: tags (private tags too) and user notes (M1.21, `V6__tags.sql`).
- `manual`, `exclusion`: manual rows and exclusions (M1.22, no migration).
- `change`: recorded, undoable changes to user-facing fields, `dry_run` previews
  (`V8__change_batch.sql`); `change::history`, `change::undo`.
- `edit`: the inspector's edits, one change batch each: split, tag, note and merchant
  name (M3.9); `link`, `confirm_links`, `reject_links`, `set_sharing`, `rename_person`
  with `Recorder::track` (M3.11), recorded on the money in's `links` field
  (`V17__change_item_links.sql`).
- `transfer`: transfer-pair detection (M1.16); `transfer/decision.rs`:
  `transfer::{confirm, reject, unpair, undo, pair_of, availability}` (M3.8);
  `transfer/mark.rs`: `transfer::{markable, mark, unmark}`, two rows marked as a transfer
  by hand (the bulk-action bar; `inbox::mark_transfer` settles an agent's suggestion).
  `inbox::link_as_refund` / `undo_refund_linked`: a suggested pair rejected and its money
  in linked as a refund, in one DB transaction (M3.P11; `Pair::refund` says when).

### Links, duplicates and reconciliation

- `link`: refunds, reimbursements and shared costs (M1.25, `V9__transaction_link.sql`,
  which redefines `ledger_line`); `link::owed`, claims, matched payments, people with
  their outstanding costs.
- `duplicate`: the possible-duplicates queue (M1.26, `V10__duplicate_candidate.sql`);
  `unmatched_source_id` candidates, the same day, amount and text under another source ID
  (R.3, `V20__duplicate_unmatched_source_id.sql`).
- `reconciliation`: balance checks and lock dates (M1.27).
- `checks`: the review queues' read model: a pair's merge planned by `duplicate`'s own
  code and rolled back; each check's base, window and the pair that explains it;
  lockable dates.

M1.20–M1.27 have no CLI commands yet.

### Reporting, budgets and schedules

- `reporting` (M2.2): the `Ledger` over `ledger_line`, `Period` (a month's name, its dates
  cut under the user's `MonthStart`, M4.11), per-currency `Totals`,
  `Limit`; `summary`, `spending`, `budget_status`, `net_worth`, `balance`, `rows`,
  `catalog` (`reports_visible_to`: a client's own reports at its level or below, R.2,
  `V19__report_access_level.sql`). App read models: `account_tree` (the accounts panel, M3.2), `register` (the
  Transactions table: every transaction, filtered, grouped by day and sorted in memory,
  category colours ranked at read time, or one flat list when sorted by a column, M3.P13; rows carry tags, notes, links, share, `pair`,
  `RowSource` and exclusion details; M3.3), `palette` (category colours ranked at read
  time), `overview` (the Overview screen's month: the chart's 12 months, the changes vs
  last month, spending by category and recurring payments due, in one currency, M4.1),
  `linking` (how a row counts, the link dialog's candidates, why a refund was
  suggested), `pairing` over `RegisterRow::pair`.
- `search` (FT.2): text as a literal FTS5 phrase for the `transaction_text` trigram index
  over transaction text (`V24__transaction_text.sql`, kept in step by triggers on
  `"transaction"`); `Ledger::matching` reads through it and confirms with `LIKE` (FT.3).
  `search::similar` (FT.4): a transaction's nearest neighbours by `bm25` over its stem.
- `budget`; `recurring`: recurring detection (M2.6); `schedule`: confirm, match at
  import, skips, missed and late (M2.10, `V12__schedule.sql`).

### Inbox, AI clients and settings

- `inbox` (M2.3, M2.14, `V13__inbox.sql`): typed payloads, filing with dedupe and caps,
  `approve`/`approve_all`/`reject`, `impact`, `counts`, `reject_all`,
  `decide_transfer` (settles an agent's pending suggestion for a pair), and `own`,
  Orre's own rule suggestions (`inbox::own::recategorize`) and category suggestions after
  an import (`inbox::similar`, FT.4). The Inbox view's read model
  is `inbox::preview` (`describe`, `preview`, `covered_by`).
- `envelope`: the untrusted-text envelope, `sanitize`, `strip_invisible`,
  `reads_like_instructions`, merchant pseudonyms (M2.13); `envelope::shown`.
- `setting`: the kill switch, the pseudonym key, the import's account key (R.13), the
  window state (`setting::WindowState`), and the month start
  (`setting::month_start`, M4.11).
- `mcp_client`: enable, level, revoke, ignore, `requested`; `enable_at`,
  `turn_off`/`turn_on` (keeping the level) and `remove` (M3.15). `V11__mcp.sql`.
- `access_log`: append, list, 12-month retention; `access_log::activity` (pages, counts,
  per-client activity, argument lines; M3.15); `access_log::pending`, calls refused while
  the database can't be opened, moved into the log later (`V14__access_log_pending.sql`).

### Backups and export

- `backup`: `VACUUM INTO` snapshots in `<data dir>/backups`, rotation (`KEEP_RECENT`, and
  one a month for `KEEP_MONTHS`, R.12), `delete`, `total_size`, checked restore;
  taken before import commits and migrations (M1.28). `backup::restoring`, the restore
  lock; `backup::gate`, the one way every per-call open checks it, before and after
  opening. `backup::detail`: what a backup holds (read `immutable`), reasons, "since",
  `assess`, `save_copy`. `backup::schema`: the restore's check of a backup's schema
  against the migrations at its version (SLI-50).
- `watch` (LU.2): `Watcher::check` notices a change another process made to the
  database: a stamp of file metadata first, then one gated open that fingerprints each
  `Topic` (Inbox, Ledger, Rules, Settings); `Watcher::rebase` before the app's own reload.
- `export` (M1.29): `export::count`, `date_span`, `Options::include_excluded`. The
  columns are documented in `docs/EXPORT.md`.

## `crates/import`

- `Importer` trait, `SourceFile` and detection (M1.6).
- `csv`: `CsvImporter` over a `CsvMapping` (M1.7); `csv::MappingDraft`, `FileLayout`,
  `CsvFile::read`, `date_formats`, `CsvImporter::problem_text` (M3.4).
- `preset`: `Preset`, `Presets::bundled` (M1.8); `Preset::to_toml`/`Preset::new`,
  `preset::slug`, `load_user_presets` and `save_user_preset` (saved presets in
  `<data dir>/presets/`, used by the import dialog, the CLI and the account inspector).
- `ofx`: `OfxImporter`, its own SGML/XML reader (M1.10).
- Presets: `presets/<region>/<bank>.toml`, all compiled in by `build.rs`;
  `presets/uk/monzo.toml` is the reference (M1.9, `[sub_account_transfer]` for pots since
  M1.17).
- `tests/preset_golden.rs`: freezes every bundled preset's fixture row external IDs and
  fingerprints against `tests/golden/<region>/<bank>.txt` (R.5); `ORRE_BLESS_GOLDEN=1`
  writes missing files.

## `crates/mcp`

- `server`: gate → call → log per call, the database opened per call,
  `tools/list_changed`.
- `access`: the gate, argument redaction, result hash. `registry`: `ToolSpec`, `Call`.
- `store`: the only writer.
- `tools/`: catalog, aggregate, rows, recurring, inbox (18 tools). `prompts`:
  `monthly_review`.
- Tests use the harness in `crates/mcp/tests/support/`, with `matrix.rs` (every tool ×
  level) and `m2_exit.rs`. Client setup: `docs/mcp-setup.md`.

## `crates/app`

Package `orre-app`, lib `orre_app`, binary `orre`. Every view opens the database
per call through its own store and changes data only through core.

### Shell and components

- `shell.rs` (M3.1): the title bar's view tabs (`View::TABS`: the four shipped ones,
  ⌘1–⌘4, in a release build; Paper's eight, ⌘1–⌘8, in tests and `dev-samples`
  builds) and Settings button (⌘,), the
  view's left panel (the accounts panel on the five money views), the status bar, and
  the window state (bounds, mode, open view) restored on launch
  (`shell::window_state`). Actions such as `shell::ImportStatement` (⌘I) and
  `shell::NewTransaction` (⌘N). Views without a screen yet show `ui::empty_state`.
  `ShellView::from_database` builds the shell over a database as `orre` opens it (the
  dev driver's tests use it too). `shell::describe` (tests and `dev-driver` only):
  `ShellView::describe`, the open view's description, the dialog over it and focus.
  `shell::live` (LU.3): live updates, the `orre_core::watch::Watcher` on a timer (only
  `shell::open` starts it), the topic map (`live::views`), and the views' own writes
  (`live::Wrote`) rebased and reloaded in coalesced rounds (`docs/architecture/app.md` →
  "Live updates"). What another process's change brought (LU.5): `shell::arrivals`, a
  plain model of the status bar's arrival line, the Inbox badge's unseen fill and the row
  marks (unseen → a 10s hold once seen → a 100ms fade), fed from the views'
  `RegisterChanged` and `inbox::arrival::Arrived` by `live::Shown`.
- `dev_driver` (U.5, `dev-driver` feature, debug builds only): the local socket an agent
  drives a dev build through (`orre dev`). `command` parses a line, `act` does it in the
  window with gpui-kit's test calls, `guard` refuses anything but a scratch
  `ORRE_DATA_DIR`, `server` is the socket, `client` is `orre dev`'s side
  (`docs/architecture/app.md` → "The dev driver").
- `ui::`: Orre's components. `ui::dialog` (also wide) and `ui::menu` (with `MenuList`,
  the menu opened from a chip, a row or the bulk-action bar, with a filter field;
  `MenuList::context` and `at_pointer` for right-click menus, `MenuGroup::footer` for a
  trailing action) on the
  unstyled `gpui-base` roots; `ui::toggle`, `ui::segmented` (also panel size,
  `Segmented::fill`), `ui::notification`, `ui::undo_bar`, `ui::tag`, and the rest of
  `crates/app/src/ui/`. Variants added for later views: `ButtonVariant::Ghost`,
  `SelectField::private`/`amount`, `CategoryCell::suffix`/`repays`, `Glyph::Owed`.
- `ui::list` (U.6): the keyboard behaviour every list shares. A view declares its
  list's keys as a `list::Keys` constant (↑↓, optionally Home/End and ⇧↑⇧↓) and binds
  them in its `init`; `on_list_move` hands it one `list::Move` per key; `list::step`
  (by id) and `list::target` (by index) find the row reached, stopping at the ends;
  `list::Reveal` scrolls it into view. The view keeps its selection and its look.
  `Keys::hint` is the toolbar's "↑↓ Select".
- `ui::keys` (U.7): a view's keys as data. A `keys::ViewKeys` constant holds its
  `list::Keys` and its decision keys (`keys::Key`: keystrokes, action, key cap, label;
  Escape `unhinted`). `bind` binds them, `keys::hints(KEYS.hints())` draws the toolbar's
  hints, a test-only `describe()` lists them, and `ViewKeys::assert_bound` (tests) checks
  the keymap holds exactly them. Used by the Inbox, the review queues (Checks) and Rules.
- `ui::panel_cursor` (U.9): the keyboard cursor for a left panel of filters (Rules and
  the Inbox). A Tab stop where ↑↓ move a cursor across sections without changing the
  selection, and Enter or Space act on its row as a click does. `PanelKeys` binds the
  keys, `PanelCursor::keyed` keeps the cursor with the panel's element,
  `panel_cursor(...)` wires the element, and `Frame::mark` gives the cursor row
  `ListRow::current` and the inset ring.
- `ui::describe` (U.1–U.3, tests and `dev-driver` only): what the views' `describe()`
  share (`Action`, `PanelRow`, `Header`, `Section`, `Inspector`, `Dialog`). Each view's
  own is a `mod describe` beside it under `#[cfg(any(test, feature = "dev-driver"))]`
  (`inbox`, `transactions`, `checks`, `rules`, `settings`, `accounts`, `overview`), built from the
  text the view draws; the tests assert on it and the dev driver prints it. Never in a
  release build or reachable from `orre mcp` (`docs/architecture/app.md`, 2026-11-18 and
  2026-11-21).
- `ui::chart`, `ui::income_chart`, `ui::ranked_bars` (M4.1): the chart elements. `chart`
  is the pure arithmetic (axis steps, compact labels, bar extents); `IncomeChart` draws
  the income and spending bars with the net line, hover tooltip and selected month
  (`chart-month-{i}` hit areas); `RankedBars` is the ranked bar list.
- `format`: writes and reads money and dates.
- `showcase`: the component showcase, `orre --showcase` (tests and `dev-samples`
  builds only, like every `fixture` module).

### Overview (M4.1)

- `overview.rs`: `OverviewView`, the selected month, the account filter (the accounts
  panel's selection) and core's `reporting::overview` read for them, on a background
  thread; reloaded by `shell::live` on Ledger and Rules. `overview::store` opens the
  database per call; `overview::text` formats every figure and caption; `overview::view`
  draws the screen (month switcher, figures, compact agent card, chart, ranked bars,
  recurring); `overview::fixture` is the sample data (DESIGN.md's Overview fixture);
  `overview::describe` (tests and `dev-driver` only). `ui::agent_card::CompactAgentCard`
  is the card's compact form.

### Transactions (M3.3)

- `transactions.rs`, opening the database through `transactions::store::TransactionStore`
  (the register on a background thread). Filtered by the accounts panel and its chips,
  sorted within each day, categories set in place.
- `transactions/write.rs`, the edit pipeline: `TransactionsView::write`/`query`, a
  policy plus one core call (e.g. `inbox::own::recategorize`, `change::undo`). Every
  change goes through it.
- `transactions/arrived.rs` (LU.5): what a read changed (new, changed, removed rows), the
  inspector's "This transaction was removed" note and the dialogs' caption;
  `TransactionsView::set_marks` draws the row marks.
- `present` turns rows into cells and menu options; `fixture` keeps the Paper rows as a
  `Register` for snapshots and tests.
- The inspector: `editor.rs` (Merchant, Tags with the tag menu, Note, the bank's note
  read-only, the split editor; M3.9), `links.rs` (Share and Links), `link_dialog.rs`
  (opened by the shell on `TransactionsEvent::LinkRequested`), `linking.rs` (wording),
  `link_fixture.rs` (D6.5's sample data; M3.11), `transfer.rs` (the Transfer section,
  M3.8), `source.rs` (the exclusion card and the Source section), `rule_prompt.rs`
  ("Create a rule from this edit?" and the Rule section; emits
  `TransactionsEvent::CreateRule`/`OpenRule`), `bulk.rs` (the bulk-action bar's Mark as
  transfer and Create rule).
- M3.10: the Excluded chip and excluded rows; the `exclude_dialog.rs`, `confirm.rs`
  (undo-import, delete) and `new_transaction.rs` dialogs; `manual_edit.rs` (a manual
  row's Date, Amount and Account).

### Accounts (M3.2, M3.17)

- `accounts/`: `Accounts` (the accounts and the selection, read on a background thread
  by `reload`, built `unloaded` by the shell; every change through `store::AccountStore`), `panel` (pots nested, balances, flagged and "No balance" rows,
  arrow keys), `inspector` (on Transactions while an account and no transaction is
  selected: details, balances, pots, imports, the Close and Delete footer; the cash
  inspector's Counts), the `new_account` and `add_balance` dialogs (also for assets and
  liabilities; `add_balance.rs` has the Count cash dialog), `valuations` (the canvas for
  an asset or liability: step-line chart and table, ⌘N), the `close` and `delete`
  dialogs, `notice_layer` (the undo bar after closing or removing a balance),
  `context_menu` (the panel rows' right-click menus, M3.P10), and `fixture` (`valued` is
  D6.9's). `add_balance.rs` also edits a balance (`AddBalanceDialog::edit`, M3.P9).

### Import (M3.4, M3.5)

- `import/`: `Imports` (the notification after an import, with Undo; tells the shell to
  reload the panel and Transactions), `dialog` (`ImportDialog`: File → Map → Review,
  Save as preset… and Save a copy…), `session` (one file in memory: preset, account,
  mapping, statement, preview rows), `store` (`ImportStore`, the database and disk on a
  background thread), `text`, `fixture` (a synthetic Monzo-style export). The Account
  menu's "New account…" hosts `NewAccountDialog` over the dialog (M3.P3). Opened by ⌘I,
  the Transactions toolbar's Import, or a file dropped on the window.

### Inbox and checks (M3.6, M3.14, M3.12)

- `inbox/`: `Inbox` (status, filters, groups, selection, decisions through
  `store::InboxStore`), `panel` (a Tab stop: ↑↓ move a keyboard cursor through INBOX,
  CHECKS, KIND and SOURCE, Enter and Space act), `view` (the grouped table, the selected suggestion open
  in place, A / R / ↑↓), `inspector` ("If you approve"), `text` (labels, untrusted text
  made plain with `envelope::shown`), `arrival` (LU.5: the suggestions a read brought,
  by client; `Inbox::set_marks` draws their row marks), `fixture` (D6.6's samples, seeded through core into
  a scratch database).
- `checks/`: the review queues, opened from the Inbox panel's CHECKS rows (the status
  bar counts failing checks; the accounts panel's flagged dot opens them). `Checks` (the
  open queue, selections, merges, dismissals, undo and locks through
  `store::ChecksStore`; read in the background, `Checks::unloaded` then `reload`),
  `duplicates` (the pair side by side, "If you merge"), `balances` (checks per account,
  the failing window, the lock prompt), `owed` (Owed to you), `lock` (lock and unlock
  dialogs), `view` (M / N / ←→ / ↑↓, the undo bar), `text`,
  `fixture` (D6.4's sample accounts).

### Rules and categories (M3.13, M3.7)

- `rules/`: `Rules` (the open section and each section's pane in `state`: its filter and
  its selection with the form it edits, `RuleEdit::Stored`/`New` holding the apply
  preview; read on a background thread by `reload`, built `unloaded` by the shell and
  first read when `RulesView` is made), `rules/write.rs` (the edit pipeline: `Rules::write`/`query`, a policy plus
  one core call), `store::RulesStore` (`open` and `load`, the read model), `form`
  (`RuleForm`, `NameForm`, amounts typed unsigned with Out/In, percentages), `panel`,
  `table` (the toolbar, Rules, the apply preview, Merchant names, Change history; rows
  dragged to reorder), `inspector`, `dialogs` (delete rule, delete name, undo a change,
  `DeleteCategoryDialog`), `text`, `fixture` (D6.7's rules).
- The Categories section (the categories editor) lives in the same panel, table and
  inspector, with a new-category form; hidden categories are marked in pickers
  (`RegisterCategory::hidden`) and by MCP's `list_categories`.

### Settings (M3.15, M3.16)

- `settings/`: `SettingsView` (every change through `store::SettingsStore`), the panel,
  `clients` (kill switch, clients, requests, the client inspector), `log` (the access log
  and call inspector; never the result hash), `backups` (list, Back up now, the backup
  inspector, the total, Delete backup…), the `enable`, `remove`, `restore` (checks shown
  one by one, then `backup::restore` on a background thread; `restore::refusal`: the
  checklist rows, each refusal's body and Details, "Choose another file…", SLI-71), `delete` (R.12) and
  `export` dialogs, `month_start` (M4.11: the start day and weekend choice, the preview
  of last, this and next month, and the inspector), and `fixture`.

### CLI (M1.18)

`orre_app::cli`: `import` (bundled and saved presets), `accounts` (`list`, `add`,
`--preset`, `close`, `reopen`), `undo-import`, `backup` (`--list`, `--delete <file>
[--yes]`), `restore <file>
[--yes]`, `export --format csv|json`, `mcp --client <name>`, `ai-access` (`list`,
`enable`, `level`, `revoke`, `ignore`, `off`, `on`, `log`), `budgets` (`list`, `set`,
`remove`), `schedules` (`detect`, `list`, `show`, `confirm`, …), `inbox` (`list`,
`show`, `approve`, `reject`, and saved reports: `reports`, `report`, `delete-report`, the
user's way under the 500-report cap) and `categorize <transaction-id>...
<category-key>`, and in a `dev-driver` build `dev <command>` (the dev driver's client).
`cli::terminal`: the commands that act as the user need an interactive stdin (R.1),
checked by `cli::run_with`.

## Fixtures and end-to-end tests

- Synthetic fixtures: `fixtures/csv/` (test-only mappings in `fixtures/csv/mappings/`),
  `fixtures/presets/`, `fixtures/ofx/`.
- Importer-path tests: `crates/import/tests/` and `crates/app/tests/` (M1.19);
  `crates/import/tests/m1_exit.rs` walks the M1 exit criteria end to end.
- MCP: `crates/mcp/tests/` (above).
- Invariant checks: `crates/core/tests/invariants.rs` scans the workspace's source for
  rules in `AGENTS.md` → "Invariants"; `.lefthook/pre-push/` holds the ones that need
  git or `cargo tree` (shipped migrations, dependency direction).
- App views: `#[gpui_kit::test]` tests next to each view (e.g.
  `transactions/write_tests.rs`), on the views' fixtures.
- `cargo run --features dev-samples --example snapshot -- <dir>` renders the app and the
  showcase headlessly to PNG for comparison with Paper.
- `crates/app/tests/release_gating.rs`: what a build without `dev-samples` ships (no
  `--showcase`, the four shipped tabs and their shortcuts).
