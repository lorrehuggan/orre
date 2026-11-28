//! Right-click menus on the accounts panel's rows (ROADMAP M3.P10; Paper
//! "Context menu · account"): the app's first context menu. It is
//! [`MenuList`] opened at the pointer ([`crate::ui::menu::at_pointer`]) with
//! its own keys, and it closes on Escape, on a click outside and after an
//! item is chosen. The row it opened on keeps the `hover` fill while it is
//! open, and a right-click doesn't change the selection.
//!
//! Every item does what the app already does elsewhere, through the same
//! [`AccountsEvent`]s the inspector's buttons use:
//!
//! - **Account** (current, savings, credit card, cash): Show transactions,
//!   New transaction…, Import into this account… (not cash: it isn't
//!   imported), Add balance… (the cash count on cash), Edit in the
//!   inspector, then Close account… and Delete account…, which is faded with
//!   core's reason ("Has transactions") when it can't go.
//! - **Pot**: Show transactions, Add balance….
//! - **Asset or liability**: Add value… (Add balance… on a liability), Edit
//!   in the inspector, then Close account… and Delete account….
//! - **Category group**: Show transactions, Open in Rules, then Rename…, New
//!   subcategory…, Hide and Delete…. Rules → Categories doesn't take a
//!   rename, a new subcategory, a hide or a delete from outside, so these four
//!   open Rules on the category, as Open in Rules does; Delete… is faded
//!   with "In use" while the group holds transactions, which every group in
//!   the panel does.
//! - **Uncategorised**: Show transactions.

use gpui_kit::{AnyElement, App, Entity, IntoElement as _, Pixels, Point, SharedString};
use orre_core::account::AccountKind;
use orre_core::account::Deletion;
use orre_core::id::AccountId;
use orre_core::reporting::account_tree::AccountNode;

use crate::theme::tokens::size;
use crate::ui::menu::{MenuGroup, MenuList, MenuOption, SelectState, at_pointer};

use super::data::AccountsData;
use super::{Accounts, AccountsEvent, CategoryPick};

/// What a context menu was opened on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextTarget {
    Account(AccountId),
    /// A top-level category group, by category ID.
    Category(i64),
    Uncategorised,
}

/// An item's key, which [`Accounts::choose_context_item`] reads.
pub mod item {
    pub const SHOW_TRANSACTIONS: &str = "show-transactions";
    pub const NEW_TRANSACTION: &str = "new-transaction";
    pub const IMPORT: &str = "import";
    pub const ADD_BALANCE: &str = "add-balance";
    pub const INSPECT: &str = "inspect";
    pub const CLOSE: &str = "close";
    pub const DELETE: &str = "delete";
    pub const OPEN_IN_RULES: &str = "open-in-rules";
    pub const RENAME: &str = "rename";
    pub const NEW_SUBCATEGORY: &str = "new-subcategory";
    pub const HIDE: &str = "hide";
    pub const DELETE_CATEGORY: &str = "delete-category";
}

/// The open menu: where it is, what it offers and its keyboard state.
pub struct ContextMenu {
    pub target: ContextTarget,
    pub position: Point<Pixels>,
    pub groups: Vec<MenuGroup>,
    pub select: SelectState,
}

/// "has transactions" as a caption: "Has transactions".
fn capital(reason: &str) -> SharedString {
    let mut chars = reason.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect::<String>())
        .unwrap_or_default()
        .into()
}

fn option(key: &'static str, label: &'static str) -> MenuOption {
    MenuOption::new(key, label)
}

/// The groups for the account with this ID; none when it isn't open.
pub fn account_groups(data: &AccountsData, id: AccountId) -> Vec<MenuGroup> {
    let Some(node) = data.node(id) else {
        return Vec::new();
    };
    let deletion = data.detail(id).map(|detail| detail.deletion);
    groups_for(node, deletion.unwrap_or_default())
}

fn groups_for(node: &AccountNode, deletion: Deletion) -> Vec<MenuGroup> {
    let kind = node.account.kind;
    let show = option(item::SHOW_TRANSACTIONS, "Show transactions");
    match kind {
        AccountKind::Pot => vec![MenuGroup::new(
            None,
            vec![show, option(item::ADD_BALANCE, "Add balance…")],
        )],
        AccountKind::Asset | AccountKind::Liability => vec![
            MenuGroup::new(
                None,
                vec![
                    option(
                        item::ADD_BALANCE,
                        if kind == AccountKind::Asset {
                            "Add value…"
                        } else {
                            "Add balance…"
                        },
                    ),
                    option(item::INSPECT, "Edit in the inspector"),
                ],
            ),
            lifecycle(node, deletion),
        ],
        AccountKind::Current
        | AccountKind::Savings
        | AccountKind::CreditCard
        | AccountKind::Cash => {
            let mut first = vec![show, option(item::NEW_TRANSACTION, "New transaction…")];
            if kind.is_imported() {
                first.push(option(item::IMPORT, "Import into this account…"));
            }
            first.push(option(item::ADD_BALANCE, "Add balance…"));
            first.push(option(item::INSPECT, "Edit in the inspector"));
            vec![MenuGroup::new(None, first), lifecycle(node, deletion)]
        }
    }
}

/// Close account… and Delete account…, which says why when it can't go.
/// An account closing on a later day has no Close account… (SLI-77).
fn lifecycle(node: &AccountNode, deletion: Deletion) -> MenuGroup {
    let mut delete = option(item::DELETE, "Delete account…");
    if let Some(lock) = deletion.refused {
        delete = delete.disabled(true).hint(capital(lock.reason()));
    }
    let mut items = Vec::new();
    if node.closes_on().is_none() {
        items.push(option(item::CLOSE, "Close account…"));
    }
    items.push(delete);
    MenuGroup::new(None, items)
}

