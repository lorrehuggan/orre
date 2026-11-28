//! What the Import dialog says (M3.4): captions, the preview's notes, the
//! Review step's figures and reconciliation block. Pure, so it's tested
//! without a window.
//!
//! Copy follows Paper (`5ZQ-0`, `6J4-0`) where drawn; the composed states'
//! wording is recorded in `docs/DESIGN.md` → "Import" → "Built in M3.4".
//! Text from the file (a merchant, a flagged value) is the user's own data,
//! shown in their window and never logged.

use chrono::NaiveDate;
use orre_core::balance::CheckResult;
use orre_core::import::{
    BalanceCheck, CheckedBalance, Flag, ImportPreview, ReconciliationStatus, RowOutcome,
};
use orre_core::money::{DecimalMark, Money, ParseAmountError};
use orre_core::statement::{RowProblem, SourceLocation, StatementLine};
use orre_import::csv::{Delimiter, TextEncoding};

use crate::format::{Sign, list_date, money};

use super::session::Chosen;

/// A piece of a sentence: plain, or in Geist Mono (a date, an amount, a
/// count, the file's own text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seg {
    pub text: String,
    pub mono: bool,
}

fn plain(text: impl Into<String>) -> Seg {
    Seg {
        text: text.into(),
        mono: false,
    }
}

fn mono(text: impl Into<String>) -> Seg {
    Seg {
        text: text.into(),
        mono: true,
    }
}

/// The segments as one string, for tests and accessible names.
pub fn joined(segs: &[Seg]) -> String {
    segs.iter().map(|seg| seg.text.as_str()).collect()
}

/// `noun`, plural when `n` isn't one.
fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}

/// A count as the app writes it: `1,906`.
pub fn count(n: usize) -> String {
    crate::settings::table::count(n as u64)
}

/// A date or time format as people write it: `%d/%m/%Y` → `DD/MM/YYYY`,
/// `%H:%M:%S` → `HH:mm:ss`.
pub fn pattern_label(pattern: &str) -> String {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let unpadded = chars.peek() == Some(&'-');
        if unpadded {
            chars.next();
        }
        let token = match (chars.next(), unpadded) {
            (Some('d'), false) => "DD",
            (Some('d'), true) => "D",
            (Some('m'), false) => "MM",
            (Some('m'), true) => "M",
            (Some('Y'), _) => "YYYY",
            (Some('y'), _) => "YY",
            (Some('b'), _) => "MMM",
            (Some('B'), _) => "MMMM",
            (Some('H'), false) => "HH",
            (Some('H'), true) => "H",
            (Some('I'), _) => "hh",
            (Some('M'), _) => "mm",
            (Some('S'), _) => "ss",
            (Some('p'), _) => "AM",
            (Some('%'), _) => "%",
            (Some(other), _) => {
                out.push('%');
                out.push(other);
                continue;
            }
            (None, _) => "%",
        };
        out.push_str(token);
    }
    out
}

/// `comma-separated`.
pub fn delimiter_name(delimiter: Delimiter) -> &'static str {
    match delimiter {
        Delimiter::Comma => "comma-separated",
        Delimiter::Semicolon => "semicolon-separated",
        Delimiter::Tab => "tab-separated",
        Delimiter::Pipe => "pipe-separated",
    }
}

/// The Map step's subtitle: `CSV · UTF-8 · comma-separated · 312 rows`.
pub fn csv_caption(encoding: TextEncoding, delimiter: Delimiter, rows: usize) -> Vec<Seg> {
    vec![
        plain(format!(
            "CSV · {} · {} · ",
            encoding.name(),
            delimiter_name(delimiter)
        )),
        mono(count(rows)),
        plain(format!(" {}", plural(rows, "row"))),
    ]
}

/// The Map step's subtitle for OFX: `OFX · 2 statements`.
pub fn ofx_caption(statements: usize) -> Vec<Seg> {
    vec![
        plain("OFX · "),
        mono(count(statements)),
        plain(format!(" {}", plural(statements, "statement"))),
    ]
}

