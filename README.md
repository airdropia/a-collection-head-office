# A Collection Head Office

Tauri 2 desktop app (React + TypeScript + Rust/SQLite) — cloth business head
office management: catalog, customers/khata, sales, reports.

## For external agents operating this app

Read `AGENTS.md` first (local machine policy: edit-only, builds via GitHub
Actions). Key facts:

- **Stack**: Vite + React 18 + Tailwind (frontend), Tauri 2 + rusqlite (backend)
- **Version bump = 4 files**: `package.json`, `src-tauri/tauri.conf.json`,
  `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` (first package entry).
  Release workflow fails fast on tag/version mismatch.
- **Release flow**: bump versions → commit → tag `vX.Y.Z` → push tag →
  `Release Windows Build` workflow builds + publishes GitHub Release.
- **CI**: `ci.yml` runs frontend build + Rust checks on push/PR/dispatch.
- **DB**: SQLite at `%APPDATA%/com.airdropia.collectionheadoffice/database.db`
  (WAL mode). App must be closed for direct DB work; copy db+wal+shm first.
- **Balance sign convention**: `customers.outstanding_balance > 0` = customer
  owes HO (udhar, green). `< 0` = HO owes customer (advance/maal wapas, red,
  since v0.34.0). Ledger invariants: `opening_debit` must be positive,
  `adjustment` may be signed, `payment` rows only via `record_customer_payment`.
- **Agent-friendly UI convention (v0.34.0+)**: interactive elements keep
  meaningful visible text (button titles, badges like `Udhar:` / `Dene hain:`)
  so UIA/accessibility-tree tools can locate and drive them.

## Features

- **Dashboard** — Stock overview, customer khata net (red/green), quick actions
- **Catalog** — Product master, profit columns, SOLD badges, catalog publishing
- **Customers** — Profiles, khata ledger (udhar/advance both directions), payments
- **Sales** — Sale recording with stock sync (via Catalog), undo, sold-item reactivation
- **Inventory** — Stock tracking, low stock alerts, best sellers
- **Reports** — Sales / inventory / customer reports
- **Automation** — Scheduled daily backup (DB + full ZIP)
- **Settings** — Backup path, catalog repo/brand/WhatsApp publish config

> v0.36.0–v0.38.0 grand cleanup: Agents system, AI Workspace/Assistant,
> Share Center, Purchase Trips, and the legacy cart/order flow were REMOVED
> (owner verdicts; pi usage audit 2026-09-26: all zero-usage features).
> Accounting data was preserved — historical agent sales remain undoable,
> dead tables are dropped automatically on app start.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Frontend | React 18, TypeScript, Vite 5, TailwindCSS 3, Zustand |
| Desktop | Tauri 2 |
| Backend | Rust (tokio, rusqlite, reqwest, image, csv) |
| Database | SQLite (embedded, local-first) |

## Development (owner only — CI does the builds)

```
npm install
npm install && npm run tauri dev    # local dev loop
bun install && bunx tsc --noEmit    # fast typecheck only (local policy)
```

Local machine policy (AGENTS.md): no builds/tests locally — GitHub Actions
owns CI + Release.

## GitHub Actions

- **CI** — TypeScript checks + Vite build + Rust check on push to master
- **Release Windows Build** — MSI/EXE on tag push (`v*`), creates GitHub Release

## License

MIT
> Note to humans: for the exact upstream README content (full history), see
> the `xpunjabi/a-collection-head-office` repository. This fork's README is
> agent-operating-focused per v0.34.0.
