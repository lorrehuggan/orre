//! Times what the UI thread and every MCP call run, on a large synthetic
//! ledger (ROADMAP L.30; decision record "performance at 100,000
//! transactions" in `docs/architecture/app.md`).
//!
//! ```sh
//! cargo run --release -p orre-core --example ledger_bench -- <dir>
//! cargo run --release -p orre-core --example ledger_bench -- <dir> --only live
//! cargo run --release -p orre-core --features bench --example ledger_bench -- <dir> --only search
//! ```
//!
//! `<dir>` is required and is never the platform data directory: the
//! ledger (`<dir>/orre.db`, its backups and its pending files) is built
//! there, deterministically, through core's public API: statements
//! committed as imports (rules, preset-style categories, pots, transfer
//! pairs and refund links found by the commit's own steps), manual cash
//! rows, rules, suggestions from two clients and access-log rows. A marker
//! file (`ledger_bench.built`) records a finished build, so a second run in
//! the same directory only measures. Delete the directory when done.
//!
//! `--only live` measures just the open and the rows for live updates
//! (`orre_core::watch`: an idle tick, a fingerprint check, a rebase of each
//! topic) and for the Settings AI clients read, skipping the slow rows. The
//! last of those is also timed on a copy of the ledger (made next to it and
//! deleted at the end) whose access log holds about 100,000 rows over the
//! retention window, as a year of heavy MCP use would leave it.
//!
//! `--only search` measures what the full-text index over bank text costs
//! (ROADMAP FT.6; decision record "full-text search over bank text" in
//! `docs/architecture/reporting.md`). It needs `--features bench`, which
//! makes `Ledger::like_only` reachable so `LIKE` can be timed alone. It
//! migrates the ledger, then makes two copies of it with `VACUUM INTO`, as a
//! backup is made, in new subdirectories of `<dir>`, deleted at the end: one
//! as it is, one with the index and its triggers dropped and vacuumed. It
//! prints their sizes (a copy's size is a backup's size), the 1,000-row import
//! on each, V24's migration on a fresh copy without the index (what restoring
//! an older backup costs), and `search_transactions`' query
//! (`reporting::rows::search`, default limit, opened as `orre mcp` opens it)
//! through the index against `LIKE` alone, for texts that match few, some and
//! nearly all rows.
//!
//! Everything is made up: merchants, amounts and descriptions. The
//! generator is a small PRNG with a fixed seed.
//!
//! Each row of the table is a call a UI-thread or per-MCP-call path makes,
//! as the app's store for that view makes it (the path opened through
//! `backup::gate::open` included): the median of several runs, after one
//! warm-up run. The budget for the UI thread is 50 ms.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use chrono::{Datelike, Days, NaiveDate};
use orre_core::access_log::activity::{self, Query};
use orre_core::access_log::{self, NewEntry, Outcome};
use orre_core::account::{self, AccountKind, NewAccount};
use orre_core::backup::gate;
use orre_core::category::{self, CategoryKey, NewCategory, Placement};
use orre_core::db::Database;
use orre_core::id::AccountId;
use orre_core::import::{self, Committed, Confirmations};
use orre_core::inbox::preview::{self, Preview};
use orre_core::inbox::{self, Filter, NewSuggestion, Suggestion, SuggestionKind, SuggestionStatus};
use orre_core::manual::{self, ManualTransaction};
use orre_core::mcp_client::{self, AccessLevel, ClientName, ClientStatus};
use orre_core::money::{Currency, Money};
use orre_core::reporting::account_tree::account_tree;
use orre_core::reporting::budget_status::budget_status;
use orre_core::reporting::net_worth::net_worth_history;
use orre_core::reporting::palette::category_palette;
use orre_core::reporting::period::Period;
use orre_core::reporting::register::Register;
use orre_core::reporting::spending::spending_by_category;
use orre_core::reporting::summary::monthly_summary;
use orre_core::rule::{
    self, AmountBounds, MatchField, MatchKind, Pattern, RuleAction, RuleDefinition,
};
use orre_core::schedule;
use orre_core::setting;
use orre_core::statement::{
    SourceFormat, SourceLocation, Statement, StatementLine, SubAccountKind, SubAccountRef,
};
use orre_core::watch::{Check, Topic, Watcher};
use serde_json::{Value, json};

const MARKER: &str = "ledger_bench.built";
const YEARS: u32 = 5;
const CLIENTS: [&str; 2] = ["claude-code", "codex"];
const SUGGESTIONS_PER_CLIENT: usize = 200;
const ACCESS_LOG_ROWS: usize = 5_000;
/// The access-log rows of the copy that stands for a year of heavy use.
const YEAR_ACCESS_LOG_ROWS: usize = 100_000;
const MERCHANTS: usize = 120;
const RULES: usize = 100;
/// The UI-thread budget.
const BUDGET: Duration = Duration::from_millis(50);

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(dir) = args.next().map(PathBuf::from) else {
        eprintln!(
            "usage: ledger_bench <dir> [--only live|search]   (a scratch directory; never the data directory)"
        );
        return ExitCode::from(2);
    };
    let mode = match (args.next(), args.next(), args.next()) {
        (None, _, _) => Mode::Everything,
        (Some(flag), Some(which), None) if flag == "--only" && which == "live" => Mode::Live,
        (Some(flag), Some(which), None) if flag == "--only" && which == "search" => Mode::Search,
        _ => {
            eprintln!("usage: ledger_bench <dir> [--only live|search]");
            return ExitCode::from(2);
        }
    };
    match run(&dir, mode) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ledger_bench: {error}");
            ExitCode::FAILURE
        }
    }
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Everything,
    Live,
    Search,
}

fn run(dir: &Path, mode: Mode) -> Result<()> {
    if mode == Mode::Search && !cfg!(feature = "bench") {
        return Err("--only search needs `--features bench` (it times `LIKE` alone)".into());
    }
    if let Some(data) = orre_core::paths::database_path()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        && std::fs::canonicalize(dir)
            .is_ok_and(|dir| std::fs::canonicalize(&data).is_ok_and(|data| dir == data))
    {
        return Err("refusing to use the platform data directory".into());
    }
    std::fs::create_dir_all(dir)?;
    let database = dir.join("orre.db");
    if dir.join(MARKER).exists() {
        println!("using the ledger already built in {}", dir.display());
    } else {
        if std::fs::read_dir(dir)?.next().is_some() {
            return Err(
                "the directory isn't empty and holds no finished ledger: use a new one".into(),
            );
        }
        let started = Instant::now();
        build(&database)?;
        std::fs::write(dir.join(MARKER), "built\n")?;
        println!("built the ledger in {:.1?}", started.elapsed());
    }
    match mode {
        Mode::Search => search_cost(dir, &database),
        Mode::Live | Mode::Everything => measure(&database, mode == Mode::Live),
    }
}

// Random numbers ---------------------------------------------------------------

/// splitmix64: small, fixed, good enough for made-up data.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 0..n.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// 0.0..1.0.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

// The made-up world ---------------------------------------------------------------

