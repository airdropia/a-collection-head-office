# A Collection Head Office — Operator CLI (Phase A: READ-ONLY)

Text-based operator interface for the Head Office app. Built for AI agents
(pi-agent) and terminal users. **Zero dependencies** — only Bun (built-in
`bun:sqlite`).

## Quick start (shop machine)

```bash
# one-time: get the repo (pi-agent already has git + gh)
git clone --depth 1 https://github.com/airdropia/a-collection-head-office.git
cd a-collection-head-office

# run (auto-finds DB at %APPDATA%\com.airdropia.collectionheadoffice\database.db)
bun cli/ac.ts customers net
bun cli/ac.ts --json dashboard
```

No Node.js, no Python, no Rust needed. Only Bun.

## Commands

| Command | Kya deta hai |
|---|---|
| `customers list` | sab customers + computed outstanding (GREEN/RED) |
| `customers net` | net udhar summary + "we owe" count |
| `customers khata <id\|name>` | ek customer ki poori ledger + running balance |
| `products list [--low-stock]` | stock view (HO/AG/SOLD qty) |
| `sales recent [N]` | aakhri N sales (reversed excluded) |
| `db health` | integrity, WAL size, counts, **balance drift check**, orphans |
| `dashboard` | one-screen snapshot (JSON mode recommended) |
| `version` | CLI phase + repo app version + DB path |

**Rules (agent ops ke liye):**

- Low-stock rule = `(qty_in_head_office + qty_with_agents) <= 2` AND `profit_status != 'sold_out'`
  (split columns; app bhi isi rule par aligned hai v0.35.0+)
- `customers khata <name>` ambiguous ho sakta hai (same naam ke customers) —
  error id + naam + masked phone dikhata hai
- Agents feature v0.36.0 mein REMOVE ho gaya — `agents`/`agent-cash` commands
  ab exist nahi karte. Purana agent data tables mein archived hai (read via
  `--db` custom queries only).

Global flags:

- `--json` — machine-readable output (agents ke liye recommended)
- `--db <path>` — custom DB path (default: `%APPDATA%\com.airdropia.collectionheadoffice\database.db`)
- `--no-mask` — phone masking off (default ON — privacy)

## Safety model

- **READ-ONLY** — DB read-only mode mein khulti hai. App khuli ho tab bhi
  chalao (WAL readers never block).
- Phone numbers masked. **Kabhi bhi asli customer names/phones ecosystem-hq
  issues mein paste na karein.**
- Outstanding values **ledger se compute** hoti hain (source of truth) —
  `db health` stored columns se drift bhi check karta hai.

## Phase B (v0.35.0) — writes via acollectionho.exe

Writes ka sanctioned path ab **`acollectionho.exe`** hai (Rust, isi repo ke
CI release se download hota hai — releases page dekho). Yeh binary **wahi
business logic** use karta hai jo GUI app use karta hai (extracted *_impl
functions) — koi duplication nahi, sign conventions guaranteed.

```text
acollectionho pay-customer <customer_id> <amount> [--notes N] [--sale ID]
acollectionho manual-entry <customer_id> <opening_debit|adjustment> <amount> [--notes N] [--date D]
acollectionho customer-add --name N [--phone P] [--location L]
acollectionho customer-edit <id> [--name N] [--phone P] [--location L]
acollectionho db-drift      # stored vs canonical ledger report
acollectionho db-fix        # outstanding caches rewrite (ledger untouched)
```

(v0.36.0: `agent-cash` command removed with the Agents feature.)

Safety: GUI app band rakhein during writes. `db health` (yeh CLI) ab
**canonical** drift check karta hai — v0.35.0 se app startup pe bhi auto-heal
chalta hai.

## Sign conventions (CRITICAL — change mat karna)

**Customer (v0.35.0 canonical)** = `SUM(sales.balance WHERE reversed=0)`
(udhar sale debts) `- SUM(payments)` `+ SUM(opening_debit)` `+
SUM(adjustment signed)`. Pre-v0.29 payment rows (entry_type NULL) = payment.
`customers.outstanding_balance` = is value ka **cache** — v0.35.0 se app
startup pe auto-heal hota hai. (Purana payments-only formula ADHOORA tha —
v0.26.x era "drift" false-positive isi wajah se tha.)
