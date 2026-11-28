# Orre — Architecture: Reporting

Budgets, the forecast, and how spending is counted over the ledger.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Reporting

`orre_core::reporting` (queries over a `Reader`), `orre_core::budget` and
`orre_core::schedule`. The MCP read tools and the app call these; none logs
transaction text. Amounts are `Money` in integer minor units, and totals are per currency
(`Totals`), never added across currencies: conversion waits for M4.6. The rule in force
is below; the records under "Decision records" are the history.

### The ledger and the spending rule

- **One source.** Every report reads transactions through `reporting::Ledger`, a query
  over the `ledger_line` view (V9): one line per unsplit transaction, per split line,
  and per confirmed link's part on the money in. It keeps only lines in reports: not
  excluded, not in a confirmed transfer, and for a link's part, its purchase isn't
  either; a line of zero is dropped.
- **Refunds** count on their own date, as money in under the purchase's category
  (`links-and-checks.md` → "refunds, reimbursements and shared costs", point 1).
- **Basis.** `Basis::Personal` (`personal_minor`, the default) counts the user's share:
  half of a shared cost, nothing of a purchase being claimed back. `Basis::Gross` is the
  whole line. Spending, budgets and MCP reads use the user's share; balances and net
  worth are always gross (same record, point 3).
- **Side** (`Side::of`, applied by `LedgerLine::side`) is the one rule for income,
  spending or neither: a category of kind `income` is income, `spending` is spending
  whatever the sign (money in there lowers spending), `transfer` is neither; an
  uncategorised line goes by the sign of the amount that counts, and zero is neither
  (decision record "one spending rule over the ledger", point 1).
- **Transfers.** A confirmed pair is out of reports whatever its legs' categories; a
  suggested pair counts by its own category until confirmed (`transactions.md` → "two
  kinds of transfer, and marking a suggested pair", point 1).
- **Text.** `Ledger::matching` finds a text in the display merchant, bank merchant or raw
  description. From 3 characters it reads candidates from the `transaction_text` FTS5
  index and confirms them with `LIKE`; under 3, or with a NUL, it uses `LIKE` alone
  (decision record "full-text search over bank text").
- **Similar transactions.** `search::similar` ranks a transaction's neighbours through
  the same index: its stem's words as literal phrases, same currency and sign, by
  `bm25` then the nearest amount (decision record "similar transactions").

### Reports

- `summary::monthly_summary`: income, spending and net per month, empty months included.
- `spending::spending_by_category`: one month's spending lines per category, nested
  under their groups (the group's own, its subcategories', and a group total).
- `budget_status::budget_status`: see "Budgets" below.
- `net_worth::net_worth_history` and `account_tree::account_tree`: see "Net worth".
- `rows`, `catalog` and `register` list rows, accounts, rules and reports, and the
  Transactions table. `palette::category_palette` ranks category colours with the
  register's ranking (`inbox.md`, decision record "Inbox", point 6).
- **Months** run from the user's month start (M4.11, `reporting::MonthStart` in the
  `reporting.month_start` setting; the default, the 1st, is calendar months): a day of
  the month or its last day, optionally moved back off weekends, named by the nominal
  start (`Period`, `YYYY-MM`). `Period`'s dates (`containing`, `first_day`, `last_day`,
  `range`, `DateRange::months`) take the rule; every report, budget, Inbox month label,
  the register's month presets and every MCP tool that says "month" follows it, and
  month results carry their `range` (MCP: `from` and `to`). Decision record "the month
  start".

### Previews of a change

A preview tallies the ledger lines a change touches before and after it, the change made
in a rolled-back DB transaction (`change::dry_run`). `Ledger::transactions(ids)` scopes
the ledger to those transactions' lines and the confirmed link parts that net against
them; `reporting::Tally` sums lines by side, category and currency, and
`Tally::change` gives the difference. The Inbox's "If you approve" and the review
queue's "If you merge" (`checks::merge_effect`) both read it, so they move what reports
move (decision record "one spending rule over the ledger", points 2, 3 and 5). When the
change would be refused, the preview shows none (point 4).

### Net worth

- **Balances** come from `balance::balances_at`: gross, from the account's stated
  balances (opening balance, entered, from a file, or a valuation) and its transactions,
  leaving out rows excluded as `duplicate` or `superseded`. A valuation account
  (`asset`, `liability`) holds its last valuation.
- **What counts** is `net_worth::counts`, shared by `account_tree` and
  `net_worth_history`: an account with a stated balance counts, and so does a pot, from
  its transfers; any other account with none is left out until a balance is entered.
  `net_worth_history` still lists such an account, flagged `unanchored`, adding nothing
  (`AccountWorth::is_counted`) (decision record "one net worth rule").
