# Changelog

All notable changes to Orre are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Orre uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). How a version is tagged and
built: [`docs/releasing.md`](docs/releasing.md).

## [0.1.0]

Draft: `v0.1.0` is not tagged yet, so this entry has no date.

The first release: a local-first personal finance dashboard that imports bank exports
into a SQLite database on your machine and lets an AI client you choose read it, with
limits you set. Not in 0.1.0: Windows builds, an Intel macOS build, a notarised macOS
app, bundled CSV presets for banks other than Monzo, and the analytics screens beyond
the monthly Overview (see the [roadmap](docs/ROADMAP.md)).

### Added

- **Downloads built by the release workflow** ([`docs/install.md`](docs/install.md)):
  - an unsigned macOS `.dmg` for Apple Silicon (ad-hoc signed, not notarised; see
    [how to open it past Gatekeeper](docs/install.md#why-macos-warns-you-and-how-to-open-it));
  - a Linux x86_64 tarball that needs glibc 2.35 or later;
  - a Nix flake for Linux, which you build from the repository;
  - a SHA-256 checksum beside the tarball and the `.dmg`.
- **Import.** OFX and QFX files from any bank that exports them; CSV with the bundled
  Monzo preset (`uk-monzo`, including Monzo pots as sub-accounts); and any other CSV
  through the column-mapping screen, which can save your mapping as a preset. Every
  import is previewed, deduplicated (the bank's ID first, a fingerprint otherwise),
  reconciled against the file's balances where it has them, committed in one
  transaction, and undoable as a batch. Rows that can't be parsed and balances that
  don't reconcile are flagged, not skipped.
- **Accounts.** Bank accounts with opening balances, pots shown under their parent,
  cash accounts, and manual assets and liabilities (a house, a car, a pension, a
  mortgage, a loan). Dated balance assertions are checked against the computed balance.
- **Transactions.** A table with filter, sort and search; categories from a default
  two-level set; split transactions; tags, including private tags that AI clients never
  see; manual transactions; and excluding a row with a recorded reason (it stays visible
  under a filter). Imported source fields (date, amount, bank text, external ID) are
  never edited.
- **Rules and categories.** Rules match on text, regex, amount range or account and can
  set a category or merchant, add a tag, set a note, exclude a row or apply a split
  template. Applying a rule to past transactions is previewed and undoable as a batch.
  You can create a rule from an edit. Bank text is cleaned into a display merchant.
- **Transfers, refunds and duplicates.** Transfer pairs between your own accounts are
  detected for you to confirm; refunds, reimbursements and shared costs can be linked;
  likely duplicates wait in a review queue to merge or dismiss.
- **Monthly Overview.** Income, spending and net for the month, a category breakdown and
  the change from last month.
- **Configurable month start.** In Settings, start the month on a day from the 1st to
  the 28th or on the last day, and move it to the Friday before when it falls on a
  weekend. The Overview, the Transactions month filters, the Inbox and MCP's monthly
  tools follow it.
- **Live updates.** The open app picks up changes made by another process, such as an
  import in a terminal or an AI client's suggestion.
- **Backups and export.** A rotating local backup before every import and migration,
  manual backups, restore with integrity and schema checks, and export to CSV or JSON.
- **Restore.** Before you confirm, Orre shows how much the restore will undo: the
  imports, edits, Inbox suggestions and hand-added transactions since the backup, and
  any hand-added transactions you've deleted since, which come back. Changes to rules,
  accounts and settings are undone too, but not counted.
- **CLI.** `orre import`, `accounts`, `undo-import`, `backup`, `restore`, `export`,
  `ai-access`, `budgets`, `schedules`, `inbox` and `categorize` work without the GUI.
  `orre schedules` turns a detected recurring payment into a schedule and flags missed
  or late payments.
- **MCP server.** `orre mcp` gives AI clients read tools over your data, off until you
  enable a client, and limited by the access level you give each one (`aggregate`,
  `pseudonymous` or `standard`). Its text search over transactions uses a SQLite FTS5
  index. A client can only file suggestions, which wait in the Inbox until you approve
  or reject them in Orre (with a preview of what each would change), and save reports.
  Every tool call is recorded in an access log you can read in Settings or with
  `orre ai-access log`. Setup for Claude Code, Claude Desktop and Codex:
  [`docs/mcp-setup.md`](docs/mcp-setup.md).
- **Documentation.** Install steps ([`docs/install.md`](docs/install.md)), where your
  data lives, backups and privacy ([`docs/data-and-privacy.md`](docs/data-and-privacy.md)),
  adding your bank as a CSV preset without writing Rust
  ([`presets/README.md`](presets/README.md)), and the project's
  [`CONTRIBUTING.md`](CONTRIBUTING.md), [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) and
  [`SECURITY.md`](SECURITY.md).

### Security

- Your data stays in a local SQLite file, created readable only by you, and Orre sends
  no telemetry.
- AI clients never see account numbers, sort codes, IBANs, notes or private tags,
  at any access level; names appear only at `standard`.
- Bank text reaches AI clients only sanitised and labelled untrusted, and text that
  reads like instructions is withheld. A kill switch in Settings and `orre ai-access`
  turns all AI access off at once.
- What Orre protects, what it doesn't, and how to report a vulnerability:
  [`SECURITY.md`](SECURITY.md).

[0.1.0]: https://github.com/TheSlimeCompany/orre/releases/tag/v0.1.0
