//! Reading and changing accounts in the database (`docs/ARCHITECTURE.md` →
//! "Decision record: accounts management").
//!
//! Like the window state ([`crate::shell::window_state`]) and the MCP
//! server, the app holds no connection between reads and writes: each call
//! opens the database at the path it was given, creating and migrating it
//! as the CLI does, through [`gate::open`], which checks the restore lock
//! before and after opening, so a restore can swap the file. Reads run
//! on a background thread ([`super::Accounts::reload`]); writes are small
//! (a person's accounts) and run on the UI thread.
//!
//! All the business rules are core's ([`orre_core::account`]): this only
//! opens the database and reads what the panel and inspector show.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;

use chrono::NaiveDate;

use orre_core::account::count::{self, Expectation, Recorded};
use orre_core::account::{self, AccountEdit, EnteredBalance, NewAccount};
use orre_core::backup::gate;
use orre_core::db::Database;
use orre_core::id::{AccountId, BalanceId};
use orre_core::import::{self, BatchStatus};
use orre_core::money::Money;
use orre_core::reporting::account_tree::account_tree;
use orre_import::preset::Presets;

use super::data::{AccountDetail, AccountsData, PresetChoice};

/// Why a read or write didn't happen. Its message is shown to the user, and
/// never holds transaction text: core's errors don't.
pub type StoreError = Box<dyn Error + Send + Sync>;

/// The bundled CSV presets, as the Preset select offers them. Empty (with a
/// warning) if they don't load, which a test rules out.
pub fn bundled_presets() -> Vec<PresetChoice> {
    match Presets::bundled() {
        Ok(presets) => presets
            .iter()
            .map(|preset| PresetChoice {
                id: preset.id().to_owned(),
                name: preset.name().to_owned(),
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "the bundled presets don't load");
            Vec::new()
        }
    }
}

/// The accounts in the database at `database`.
#[derive(Clone, Debug)]
pub struct AccountStore {
    database: PathBuf,
}

impl AccountStore {
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    /// The presets the Preset select offers: the bundled ones and those
    /// the user saved from the import screen (M3.5), in the data dir next
    /// to the database. A saved preset that can't be used is left out.
    pub fn presets(&self) -> Vec<PresetChoice> {
        let dir = orre_core::paths::presets_dir(&self.database);
        match orre_import::preset::load_user_presets(&dir) {
            Ok((presets, problems)) => {
                for problem in problems {
                    tracing::warn!(%problem, "a saved preset can't be used");
                }
                presets
                    .iter()
                    .map(|preset| PresetChoice {
                        id: preset.id().to_owned(),
                        name: preset.name().to_owned(),
                    })
                    .collect()
            }
            Err(error) => {
                tracing::warn!(%error, "the bundled presets don't load");
                Vec::new()
            }
        }
    }

    fn open(&self) -> Result<Database, StoreError> {
        Ok(gate::open(&self.database, Database::open)?)
    }

    /// Every account open on `today` (the local date: an account closing
    /// on a later day is still open) with what the panel and inspector
    /// show.
    pub fn load(
        &self,
        presets: Vec<PresetChoice>,
        today: NaiveDate,
    ) -> Result<AccountsData, StoreError> {
        let db = self.open()?;
        let tree = account_tree(&db.reader()?, today)?;
        // Every account, closed ones and pots included, as `orre accounts
        // reopen` reads them: which names it can't tell apart.
        let shared_names = super::data::shared_names(&account::list(&db)?);
        let mut details = BTreeMap::new();
        for node in tree.rows() {
            let id = node.account.id;
            let mut imports: Vec<_> = import::batches(&db, id)?
                .into_iter()
                .filter(|batch| batch.status == BatchStatus::Committed)
                .collect();
            imports.reverse();
            details.insert(
                id,
                AccountDetail {
                    usage: account::usage(&db, id)?,
                    balances: account::entered_balances(&db, id)?,
                    imports,
                    // One closing on a later day is listed too, and can't
                    // close again.
                    close_from: match node.closes_on() {
                        Some(_) => None,
                        None => account::close::closing(&db, id)?.earliest,
                    },
                    deletion: account::close::deletion(&db, id)?,
                    cash_expected: None,
                },
            );
        }
        Ok(AccountsData {
            tree,
            details,
            presets,
            today,
            shared_names,
        })
    }