/// The caption under the Account select: how the mapping was chosen.
pub fn mapping_caption(chosen: &Chosen, preset: Option<&str>, unchanged: bool) -> String {
    match (chosen, preset) {
        (_, Some(name)) if !unchanged => {
            format!("Changed from {name}. Save it as a preset to use it again.")
        }
        (Chosen::Header, _) => {
            "Detected from the header row. Change any column below; the preview updates as you go."
                .to_owned()
        }
        (Chosen::Tie, _) => "More than one preset has this header row: check this is the right \
                             one. The preview updates as you go."
            .to_owned(),
        (Chosen::Account, _) => "The account's preset. Change any column below; the preview \
                                 updates as you go."
            .to_owned(),
        (Chosen::User, _) => "Change any column below; the preview updates as you go.".to_owned(),
        (Chosen::Nothing, _) => "No preset recognises this file. Choose its columns below; the \
                                 preview updates as you go."
            .to_owned(),
    }
}

/// What a preview row shows in its Merchant column, and whether it's a
/// transfer to or from a pot (muted, with the transfer icon): `To Rainy
/// day`, `From Holiday`.
pub fn row_text(line: &StatementLine) -> (String, bool) {
    if let Some(pot) = &line.sub_account {
        let name = pot.name.as_deref().unwrap_or(&pot.key);
        let direction = if line.amount.is_negative() {
            "To"
        } else {
            "From"
        };
        return (format!("{direction} {name}"), true);
    }
    let text = line
        .merchant
        .clone()
        .filter(|merchant| !merchant.trim().is_empty())
        .unwrap_or_else(|| line.description_raw.clone());
    (text.trim().to_owned(), false)
}

/// A row's amount: `−£6.45`, `+£34.00`.
pub fn row_amount(line: &StatementLine) -> String {
    money(line.amount, Sign::Always)
}

/// A row's date in the preview: `9 Nov`.
pub fn row_date(line: &StatementLine, today: NaiveDate) -> String {
    list_date(line.date, today)
}

/// The note under the preview for a flagged row: `Row 8: “8,99” isn't an
/// amount with a decimal point. It will not be imported; it is recorded
/// with the import.`
pub fn flag_note(
    number: usize,
    problem: &RowProblem,
    text: Option<&str>,
    decimal: DecimalMark,
    more: usize,
) -> Vec<Seg> {
    let mut segs = vec![plain("Row "), mono(count(number)), plain(": ")];
    match (problem, text) {
        (
            RowProblem::Amount {
                error: error @ ParseAmountError::OutOfRange { .. },
                ..
            },
            Some(text),
        ) => {
            segs.push(mono(format!("“{text}”")));
            segs.push(plain(format!(" is too large: {error}.")));
        }
        (RowProblem::Amount { .. }, Some(text)) => {
            segs.push(mono(format!("“{text}”")));
            segs.push(plain(format!(
                " isn’t an amount with a decimal {}.",
                match decimal {
                    DecimalMark::Point => "point",
                    DecimalMark::Comma => "comma",
                }
            )));
        }
        (RowProblem::Unreadable { field, .. } | RowProblem::Missing(field), Some(text)) => {
            segs.push(mono(format!("“{text}”")));
            segs.push(plain(format!(" isn’t a {} Orre can read.", field.as_str())));
        }
        _ => segs.push(plain(super::session::sentence(&problem.to_string()))),
    }
    segs.push(plain(
        " It will not be imported; it is recorded with the import.",
    ));
    if more > 0 {
        segs.push(plain(" "));
        segs.push(mono(count(more)));
        segs.push(plain(format!(
            " more {} flagged too.",
            if more == 1 { "row is" } else { "rows are" }
        )));
    }
    segs
}