- A liability always subtracts. `net_worth_history` leaves a closed account out of every
  month that ends after its `closed_on`; `account_tree` is as of the `today` it's given and
  lists the accounts open on it, each at its latest balance, on its own last active day. An
  account closing on a later day is still open there, at its balance as of today, so the
  £0 valuation for its closing day doesn't count yet (SLI-77).

### Budgets

What is built (`orre_core::budget`, `orre budgets`, the MCP tool `budget_status`):

- A budget belongs to one `spending` category, a group or a subcategory
  (`Error::NotSpending` otherwise). It is standing (every month) or for one month,
  which overrides the standing one that month; at most one of each per category.
- Its group (`BudgetGroup`) is `fixed`, `flex` or `non_monthly`. The schema's CHECKs fix
  rollover by group: `fixed` never, `non_monthly` always; `flex` doesn't yet
  (`BudgetGroup::rollover`). A non-monthly budget may store an
  `opening_set_aside_minor`.
- `budget_status` reports, per budget, the user's share spent in the month in its
  category, a group's including its subcategories, with refunds lowering it. Only the
  budget's currency counts against it; other currencies are listed apart
  (`spent_other_currencies`). Remaining is budget minus spent, negative when over.
  Rollover and the set-aside are reported, not applied.

Decided but not built (M4.8, M4.9; decision record "budgets and the forecast"): rollover
both ways, derived from `ledger_line` and never stored (point 3), with a toggle for
`flex`; pace linear by day of the month (point 4); a non-monthly amount seeded once from
the category's yearly schedules, accruing 1/12 a month (point 5); the forecast (below).

### Schedules and the forecast

- **Missed** is derived in `schedule` (`OccurrenceState`): an occurrence's date plus
  `tolerance_days` has passed, nothing matched or skipped it, and the account's imports
  cover that date. Until then it is upcoming, and nothing is flagged (decision record
  "budgets and the forecast", point 2). Skips and the user's matches are stored
  (`schedule_skip`, `schedule_match`), and a series marked "Not recurring" in
  `recurring_dismissal`.
- **The forecast** (M4.8) is not built. As decided: the last known balance plus schedule
  occurrences, plus each Flex budget's remainder spread evenly over the rest of the
  month and across accounts by the last 3 months' spending, in integer minor units
  (point 1); a missed occurrence counted from tomorrow until matched or skipped (point
  2); the horizon is the account's next scheduled income, else 30 days (point 6).

## Decision records

Oldest first.

### Decision record: budgets and the forecast (2026-11-10)

