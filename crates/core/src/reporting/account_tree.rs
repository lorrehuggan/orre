//! `account_tree`: the open accounts as the app's accounts panel lists them
//! (ROADMAP M3.2; `DESIGN.md` → "Accounts panel"): pots under their parent,
//! each account's latest balance and balance-check status, and net worth,
//! all as of today (SLI-77: the caller passes today's local date).
//!
//! - **Order**: accounts as they were created; each pot after its parent,
//!   in the order the pots were created.
//! - **Closed accounts** (`closed_on` today or earlier) are left out, pots
//!   and all. One closing on a later day is still open: it is listed and
//!   counted until that day ([`AccountNode::closes_on`]), at its balance as
//!   of today, so the £0 valuation closing wrote for that day doesn't count
//!   yet.
//! - **No balance**: an account other than a pot with no stated balance
//!   (nothing entered, no balance from a file, no valuation) shows "No
//!   balance" and is left out of net worth until one is entered: its sum
//!   from zero is right only if it started empty. `net_worth_history`
//!   follows the same rule ([`net_worth::counts`]): it lists such an
//!   account, flagged, but leaves it out of its totals. A pot is counted from its
//!   transfers even without one ("Decision record: sub-accounts"), as the
//!   balance checks treat it ("pots follow their transfers").
//! - **Net worth** is per currency, never converted: the latest balance of
//!   every counted account, a liability always subtracting
//!   ([`super::net_worth`]). Each account's latest balance is at the end of
//!   its own last active day, so the accounts' dates can differ.
//! - **Flagged**: an account with a failed balance check
//!   ([`Status::Mismatch`]).
//!
//! Nothing here reads transaction text.

use chrono::NaiveDate;

use crate::account::{Account, AccountKind};
use crate::db::Reader;
use crate::money::Money;
use crate::reconciliation::{self, Status};

use super::balance::{self, AccountBalance, Anchor};
use super::net_worth;
use super::{Error, Totals};

/// The open accounts, with their pots, and net worth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountTree {
    /// Accounts with no parent, as they were created. Their pots are in
    /// [`AccountNode::pots`].
    pub accounts: Vec<AccountNode>,
    /// The sum of every counted account's latest balance, per currency
    /// (module docs). Empty when no account has a balance.
    pub net_worth: Totals,
}

impl AccountTree {
    /// Every account, each followed by its pots: the panel's rows in order.
    pub fn rows(&self) -> impl Iterator<Item = &AccountNode> {
        self.accounts
            .iter()
            .flat_map(|node| std::iter::once(node).chain(&node.pots))
    }
}

/// One account in the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountNode {
    pub account: Account,
    /// Its balance at the end of its last active day; `None` when it has
    /// [no balance](AccountNode::has_balance).
    pub latest: Option<AccountBalance>,
    /// Its balance checks: [`Status::Mismatch`] when one failed.
    pub status: Status,
    /// Its open pots, as they were created. Empty for every kind but
    /// current and savings.
    pub pots: Vec<AccountNode>,
}

impl AccountNode {
    /// The day it closes on, still to come: it leaves the tree that day.
    /// `None` when it isn't closing (a closed one isn't in the tree).
    pub fn closes_on(&self) -> Option<NaiveDate> {
        self.account.closed_on
    }

    /// Whether it shows a balance and counts toward net worth (module docs).
    pub fn has_balance(&self) -> bool {
        self.latest.is_some()
    }

    /// Whether its balance is below zero where that means overdrawn: a
    /// current, savings, cash or pot account. A liability is negative by
    /// nature and a credit card owes money as a negative balance, so
    /// neither is (`DESIGN.md` → "Money").
    pub fn is_overdrawn(&self) -> bool {
        let overdraws = matches!(
            self.account.kind,
            AccountKind::Current | AccountKind::Savings | AccountKind::Cash | AccountKind::Pot
        );
        overdraws
            && self
                .latest
                .is_some_and(|latest| latest.balance.is_negative())
    }

    /// The total of its pots' balances, `None` when it has no pots with a
    /// balance.
    pub fn pots_total(&self) -> Result<Option<Money>, Error> {
        let mut total: Option<Money> = None;
        for pot in &self.pots {
            if let Some(latest) = pot.latest {
                total = Some(match total {
                    Some(sum) => sum.checked_add(latest.balance)?,
                    None => latest.balance,
                });
            }
        }
        Ok(total)
    }
}

/// The accounts open on `today`, with their pots, balances and check
/// status, and net worth (module docs).
pub fn account_tree(reader: &Reader, today: NaiveDate) -> Result<AccountTree, Error> {
    let open: Vec<Account> = balance::accounts(reader)?
        .into_iter()
        .filter(|account| account.closed_on.is_none_or(|closed| closed > today))
        .collect();
    let mut net_worth = Totals::new();
    let mut accounts = Vec::new();
    for account in open.iter().filter(|account| account.parent.is_none()) {
        let mut node = node(reader, account.clone(), today, &mut net_worth)?;
        for pot in open.iter().filter(|pot| pot.parent == Some(account.id)) {
            node.pots
                .push(self::node(reader, pot.clone(), today, &mut net_worth)?);
        }
        accounts.push(node);
    }
    Ok(AccountTree {
        accounts,
        net_worth,
    })
}

/// `account`'s node without its pots; adds its balance to `net_worth` when
/// it counts. One closing on a later day is taken as of `today`.
fn node(
    reader: &Reader,
    account: Account,
    today: NaiveDate,
    net_worth: &mut Totals,
) -> Result<AccountNode, Error> {
    let latest = match account.closed_on {
        Some(_) => balance::latest_balance_through(reader, &account, today)?,
        None => balance::latest_balance(reader, &account)?,
    }
    .filter(|latest| net_worth::counts(account.kind, latest.anchor != Anchor::Unanchored));
    if let Some(latest) = latest {
        net_worth.add(net_worth::counted(&account, latest.balance)?)?;
    }
    let status = reconciliation::load(reader.conn(), &account)?.status();
    Ok(AccountNode {
        account,
        latest,
        status,
        pots: Vec::new(),
    })
}

#[cfg(test)]
mod tests;