/// `Pot transfers are paired with your Holiday and Rainy day pots
/// automatically.`
pub fn pots_note(names: &[String]) -> Option<String> {
    let list = match names {
        [] => return None,
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    Some(format!(
        "Pot transfers are paired with your {list} {} automatically.",
        if names.len() == 1 { "pot" } else { "pots" }
    ))
}

/// The Review step's subtitle: `Monzo (current account) → Current account ·
/// 1 Nov – 9 Nov 2026`.
pub fn review_caption(
    read_with: &str,
    account: &str,
    span: Option<(NaiveDate, NaiveDate)>,
) -> Vec<Seg> {
    let mut segs = vec![plain(format!("{read_with} → {account}"))];
    if let Some((first, last)) = span {
        segs.push(plain(" · "));
        let first = if first.format("%Y").to_string() == last.format("%Y").to_string() {
            first.format("%-d %b").to_string()
        } else {
            first.format("%-d %b %Y").to_string()
        };
        segs.push(mono(format!("{first} – {}", last.format("%-d %b %Y"))));
    }
    segs
}

/// The first and last dates among the statement's rows.
pub fn span(preview: &ImportPreview) -> Option<(NaiveDate, NaiveDate)> {
    let dates = preview
        .rows()
        .filter_map(|row| row.line.map(|line| line.date));
    dates.fold(None, |span, date| match span {
        None => Some((date, date)),
        Some((first, last)) => Some((first.min(date), last.max(date))),
    })
}

/// Beside the Review figures: `16 rows match transactions you already
/// have, so they are skipped.`
pub fn already_caption(already: usize) -> Option<String> {
    (already > 0).then(|| {
        format!(
            "{} {} transactions you already have, so {} skipped.",
            count(already),
            if already == 1 {
                "row matches"
            } else {
                "rows match"
            },
            if already == 1 { "it is" } else { "they are" }
        )
    })
}

/// The bank's name for "Categorised from Monzo's categories": a preset's
/// name without its parenthesis.
pub fn bank_name(preset: &str) -> &str {
    preset.split(" (").next().unwrap_or(preset).trim()
}

/// How a Review block looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Hairline border, the info icon: nothing to check.
    Neutral,
    /// Hairline border, the check: reconciled.
    Reconciled,
    /// `danger-soft`: a mismatch.
    Mismatch,
    /// `warning-soft`, the lock: rows in a locked period.
    Locked,
    /// `warning-soft`, the warning triangle: a file for another account.
    OtherAccount,
}

/// One block on the Review step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub tone: Tone,
    pub title: String,
    pub body: Vec<Seg>,
}

fn day(date: NaiveDate) -> Seg {
    mono(date.format("%-d %b").to_string())
}

fn amount(money_value: Money) -> Seg {
    mono(money(money_value, crate::format::Sign::Negative))
}

/// The reconciliation block (Paper `6N3-0`, `6NI-0`, `6NP-0`).
/// `exporter` names whose files have no balances ("Monzo exports").
pub fn reconciliation(preview: &ImportPreview, exporter: &str) -> Block {
    let account = &preview.account().name;
    let reconciliation = preview.reconciliation();
    match reconciliation.status() {
        ReconciliationStatus::NoBalances => Block {
            tone: Tone::Neutral,
            title: "Not reconciled".to_owned(),
            body: vec![plain(format!(
                "{exporter} carry no balances, so the totals can’t be checked against the bank. \
                 Enter today’s balance for {account} after importing to anchor it."
            ))],
        },
        ReconciliationStatus::StartingPoint => {
            let first = preview.balances().first();
            let mut body = vec![plain("The expected balance")];
            if let Some(balance) = first {
                body.extend([
                    plain(" on "),
                    day(balance.date),
                    plain(" is "),
                    amount(balance.balance),
                    plain(","),
                ]);
            }
            body.push(plain(format!(
                " the first {account} has, so there’s nothing to check it against yet. Later \
                 imports are checked from it."
            )));
            Block {
                tone: Tone::Neutral,
                title: "Starting point".to_owned(),
                body,
            }
        }
        ReconciliationStatus::Reconciled => {
            let latest = reconciliation
                .checks
                .iter()
                .filter(|check| matches!(check.balance, CheckedBalance::FromFile(_)))
                .max_by_key(|check| check.date)
                .or_else(|| reconciliation.checks.iter().max_by_key(|check| check.date));
            let body = match latest {
                Some(check) => vec![
                    plain("The expected balance on "),
                    day(check.date),
                    plain(" is "),
                    amount(check.stated),
                    plain(", and the imported rows add up to it exactly."),
                ],
                None => match &reconciliation.running {
                    Some(running) => vec![
                        plain("Every running balance in the file follows from the rows: "),
                        mono(count(running.checked)),
                        plain(format!(" {}.", plural(running.checked, "check"))),
                    ],
                    None => vec![plain("Every balance check passed.")],
                },
            };
            Block {
                tone: Tone::Reconciled,
                title: "Reconciled".to_owned(),
                body,
            }
        }
        ReconciliationStatus::Mismatch => Block {
            tone: Tone::Mismatch,
            title: "Balances don’t match".to_owned(),
            body: mismatch_body(preview),
        },
    }
}