The open questions from the D6.10 designs (`DESIGN.md` → "Budgets: groups, rollover,
pace and upcoming"), decided by the user on the designer's recommendation:

1. **Flex spending leaves accounts in proportion to history.** The forecast spreads each
   Flex budget's remainder evenly over the rest of the month. It splits that remainder
   across accounts by where the category was spent over the last 3 months. An account
   with no Flex spending in that window (savings, pots, Joint in the fixture) gets none.
   Later months spend their full Flex budget the same way. Every step is integer minor
   units, with remainders spread so each day's amounts sum exactly. Fixed and
   non-monthly budgets add nothing to the forecast beyond their schedules, which avoids
   counting them twice.
2. **A payment is missed only once imports cover its date.** A schedule occurrence is
   *missed* when its date plus `tolerance_days` has passed, nothing matched it, and the
   account's latest imported transaction is on or after that date. Until then it is only
   *not imported yet*, and nothing is flagged. A missed occurrence stays in the forecast,
   counted from tomorrow, until the user matches it to a transaction or skips it.
   Missed is derived, not stored. A skip is stored (`schedule_skip(schedule_id,
   occurrence_date, skipped_at)`), and so is a user's match (`schedule_match(schedule_id,
   occurrence_date, transaction_id)`). A series the user marks "Not recurring" is not suggested again.
3. **Rollover carries both ways, derived from history.** With rollover on, a month's
   carry-in is the previous month's LEFT, positive or negative, so it accumulates.
   Nothing about it is stored: it's recomputed from `ledger_line` and the budgets, so
   editing or recategorising a past transaction flows into later months. Turning
   rollover off starts the budget fresh from the next month; turning it on starts
   carrying from the current month. Fixed budgets don't offer rollover, and non-monthly
   budgets always roll over.
4. **Pace is linear by day of the month in v1.** On day *d* of an *n*-day month, pace is
   *d*/*n* of the month's amount (budget plus rollover). Pace weighted by past months'
   daily pattern can come later; it's a reporting choice, not a schema one.
5. **A non-monthly amount is seeded once, then belongs to the user.** A new non-monthly
   budget's yearly amount starts as the total of the category's active yearly
   schedules. After that it's the user's number and doesn't follow schedule changes.
   A new yearly schedule in that category asks before adding its amount (the schedule
   dialog's "Set aside" toggle). The set-aside amount accrues from the month the budget
   is created. It starts at £0 unless the user enters what's already saved, which is
   stored as an opening set-aside. The 1/12 accrual stays a default (decision record of
   2026-11-09, point 4).
6. **Without scheduled income, the horizon is 30 days.** "Lowest before payday" uses the
   account's next scheduled income as the horizon. An account with none shows "Lowest in
   the next 30 days", and the Payday figure is left out. `cash_flow_forecast` (M4.8)
   reports the same horizon and says which one it used.

### Decision record: one spending rule over the ledger (2026-11-16)

From an architecture review of `reporting`. Nothing the user sees changes except the
Inbox's "If you approve" amounts (point 3) and the review queue's "If you merge" (point
5).

1. **Whether a line counts as income, spending or neither is decided in one place:**
   `reporting::Side::of(category kind, counted amount)`, applied by
   `LedgerLine::side(basis)`. The category's kind decides; an uncategorised line goes by
   the sign of the amount that counts, and zero is neither. Summary, spending by
   category, budgets, the Inbox preview and the merge preview use it. The copies in
   `summary`, `inbox::preview` (`is_spending`, `is_income`) and `checks`
   (`spending_of`) are gone. `register::rank_groups` asks a different question (which
   groups get colours, decision record "the Transactions view", point 2) and is left
   alone.
2. **A preview of a change reads the ledger as reports do.** `Ledger::transactions(ids)`
   scopes the ledger to those transactions' lines and the confirmed link parts that net
   against them (a refund's part moves with its purchase); `Ledger::lines_in` runs it on
   the connection of a `change::dry_run`. `reporting::Tally` sums lines per side,
   category and currency, and a preview shows the change from before to after. Amounts
   are the user's share ("Spending is your share by default", M2 C.1 and C.2).
3. **The Inbox's "If you approve"** (spending changes for a categorisation or a rule,
   and the income and spending a transfer pair or refund link takes away) comes from
   one rolled-back approval made as the Inbox makes it (a rule applied to history,
   decision record "Inbox", point 2), with the touched rows' ledger read
   before and after. Before, it summed whole transaction amounts: recategorising a
   shared £100 purchase showed £100 moving where reports move £50, and a purchase being
   claimed back moved its full amount where reports move nothing. A refund link lists
   every category whose spending moves, in the refund's month, as a categorisation
   does: a split purchase's refund nets against each line's category, and a refund
   already in another spending category moves out of it.
4. **When approving would be refused, the preview shows no change:** no spending
   changes, and no income or spending amounts on a transfer or link effect.
   `Impact::blocked` or `RuleImpact::history_blocked` says why. A ledger total that
   overflows is `ApproveError::Reporting`.
5. **The merge's "If you merge" spending** is the same tally of the pair's ledger before
   and after the planned merge, so it now also counts a refund of the excluded row
   leaving reports.
6. A wide rule's per-merchant `total` stays a gross listing sum; it lists the rows, it
   doesn't report spending. `inbox::impact`'s own "moves income" test is not changed
   here.

### Decision record: one net worth rule (2026-11-19)

The accounts panel and `net_worth_history` disagreed about an account with no stated
balance (nothing entered, no balance from a file, no valuation). The panel showed "No
balance" and left it out of net worth; the history summed it from zero, added it to the
totals and only marked it `unanchored`.

1. **One rule, in both (user).** An account other than a pot with no stated balance is
   left out of net worth until a balance is entered, because its sum from zero is right
   only if it started empty. A pot is counted from its transfers even without one
   ("Decision record: sub-accounts"). A valuation account is still left out before its
   first valuation. `net_worth::counts` is the one function that says which, used by
   `account_tree` and `net_worth_history`, so they can't drift.
2. **What changed in `net_worth_history`.** Such an account is still listed for each
   month, flagged `unanchored`, so nothing is silently dropped, but its `counted` is
   zero and it adds nothing to the month's totals (`AccountWorth::is_counted` says
   which). A pot stays counted from its transfers, as before, and is still flagged
   `unanchored`. The MCP tool's description says so.
3. **Rejected: count it from zero and mark it everywhere.** An account opened
   mid-history with money already in it would understate or overstate net worth without
   the user seeing a number to doubt.

### Decision record: full-text search over bank text (2026-11-23)

ROADMAP FT.1, before FT.2 and FT.3. The roadmap's proposals, composed by the lead from
reading `reporting::ledger` and the `ledger_line` view, and confirmed by the user on
2026-11-24 with point 7's interim choice. FT.6's measurements are point 9.

1. **What's indexed.** A transaction's display merchant, bank merchant and raw
   description: the three columns `Ledger::matching` searches today. Notes stay out:
   they never reach MCP at any level. Every row of `"transaction"` is indexed, excluded
   and superseded rows too; the ledger already decides which lines count. One entry
   per transaction covers every ledger line, because each line's display merchant is
   its own transaction's, refund, reimbursement and settlement parts included
   (`ledger_line`, V21).
2. **The table.** `transaction_text`, an external-content FTS5 table over
   `"transaction"` (`content_rowid` is the transaction ID), so the text isn't stored
   twice. It uses the `trigram` tokenizer with `case_sensitive 0` and no diacritic
   removal. The bundled SQLite is built with FTS5.
3. **How it stays in sync.** Three triggers on `"transaction"`: after an insert, after a
   delete, and after an update of one of the three columns. The display merchant is
   stored on that row (V7), so no other table needs a trigger. Imports, undo, edits,
   rules, merges and supersedes all reach the index through those triggers. The
   migration builds the index for the rows already there, so restoring an older backup
   builds it too. A migration that rebuilds `"transaction"` (the 12-step procedure)
   drops the triggers and must recreate them; the IDs carry over, so the index stays
   valid. A schema test checks the triggers exist and that FTS5's integrity check
   passes against the table. MCP never writes `"transaction"`, so "MCP writes go to the
   inbox only" is untouched.
4. **Query text is a literal.** Text from the user or an MCP client becomes one FTS5
   string (a double-quoted phrase with its quotes doubled), never FTS5 syntax: `OR`,
   `NEAR`, `*`, column filters and quotes can't change what a search means.
5. **The index narrows, `LIKE` decides.** The trigram tokenizer folds case over all of
   Unicode; SQLite's `LIKE` folds ASCII only. On its own the index would match more
   than today (`É` finding `é`). So a query of 3 characters or more keeps the rows
   whose transaction the index matches, then applies today's `LIKE` test to them: the
   same rows as before, read from the index rather than a scan of every row. A query
   under 3 characters, which trigrams can't index, uses `LIKE` alone, and so does one
   holding a NUL: FTS5 reads a query as a C string, so the phrase would end at the NUL
   and be refused as a syntax error.
6. **Untrusted text.** The index holds bank text, which is untrusted, and it's only
   matched against, as now. Nothing reads text back out of it.
7. **Access levels unchanged.** Text search stays at `standard` only (`mcp.md`, "Text
   search"); `search_transactions`' reach is unchanged. Whether it's faster is FT.6's
   to measure: a first look at 100,000 synthetic rows (the review of FT.3) found a rare
   text 300 times faster than `LIKE`, a text in 1 row in 15 four times faster, and a
   text in every row about 1.7 times slower. The index was 47% of the file, and raw
   inserts took about twice as long. FT.6 measures these on the import path and
   decides. Until then FT.3 reads through the index (user, 2026-11-24): a search is
   usually for one merchant, where the gain is large, and a text in every row costs
   about a tenth of a second more. The size of the index, and so of backups, is the
   cost FT.6 weighs most. FT.6 measured it (point 9).
8. **Rejected.** A table holding its own copy of the text: the text twice on disk and
   in every backup, on top of the index. The index alone as the matcher: it would
   change which rows a search returns, and FT.3 promises the same rows. Diacritic
   removal: it would widen matches further, for the same reason.
9. **Measured (FT.6, 2026-11-24).** `ledger_bench --only search` (release, with
   `--features bench`, which makes `Ledger::like_only` reachable; `app.md`, "performance
   at 100,000 transactions", has the ledger and the machine). It builds the 100,606
   transaction ledger afresh, so its triggers fill the index as rows are imported. For
   the sizes and the imports it copies the ledger with `VACUUM INTO`, as a backup is
   made, once as it is and once with the index and its triggers dropped. The searches
   run on the ledger itself, as `search_transactions` makes them: each opens the
   database as `orre mcp` does (through the gate, with `open_existing`, which runs no
   migrations), then `rows::search` with the default page of 50, against
   the same ledger on `LIKE` alone. Both paths return the same lines; the bench checks
   every one. Medians; the size and import rows span four runs of the bench, the
   searches are the last run's:

   | | With the index | `LIKE` alone / without it |
   |---|---|---|
   | A backup (a `VACUUM INTO` copy) | 55.0 MiB | 34.8 MiB |
   | Import: commit of 1,000 rows, backup included | 456–501 ms | 372–396 ms |
   | Import: the Review step's "After import" | 309–310 ms | 253–267 ms |
   | `search_transactions`, a text in 30 lines | 5.7 ms | 182 ms |
   | … a merchant in 438 lines (0.4%) | 10 ms | 178 ms |
   | … a merchant in 9,086 lines (9%) | 71 ms | 181 ms |
   | … a text in 97,254 lines (97%) | 339 ms | 227 ms |

   The open is 4 to 6 ms of each search. V24's migration, which runs once when an
   older backup is restored, took 0.46 to 0.48 s: its SQL on a fresh copy of the
   ledger without the index, creating the index and filling it for the first time.
   (Rebuilding an index that already exists took 0.53 to 0.56 s.)

   **(composed)** It pays, and FT.3 keeps reading through the index. Most merchants in
   the bench ledger are in under 1% of its lines. A search for the rarest of them (0.4%
   of lines), or for rarer text, is 17 to 32 times faster: about 180 ms down to 6 to
   10 ms. The busiest merchant (9% of lines) is 2.5 times faster. A text in nearly
   every row is 1.5 times slower, about a tenth of a second more, as point 7 found. The costs: the index is
   20.2 MiB, 37% of a backup, and with up to 22 automatic backups kept
   (`backup::KEEP_RECENT` and `KEEP_MONTHS`) that's at most about 0.43 GiB more on disk
   at this size, less in practice since older backups hold fewer rows. A 1,000-row
   import commits 67 to 105 ms later (17 to 26%), its larger backup included. Neither
   cost changes with FT.3's choice: FT.4 ranks by `bm25` over the same index, so going
   back to `LIKE` would give up the speed and keep the costs.

### Decision record: similar transactions (2026-11-24)

ROADMAP FT.4, over FT.1's index. Points 2, 3 and 4, the cap in point 4, the word check
in point 1 and the cost in point 8 decided by the user on 2026-11-24 on the lead's
recommendation; the rest composed by the lead from `rule::proposal`, `inbox::own` and
FT.1's record. No migration.

1. **What a neighbour is.** `search::similar` finds a transaction's nearest neighbours
   through `transaction_text`. Its text is the row's stem as `rule::proposal` cuts it
   (`rule::stem` of the cleaned bank merchant); a row with no bank merchant uses its
   cleaned description's stem, against the description column. The index holds the raw
   column, and cleaning collapses whitespace, so the stem isn't one phrase: each of its
   words of 3 characters or more is a literal phrase (`search::phrase`), ANDed, under a
   column filter for that one column (`{merchant} : ("AMAZON" AND "PRIME")`). No stem,
   or no word of 3 characters, means no neighbours. The index narrows and the words
   decide (user, from the review): trigrams match substrings and skip short words, so
   `EE LIMITED` would match every `LIMITED` and `WH SMITH` would match `SMITHSON`. A
   match is kept only when its column, cleaned and case-folded as the stem is, holds
   every stem word, short ones too, as a word of its own. The words are case-folded as
   the index folds them, one character to one character, not with `rule::fold`
   (review, 2026-11-24): the trigram index can't find `Straße` from `STRASSE`, so the
   words don't call them the same either, and neither spelling votes for the other.
   Matching them would take a different tokenizer, a migration. A neighbour has the same
   currency and the same sign (a zero amount has none), isn't the row itself, and isn't
   excluded as `duplicate` or `superseded`, which are copies of other rows.
2. **Ranked by `bm25`, then amount (user).** Best `bm25` first, then the amount closest
   to the row's (`abs` of the difference in minor units), then the newest, then the
   highest ID. The amount orders; it doesn't filter. `bm25` weighs a row's whole indexed
   text, its description included, so the amount mostly orders rows whose text is the
   same, such as one merchant's repeated payments. No float reaches Rust: one query per
   column, words, currency, sign and filter (a pool) ranks its matches with an integer
   `dense_rank()` of `bm25`, and each row orders the pool by that rank, then amount.
   Rows that share a stem share a pool, so an import runs one query per merchant, not
   per row.
3. **Orre suggests a category when 3 of the 5 nearest agree (user).** After an import,
   each imported row of the batch that takes a category of its own and has none, isn't
   split and isn't in a suggested or confirmed transfer gets its 5 nearest categorised
   neighbours (any category source; a split row is not categorised). When at least 3
   hold one category, Orre proposes it for the row: a `categorization` suggestion from
   `orre` (`ORRE_SOURCE`), never an AI one. A confirmed transfer's legs and a confirmed
   refund's money in don't vote: they keep a category they no longer use. One
   suggestion per category per import, its items in ID order, at most
   `payload::MAX_ITEMS` each; the rationale is a fixed sentence with no bank text.
   Approving one sets the category with source `ai_approved`, as any approved
   categorisation does (`inbox.md` → "Approving and rejecting").
4. **Once per row (user).** A row named in any `categorization` suggestion, pending or
   decided, from any source, isn't proposed again: the user's no stands, and an AI
   client's proposal isn't doubled. **(user)** Orre's rule suggestions
   (`inbox::own`) share its cap of 200 pending, so category suggestions stop at 150
   pending from `orre` (`MAX_PENDING_CATEGORY_SUGGESTIONS`), leaving room for rules.
5. **After the commit, never inside it.** `import::commit` files the suggestions in a
   DB transaction of its own once the import has committed. If that fails, the import
   still stands: the warning names the batch's ID only (an SQLite error can quote the
   query), the error itself is logged at `debug`, and the commit returns as before. `import::rehearse` files nothing, and the import stays
   deterministic: no model, no network, the same database gives the same suggestions.
   Undoing the import leaves a pending suggestion naming its rows, which the Inbox
   already shows as gone, as for an AI client's.
6. **Untrusted text.** The stem is bank text, used only as literal phrases to match,
   as FT.1 point 6. A match's column is read from `"transaction"` only to compare its
   words with the stem's (point 1), never kept or logged; nothing is read out of the
   index, and the suggestion names IDs and a category key.
7. **Left for later.** "Similar" in the transaction inspector waits for its Paper
   design (Paper wasn't reachable on 2026-11-24). The read tool at `standard` waits for
   L.32. The Inbox labels every suggestion from `orre` "from your edits"
   (`inbox.md` → "The Inbox screen"), which doesn't fit this one; its copy is for the
   Paper design too.
8. **Measured (2026-11-24).** `ledger_bench --only search` (FT.1's record, point 9) at
   100,606 transactions; medians of 3. The new case is 1,000 uncategorised rows of a
   merchant with 8,862 categorised rows.

   | A 1,000-row import | Before FT.4 (two runs) | With FT.4 |
   |---|---|---|
   | Commit alone, with the index | 489–541 ms | 851 ms |
   | Commit, then undo, with the index | 517–581 ms | 825 ms |
   | Commit alone, 1,000 uncategorised rows of one categorised merchant | | 470 ms |

   With point 1's word check; runs vary by about 15% (the commit without the index
   took 354 to 425 ms across them).

   The step costs one query per pool, not per row. The bench's own import file is
   close to the worst shape: no merchant column, so each row's stem is its whole
   description, `<NAME> <NAME> LONDON` with no reference to cut at. That gives 120
   pools, and each ANDs `LONDON`, which is in 97% of rows, so most likely every query
   reads that word's whole list. About 370 ms more on a commit that runs off the UI
   thread.
   **(user)** Accepted. If real files show the
   same shape, the next step is to leave out of the query a stem word that the index
   says is in most rows.

### Decision record: the month start (2026-11-24)

ROADMAP L.15, decided before M4.1 because every report reads months. Decided by the user
on 2026-11-24 on the agent's recommendation, without a Paper pass: where the setting
lives in Settings is for Paper (point 9). No migration: the setting is a row in
`setting`.

1. **One rule for the whole database.** A *month* in Orre is the period from one month
   start to the day before the next. The rule is one setting, not per account or per
   budget, so every report, budget and MCP tool cuts the same months. The default is
   the 1st, which is today's calendar months: nothing changes until the user sets it.
   *Amended 2026-11-25 (M4.11): a stored value that can't be read also means the 1st,
   logged at `warn` without the value, so a bad row never stops a report; only a
   database error does. Fields the rule doesn't know are ignored, so point 10's
   overrides can be added to the same value without an older build failing to read
   it.*
2. **The start is a day of the month, or its last day, optionally moved off weekends.**
   - **Day 1 to 28,** or **the last day of the month.** Not 29 to 31, which some months
     lack; "the last day" covers the employers who pay at month end.
   - **Weekends:** off, or **the Friday before.** With it on, a start that falls on a
     Saturday or Sunday moves back to the Friday, as UK payroll does. Only weekends:
     Orre knows no bank holidays (a calendar would be regional data to maintain, and a
     fetch would be a second network call), so a payday moved early by a holiday, such
     as the 25th in December, lands in the month before. The fix is point 10's
     per-month override, not a holiday table.
3. **A month is named by its nominal start.** Days 1 to 15 use the calendar month of
   that start. Days 16 to 28 and the last day use the next calendar month. With the
   25th, 25 Oct to 24 Nov is November, which is the month that pay is for. This is a
   fixed rule, not a count of days: with the 16th, 16 Mar to 15 Apr has more days in
   March but is named April. The name uses the nominal start, not the weekend-moved
   one, so a Friday move never renames a month. The `YYYY-MM` key (`Period`) stays
   what it is now, the name; only its dates come from the rule. So `budget.month`, one-month budget
   overrides and every tool's `month` argument keep their meaning, and stored data
   needs nothing rewritten.
4. **History is cut by the rule in force, never stored.** Changing the setting re-cuts
   every month, past ones included, as editing a past transaction already flows into
   rollover (record "budgets and the forecast", point 3). Nothing stores which rule cut
   a month. Rejected: a dated history of rules (the new start from a date on), since
   reports across the change would mix two kinds of month for the sake of a setting
   most people set once. A budget keyed `2026-12` stays December's under any rule.
5. **What follows it.** Everything that says "month":
   - `monthly_summary`, `spending_by_category`, `budget_status`, and the
     `monthly_review` prompt.
   - `net_worth_history`: one point per month, at its last day.
   - Inbox impact previews: the month labels on transfer and refund effects ("less in
     Nov"), so a preview names the month a report will count the money in.
   - Budgets: rollover from month to month, pace as *d*/*n* with *n* the month's days
     (point 4 of the budgets record), the non-monthly 1/12 accrual per month.
   - The forecast's Flex remainder, spread over the rest of the month (M4.8).
   - Transactions' This month, Last month and Last 3 months presets: the register is
     where a budget's spending is checked, so it cuts the same months. Whenever a
     preset covers a different range than the calendar month would, the Date chip's
     menu shows its dates: any start but the 1st, and the 1st with the Friday move when
     the 1st falls on a weekend (1 Aug 2026 is a Saturday, so August starts 31 Jul).
   - Not weeks (ISO, Monday to Sunday, as M3.P12 decided), not This year (calendar), and
     not dated things: schedules, the forecast's payday horizon, exports.
6. **`Period` becomes a month under a rule.** `Period::containing`, `first_day`,
   `last_day`, `next` and `previous` take the rule, read once per report from
   `setting`. A month is never empty and months never overlap or leave a gap, which
   tests check for every start and weekend choice across leap years. Callers that mean
   a calendar month (none today) say so.
   *Amended 2026-11-25 (M4.11): `next`, `previous` and `through` step names, so they
   take no rule; only a month's dates do (`containing`, `first_day`, `last_day`,
   `range`, `DateRange::months` and `periods`). A caller meaning calendar months passes
   `MonthStart::CALENDAR`. The first and last names are held to the dates a
   `YYYY-MM-DD` can hold: `0001-01` starts on 0001-01-01 and `9999-12` ends on
   9999-12-31.*
7. **MCP reads it, says it, and can't change it.** Live month queries return each
   month's `from` and `to` dates, so a client never assumes the 1st; their
   descriptions say months follow the user's month start. Saved reviews keep the
   author's period label (`list_reports` returns it as written, and `save_report`
   takes labels such as `2026-Q3`): Orre never gives a saved report dates from the
   current rule, which may not be the one it was written under. Instead
   `monthly_review` asks the client to include the dates it reviewed in the saved
   text. The rule is not personal data and is returned at every level. MCP can't
   write it ("MCP writes go to the inbox only").
   *Amended 2026-11-25 (M4.11): MCP returns the rule's effect, each month's `from` and
   `to`, at every level, not the rule itself (its day and weekend choice): no tool
   needs it yet.*
8. **Rejected: months that start on payday as it happened.** Periods anchored to the
   salary itself, found by matching income or a schedule's matched occurrences, would
   handle holidays and late pay. But a month's dates would move when an import or a
   match changes, so a report already read could change without any edit to the
   months it covers, and a month would have no dates before its salary arrives.
   Four-weekly and fortnightly periods are left out too: budgets are monthly (13
   periods a year would need a different budget model).
9. **Where it's set.** Settings in the app, designed in Paper first. No CLI command in
   the first cut; `orre budgets` and every report follow the setting.
   *Amended 2026-11-25 (M4.11, built from D6.12, Paper `K57-0` and `KEQ-0`; decided by
   the user): Settings → CALENDAR → Month start, the section named "Calendar" rather
   than the artboard's "Reports". A choice applies at once, with no Save or confirm:
   nothing is rewritten, so it can be changed back. When a month preset's range differs
   from the calendar month, the Date chip's label shows its dates too ("This month · 23
   Oct – 24 Nov"), as its menu does. The Date chip has no "Pick a month…": the
   Overview's switcher picks months, and shows the month's dates beside it when they
   differ. Each preset shows its dates only when its own range differs, so with the 1st
   and the Friday before, Last 3 months (1 Sep to 30 Nov in 2026) shows none.*
10. **Later: one month's start moved by hand.** A per-month override ("December starts
   on 20 Dec") is the answer to holidays and early pay (point 2). It is a list of dates
   in the same setting, no migration, so it can follow without changing this record's
   rule; it waits until the fixed rule is in use.

### Decision record: the Overview (2026-11-24)

ROADMAP M4.1, the monthly overview (`DESIGN.md` → "Overview", Paper `22V-0`). Point 1 was
decided by the user on 2026-11-24; the rest were decided by the agent in the build, under
the spec in `.scratch/m4-1-overview/spec.md`. No migration, and MCP is unchanged.

1. **Built before M4.11, on calendar months (user).** The roadmap put the month start
   first; the user chose the Overview first. Every month the Overview reads comes from
   `Period` (`containing`, `range`, `next`, `previous`, `through`, `DateRange::months`),
   so when M4.11 lands, the Overview follows the month start without its own change.
2. **One read model, in core.** `reporting::overview::overview` reads everything the
   screen shows for one month and one account or all, as of today: the chart's months,
   the selected and previous month, the changes, spending by category and the recurring
   payments due. The app formats it and draws it. Lines are classified as
   `monthly_summary` classifies them (the user's share, `Side`), and for all accounts
   the months agree with it.
3. **One currency on screen.** Amounts in different currencies are never added, so the
   Overview shows the currency with the most counted lines in the chart's 12 months
   (ties by code), and says which other currencies it leaves out ("Amounts in EUR
   aren't included"). M4.6 replaces this with conversion to the home currency.
4. **The chart's window.** The 12 months ending at today's month, unless the selected
   month is before them; then the 12 months ending at the selected month. So clicking
   a month in the window that ends today selects it without moving the window, while a
   window already moved back follows the selection, which is then its last month. The caption is "Last
   12 months", or "12 months to Mar 2025" when the window doesn't end at today's month.
   The switcher stops at the earliest month with a counted line and at today's month.
5. **Changes vs last month.** Spent and Income as a whole-number percentage of the
   previous month, rounded half away from zero in integer arithmetic, and "None in Sep"
   when the previous month had none. Net as the signed amount ("+£212.40 vs Sep"), not
   the percentage Paper shows: a percentage of a net that changes sign, or is near zero,
   misleads. To be confirmed in Paper.
6. **Categories.** One entry per top-level group. The groups with a colour slot (the
   six ranked by `reporting::palette`) are named; every other group adds into "Other";
   uncategorised spending is its own entry. Named entries by amount, then Other, then
   Uncategorised, as `DESIGN.md` orders them. A group whose refunds outweigh its
   purchases is listed with no bar.
7. **Category colours stay ranked at read time.** The Transactions view record (point
   2) said M4.1 would store the colours on `category`. `reporting::palette` already
   gives every screen the same ranking, so the Overview, Transactions and the Inbox
   agree without it. Storing needs a migration and a rule for when a stored colour
   changes (a category that falls out of the top six, a new one that rises), so it waits
   for the user's decision.
8. **Recurring · Next 30 days** lists detected series of money out
   (`recurring::find_with_reader`) that haven't lapsed, on the account filter and in
   the screen's currency, due from today through 30 days on, whatever month is
   selected, with their total unsigned. Recurring income, such as a salary, isn't a
   payment and isn't listed. Two gaps until M4.3 and M4.8:
   there is no price-rise flag (detection doesn't report one), and a series the user
   has turned into a schedule leaves detection, so it isn't listed. A series whose
   expected date passed with no import showing it (the statements lag) rolls forward
   along its cadence to its first date on or after today, but only when it is at most
   30 days overdue: longer, and the account has more likely stopped being imported (an
   old card) than lags, and detection never calls such a series lapsed, since the
   account has no later rows. Each series is one row with its next date, so a weekly
   payment counts once in the 30-day total.
9. **Cost.** Each read reads the window and the month before it, every older line once
   for the earliest month with data, and runs recurring detection over the ledger. It
   runs on a background thread, on each month step and reload. Not yet measured at
   100,000 transactions; if it's slow, the earliest month is the first thing to cache
   or read with a bounded query.
10. **The month it opens on.** Today's month, unless it has no counted line yet; then
    the latest earlier month that has one (`reporting::overview::landing_month`).
    Statements lag: the first run in the app (the Monzo sample fixture, imported on 21
    November, whose rows stop on 13 October) opened on a November of zeros and "−100%
    vs Oct".
    The screen keeps landing so on each read (a reload, the account filter changing)
    until the user picks a month; the switcher still reaches today's month. This
    replaces the spec's "the selected month starts at today's month". It reads the
    12 months to today first, and older lines only when those have none. The month is
    chosen with the display currency (point 3): it has lines in the currency the
    overview for it displays, so a lone EUR payment in November doesn't open a
    November of £0 over a GBP October. The landing month and its overview are read in
    one read transaction (`opening_overview`), as is every overview read, so an import
    committing mid-read can't leave the figures and the empty state disagreeing.
11. **The chart with little data.** The net line starts at the first month with a
    counted line, so months before any import don't read as months of breaking even;
    hovering one still shows its tooltip, without the net point. A selected month with
    no bars (or tiny ones) is still framed a bar's width tall (16px from the zero
    line), so its outline and handles aren't flattened onto the axis. Neither case is
    drawn in Paper; to be confirmed there.