/// The groups for a category group holding `rows` transactions.
pub fn category_groups(rows: usize) -> Vec<MenuGroup> {
    let mut delete = option(item::DELETE_CATEGORY, "Delete…");
    if rows > 0 {
        delete = delete.disabled(true).hint("In use");
    }
    vec![
        MenuGroup::new(
            None,
            vec![
                option(item::SHOW_TRANSACTIONS, "Show transactions"),
                option(item::OPEN_IN_RULES, "Open in Rules"),
            ],
        ),
        MenuGroup::new(
            None,
            vec![
                option(item::RENAME, "Rename…"),
                option(item::NEW_SUBCATEGORY, "New subcategory…"),
                option(item::HIDE, "Hide"),
                delete,
            ],
        ),
    ]
}

/// The groups for the Uncategorised row.
pub fn uncategorised_groups() -> Vec<MenuGroup> {
    vec![MenuGroup::new(
        None,
        vec![option(item::SHOW_TRANSACTIONS, "Show transactions")],
    )]
}

/// What choosing `key` on `target` asks the shell for.
pub fn event(target: ContextTarget, key: &str) -> Option<AccountsEvent> {
    Some(match (target, key) {
        (ContextTarget::Account(id), item::SHOW_TRANSACTIONS) => {
            AccountsEvent::ShowTransactionsRequested(id)
        }
        (ContextTarget::Account(id), item::NEW_TRANSACTION) => {
            AccountsEvent::NewTransactionRequested(id)
        }
        (ContextTarget::Account(id), item::IMPORT) => AccountsEvent::ImportRequested(id),
        (ContextTarget::Account(id), item::ADD_BALANCE) => AccountsEvent::AddBalanceRequested(id),
        (ContextTarget::Account(id), item::INSPECT) => AccountsEvent::InspectRequested(id),
        (ContextTarget::Account(id), item::CLOSE) => AccountsEvent::CloseRequested(id),
        (ContextTarget::Account(id), item::DELETE) => AccountsEvent::DeleteRequested(id),
        (ContextTarget::Category(id), item::SHOW_TRANSACTIONS) => {
            AccountsEvent::CategoryShowRequested(CategoryPick::Category(id))
        }
        (
            ContextTarget::Category(id),
            item::OPEN_IN_RULES
            | item::RENAME
            | item::NEW_SUBCATEGORY
            | item::HIDE
            | item::DELETE_CATEGORY,
        ) => AccountsEvent::CategoryRulesRequested(id),
        (ContextTarget::Uncategorised, item::SHOW_TRANSACTIONS) => {
            AccountsEvent::CategoryShowRequested(CategoryPick::Uncategorised)
        }
        _ => return None,
    })
}

/// The open menu, drawn at the pointer.
pub fn render(menu: &ContextMenu, accounts: &Entity<Accounts>, _: &App) -> AnyElement {
    let (choose, step, dismiss) = (accounts.clone(), accounts.clone(), accounts.clone());
    let list = MenuList::new("Actions", menu.groups.clone(), None, &menu.select)
        .context()
        .own_keys()
        .width(size::SEARCH_FIELD)
        .on_choose(move |key, window, cx| {
            let key = key.clone();
            choose.update(cx, |accounts, cx| {
                accounts.choose_context_item(&key, window, cx)
            })
        })
        .on_move(move |by, _, cx| {
            step.update(cx, |accounts, cx| accounts.move_context_highlight(by, cx))
        })
        .on_dismiss(move |window, cx| {
            dismiss.update(cx, |accounts, cx| accounts.close_context_menu(window, cx))
        });
    at_pointer(menu.position, list).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(groups: &[MenuGroup]) -> Vec<Vec<&str>> {
        groups
            .iter()
            .map(|group| group.options.iter().map(|o| o.key.as_ref()).collect())
            .collect()
    }

    #[test]
    fn a_category_group_in_use_cant_be_deleted() {
        let groups = category_groups(12);
        assert_eq!(
            keys(&groups),
            [
                vec!["show-transactions", "open-in-rules"],
                vec!["rename", "new-subcategory", "hide", "delete-category"],
            ]
        );
        let delete = &groups[1].options[3];
        assert!(delete.disabled);
        assert_eq!(delete.hint.as_deref(), Some("In use"));
        assert!(!groups[1].options[0].disabled);
        assert!(!category_groups(0)[1].options[3].disabled);
    }

    #[test]
    fn a_category_item_shows_transactions_or_opens_rules() {
        let target = ContextTarget::Category(7);
        assert_eq!(
            event(target, item::SHOW_TRANSACTIONS),
            Some(AccountsEvent::CategoryShowRequested(
                CategoryPick::Category(7)
            ))
        );
        for key in [
            item::OPEN_IN_RULES,
            item::RENAME,
            item::NEW_SUBCATEGORY,
            item::HIDE,
            item::DELETE_CATEGORY,
        ] {
            assert_eq!(
                event(target, key),
                Some(AccountsEvent::CategoryRulesRequested(7)),
                "{key}"
            );
        }
        assert_eq!(
            event(ContextTarget::Uncategorised, item::SHOW_TRANSACTIONS),
            Some(AccountsEvent::CategoryShowRequested(
                CategoryPick::Uncategorised
            ))
        );
        assert_eq!(event(target, item::CLOSE), None);
    }

    #[test]
    fn reasons_start_with_a_capital() {
        assert_eq!(capital("has transactions"), "Has transactions");
        assert_eq!(capital(""), "");
    }
}