/// The mismatch block's sentence: the first failing check, or the first
/// running balance that doesn't follow.
fn mismatch_body(preview: &ImportPreview) -> Vec<Seg> {
    let failed: Option<&BalanceCheck> = preview
        .reconciliation()
        .checks
        .iter()
        .filter(|check| check.after.is_failed())
        .min_by_key(|check| check.date);
    let ending = " Importing keeps the mismatch flagged on the account until it’s resolved.";
    if let Some(check) = failed
        && let CheckResult::Failed { since, .. } = check.after
        && let Some(computed) = check.computed()
    {
        let gap = Money::new(
            (computed.minor() - check.stated.minor()).abs(),
            check.stated.currency(),
        );
        let mut body = vec![
            plain("It should be "),
            amount(check.stated),
            plain(" on "),
            day(check.date),
            plain("; the imported rows give "),
            amount(computed),
            plain(", "),
            amount(gap),
            plain(if computed.minor() < check.stated.minor() {
                " short."
            } else {
                " over."
            }),
        ];
        if since < check.date {
            body.extend([
                plain(" The gap is between "),
                day(since),
                plain(", the last balance that matched, and "),
                day(check.date),
                plain("."),
            ]);
        }
        body.push(plain(ending));
        return body;
    }
    if let Some(running) = &preview.reconciliation().running
        && let Some(first) = running.mismatches.first()
    {
        let number = preview
            .rows()
            .position(|row| row.location == first.location)
            .map_or(0, |index| index + 1);
        return vec![
            plain("Row "),
            mono(count(number)),
            plain(" says the balance is "),
            amount(first.stated),
            plain("; the rows before it give "),
            amount(first.computed),
            plain(format!(
                ".{}",
                if running.mismatches.len() > 1 {
                    format!(
                        " {} more running balances don’t follow either.",
                        running.mismatches.len() - 1
                    )
                } else {
                    String::new()
                }
            )),
            plain(ending),
        ];
    }
    vec![plain(
        "A balance doesn’t agree with the transactions.".to_owned() + ending,
    )]
}

/// The locked-period block (Paper `6NX-0`), when rows fall on or before
/// the account's lock date. Unlocking is in Reviews (M3.12).
pub fn locked(preview: &ImportPreview) -> Option<Block> {
    let mut through = None;
    let mut rows = 0;
    for row in preview.rows() {
        if let RowOutcome::Flagged(Flag::Locked { through: date }) = row.outcome {
            rows += 1;
            through = Some(*date);
        }
    }
    let through = through?;
    let others = preview.counts().new;
    Some(Block {
        tone: Tone::Locked,
        title: format!("{} {} in a locked period", count(rows), {
            if rows == 1 { "row falls" } else { "rows fall" }
        }),
        body: vec![
            plain(format!("{} is reconciled through ", preview.account().name)),
            day(through),
            plain(format!(
                ". {} out unless you unlock it in Reviews; the other ",
                if rows == 1 {
                    "This row stays"
                } else {
                    "These rows stay"
                }
            )),
            mono(count(others)),
            plain(" import as usual."),
        ],
    })
}

/// The other-account block (R.13), when the file states an account number
/// the account has never had imported into it, though it has had others or
/// another account has had this one: a warning, not a gate. Names the account the number belongs to when
/// another has it. Never shows the number, or any part of it.
pub fn other_account(preview: &ImportPreview) -> Option<Block> {
    let mismatch = preview.source_account_mismatch()?;
    let account = &preview.account().name;
    Some(Block {
        tone: Tone::OtherAccount,
        title: match &mismatch.other_account {
            Some(other) => format!("This file looks like it’s from {}", other.name),
            None => "This file looks like it’s from a different account".to_owned(),
        },
        body: vec![plain(match &mismatch.other_account {
            Some(other) if !mismatch.account_has_others => format!(
                "Files with this account number have gone into {}. If this is the right \
                 account, import it anyway: {account} will recognise this file’s account \
                 number from then on.",
                other.name
            ),
            _ => format!(
                "{account} has only had files with other account numbers. If this is the \
                 right account, import it anyway: {account} will recognise this file’s \
                 account number from then on."
            ),
        })],
    })
}

