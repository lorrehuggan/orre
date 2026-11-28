# Your data: where it lives, backups and privacy

Everything Orre knows about your money is on your own computer. This page says where it
is, how to back it up and restore it, what stays private, and how to turn AI access off.
For the full security picture, see [`SECURITY.md`](../SECURITY.md).

## Where your data lives

Orre keeps everything in one folder, the **data folder**:

| System | Data folder |
|--------|-------------|
| Linux | `$XDG_DATA_HOME/orre`, which is `~/.local/share/orre` by default |
| macOS | `~/Library/Application Support/Orre` |

On Linux, Orre uses `XDG_DATA_HOME` only when it's set to an absolute path; otherwise
the folder is `~/.local/share/orre`.

Inside it:

- **`orre.db`**: your database. Every account, transaction, category, rule and setting is
  in this one SQLite file. While the app, a CLI command or the MCP server has it open,
  `orre.db-wal` and `orre.db-shm` sit next to it and hold its most recent changes.
- **`backups/`**: backups of the database (see below).
- **`presets/`**: bank formats you saved from the import screen.

You can find the exact path at the foot of Settings (the Settings button, or ⌘, on
macOS, Ctrl+, on Linux).

**Don't copy `orre.db` by hand while Orre is running.** Its latest changes may be only
in `orre.db-wal`, so the copy can be incomplete or broken. Take a backup instead
(Settings → Backups → **Back up now**, or `orre backup`): it writes a complete copy of
the database, those changes included, as a single file in `backups/`, and it's safe
while Orre is running. Or quit Orre and any AI client using it first, then copy
`orre.db` together with any `orre.db-wal` and `orre.db-shm` next to it.

**Using another folder.** Set the `ORRE_DATA_DIR` environment variable to a folder, and
Orre uses that instead, for the database, its backups and your presets alike:

```sh
ORRE_DATA_DIR=~/finances/orre orre
```

## Who can read it

- On Linux and macOS, Orre creates the database readable only by you (`0600`), and
  tightens it if it finds it looser. A data folder Orre creates itself is `0700` (only
  you can open it); a folder you chose with `ORRE_DATA_DIR` keeps the permissions you
  gave it.
- **The database is not encrypted.** Anything that can read your files can read your
  finances: malware, another program running as you, someone with admin rights, or a
  copy of an unencrypted disk. Database encryption is planned but not built.
- **Turn on full-disk encryption**: FileVault on macOS, LUKS on Linux. That protects
  the database, its backups and your exports if the computer is lost or stolen.

## Backups

Orre takes a backup by itself:

- before every import that adds anything,
- before it updates the database to a new version,
- before every restore.

It keeps the **10 most recent** of these automatic backups, plus **one a month for the
12 months before this one**, and deletes older ones as it takes new ones.

**Back up yourself** at any time:

- in the app: Settings → Backups → **Back up now**, or
- in a terminal: `orre backup`.

Backups you take yourself are **never deleted automatically**: they stay until you
delete them. `orre backup --list` lists every backup and their total size; Settings →
Backups shows the same.

Each backup is a single file in `backups/`, named after when and why it was taken, e.g.
`orre-20261111T153012.345Z-manual.db`. It's a complete copy of the database, so you can
also copy it somewhere safe.

Things to know:

- **Backups are not encrypted.** Protect them like the database: keep any copies on an
  encrypted disk.
- **Backups keep what you later delete.** An undone import, a removed account or a deleted
  report stays in older backups until they're rotated out or you delete them. To get rid
  of it, delete those backups: in Settings → Backups, select one and choose **Delete
  backup…**, or run `orre backup --delete <file>` (it shows what it would delete; add
  `--yes` to delete it).
- **Your saved bank formats (`presets/`) aren't in backups.** Copy that folder yourself
  if you move to another computer.

## Restoring a backup

- **In the app:** Settings → Backups, select a backup, then **Restore…**. To restore a
  backup file from somewhere else, use **Restore from a file…**.
- **In a terminal:**

  ```sh
  orre backup --list                 # find the backup's file name
  orre restore <file>                # check it and show what restoring would do
  orre restore <file> --yes          # restore it
  ```

Before restoring, Orre checks the backup is intact and that this version of Orre can
read it. It also shows what the restore will undo: the imports, edits and Inbox
suggestions since the backup, the transactions you've added by hand since (they're
removed) and the hand-added ones you've deleted since (they come back). Other changes,
such as to rules, accounts or settings, are undone too but not counted. It then **backs
up your current database first**, so a restore can itself be undone by restoring that
backup. If the current database can't be backed up, nothing is restored.

If an AI client is connected while you restore, see
[Restoring a backup while a client is connected](mcp-setup.md#restoring-a-backup-while-a-client-is-connected).

## Privacy and AI

- **Your data stays on your computer.** There's no Orre account, Orre never asks for
  your bank login, and there's no telemetry. You import files you downloaded from your
  bank yourself.
- **Orre makes no network calls.** The only one planned is an optional exchange-rate
  download, which you'd have to turn on and which isn't built yet.
- **Imports never go through AI.** Reading a bank file is done by Orre alone, with no
  AI model and no network.

### AI access (MCP)

Orre can answer questions from an AI client such as Claude or Codex through its MCP
server. That server runs only when the AI client starts it, opens no network port, and
**gives a client nothing until you enable it** in Settings → Clients or with
`orre ai-access enable <name>`. You choose each client's level:

- **`aggregate`**: totals only (where every new client starts).
- **`pseudonymous`**: transactions, with merchants as tokens and accounts as labels.
  Amounts and dates are exact, so it isn't anonymous.
- **`standard`**: full access, including account and merchant names and bank
  descriptions.

An AI client can't change your data: it can only file suggestions, which wait in Orre's
inbox for you to approve, and save reports. Notes, share labels, private tags, merchant
addresses and attachments never go to an AI client at any level. Nor does bank text that
looks like it holds account details (a sort code, an account or card number of 8 or more
digits, or an IBAN): at every level, such a description is replaced with
`[withheld: account details]` and such a merchant name is left out. That's a check for
those patterns, so details written another way can get through. Remember that whatever a
client reads, it sends to its AI provider.

**To turn AI access off for every client**, use the **AI access** switch in Settings →
Clients, or run:

```sh
orre ai-access off    # every client is refused from its next call
orre ai-access on     # turn it back on
```

To see what each client read: Settings → Access log, or `orre ai-access log`.

Setting up a client, the levels in detail, and the access log:
[`docs/mcp-setup.md`](mcp-setup.md). What's protected and what isn't:
[`SECURITY.md`](../SECURITY.md).

## Exports

An export (Settings → Export…, or `orre export`) is a plain CSV or JSON file saved
where you choose. It's created readable only by you, but it isn't encrypted, and once
it's saved it's yours to look after. Notes and private tags are left out unless you ask
for them. The columns are described in [`docs/EXPORT.md`](EXPORT.md).