    /// Adds an account, with its first balance when given.
    pub fn create(
        &self,
        account: &NewAccount,
        balance: Option<&EnteredBalance>,
    ) -> Result<AccountId, StoreError> {
        let mut db = self.open()?;
        Ok(account::create_with_balance(&mut db, account, balance)?)
    }

    /// Changes an account's name, bank, kind, currency or preset.
    pub fn update(&self, id: AccountId, edit: &AccountEdit) -> Result<(), StoreError> {
        let mut db = self.open()?;
        Ok(account::update(&mut db, id, edit)?)
    }

    /// Records a balance the user entered, or a valuation.
    pub fn add_balance(&self, id: AccountId, balance: &EnteredBalance) -> Result<(), StoreError> {
        let mut db = self.open()?;
        account::add_balance(&mut db, id, balance)?;
        Ok(())
    }

    /// Changes one balance the user entered.
    pub fn update_balance(
        &self,
        id: AccountId,
        balance: BalanceId,
        entered: &EnteredBalance,
    ) -> Result<(), StoreError> {
        let mut db = self.open()?;
        Ok(account::update_balance(&mut db, id, balance, entered)?)
    }

    /// Removes one balance the user entered; returns it, to put back.
    pub fn remove_balance(
        &self,
        id: AccountId,
        balance: BalanceId,
    ) -> Result<EnteredBalance, StoreError> {
        let mut db = self.open()?;
        Ok(account::remove_balance(&mut db, id, balance)?)
    }

    /// What Orre expects of a cash count of `counted` on `date`.
    pub fn cash_expectation(
        &self,
        id: AccountId,
        date: NaiveDate,
        counted: Money,
    ) -> Result<Expectation, StoreError> {
        let db = self.open()?;
        Ok(count::expectation(&db, id, date, counted)?)
    }

    /// Records a cash count, with the "Cash count difference" transaction
    /// when `add_difference`, in one DB transaction.
    pub fn count_cash(
        &self,
        id: AccountId,
        count: &EnteredBalance,
        add_difference: bool,
    ) -> Result<Recorded, StoreError> {
        let mut db = self.open()?;
        Ok(count::record(&mut db, id, count, add_difference)?)
    }

    /// Closes an account (and its open pots) on `on`; returns every
    /// account it closed.
    pub fn close(&self, id: AccountId, on: NaiveDate) -> Result<Vec<AccountId>, StoreError> {
        let mut db = self.open()?;
        Ok(account::close::close(&mut db, id, on)?)
    }

    /// Reopens a closed account, with the pots that closed with it.
    pub fn reopen(&self, id: AccountId) -> Result<Vec<AccountId>, StoreError> {
        let mut db = self.open()?;
        Ok(account::close::reopen(&mut db, id)?)
    }

    /// Deletes an account that nothing refers to, with its values; returns
    /// how many values went with it.
    pub fn delete(&self, id: AccountId) -> Result<usize, StoreError> {
        let mut db = self.open()?;
        Ok(account::close::delete(&mut db, id)?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use orre_core::account::{AccountKind, EnteredSource};
    use orre_core::money::{Currency, Money};
    use tempfile::TempDir;

    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 28).unwrap()
    }

    #[test]
    fn the_bundled_presets_load() {
        let presets = bundled_presets();
        assert!(presets.iter().any(|preset| preset.id == "uk-monzo"));
    }