/// The corrections block (R.4), when the file has the bank's new version
/// of rows the account holds: how many, and what importing does to them. A
/// neutral block with words only (the figures row has no room for a fourth
/// figure without a design); the Import button names them and accepts
/// them, as it does a mismatch.
pub fn corrections(preview: &ImportPreview) -> Option<Block> {
    let corrections: Vec<_> = preview.corrections().collect();
    let rows = corrections.len();
    if rows == 0 {
        return None;
    }
    let kept_back = corrections
        .iter()
        .filter(|(_, correction)| !correction.not_carried.is_empty())
        .count();
    let mut body = vec![plain(format!(
        "The bank sent a new date or amount for {} you already have. Importing replaces {}: \
         {} under the Excluded filter as replaced, and what you set (category, notes, tags) \
         moves to the new {} where it fits.",
        if rows == 1 {
            "a transaction"
        } else {
            "transactions"
        },
        if rows == 1 { "it" } else { "each" },
        if rows == 1 {
            "the old one stays"
        } else {
            "the old ones stay"
        },
        if rows == 1 { "one" } else { "ones" },
    ))];
    if kept_back > 0 {
        body.extend([
            plain(" "),
            mono(count(kept_back)),
            plain(format!(
                " {} a split or share that doesn’t fit the new amount; it stays on the old {}.",
                if kept_back == 1 { "has" } else { "have" },
                if kept_back == 1 { "one" } else { "ones" },
            )),
        ]);
    }
    Some(Block {
        tone: Tone::Neutral,
        title: if rows == 1 {
            "1 row is the bank’s correction".to_owned()
        } else {
            format!("{} rows are the bank’s corrections", count(rows))
        },
        body,
    })
}

/// The Review step's primary button.
pub fn primary_label(preview: &ImportPreview) -> String {
    if preview.is_nothing_new() {
        return "Nothing new to import".to_owned();
    }
    if preview.needs_confirmation().balance_mismatch {
        return "Import and flag the mismatch".to_owned();
    }
    let counts = preview.counts();
    let corrections = (counts.corrections > 0).then(|| {
        format!(
            "{} {}",
            count(counts.corrections),
            plural(counts.corrections, "correction")
        )
    });
    match (counts.new, corrections) {
        (0, None) => "Import the balance".to_owned(),
        (0, Some(corrections)) => format!("Import {corrections}"),
        (n, None) => format!("Import {} {}", count(n), plural(n, "transaction")),
        (n, Some(corrections)) => format!(
            "Import {} {} and {corrections}",
            count(n),
            plural(n, "transaction")
        ),
    }
}

/// The notification after an import: `Imported 296 transactions from
/// monzo-november.csv`, then the rows that weren't imported (`omitted`, the
/// flagged rows: at most three listed) and how many running balances were
/// flagged: `… 3 rows not imported (rows 12, 40, 41). The file is
/// unchanged. 1 running balance was flagged.`
pub fn imported(
    transactions: usize,
    file: &str,
    omitted: &[SourceLocation],
    running_flags: usize,
) -> Vec<Seg> {
    const LISTED: usize = 3;
    let mut segs = vec![
        plain("Imported "),
        mono(count(transactions)),
        plain(format!(" {} from ", plural(transactions, "transaction"))),
        mono(file),
    ];
    if omitted.is_empty() && running_flags == 0 {
        return segs;
    }
    segs.push(plain("."));
    if !omitted.is_empty() {
        let entries = omitted
            .iter()
            .all(|location| matches!(location, SourceLocation::Entry(_)));
        let lines = omitted
            .iter()
            .all(|location| matches!(location, SourceLocation::Line(_)));
        let n = omitted.len();
        let noun = match (entries, n == 1) {
            (true, true) => "entry",
            (true, false) => "entries",
            (false, true) => "row",
            (false, false) => "rows",
        };
        segs.push(plain(" "));
        segs.push(mono(count(n)));
        segs.push(plain(format!(" {noun} not imported (")));
        // One kind throughout: `rows 12, 40`; a mix labels each one.
        let shared = (entries || lines).then_some(noun);
        if let Some(noun) = shared {
            segs.push(plain(format!("{noun} ")));
        }
        for (i, location) in omitted.iter().take(LISTED).enumerate() {
            if i > 0 {
                segs.push(plain(", "));
            }
            let (label, number) = match *location {
                SourceLocation::Line(number) => ("row", number),
                SourceLocation::Entry(number) => ("entry", number),
            };
            if shared.is_none() {
                segs.push(plain(format!("{label} ")));
            }
            segs.push(mono(crate::settings::table::count(number)));
        }
        if n > LISTED {
            segs.push(plain(", and "));
            segs.push(mono(count(n - LISTED)));
            segs.push(plain(" more"));
        }
        segs.push(plain("). The file is unchanged."));
    }
    if running_flags > 0 {
        segs.push(plain(" "));
        segs.push(mono(count(running_flags)));
        segs.push(plain(if running_flags == 1 {
            " running balance was flagged."
        } else {
            " running balances were flagged."
        }));
    }
    segs
}