const PREFIXES: [&str; 30] = [
    "Brightfield",
    "Corvane",
    "Dunmore",
    "Eastlake",
    "Fernhill",
    "Greywell",
    "Hollin",
    "Ivybridge",
    "Juniper",
    "Kestrel",
    "Larkspur",
    "Marlow",
    "Nettle",
    "Oakhurst",
    "Pennant",
    "Quillon",
    "Ravenscar",
    "Saltmarsh",
    "Thornby",
    "Umber",
    "Vantage",
    "Wrenfield",
    "Yarrow",
    "Zephyr",
    "Alder",
    "Birch",
    "Cobalt",
    "Drover",
    "Ember",
    "Fallow",
];
const SUFFIXES: [&str; 4] = ["Stores", "Cafe", "Online", "Express"];

/// Spending categories the made-up merchants fall into, and a typical
/// amount in pence.
const SPENDING: [(&str, i64); 20] = [
    ("food.groceries", 3_200),
    ("food.eating_out", 2_400),
    ("food.takeaway", 1_900),
    ("transport.public", 800),
    ("transport.fuel", 5_500),
    ("transport.taxi", 1_600),
    ("shopping.clothing", 4_500),
    ("shopping.electronics", 9_000),
    ("entertainment.events", 3_500),
    ("entertainment.hobbies", 2_500),
    ("health.medical", 2_000),
    ("health.fitness", 3_000),
    ("bills.subscriptions", 1_100),
    ("bills.phone", 2_500),
    ("home.household", 2_800),
    ("home.maintenance", 7_000),
    ("giving.gifts", 3_000),
    ("family.pets", 2_600),
    ("travel.accommodation", 12_000),
    ("finance.fees", 500),
];

/// The user's own categories, under groups that exist.
const USER_CATEGORIES: [(&str, &str); 4] = [
    ("Coffee", "food"),
    ("Lunch", "food"),
    ("Climbing", "health"),
    ("Allotment", "home"),
];

struct Merchant {
    name: String,
    category: String,
    typical: i64,
}

fn merchants(rng: &mut Rng) -> Vec<Merchant> {
    (0..MERCHANTS)
        .map(|index| {
            let (category, typical) = SPENDING[rng.below(SPENDING.len())];
            Merchant {
                name: format!(
                    "{} {}",
                    PREFIXES[index % PREFIXES.len()],
                    SUFFIXES[index / PREFIXES.len()]
                ),
                category: category.to_owned(),
                typical,
            }
        })
        .collect()
}

/// A merchant, favouring the first few: a few shops take most of the rows.
fn pick(rng: &mut Rng, merchants: &[Merchant]) -> usize {
    let u = rng.unit();
    ((u * u * merchants.len() as f64) as usize).min(merchants.len() - 1)
}

fn spend(rng: &mut Rng, typical: i64) -> i64 {
    (typical as f64 * (0.2 + rng.unit() * 1.8)) as i64 + 1
}

struct Day {
    date: NaiveDate,
    line: StatementLine,
}

fn line(date: NaiveDate, minor: i64, description: String, external_id: Option<String>) -> Day {
    let mut line = StatementLine::new(date, Money::new(minor, Currency::GBP), description);
    line.external_id = external_id;
    Day { date, line }
}

// Building the ledger ----------------------------------------------------------

struct Built {
    current: AccountId,
    joint: AccountId,
    savings: AccountId,
    card: AccountId,
    cash: AccountId,
}