    /// SLI-77: the load sees every account's name, closed ones included,
    /// so the Close dialog names an account by ID when another account
    /// has its exact name, as `orre accounts reopen` would refuse it.
    #[test]
    fn a_name_shared_with_a_closed_account_is_reopened_by_id() {
        use crate::accounts::close::CloseFacts;

        let dir = TempDir::new().unwrap();
        let store = AccountStore::new(dir.path().join("orre.db"));
        let add = |name: &str| {
            store
                .create(
                    &NewAccount::new(name, AccountKind::Savings, Currency::GBP),
                    None,
                )
                .unwrap()
        };
        let open_isa = add("Joint ISA");
        let closed_isa = add("Joint ISA");
        let lower = add("joint isa");
        let spaced = add("Joint ISA ");
        let twin_a = add("Twin");
        let twin_b = add("Twin");
        let solo = add("Solo");
        let on = NaiveDate::from_ymd_opt(2026, 11, 6).unwrap();
        store.close(closed_isa, on).unwrap();

        let data = store.load(Vec::new(), today()).unwrap();
        assert!(!data.contains(closed_isa), "the panel doesn't list it");
        assert_eq!(
            data.shared_names
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["Joint ISA", "Twin"]
        );
        let command = |id| CloseFacts::of(&data, id).unwrap().reopen_command();
        // Shared with a closed account, and with another open one: the ID.
        assert_eq!(
            CloseFacts::of(&data, open_isa)
                .unwrap()
                .body(NaiveDate::from_ymd_opt(2026, 11, 29)),
            format!(
                "Joint ISA leaves the Accounts panel and net worth from 29 Nov 2026. Its transactions, balances and imports stay, so earlier net worth doesn't change. To reopen it, run orre accounts reopen {open_isa} in a terminal."
            )
        );
        assert_eq!(command(twin_a), format!("orre accounts reopen {twin_a}"));
        assert_eq!(command(twin_b), format!("orre accounts reopen {twin_b}"));
        // `reopen` compares exactly: another case isn't the same name.
        assert_eq!(command(lower), "orre accounts reopen \"joint isa\"");
        // Nor is a trailing space; that name gets the ID anyway, because
        // the dialog shows it trimmed.
        assert!(!data.shared_names.contains("Joint ISA "));
        assert_eq!(command(spaced), format!("orre accounts reopen {spaced}"));
        // A name no other account has: quoted.
        assert_eq!(command(solo), "orre accounts reopen \"Solo\"");
    }

    #[test]
    fn closes_reopens_and_deletes() {
        let dir = TempDir::new().unwrap();
        let store = AccountStore::new(dir.path().join("orre.db"));
        let house = store
            .create(
                &NewAccount::new("House", AccountKind::Asset, Currency::GBP),
                Some(&EnteredBalance {
                    date: NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
                    balance: Money::new(38_500_000, Currency::GBP),
                    source: EnteredSource::Valuation,
                    note: None,
                }),
            )
            .unwrap();
        let data = store.load(Vec::new(), today()).unwrap();
        let detail = data.detail(house).unwrap();
        assert_eq!(
            detail.close_from,
            NaiveDate::from_ymd_opt(2026, 10, 13),
            "after its last value"
        );
        assert_eq!(detail.deletion.values, 1);
        assert_eq!(detail.deletion.refused, None);

        let on = NaiveDate::from_ymd_opt(2026, 11, 6).unwrap();
        assert_eq!(store.close(house, on).unwrap(), [house]);
        assert!(!store.load(Vec::new(), today()).unwrap().contains(house));
        // As of a day before it closed, it's still there, closing, and
        // can't close again (SLI-77).
        let before = store.load(Vec::new(), on.pred_opt().unwrap()).unwrap();
        assert_eq!(before.node(house).unwrap().closes_on(), Some(on));
        assert_eq!(before.detail(house).unwrap().close_from, None);
        assert_eq!(store.reopen(house).unwrap(), [house]);
        assert!(store.load(Vec::new(), today()).unwrap().contains(house));
        assert_eq!(store.delete(house).unwrap(), 1);
        assert!(!store.load(Vec::new(), today()).unwrap().contains(house));
    }

    #[test]
    fn creates_edits_and_reads_back_accounts() {
        let dir = TempDir::new().unwrap();
        let store = AccountStore::new(dir.path().join("orre.db"));
        assert!(
            store
                .load(Vec::new(), today())
                .unwrap()
                .tree
                .accounts
                .is_empty()
        );

        let mut new = NewAccount::new("Current account", AccountKind::Current, Currency::GBP);
        new.preset = Some("uk-monzo".into());
        let balance = EnteredBalance {
            date: NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(),
            balance: Money::new(128_410, Currency::GBP),
            source: EnteredSource::Manual,
            note: None,
        };
        let id = store.create(&new, Some(&balance)).unwrap();

        let data = store.load(bundled_presets(), today()).unwrap();
        let node = data.node(id).unwrap();
        assert_eq!(node.latest.unwrap().balance.minor(), 128_410);
        assert_eq!(data.preset_name("uk-monzo"), "Monzo");
        assert_eq!(data.detail(id).unwrap().balances.len(), 1);

        let mut edit = AccountEdit::of(&node.account);
        edit.name = "Monzo".into();
        store.update(id, &edit).unwrap();
        assert_eq!(
            store
                .load(Vec::new(), today())
                .unwrap()
                .node(id)
                .unwrap()
                .account
                .name,
            "Monzo"
        );

        // A refused write says why.
        edit.name = " ".into();
        let error = store.update(id, &edit).unwrap_err();
        assert_eq!(error.to_string(), "an account needs a name");
    }
}