/// What an import of `preview` leaves out and flags: the locations of its
/// flagged rows (unreadable, a source ID conflict, a locked period), which
/// aren't imported, and how many running balances don't add up (those rows
/// are imported).
pub fn not_imported(preview: &ImportPreview) -> (Vec<SourceLocation>, usize) {
    let omitted = preview
        .rows()
        .filter(|row| matches!(row.outcome, RowOutcome::Flagged(_)))
        .map(|row| row.location)
        .collect();
    let running = preview
        .reconciliation()
        .running
        .as_ref()
        .map_or(0, |running| running.mismatches.len());
    (omitted, running)
}

/// The notification after undoing it.
pub fn undone(file: &str) -> Vec<Seg> {
    vec![plain("Undid the import of "), mono(file)]
}

#[cfg(test)]
mod tests {
    use orre_core::account::{self, AccountKind, EnteredBalance, EnteredSource, NewAccount};
    use orre_core::db::Database;
    use orre_core::id::AccountId;
    use orre_core::import::preview;
    use orre_core::money::{Currency, ParseAmountError};
    use orre_core::statement::{
        DatedBalance, Field, SourceFormat, Statement, SubAccountKind, SubAccountRef,
    };
    use tempfile::TempDir;

    use super::*;

    fn date(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    #[test]
    fn patterns_read_as_people_write_them() {
        assert_eq!(pattern_label("%d/%m/%Y"), "DD/MM/YYYY");
        assert_eq!(pattern_label("%-d/%-m/%y"), "D/M/YY");
        assert_eq!(pattern_label("%Y-%m-%dT%H:%M:%S"), "YYYY-MM-DDTHH:mm:ss");
        assert_eq!(pattern_label("%d %b %Y"), "DD MMM YYYY");
        assert_eq!(pattern_label("%I:%M %p"), "hh:mm AM");
    }

    #[test]
    fn the_csv_caption() {
        let caption = csv_caption(TextEncoding::Utf8, Delimiter::Comma, 312);
        assert_eq!(joined(&caption), "CSV · UTF-8 · comma-separated · 312 rows");
        assert!(caption[1].mono);
    }

    #[test]
    fn pot_rows_say_which_way() {
        let mut line = StatementLine::new(date(9), Money::new(-25_000, Currency::GBP), "");
        line.sub_account = Some(SubAccountRef {
            kind: SubAccountKind::Pot,
            key: "Rainy day".to_owned(),
            name: Some("Rainy day".to_owned()),
        });
        assert_eq!(row_text(&line), ("To Rainy day".to_owned(), true));
        let mut line = StatementLine::new(date(9), Money::new(-645, Currency::GBP), "PRET 1");
        assert_eq!(row_text(&line), ("PRET 1".to_owned(), false));
        line.merchant = Some("Pret A Manger".to_owned());
        assert_eq!(row_text(&line).0, "Pret A Manger");
    }

    #[test]
    fn the_flag_note_quotes_the_files_text() {
        let problem = RowProblem::Amount {
            field: Field::Amount,
            error: ParseAmountError::Malformed("x"),
        };
        assert_eq!(
            joined(&flag_note(8, &problem, Some("8,99"), DecimalMark::Point, 0)),
            "Row 8: “8,99” isn’t an amount with a decimal point. It will not be imported; \
             it is recorded with the import."
        );
        let problem = RowProblem::Amount {
            field: Field::Amount,
            error: ParseAmountError::OutOfRange {
                currency: Currency::GBP,
            },
        };
        assert_eq!(
            joined(&flag_note(
                4,
                &problem,
                Some("92233720368547758.07"),
                DecimalMark::Point,
                0
            )),
            "Row 4: “92233720368547758.07” is too large: amounts can't exceed \
             10,000,000,000.00 GBP. It will not be imported; it is recorded with the import."
        );
        let problem = RowProblem::ColumnCount {
            expected: 3,
            found: 2,
        };
        assert_eq!(
            joined(&flag_note(2, &problem, None, DecimalMark::Point, 2)),
            "Row 2: The row has 2 columns, but the file has 3. It will not be imported; it is \
             recorded with the import. 2 more rows are flagged too."
        );
    }

    fn toast(
        transactions: usize,
        file: &str,
        omitted: &[SourceLocation],
        running: usize,
    ) -> String {
        let segs = imported(transactions, file, omitted, running);
        let text = joined(&segs);
        println!("toast: {text}");
        text
    }

    fn lines(numbers: &[u64]) -> Vec<SourceLocation> {
        numbers.iter().copied().map(SourceLocation::Line).collect()
    }

    fn entries(numbers: &[u64]) -> Vec<SourceLocation> {
        numbers.iter().copied().map(SourceLocation::Entry).collect()
    }

    #[test]
    fn the_toast_without_omitted_rows_is_unchanged() {
        assert_eq!(
            toast(296, "monzo-november.csv", &[], 0),
            "Imported 296 transactions from monzo-november.csv"
        );
        assert_eq!(
            toast(1, "file.csv", &[], 0),
            "Imported 1 transaction from file.csv"
        );
        let segs = imported(296, "monzo-november.csv", &[], 0);
        assert!(segs[1].mono && segs[3].mono);
    }

    #[test]
    fn the_toast_lists_rows_not_imported() {
        assert_eq!(
            toast(296, "monzo-november.csv", &lines(&[12]), 0),
            "Imported 296 transactions from monzo-november.csv. 1 row not imported (row 12). \
             The file is unchanged."
        );
        assert_eq!(
            toast(1, "file.csv", &lines(&[12, 40, 41]), 0),
            "Imported 1 transaction from file.csv. 3 rows not imported (rows 12, 40, 41). The \
             file is unchanged."
        );
        assert_eq!(
            toast(296, "monzo-november.csv", &lines(&[12, 40, 41, 50, 60]), 0),
            "Imported 296 transactions from monzo-november.csv. 5 rows not imported (rows 12, \
             40, 41, and 2 more). The file is unchanged."
        );
        // Counts and locations are mono; labels aren't.
        let segs = imported(1, "file.csv", &lines(&[12, 40, 41, 50, 60]), 0);
        let mono: Vec<&str> = segs
            .iter()
            .filter(|seg| seg.mono)
            .map(|seg| seg.text.as_str())
            .collect();
        assert_eq!(mono, ["1", "file.csv", "5", "12", "40", "41", "2"]);
    }

    #[test]
    fn the_toast_lists_ofx_entries_not_imported() {
        assert_eq!(
            toast(4, "file.ofx", &entries(&[2]), 0),
            "Imported 4 transactions from file.ofx. 1 entry not imported (entry 2). The file \
             is unchanged."
        );
        assert_eq!(
            toast(4, "file.ofx", &entries(&[2, 5, 9]), 0),
            "Imported 4 transactions from file.ofx. 3 entries not imported (entries 2, 5, 9). \
             The file is unchanged."
        );
        assert_eq!(
            toast(4, "file.ofx", &entries(&[2, 5, 9, 11, 12]), 0),
            "Imported 4 transactions from file.ofx. 5 entries not imported (entries 2, 5, 9, \
             and 2 more). The file is unchanged."
        );
    }

    #[test]
    fn a_mix_of_rows_and_entries_labels_each() {
        let omitted = [SourceLocation::Line(12), SourceLocation::Entry(2)];
        assert_eq!(
            toast(1, "file", &omitted, 0),
            "Imported 1 transaction from file. 2 rows not imported (row 12, entry 2). The file \
             is unchanged."
        );
    }

    #[test]
    fn running_balance_flags_have_their_own_sentence() {
        assert_eq!(
            toast(4, "file.ofx", &[], 1),
            "Imported 4 transactions from file.ofx. 1 running balance was flagged."
        );
        assert_eq!(
            toast(4, "file.csv", &[], 2),
            "Imported 4 transactions from file.csv. 2 running balances were flagged."
        );
        assert_eq!(
            toast(1, "file.ofx", &entries(&[2]), 1),
            "Imported 1 transaction from file.ofx. 1 entry not imported (entry 2). The file \
             is unchanged. 1 running balance was flagged."
        );
        assert_eq!(
            toast(1, "file.csv", &lines(&[12, 40, 41]), 2),
            "Imported 1 transaction from file.csv. 3 rows not imported (rows 12, 40, 41). The \
             file is unchanged. 2 running balances were flagged."
        );
        let only_running = toast(4, "file.csv", &[], 3);
        assert!(!only_running.contains("not imported"));
        assert!(!only_running.contains("The file is unchanged"));
    }

    #[test]
    fn pots_are_listed() {
        assert_eq!(pots_note(&[]), None);
        assert_eq!(
            pots_note(&["Holiday".to_owned(), "Rainy day".to_owned()]).unwrap(),
            "Pot transfers are paired with your Holiday and Rainy day pots automatically."
        );
        assert_eq!(
            pots_note(&["Holiday".to_owned()]).unwrap(),
            "Pot transfers are paired with your Holiday pot automatically."
        );
    }

    #[test]
    fn the_review_caption() {
        assert_eq!(
            joined(&review_caption(
                "Monzo (current account)",
                "Current account",
                Some((date(1), date(9)))
            )),
            "Monzo (current account) → Current account · 1 Nov – 9 Nov 2026"
        );
    }

    #[test]
    fn small_words() {
        assert_eq!(
            already_caption(16).unwrap(),
            "16 rows match transactions you already have, so they are skipped."
        );
        assert_eq!(already_caption(0), None);

        assert_eq!(bank_name("Monzo (current account)"), "Monzo");
        assert_eq!(bank_name("Monzo"), "Monzo");
    }

    struct Ledger {
        db: Database,
        _dir: TempDir,
    }

    impl Ledger {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let db = Database::open(&dir.path().join("orre.db")).unwrap();
            Self { db, _dir: dir }
        }

        fn account(&mut self, name: &str) -> AccountId {
            account::create(
                &mut self.db,
                &NewAccount::new(name, AccountKind::Current, Currency::GBP),
            )
            .unwrap()
        }

        fn entered(&mut self, account: AccountId, d: u32, minor: i64) {
            account::add_balance(
                &mut self.db,
                account,
                &EnteredBalance {
                    date: date(d),
                    balance: Money::new(minor, Currency::GBP),
                    source: EnteredSource::Manual,
                    note: None,
                },
            )
            .unwrap();
        }

        fn preview(
            &self,
            account: AccountId,
            statement: Statement,
        ) -> orre_core::import::ImportPreview {
            preview(&self.db, account, "statement.csv", statement).unwrap()
        }
    }