fn build(database: &Path) -> Result<()> {
    let mut rng = Rng(0xC0FF_EE00_D15E_A5E5);
    let mut db = Database::open(database)?;
    let end = schedule::local_today()?
        .checked_sub_days(Days::new(1))
        .unwrap();
    let start = NaiveDate::from_ymd_opt(end.year() - YEARS as i32, end.month(), 1).unwrap();

    for (label, group) in USER_CATEGORIES {
        category::add(
            &mut db,
            &NewCategory {
                label: label.to_owned(),
                placement: Placement::Under(CategoryKey::new(group)?),
            },
        )?;
    }
    let mut world = merchants(&mut rng);
    // Every ninth merchant files under one of the user's own categories.
    let own: Vec<CategoryKey> = {
        let tree = db.categories()?;
        USER_CATEGORIES
            .iter()
            .filter_map(|(label, _)| tree.iter().find(|c| c.label() == *label))
            .map(|c| c.key().clone())
            .collect()
    };
    for (index, merchant) in world.iter_mut().enumerate() {
        if index % 9 == 8 {
            merchant.category = own[(index / 9) % own.len()].as_str().to_owned();
        }
    }

    let accounts = Built {
        current: account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )?,
        joint: account::create(
            &mut db,
            &NewAccount::new("Joint", AccountKind::Current, Currency::GBP),
        )?,
        savings: account::create(
            &mut db,
            &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
        )?,
        card: account::create(
            &mut db,
            &NewAccount::new("Credit card", AccountKind::CreditCard, Currency::GBP),
        )?,
        cash: account::create(
            &mut db,
            &NewAccount::new("Cash", AccountKind::Cash, Currency::GBP),
        )?,
    };

    // About 100 rules, first-listed winning: the first RULES merchants, by
    // name, to their category. The rest stay uncategorised.
    for merchant in world.iter().take(RULES) {
        let definition = RuleDefinition::new(
            MatchField::Description,
            Pattern::new(MatchKind::Contains, &merchant.name)?,
            AmountBounds::new(None, None)?,
            None,
            vec![RuleAction::SetCategory(CategoryKey::new(
                &merchant.category,
            )?)],
        )?;
        rule::create(&mut db, &definition)?;
    }

    // Statements, by account, with the days they cover.
    let days = (end - start).num_days();
    let mut by_account: Vec<(AccountId, Vec<Day>)> = vec![
        (accounts.current, Vec::new()),
        (accounts.joint, Vec::new()),
        (accounts.savings, Vec::new()),
        (accounts.card, Vec::new()),
    ];
    let mut serial = 0u64;
    let mut next_id = |prefix: &str| {
        serial += 1;
        format!("{prefix}-{serial:07}")
    };

    // Shop spending: the card takes the most, then the current account and
    // the joint account. About 97,000 rows in all.
    let spending_rows = [(0usize, 42_000usize), (1, 17_000), (3, 38_000)];
    for (account, rows) in spending_rows {
        let prefix = ["cur", "jnt", "sav", "crd"][account];
        for _ in 0..rows {
            let merchant = &world[pick(&mut rng, &world)];
            let date = start + Days::new(rng.below(days as usize) as u64);
            let minor = -spend(&mut rng, merchant.typical);
            let reference = rng.below(900_000) + 100_000;
            let description = format!("{} {reference} LONDON", merchant.name.to_uppercase());
            // The card's export has no row IDs: its rows are matched by
            // fingerprint, so each has a reference in its text.
            let external = (account != 3).then(|| next_id(prefix));
            let mut day = line(date, minor, description, external);
            day.line.merchant = Some(merchant.name.clone());
            by_account[account].1.push(day);
        }
    }

    // Monthly: pay, rent, a card payment, savings, a pot, interest, and now
    // and then a refund on the card.
    let mut month = start;
    while month <= end {
        let on = |day: u64| month + Days::new(day);
        let current = &mut by_account[0].1;
        current.push(line(
            on(24),
            285_000,
            "ACME LTD SALARY".to_owned(),
            Some(next_id("cur")),
        ));
        current.push(line(
            on(1),
            -95_000,
            "LANDLORD RENT".to_owned(),
            Some(next_id("cur")),
        ));
        let card_payment = 40_000 + rng.below(30_000) as i64;
        current.push(line(
            on(27),
            -card_payment,
            "CREDIT CARD PAYMENT".to_owned(),
            Some(next_id("cur")),
        ));
        let save = 20_000 + rng.below(10_000) as i64 * 5;
        current.push(line(
            on(25),
            -save,
            "TO SAVINGS".to_owned(),
            Some(next_id("cur")),
        ));
        let mut pot = line(
            on(26),
            -5_000,
            "POT TRANSFER".to_owned(),
            Some(next_id("cur")),
        );
        pot.line.sub_account = Some(SubAccountRef {
            kind: SubAccountKind::Pot,
            key: "holiday".to_owned(),
            name: Some("Holiday".to_owned()),
        });
        current.push(pot);
        let mut pot = line(
            on(26),
            -2_500,
            "POT TRANSFER".to_owned(),
            Some(next_id("cur")),
        );
        pot.line.sub_account = Some(SubAccountRef {
            kind: SubAccountKind::Pot,
            key: "car".to_owned(),
            name: Some("Car".to_owned()),
        });
        current.push(pot);
        by_account[2].1.push(line(
            on(25),
            save,
            "FROM CURRENT".to_owned(),
            Some(next_id("sav")),
        ));
        by_account[2].1.push(line(
            on(28),
            1_100 + rng.below(500) as i64,
            "INTEREST".to_owned(),
            Some(next_id("sav")),
        ));
        by_account[3].1.push(line(
            on(27),
            card_payment,
            "PAYMENT THANK YOU".to_owned(),
            None,
        ));
        by_account[1].1.push(line(
            on(2),
            120_000,
            "PARTNER CONTRIBUTION".to_owned(),
            Some(next_id("jnt")),
        ));
        month = month.checked_add_months(chrono::Months::new(1)).unwrap();
    }
    // Refunds: a later credit of the same amount at the same merchant.
    let refunds: Vec<Day> = by_account[3]
        .1
        .iter()
        .step_by(150)
        .map(|day| {
            let mut refund = line(
                day.date + Days::new(5),
                -day.line.amount.minor(),
                format!("REFUND {}", day.line.description_raw),
                None,
            );
            refund.line.merchant = day.line.merchant.clone();
            refund
        })
        .filter(|refund| refund.date <= end)
        .collect();
    by_account[3].1.extend(refunds);

    // Rows the suggestions will name: transfer halves far apart, and a
    // purchase and its refund far apart, with amounts nothing else has.
    for pair in 0..30i64 {
        let out = -(900_000 + pair);
        let date = start + Days::new(200 + pair as u64 * 7);
        by_account[0].1.push(line(
            date,
            out,
            "ODD TRANSFER OUT".to_owned(),
            Some(next_id("cur")),
        ));
        by_account[2].1.push(line(
            date + Days::new(20),
            -out,
            "ODD TRANSFER IN".to_owned(),
            Some(next_id("sav")),
        ));
        by_account[0].1.push(line(
            date,
            -(500_000 + pair),
            "ODD PURCHASE".to_owned(),
            Some(next_id("cur")),
        ));
        by_account[0].1.push(line(
            date + Days::new(40),
            500_000 + pair,
            "ODD REFUND".to_owned(),
            Some(next_id("cur")),
        ));
    }

    // Commit them in half-year steps across the accounts, so a transfer's
    // second half finds its first.
    let mut cursor = start;
    let mut committed = 0usize;
    while cursor <= end {
        let next = cursor.checked_add_months(chrono::Months::new(6)).unwrap();
        for (account, rows) in &mut by_account {
            let mut batch: Vec<&Day> = rows
                .iter()
                .filter(|day| day.date >= cursor && day.date < next)
                .collect();
            if batch.is_empty() {
                continue;
            }
            batch.sort_by_key(|day| day.date);
            let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
            for (index, day) in batch.iter().enumerate() {
                statement.push(SourceLocation::Line(index as u64 + 2), Ok(day.line.clone()));
            }
            let preview = import::preview(&db, *account, "bench.csv", statement)?;
            let confirmed = Confirmations {
                flagged_rows: true,
                balance_mismatch: true,
                corrections: true,
            };
            if let Committed::Batch(summary) = import::commit(&mut db, &preview, confirmed)? {
                committed += summary.transactions;
            }
        }
        cursor = next;
    }

    // Cash, entered by hand.
    for _ in 0..2_500 {
        let date = start + Days::new(rng.below(days as usize) as u64);
        let merchant = &world[pick(&mut rng, &world)];
        manual::create(
            &mut db,
            &ManualTransaction {
                account: accounts.cash,
                date,
                time: None,
                amount: Money::new(-spend(&mut rng, merchant.typical / 3 + 100), Currency::GBP),
                merchant: Some(merchant.name.clone()),
                description: merchant.name.clone(),
                category: None,
            },
        )?;
        committed += 1;
    }

    // Suggested links the user confirmed.
    let suggested = orre_core::link::suggestions(&db)?;
    for link in suggested.iter().take(60) {
        orre_core::link::confirm(&mut db, link.id)?;
    }

    // Two clients, each with 200 pending suggestions.
    for client in CLIENTS {
        mcp_client::enable_at(&mut db, &ClientName::new(client)?, AccessLevel::Standard)?;
    }
    file_suggestions(&mut db, &mut rng, &world)?;

    // The access log: rows the MCP server wrote over the retention window.
    for index in 0..ACCESS_LOG_ROWS {
        access_log::append(
            &mut db,
            &NewEntry {
                client: ClientName::new(CLIENTS[index % 2])?,
                tool: [
                    "list_transactions",
                    "spending_by_category",
                    "monthly_summary",
                ][index % 3]
                    .to_owned(),
                access_level: Some(AccessLevel::Standard),
                outcome: Outcome::Ok,
                purpose: Some("monthly review".to_owned()),
                args_redacted_json: "{\"period\":\"2026-10\"}".to_owned(),
                row_count: (index % 50) as u64,
                result_hash: Some(format!("{:064x}", index)),
                withheld_count: 0,
            },
        )?;
    }

    let total: i64 =
        raw(db.path())?.query_row("SELECT COUNT(*) FROM \"transaction\"", [], |row| row.get(0))?;
    println!(
        "{total} transactions ({committed} written), {} accounts",
        account::list(&db)?.len()
    );
    Ok(())
}

