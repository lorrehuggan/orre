//! What the accounts panel and inspector show: the account tree from core
//! ([`orre_core::reporting::account_tree`]) and, per account, what the
//! inspector lists. Plain data, read in one go by [`super::store`] or built
//! by `super::fixture`.

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use orre_core::account::{Account, Deletion, StoredBalance, Usage};
use orre_core::id::AccountId;
use orre_core::import::BatchInfo;
use orre_core::money::Money;
use orre_core::reporting::account_tree::{AccountNode, AccountTree};

/// A CSV preset the Preset select offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetChoice {
    /// Its stable ID, stored on the account (`uk-monzo`).
    pub id: String,
    /// What the select shows ("Monzo").
    pub name: String,
}

/// The inspector's lists for one account.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountDetail {
    /// What it holds, which decides what about it can change.
    pub usage: Usage,
    /// Balances the user entered, and the opening balance, newest first.
    pub balances: Vec<StoredBalance>,
    /// Its committed imports, newest first.
    pub imports: Vec<BatchInfo>,
    /// The earliest day it can close on, its pots included; `None` when
    /// any day will do ([`orre_core::account::close::closing`]).
    pub close_from: Option<NaiveDate>,
    /// What deleting it would remove, or why it can't be deleted.
    pub deletion: Deletion,
    /// A cash account's last count plus the transactions since, for sample
    /// data, which has no transactions to add up
    /// ([`super::Accounts::cash_expectation`]).
    pub cash_expected: Option<Money>,
}

/// Everything the accounts panel and inspector show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountsData {
    pub tree: AccountTree,
    pub details: BTreeMap<AccountId, AccountDetail>,
    pub presets: Vec<PresetChoice>,
    /// The day the data was read, which decides how dates are written.
    pub today: NaiveDate,
    /// Every name, exactly as stored, that more than one account has, open
    /// or closed, pots included: `orre accounts reopen` can't tell them
    /// apart by name ([`shared_names`]).
    pub shared_names: BTreeSet<String>,
}

/// The names more than one of `accounts` has, compared as `orre accounts
/// reopen` compares them (`cli::accounts::find`): exactly, case and spaces
/// included.
pub fn shared_names(accounts: &[Account]) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut shared = BTreeSet::new();
    for account in accounts {
        if !seen.insert(account.name.as_str()) {
            shared.insert(account.name.clone());
        }
    }
    shared
}

impl AccountsData {
    /// No accounts.
    pub fn empty(presets: Vec<PresetChoice>, today: NaiveDate) -> Self {
        Self {
            tree: AccountTree {
                accounts: Vec::new(),
                net_worth: Default::default(),
            },
            details: BTreeMap::new(),
            presets,
            today,
            shared_names: BTreeSet::new(),
        }
    }

    /// The account with this ID, a pot included, if it's open.
    pub fn node(&self, id: AccountId) -> Option<&AccountNode> {
        self.tree.rows().find(|node| node.account.id == id)
    }

    /// The account a pot belongs to.
    pub fn parent_of(&self, pot: AccountId) -> Option<&AccountNode> {
        self.tree
            .accounts
            .iter()
            .find(|node| node.pots.iter().any(|child| child.account.id == pot))
    }

    pub fn detail(&self, id: AccountId) -> Option<&AccountDetail> {
        self.details.get(&id)
    }

    /// Whether this computer has the preset with ID `id`, bundled or saved.
    pub fn has_preset(&self, id: &str) -> bool {
        self.presets.iter().any(|preset| preset.id == id)
    }

    /// The name of the preset with ID `id`; the ID itself for one this build
    /// doesn't have.
    pub fn preset_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.presets
            .iter()
            .find(|preset| preset.id == id)
            .map_or(id, |preset| preset.name.as_str())
    }

    /// Whether an account selected before a reload is still there.
    pub fn contains(&self, id: AccountId) -> bool {
        self.node(id).is_some()
    }
}