    fn gbp_line(d: u32, minor: i64, description: &str) -> StatementLine {
        StatementLine::new(date(d), Money::new(minor, Currency::GBP), description)
    }

    fn rows(lines: Vec<StatementLine>) -> Statement {
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        for (index, line) in lines.into_iter().enumerate() {
            statement.push(SourceLocation::Line(index as u64 + 2), Ok(line));
        }
        statement
    }

    fn ofx_with_closing(lines: Vec<StatementLine>, closing_day: u32, minor: i64) -> Statement {
        let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
        for (index, line) in lines.into_iter().enumerate() {
            statement.push(SourceLocation::Entry(index as u64 + 1), Ok(line));
        }
        statement
            .push_balance(DatedBalance {
                date: date(closing_day),
                balance: Money::new(minor, Currency::GBP),
            })
            .unwrap();
        statement
    }

    fn body(preview: &orre_core::import::ImportPreview) -> String {
        joined(&reconciliation(preview, "Monzo exports").body)
    }

    #[test]
    fn the_starting_point_names_the_expected_balance() {
        let mut ledger = Ledger::new();
        let account = ledger.account("Current");
        let preview = ledger.preview(
            account,
            ofx_with_closing(vec![gbp_line(2, -500, "Coffee")], 5, 10_000),
        );
        assert_eq!(
            body(&preview),
            "The expected balance on 5 Nov is £100.00, the first Current has, so there’s nothing to check it against yet. Later imports are checked from it."
        );
    }

    #[test]
    fn a_reconciled_file_names_the_expected_balance() {
        let mut ledger = Ledger::new();
        let account = ledger.account("Current");
        ledger.entered(account, 1, 10_000);
        ledger.entered(account, 10, 9_000);
        let preview = ledger.preview(
            account,
            rows(vec![gbp_line(3, -600, "Rent"), gbp_line(8, -400, "Gas")]),
        );
        assert_eq!(
            body(&preview),
            "The expected balance on 10 Nov is £90.00, and the imported rows add up to it exactly."
        );
    }

    #[test]
    fn a_mismatch_says_what_the_balance_should_be() {
        let mut ledger = Ledger::new();
        let account = ledger.account("Current");
        ledger.entered(account, 1, 10_000);
        ledger.entered(account, 10, 9_000);
        let preview = ledger.preview(account, rows(vec![gbp_line(3, -600, "Rent")]));
        assert_eq!(
            body(&preview),
            "It should be £90.00 on 10 Nov; the imported rows give £94.00, £4.00 over. The gap is between 1 Nov, the last balance that matched, and 10 Nov. Importing keeps the mismatch flagged on the account until it’s resolved."
        );
    }
}