fn file_suggestions(db: &mut Database, rng: &mut Rng, world: &[Merchant]) -> Result<()> {
    let reader = raw(db.path())?;
    let uncategorised: Vec<i64> = {
        let mut statement = reader.prepare(
            "SELECT id FROM \"transaction\" WHERE category_id IS NULL AND transfer_group_id IS NULL \
             AND amount_minor < 0 AND amount_minor > -100000 ORDER BY id",
        )?;
        statement
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    let by_amount = |minor: i64| -> Result<i64> {
        Ok(reader.query_row(
            "SELECT id FROM \"transaction\" WHERE amount_minor = ?1 ORDER BY id LIMIT 1",
            [minor],
            |row| row.get(0),
        )?)
    };
    let categories: Vec<&str> = SPENDING.iter().map(|(key, _)| *key).collect();
    for (number, client) in CLIENTS.iter().enumerate() {
        let name = ClientName::new(client)?;
        let mut filed = 0usize;
        let mut attempt = 0usize;
        while filed < SUGGESTIONS_PER_CLIENT {
            attempt += 1;
            let slot = filed % 20;
            let suggestion: (SuggestionKind, Value) = match slot {
                0..=11 => {
                    let count = 1 + rng.below(8);
                    let mut ids: Vec<i64> = Vec::new();
                    while ids.len() < count {
                        let id = uncategorised[rng.below(uncategorised.len())];
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                    let category = categories[rng.below(categories.len())];
                    let items: Vec<Value> = ids
                        .iter()
                        .map(|id| json!({"transaction_id": id, "category": category}))
                        .collect();
                    (SuggestionKind::Categorization, json!({"items": items}))
                }
                12..=16 => {
                    let merchant = &world[RULES.min(world.len() - 1) - rng.below(5)];
                    let match_kind = ["contains", "starts_with", "is"][rng.below(3)];
                    (
                        SuggestionKind::Rule,
                        json!({
                            "match_field": "merchant",
                            "match_kind": match_kind,
                            "pattern": merchant.name,
                            "amount_min": -(1_000 + (attempt as i64 + number as i64 * 1000) * 7),
                            "actions": [{"kind": "set_category", "value": categories[rng.below(categories.len())]}],
                        }),
                    )
                }
                17 => {
                    let merchant = &world[rng.below(world.len())];
                    (
                        SuggestionKind::MerchantAlias,
                        json!({
                            "match_kind": "exact",
                            "pattern": format!("{} {attempt}", merchant.name.to_uppercase()),
                            "display_name": merchant.name,
                        }),
                    )
                }
                18 => {
                    let pair = (attempt % 30) as i64;
                    (
                        SuggestionKind::TransferPair,
                        json!({
                            "outflow_id": by_amount(-(900_000 + pair))?,
                            "inflow_id": by_amount(900_000 + pair)?,
                        }),
                    )
                }
                _ => {
                    let pair = (attempt % 30) as i64;
                    (
                        SuggestionKind::RefundLink,
                        json!({
                            "refund_id": by_amount(500_000 + pair)?,
                            "purchase_id": by_amount(-(500_000 + pair))?,
                            "link_kind": "refund",
                            "amount_minor": 100 + attempt as i64,
                        }),
                    )
                }
            };
            let (kind, payload) = suggestion;
            match inbox::file_suggestion(
                db,
                &name,
                &NewSuggestion {
                    kind,
                    payload,
                    rationale: Some("Looks like the same shop every time".to_owned()),
                },
            ) {
                Ok(done) if !done.duplicate => filed += 1,
                Ok(_) | Err(inbox::Error::Invalid(_)) => {}
                Err(error) => return Err(error.into()),
            }
            if attempt > SUGGESTIONS_PER_CLIENT * 20 {
                return Err("couldn't file enough distinct suggestions".into());
            }
        }
    }
    Ok(())
}

// Measuring ---------------------------------------------------------------------

struct Row {
    name: String,
    median: Duration,
    max: Duration,
    runs: usize,
    ui_thread: bool,
}

/// Runs `call` once to warm up, then up to `runs` times (fewer when it is
/// slow), and returns the median.
fn time<T>(
    rows: &mut Vec<Row>,
    name: &str,
    ui_thread: bool,
    runs: usize,
    mut call: impl FnMut() -> Result<T>,
) -> Result<T> {
    let started = Instant::now();
    let mut last = call()?;
    let first = started.elapsed();
    // A call this slow is timed by its warm-up run alone.
    let runs = if first > Duration::from_secs(10) {
        0
    } else if first > Duration::from_secs(2) {
        3.min(runs)
    } else {
        runs
    };
    let mut times = Vec::with_capacity(runs.max(1));
    if runs == 0 {
        times.push(first);
    }
    for _ in 0..runs {
        let started = Instant::now();
        last = call()?;
        times.push(started.elapsed());
    }
    times.sort();
    rows.push(Row {
        name: name.to_owned(),
        median: times[times.len() / 2],
        max: *times.last().unwrap(),
        runs: times.len(),
        ui_thread,
    });
    Ok(last)
}

/// A read-only connection of the example's own, for the counts and lookups
/// core's API has no call for.
fn raw(database: &Path) -> Result<rusqlite::Connection> {
    Ok(rusqlite::Connection::open_with_flags(
        database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?)
}

fn open(database: &Path) -> Result<Database> {
    Ok(gate::open(database, Database::open)?)
}

fn measure(database: &Path, only_live: bool) -> Result<()> {
    let mut rows = Vec::new();
    let today = schedule::local_today()?;
    let db = open(database)?;
    let transactions: i64 =
        raw(database)?.query_row("SELECT COUNT(*) FROM \"transaction\"", [], |row| row.get(0))?;
    let rules: i64 = raw(database)?.query_row("SELECT COUNT(*) FROM rule", [], |row| row.get(0))?;
    let pending: i64 = raw(database)?.query_row(
        "SELECT COUNT(*) FROM suggestion WHERE status = 'pending'",
        [],
        |row| row.get(0),
    )?;
    let log: i64 =
        raw(database)?.query_row("SELECT COUNT(*) FROM mcp_access_log", [], |row| row.get(0))?;
    println!(
        "ledger: {transactions} transactions, {rules} rules, {pending} pending suggestions, {log} access-log rows"
    );
    drop(db);

    time(
        &mut rows,
        "open (gate::open + migration check)",
        true,
        9,
        || open(database),
    )?;

    if only_live {
        live_rows(&mut rows, database)?;
        print_table(&rows);
        return Ok(());
    }

    // Accounts view: AccountStore::load.
    time(&mut rows, "  account_tree alone", true, 7, || {
        let db = open(database)?;
        account_tree(&db.reader()?, orre_core::schedule::local_today()?)?;
        Ok(())
    })?;
    time(&mut rows, "Accounts view load", true, 7, || {
        let db = open(database)?;
        let tree = account_tree(&db.reader()?, orre_core::schedule::local_today()?)?;
        for node in tree.rows() {
            let id = node.account.id;
            let _ = import::batches(&db, id)?;
            let _ = account::usage(&db, id)?;
            let _ = account::entered_balances(&db, id)?;
            let _ = account::close::closing(&db, id)?;
            let _ = account::close::deletion(&db, id)?;
        }
        Ok(())
    })?;

    // Inbox: InboxStore::load for the pending list.
    let mut pending_list: Vec<Suggestion> = Vec::new();
    time(
        &mut rows,
        "Inbox: counts + sources + list + describe (no previews)",
        true,
        7,
        || {
            let db = open(database)?;
            inbox::counts(&db)?;
            let _ = mcp_client::list(&db.reader()?)?
                .into_iter()
                .filter(|client| client.status == ClientStatus::Known)
                .count();
            pending_list = inbox::list(
                &db,
                &Filter {
                    status: Some(SuggestionStatus::Pending),
                    kind: None,
                },
            )?;
            for suggestion in &pending_list {
                preview::describe(&db, suggestion)?;
            }
            Ok(())
        },
    )?;
    time(
        &mut rows,
        "Inbox view load (InboxStore::load, pending)",
        true,
        5,
        || {
            let db = open(database)?;
            inbox_load(&db)
        },
    )?;

    // One preview of each kind, to say where the Inbox's time goes.
    {
        let db = open(database)?;
        for kind in [
            SuggestionKind::Categorization,
            SuggestionKind::Rule,
            SuggestionKind::MerchantAlias,
            SuggestionKind::TransferPair,
            SuggestionKind::RefundLink,
        ] {
            let sample: Vec<&Suggestion> = pending_list
                .iter()
                .filter(|s| s.kind == kind)
                .take(6)
                .collect();
            if sample.is_empty() {
                continue;
            }
            let mut at = 0;
            time(
                &mut rows,
                &format!("  one {} preview, alone", kind.as_str()),
                false,
                sample.len() - 1,
                || {
                    let id = sample[at % sample.len()].id;
                    at += 1;
                    let _: Preview = preview::preview(&db, id)?;
                    Ok(())
                },
            )?;
        }
    }

    // Selecting a categorisation looks up the rule that would cover it, on
    // the UI thread.
    {
        let db = open(database)?;
        let categorisation = pending_list
            .iter()
            .find(|suggestion| suggestion.kind == SuggestionKind::Categorization)
            .ok_or("no pending categorisation")?
            .id;
        time(
            &mut rows,
            "Inbox: select a categorisation (covered_by)",
            true,
            7,
            || Ok(preview::covered_by(&db, categorisation)?.is_some()),
        )?;
    }

    // Rules view.
    time(&mut rows, "Rules view: rule::overview", true, 5, || {
        let db = open(database)?;
        Ok(rule::overview(&db)?.len())
    })?;
    time(
        &mut rows,
        "Rules view load (overview + names + history + lists)",
        true,
        5,
        || {
            let db = open(database)?;
            rule::overview(&db)?;
            orre_core::merchant::overview(&db)?;
            orre_core::change::history(&db)?;
            category::overview(&db)?;
            account::list(&db)?;
            orre_core::tag::list(&db)?;
            category_palette(&db.reader()?, today)?;
            Ok(())
        },
    )?;

    // Transactions register (already on a background thread in the app).
    time(
        &mut rows,
        "Transactions register: Register::load",
        false,
        5,
        || {
            let db = open(database)?;
            Ok(Register::load(&db.reader()?, today)?.rows().len())
        },
    )?;

    // Reporting reads (MCP tools and the Overview's figures).
    let month_start = setting::month_start(&open(database)?.reader()?)?;
    let this_month = Period::containing(today, month_start);
    let year_ago = Period::containing(
        today.checked_sub_months(chrono::Months::new(11)).unwrap(),
        month_start,
    );
    time(
        &mut rows,
        "report: monthly_summary, 12 months",
        false,
        5,
        || {
            let db = open(database)?;
            Ok(monthly_summary(&db.reader()?, year_ago, this_month)?
                .items
                .len())
        },
    )?;
    time(
        &mut rows,
        "report: net_worth_history, 12 months",
        false,
        5,
        || {
            let db = open(database)?;
            Ok(net_worth_history(&db.reader()?, year_ago, this_month)?
                .items
                .len())
        },
    )?;
    time(
        &mut rows,
        "report: spending_by_category, 1 month",
        false,
        5,
        || {
            let db = open(database)?;
            spending_by_category(&db.reader()?, this_month)?;
            Ok(())
        },
    )?;
    time(
        &mut rows,
        "report: budget_status, 1 month",
        false,
        5,
        || {
            let db = open(database)?;
            budget_status(&db.reader()?, this_month)?;
            Ok(())
        },
    )?;

    // One MCP call's access-log write, on the server's long-lived connection.
    {
        let mut db = open(database)?;
        let entry = NewEntry {
            client: ClientName::new(CLIENTS[0])?,
            tool: "monthly_summary".to_owned(),
            access_level: Some(AccessLevel::Standard),
            outcome: Outcome::Ok,
            purpose: Some("monthly review".to_owned()),
            args_redacted_json: "{\"period\":\"2026-10\"}".to_owned(),
            row_count: 12,
            result_hash: Some(format!("{:064x}", 7)),
            withheld_count: 0,
        };
        time(
            &mut rows,
            "MCP call: access_log::append (synchronous = FULL)",
            false,
            101,
            || Ok(access_log::append(&mut db, &entry)?),
        )?;
        time(
            &mut rows,
            "MCP call: pending::flush check (nothing waiting)",
            false,
            101,
            || Ok(access_log::pending::flush(&mut db)?),
        )?;
    }

    import_rows(&mut rows, database, today, "")?;

    live_rows(&mut rows, database)?;
    print_table(&rows);
    Ok(())
}

/// An import of 1,000 rows on the 100k ledger (a background thread in the
/// app). Each run is undone, so the ledger is as it was. `suffix` ends each
/// row's name, to tell the ledgers apart.
fn import_rows(rows: &mut Vec<Row>, database: &Path, today: NaiveDate, suffix: &str) -> Result<()> {
    let mut db = open(database)?;
    let current = account::list(&db)?
        .into_iter()
        .find(|account| account.name == "Current")
        .ok_or("no Current account")?
        .id;
    let mut serial = 0u64;
    let mut statement = || {
        serial += 1;
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        for row in 0..1_000u64 {
            let mut line = StatementLine::new(
                today.checked_sub_days(Days::new(row % 25)).unwrap(),
                Money::new(-(150 + (row as i64 * 37) % 4_000), Currency::GBP),
                format!(
                    "{} {} LONDON",
                    PREFIXES[(row % 30) as usize].to_uppercase(),
                    SUFFIXES[(row % 4) as usize].to_uppercase()
                ),
            );
            line.external_id = Some(format!("bench-import-{serial}-{row}"));
            statement.push(SourceLocation::Line(row + 2), Ok(line));
        }
        statement
    };
    let confirmed = Confirmations::default();
    let mut previews = Vec::new();
    time(
        rows,
        &format!("import: preview of a 1,000-row file{suffix}"),
        false,
        5,
        || {
            let preview = import::preview(&db, current, "bench.csv", statement())?;
            previews.push(preview);
            Ok(())
        },
    )?;
    let preview = import::preview(&db, current, "bench.csv", statement())?;
    time(
        rows,
        &format!("import: rehearse (the Review step's After import){suffix}"),
        false,
        3,
        || Ok(import::rehearse(&db, &preview)?.is_some()),
    )?;
    time(
        rows,
        &format!("import: commit of 1,000 rows (incl. backup), then undo{suffix}"),
        false,
        3,
        || {
            let preview = import::preview(&db, current, "bench.csv", statement())?;
            let started = Instant::now();
            let Committed::Batch(summary) = import::commit(&mut db, &preview, confirmed)? else {
                return Err("nothing was new".into());
            };
            let committed = started.elapsed();
            import::undo(&mut db, summary.batch)?;
            Ok(committed)
        },
    )?;
    // The commit alone: `time` measured commit + undo; report the commit.
    let mut commits = Vec::new();
    for _ in 0..3 {
        let preview = import::preview(&db, current, "bench.csv", statement())?;
        let started = Instant::now();
        let Committed::Batch(summary) = import::commit(&mut db, &preview, confirmed)? else {
            return Err("nothing was new".into());
        };
        commits.push(started.elapsed());
        import::undo(&mut db, summary.batch)?;
    }
    commits.sort();
    rows.push(Row {
        name: format!("import: commit alone{suffix}"),
        median: commits[1],
        max: commits[2],
        runs: 3,
        ui_thread: false,
    });
    Ok(())
}

/// An import of 1,000 rows of the most picked merchant, none of them
/// categorised on the way in: the commit's suggestion step
/// (`inbox::similar`) then has 1,000 candidates whose stem matches thousands
/// of categorised rows. It runs on a scratch copy of `database` with its
/// rules deleted (the rules would categorise the rows), and each run is
/// undone and its suggestions cleared, so the runs are alike.
#[cfg(feature = "bench")]
fn import_known_merchant(
    rows: &mut Vec<Row>,
    dir: &Path,
    database: &Path,
    today: NaiveDate,
) -> Result<()> {
    let copy = Scratch::new(dir, "search-known-merchant-")?;
    vacuum_into(database, &copy.database())?;
    let writer = rusqlite::Connection::open(copy.database())?;
    writer.execute_batch(
        "UPDATE rule SET match_kind = 'is', match_field = 'merchant', pattern = 'no merchant is named this';",
    )?;
    let name = format!("{} {}", PREFIXES[0], SUFFIXES[0]);
    let categorised: i64 = writer.query_row(
        "SELECT COUNT(*) FROM \"transaction\" WHERE merchant = ?1 AND category_id IS NOT NULL",
        [&name],
        |row| row.get(0),
    )?;
    println!("\nknown merchant {name:?}: {categorised} categorised rows already in the copy");

    let mut db = open(&copy.database())?;
    let current = account::list(&db)?
        .into_iter()
        .find(|account| account.name == "Current")
        .ok_or("no Current account")?
        .id;
    let mut serial = 0u64;
    let mut commits = Vec::new();
    for _ in 0..3 {
        serial += 1;
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        for row in 0..1_000u64 {
            let mut line = StatementLine::new(
                today.checked_sub_days(Days::new(row % 25)).unwrap(),
                Money::new(-(150 + (row as i64 * 37) % 4_000), Currency::GBP),
                format!("{} KNOWN{row} LONDON", name.to_uppercase()),
            );
            line.external_id = Some(format!("bench-known-{serial}-{row}"));
            line.merchant = Some(name.clone());
            statement.push(SourceLocation::Line(row + 2), Ok(line));
        }
        let preview = import::preview(&db, current, "bench.csv", statement)?;
        let started = Instant::now();
        let Committed::Batch(summary) =
            import::commit(&mut db, &preview, Confirmations::default())?
        else {
            return Err("nothing was new".into());
        };
        commits.push(started.elapsed());
        let filed: i64 = writer.query_row(
            "SELECT COUNT(*) FROM suggestion WHERE kind = 'categorization'",
            [],
            |row| row.get(0),
        )?;
        println!(
            "  run {serial}: {} uncategorised rows, {filed} categorization suggestions filed",
            summary.uncategorized
        );
        import::undo(&mut db, summary.batch)?;
        writer.execute_batch("DELETE FROM suggestion;")?;
    }
    commits.sort();
    rows.push(Row {
        name: "import: commit alone, 1,000 uncategorised rows of a known merchant [with index]"
            .to_owned(),
        median: commits[1],
        max: commits[2],
        runs: 3,
        ui_thread: false,
    });
    Ok(())
}

// The full-text index's cost --------------------------------------------------

/// A ledger's directory (the database, its `-wal` and `-shm`, and the
/// backups an import takes): a new directory in `<dir>`, never one already
/// there, deleted when dropped.
#[cfg(feature = "bench")]
struct Scratch {
    dir: tempfile::TempDir,
}

#[cfg(feature = "bench")]
impl Scratch {
    fn new(parent: &Path, prefix: &str) -> Result<Scratch> {
        let dir = tempfile::Builder::new().prefix(prefix).tempdir_in(parent)?;
        Ok(Scratch { dir })
    }

    fn database(&self) -> PathBuf {
        self.dir.path().join("orre.db")
    }
}

#[cfg(feature = "bench")]
fn mebibytes(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

#[cfg(feature = "bench")]
fn file_size(path: &Path) -> Result<u64> {
    Ok(std::fs::metadata(path)?.len())
}

/// A `VACUUM INTO` copy of `database` at `to`, as a backup is made.
#[cfg(feature = "bench")]
fn vacuum_into(database: &Path, to: &Path) -> Result<()> {
    raw(database)?.execute(
        "VACUUM INTO ?1",
        [to.to_str().ok_or("the path isn't UTF-8")?],
    )?;
    Ok(())
}

/// The bytes the `transaction_text` shadow tables hold, by `dbstat`, when
/// this SQLite has it.
#[cfg(feature = "bench")]
fn index_bytes(database: &Path) -> Option<i64> {
    raw(database)
        .ok()?
        .query_row(
            "SELECT SUM(pgsize) FROM dbstat WHERE name LIKE 'transaction_text%'",
            [],
            |row| row.get(0),
        )
        .ok()
}

/// The V24 migration's SQL, timed by `--only search`.
#[cfg(feature = "bench")]
const V24: &str = include_str!("../migrations/V24__transaction_text.sql");

/// The database and its reader, opened as `orre mcp` opens them for each
/// call (`orre_mcp::server`): through the gate, with `open_existing`, which
/// runs no migrations.
#[cfg(feature = "bench")]
fn open_as_mcp(database: &Path) -> Result<(Database, orre_core::db::Reader)> {
    Ok(gate::open(database, |path| {
        Database::open_existing(path).and_then(|db| db.reader().map(|reader| (db, reader)))
    })?)
}

#[cfg(not(feature = "bench"))]
fn search_cost(_dir: &Path, _database: &Path) -> Result<()> {
    Err("--only search needs `--features bench` (it times `LIKE` alone)".into())
}

/// The texts timed, with what they stand for in the made-up ledger: shop
/// rows read `<MERCHANT NAME> <reference> LONDON`, merchants are picked
/// favouring the first few (`pick`), and a few fixed rows carry fixed text.
#[cfg(feature = "bench")]
const SEARCH_TEXTS: [(&str, &str); 4] = [
    ("odd refund", "a handful of rows"),
    ("Fallow Express", "the least picked merchant"),
    ("Brightfield Stores", "the most picked merchant"),
    ("LONDON", "every shop row"),
];

#[cfg(feature = "bench")]
fn search_cost(dir: &Path, database: &Path) -> Result<()> {
    use orre_core::reporting::rows::{self, Search};
    use orre_core::reporting::{Ledger, Limit};

    let today = schedule::local_today()?;
    let with = Scratch::new(dir, "search-with-index-")?;
    let without = Scratch::new(dir, "search-without-index-")?;
    // A ledger saved before V24 has no index to drop: migrate it first.
    drop(open(database)?);
    vacuum_into(database, &with.database())?;
    vacuum_into(database, &without.database())?;
    {
        let conn = rusqlite::Connection::open(without.database())?;
        conn.execute_batch(
            "DROP TRIGGER transaction_text_insert; \
             DROP TRIGGER transaction_text_delete; \
             DROP TRIGGER transaction_text_update; \
             DROP TABLE transaction_text; \
             VACUUM;",
        )?;
    }
    // Nothing has the copies open, so their files are what a backup is.
    let sizes = (
        file_size(&with.database())?,
        file_size(&without.database())?,
    );

    // The live file, after a checkpoint, for reference.
    {
        // Read-write: a read-only connection can't checkpoint a WAL with frames in it.
        let conn = rusqlite::Connection::open(database)?;
        let _: (i64, i64, i64) = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    }
    let live = file_size(database)?;
    let index = sizes.0.saturating_sub(sizes.1);
    println!();
    println!("size (VACUUM INTO copies: a copy's size is a backup's size)");
    println!(
        "  live ledger, after a WAL checkpoint   {:>12}",
        mebibytes(live)
    );
    println!(
        "  copy with the index                   {:>12}",
        mebibytes(sizes.0)
    );
    println!(
        "  copy without the index                {:>12}",
        mebibytes(sizes.1)
    );
    println!(
        "  the index (the difference)            {:>12}  {:.1}% of the copy with it, {:.1}% over the copy without it",
        mebibytes(index),
        index as f64 * 100.0 / sizes.0 as f64,
        index as f64 * 100.0 / sizes.1 as f64
    );
    match index_bytes(&with.database()) {
        Some(bytes) => println!(
            "  transaction_text* pages (dbstat)      {:>12}",
            mebibytes(bytes.max(0) as u64)
        ),
        None => println!("  transaction_text* pages (dbstat)      not available"),
    }

    let mut rows = Vec::new();

    // The import on each copy, both opened the same way.
    let transactions: i64 =
        raw(&with.database())?
            .query_row("SELECT COUNT(*) FROM \"transaction\"", [], |row| row.get(0))?;
    println!("\ncopies: {transactions} transactions each");
    import_rows(&mut rows, &with.database(), today, " [with index]")?;
    import_rows(&mut rows, &without.database(), today, " [without index]")?;
    import_known_merchant(&mut rows, dir, &with.database(), today)?;

    // What the V24 migration costs when it runs on an older backup: its SQL
    // on a fresh copy of the ledger without the index each run, so it
    // creates the index and fills it for the first time, as V24 does.
    let migrated = Scratch::new(dir, "search-v24-")?;
    let mut runs = Vec::new();
    for _ in 0..4 {
        let _ = std::fs::remove_file(migrated.database());
        vacuum_into(&without.database(), &migrated.database())?;
        let mut conn = rusqlite::Connection::open(migrated.database())?;
        let started = Instant::now();
        let tx = conn.transaction()?;
        tx.execute_batch(V24)?;
        tx.commit()?;
        runs.push(started.elapsed());
        drop(conn);
    }
    // The first run is the warm-up.
    runs.remove(0);
    runs.sort();
    rows.push(Row {
        name: "V24's migration: create and fill the index (restoring an older backup)".to_owned(),
        median: runs[1],
        max: runs[2],
        runs: 3,
        ui_thread: false,
    });
    drop(migrated);

    // The text query: `search_transactions`' call, through the index, and
    // the same ledger on `LIKE` alone, on the ledger itself. Each timed run
    // opens the database as an MCP call does (`open_as_mcp`), with no other
    // connection open.
    let limit = Limit::new(None);
    println!();
    println!(
        "search_transactions' text query, {} rows a page",
        limit.get()
    );
    for (text, stands_for) in SEARCH_TEXTS {
        let search = Search {
            text: Some(text.to_owned()),
            ..Search::default()
        };
        let base = || Ledger::new().newest_first().with_bank_text().matching(text);
        // The checks' connection is dropped before the timed runs.
        let db = open(database)?;
        let reader = db.reader()?;
        let every = base().lines(&reader)?;
        let every_like = base().like_only().lines(&reader)?;
        let total = every.len();
        if every != every_like {
            return Err(format!(
                "{text:?}: the index path found {total} lines and LIKE alone {}, or other lines",
                every_like.len()
            )
            .into());
        }
        let through_index = rows::search(&reader, &search, limit)?;
        let through_like = base().like_only().lines_capped(&reader, limit)?;
        if through_index != through_like {
            return Err(format!("{text:?}: the first page differs between the two paths").into());
        }
        drop(reader);
        drop(db);
        println!(
            "  {text:?} ({stands_for}): {total} ledger lines, {:.2}% of {transactions} transactions",
            total as f64 * 100.0 / transactions as f64
        );
        time(
            &mut rows,
            &format!("search {text:?} ({total} lines): index (production path)"),
            false,
            9,
            || {
                let (_db, reader) = open_as_mcp(database)?;
                Ok(rows::search(&reader, &search, limit)?.len())
            },
        )?;
        time(
            &mut rows,
            &format!("search {text:?} ({total} lines): LIKE alone"),
            false,
            9,
            || {
                let (_db, reader) = open_as_mcp(database)?;
                Ok(base().like_only().lines_capped(&reader, limit)?.len())
            },
        )?;
    }
    print_table(&rows);
    Ok(())
}

/// The rows for live updates and for the Settings AI clients read.
fn live_rows(rows: &mut Vec<Row>, database: &Path) -> Result<()> {
    // The app's watcher ticks off the UI thread (`orre_core::watch`).
    let mut idle = Watcher::new(database);
    idle.check()?;
    // The baseline's own open can leave the files changed (the last
    // connection closing checkpoints): the next check compares again, and
    // the one after is the idle tick.
    for _ in 0..5 {
        if idle.check()? == Check::Unchanged {
            break;
        }
    }
    time(
        rows,
        "Live updates: idle tick (stamp unchanged)",
        false,
        2_000,
        || match idle.check()? {
            Check::Unchanged => Ok(()),
            other => Err(format!("the idle tick found {other:?}").into()),
        },
    )?;

    time(
        rows,
        "Live updates: fingerprint check, every topic",
        false,
        9,
        || {
            // A new watcher's first check is the baseline: stamp, one open
            // through the gate, every topic's queries, stamp again. That is
            // a check after a changed stamp.
            let check = Watcher::new(database).check()?;
            if check != Check::Changed(Default::default()) {
                return Err(format!("the baseline reported {check:?}").into());
            }
            Ok(())
        },
    )?;

    let mut rebased = Watcher::new(database);
    rebased.check()?;
    for topic in Topic::ALL {
        time(
            rows,
            &format!("Live updates: rebase of {topic:?} (includes the open)"),
            false,
            9,
            || Ok(rebased.rebase(&[topic])?),
        )?;
    }

    time(rows, "Settings: AI clients read (load_ai)", true, 7, || {
        load_ai(database)
    })?;

    let copy = YearCopy::make(database)?;
    time(
        rows,
        &format!(
            "Settings: AI clients read, access log of {YEAR_ACCESS_LOG_ROWS} rows over a year"
        ),
        true,
        5,
        || load_ai(&copy.path),
    )?;
    Ok(())
}

/// The tools that write to the Inbox, copied from the MCP server's registry
/// (`orre_mcp::tools::registry`, where `writes` is not `Writes::Nothing`):
/// core doesn't know them and an example can't depend on the mcp crate. Keep
/// in step with the registry.
fn writes(tool: &str) -> bool {
    [
        "propose_categorizations",
        "propose_rule",
        "propose_transfer_pair",
        "propose_merchant_alias",
        "propose_refund_link",
        "save_report",
    ]
    .contains(&tool)
}

/// What the app's `SettingsStore::load_ai` reads, from core's calls
/// (`crates/app/src/settings/store.rs`): it mirrors that function and must
/// be kept in step with it.
fn load_ai(database: &Path) -> Result<()> {
    let mut db = open(database)?;
    access_log::pending::flush(&mut db)?;
    let reader = db.reader()?;
    let week = activity::timestamp(orre_core::clock::now()? - chrono::Duration::days(7));
    let _ = activity::activity(&reader, Some(&week), writes)?;
    let _ = activity::activity(&reader, None, writes)?;
    let _ = activity::written(&reader, &week)?;
    let _ = activity::count(
        &reader,
        &Query {
            since: Some(week),
            ..Query::default()
        },
    )?;
    let _ = mcp_client::list(&reader)?;
    let _ = setting::mcp_enabled(&reader)?;
    Ok(())
}

/// A copy of the built ledger beside it whose access log holds about
/// [`YEAR_ACCESS_LOG_ROWS`] rows, spread over the retention window, deleted
/// when dropped.
struct YearCopy {
    path: PathBuf,
}

impl YearCopy {
    fn make(database: &Path) -> Result<YearCopy> {
        let path = database.with_file_name("orre-year.db");
        let copy = YearCopy { path };
        copy.remove();
        // `VACUUM INTO` writes a consistent copy of the database file alone,
        // whatever the original's `-wal` holds, with no write to the original.
        raw(database)?.execute(
            "VACUUM INTO ?1",
            [copy.path.to_str().ok_or("the path isn't UTF-8")?],
        )?;

        // The rows go in with a raw statement: `access_log::append` stamps
        // each row with the current time, and the rows have to be spread
        // over the year.
        let mut conn = rusqlite::Connection::open(&copy.path)?;
        let have: i64 =
            conn.query_row("SELECT COUNT(*) FROM mcp_access_log", [], |row| row.get(0))?;
        let now = orre_core::clock::now()?;
        let span = i64::from(access_log::RETENTION_MONTHS) * 30 * 24 * 60 * 60 - 24 * 60 * 60;
        let transaction = conn.transaction()?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO mcp_access_log (at, client, tool, access_level, outcome, purpose, \
                 args_redacted_json, row_count, result_hash, withheld_count) \
                 VALUES (?1, ?2, ?3, 'standard', 'ok', 'monthly review', ?4, ?5, ?6, 0)",
            )?;
            let tools = [
                "list_transactions",
                "spending_by_category",
                "monthly_summary",
                "propose_categorizations",
            ];
            for index in 0..YEAR_ACCESS_LOG_ROWS.saturating_sub(have as usize) {
                let seconds_ago = span - span * index as i64 / YEAR_ACCESS_LOG_ROWS as i64;
                let at = (now - chrono::Duration::seconds(seconds_ago))
                    .format("%Y-%m-%dT%H:%M:%S.000Z")
                    .to_string();
                insert.execute(rusqlite::params![
                    at,
                    CLIENTS[index % 2],
                    tools[index % tools.len()],
                    "{\"period\":\"2026-10\"}",
                    (index % 50) as i64,
                    format!("{index:064x}"),
                ])?;
            }
        }
        transaction.commit()?;
        Ok(copy)
    }

    /// The copy and its `-wal` and `-shm`, where they exist.
    fn remove(&self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut name = self.path.clone().into_os_string();
            name.push(suffix);
            let _ = std::fs::remove_file(name);
        }
    }
}

impl Drop for YearCopy {
    fn drop(&mut self) {
        self.remove();
    }
}

/// `InboxStore::load` for pending suggestions, on an open database (the
/// app's `inbox::store::load`).
fn inbox_load(db: &Database) -> Result<()> {
    let counts = inbox::counts(db)?;
    let _ = counts.by_source.len();
    let _ = mcp_client::list(&db.reader()?)?;
    let suggestions = inbox::list(
        db,
        &Filter {
            status: Some(SuggestionStatus::Pending),
            kind: None,
        },
    )?;
    let ids: Vec<_> = suggestions.iter().map(|suggestion| suggestion.id).collect();
    for suggestion in &suggestions {
        preview::describe(db, suggestion)?;
    }
    let _ = preview::preview_all(db, &ids);
    category_palette(&db.reader()?, schedule::local_today()?)?;
    Ok(())
}

fn print_table(rows: &[Row]) {
    let width = rows.iter().map(|row| row.name.len()).max().unwrap_or(0);
    println!();
    println!(
        "{:<width$}  {:>11}  {:>11}  {:>4}  budget",
        "call", "median", "max", "runs"
    );
    for row in rows {
        let verdict = match (row.ui_thread, row.median > BUDGET) {
            (true, true) => "OVER (UI thread)",
            (true, false) => "ok (UI thread)",
            (false, true) => "over 50 ms (already off the UI thread)",
            (false, false) => "ok",
        };
        println!(
            "{:<width$}  {:>11}  {:>11}  {:>4}  {verdict}",
            row.name,
            format!("{:.2?}", row.median),
            format!("{:.2?}", row.max),
            row.runs
        );
    }
}
