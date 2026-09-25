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
bun cli/ac.ts agents list
bun cli/ac.ts --json dashboard
```

No Node.js, no Python, no Rust needed. Only Bun.

## Commands

| Command | Kya deta hai |
|---|---|
| `customers list` | sab customers + computed outstanding (GREEN/RED) |
| `customers net` | net udhar summary + "we owe" count |
| `customers khata <id\|name>` | ek customer ki poori ledger + running balance |
| `agents list` | sab agents + outstanding + stock units |
| `agents ledger <id\|code\|name>` | agent ledger + `rs_after` / `units_after` running columns |
| `products list [--low-stock]` | stock view (HO/AG/SOLD qty) |
| `sales recent [N]` | aakhri N sales (reversed excluded) |
| `db health` | integrity, WAL size, counts, **balance drift check**, orphans |
| `dashboard` | one-screen snapshot (JSON mode recommended) |
| `version` | CLI phase + repo app version + DB path |

**Rules (agent ops ke liye):**

- Low-stock rule = `(qty_in_head_office + qty_with_agents) <= 2` AND `profit_status != 'sold_out'`
  (v0.27+ split columns — app ke legacy `stock_quantity`-based rule se different,
  jaan boojh kar; v0.35.0 mein app align hoga)
- Agent codes real DB mein `AGT-<timestamp>` format mein hain (jaise `AGT-1783416246276650400`)
  — mock docs ids (`AG-001`) real DB mein nahi milte
- `customers khata <name>` ambiguous ho sakta hai (same naam ke customers) —
  error ab id + naam + masked phone dikhata hai

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

## Phase B (writes) — roadmap

Writes (`add-udhar`, `return-cash`, etc.) **is CLI mein NAHI hain** jaan boojh
kar. Business rules (sign conventions, validations, side effects) Rust command
layer mein hain — duplicate karne se ledger corrupt hota hai. Phase B mein
`acollectionho.exe` (Rust, CI-built, releases se download) aayega jo **wahi
command layer** use karta hai. Tab tak koi bhi write sirf GUI se ya owner ke
through.

## Sign conventions (CRITICAL — change mat karna)

Yeh formulas `src-tauri/src/agents/mod.rs` ke `get_agent_summary()` se liye
gayi hain:

- Agent outstanding = `stock_sent.value - cash_received - stock_returned.value
  + SUM(-amount WHERE balance_adjustment)` (adjustment DB mein negated hota hai)
- `sale_reported` = sirf stock units, money outstanding pe asar nahi
- Customer: `payment → -amount`, `opening_debit → +amount`,
  `adjustment → +amount` (stored signed)
