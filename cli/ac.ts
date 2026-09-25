#!/usr/bin/env bun
/**
 * A Collection Head Office — Operator CLI (Phase A: READ-ONLY)
 * ============================================================
 * Owner: airdropia | Scope master: architect-agent
 *
 * Text-based operations for AI agents (pi-agent) and power users.
 * Opens the app's SQLite database in READ-ONLY mode — safe to run
 * while the app is open (WAL readers never block, never corrupt).
 *
 * WHY READ-ONLY: all business rules (sign conventions, validations,
 * side effects) live in the Rust command layer (src-tauri/src/).
 * Reimplementing writes here would duplicate logic and risk silent
 * ledger corruption. Writes ship in Phase B as `acollectionho.exe`
 * built by CI from the SAME Rust layer.
 *
 * Usage:
 *   bun cli/ac.ts [--json] [--db <path>] <command> [subcommand] [args]
 *
 * Commands:
 *   customers list                     all customers + computed outstanding
 *   customers khata <id|name>          one customer + full ledger
 *   customers net                      net udhar summary (green/red)
 *   agents list                        all agents + computed outstanding
 *   agents ledger <id|code|name>       agent ledger + running balances
 *   products list [--low-stock]        stock view
 *   sales recent [N]                   last N sales (default 10)
 *   db health                          integrity, counts, drift checks
 *   dashboard                          one-screen business snapshot
 *   version                            repo/app version info
 *
 * Sign conventions (MUST match src-tauri/src — do not change here):
 *   customer_payments: payment -> -amount | opening_debit -> +amount
 *                      adjustment -> +amount (stored signed)
 *   agent_ledger_entries outstanding:
 *       stock_sent.value - cash_received - stock_returned.value
 *       + SUM(-amount WHERE balance_adjustment)   [adjustment stored negated]
 *   sale_reported affects STOCK UNITS only, not money outstanding.
 *
 * Privacy: phone numbers are masked by default. NEVER paste real
 * customer names/phones into ecosystem-hq issues — mask or use IDs.
 */

import { Database } from "bun:sqlite";
import { existsSync, statSync } from "node:fs";
import { join } from "node:path";

// ---------------------------------------------------------------- args
const args = process.argv.slice(2);
function takeFlag(flag: string): boolean {
  const i = args.indexOf(flag);
  if (i >= 0) { args.splice(i, 1); return true; }
  return false;
}
const JSON_MODE = takeFlag("--json");
const MASK = !takeFlag("--no-mask");
let dbOverride: string | null = null;
{
  const i = args.indexOf("--db");
  if (i >= 0) { dbOverride = args[i + 1] ?? null; args.splice(i, 2); }
}

function defaultDbPath(): string {
  if (process.platform === "win32" && process.env.APPDATA) {
    return join(process.env.APPDATA, "com.airdropia.collectionheadoffice", "database.db");
  }
  // linux/mac dev fallback (not the shop machine, but handy for tests)
  return join(process.env.HOME ?? ".", ".local/share/com.airdropia.collectionheadoffice/database.db");
}

const DB_PATH = dbOverride ?? defaultDbPath();

if (!existsSync(DB_PATH)) {
  console.error(`ERROR: database not found at:\n  ${DB_PATH}`);
  console.error(`Use --db <path> to point at the real file, e.g.:`);
  console.error(`  bun cli/ac.ts --db "%APPDATA%\\com.airdropia.collectionheadoffice\\database.db" customers list`);
  process.exit(2);
}

let db: Database;
try {
  db = new Database(DB_PATH, { readonly: true });
  db.exec("PRAGMA busy_timeout = 3000");
} catch (e) {
  console.error(`ERROR: cannot open database read-only: ${e}`);
  process.exit(2);
}

// ------------------------------------------------------------- helpers
const rs = (n: number | null | undefined): string =>
  "Rs. " + Math.round(Number(n ?? 0)).toLocaleString("en-PK");

function maskPhone(p: string | null | undefined): string {
  if (!p) return "-";
  if (!MASK) return p;
  const digits = p.replace(/\D/g, "");
  if (digits.length < 4) return "***";
  return "***-***-" + digits.slice(-3);
}

function out(data: unknown, printFn: () => void) {
  if (JSON_MODE) console.log(JSON.stringify(data, null, 2));
  else printFn();
}

function die(msg: string): never {
  if (JSON_MODE) console.log(JSON.stringify({ error: msg }, null, 2));
  else console.error("ERROR: " + msg);
  process.exit(1);
}

// Customer outstanding, computed from ledger (source of truth).
// entry_type signs: payment -> -amount, opening_debit -> +amount,
// adjustment -> +amount (amount stored signed, may be negative).
const CUSTOMER_OUTSTANDING_SQL = `
  COALESCE(SUM(CASE entry_type
    WHEN 'payment'       THEN -amount
    WHEN 'opening_debit' THEN  amount
    WHEN 'adjustment'    THEN  amount
    ELSE 0 END), 0.0)`;

// Agent outstanding, mirrors get_agent_summary() in src-tauri/src/agents/mod.rs:
//   outstanding = stock_sent.value - cash_received - stock_returned.value
//                 + SUM(-amount WHERE balance_adjustment)
const AGENT_OUTSTANDING_SQL = `
  COALESCE(SUM(CASE WHEN entry_type = 'stock_sent'         THEN  amount ELSE 0 END), 0.0)
- COALESCE(SUM(CASE WHEN entry_type = 'cash_received'      THEN  amount ELSE 0 END), 0.0)
- COALESCE(SUM(CASE WHEN entry_type = 'stock_returned'     THEN  amount ELSE 0 END), 0.0)
+ COALESCE(SUM(CASE WHEN entry_type = 'balance_adjustment' THEN -amount ELSE 0 END), 0.0)`;

const AGENT_STOCK_UNITS_SQL = `
  COALESCE(SUM(CASE WHEN entry_type = 'stock_sent'    THEN qty ELSE 0 END)
-       SUM(CASE WHEN entry_type = 'stock_returned' THEN qty ELSE 0 END)
-       SUM(CASE WHEN entry_type = 'sale_reported'  THEN qty ELSE 0 END), 0)`;

function direction(n: number): "RECEIVABLE" | "PAYABLE" | "ZERO" {
  if (n > 0.004) return "RECEIVABLE"; // green: HO ko lena hai
  if (n < -0.004) return "PAYABLE";   // red:   HO ko dena hai
  return "ZERO";
}

function count_q(sql: string): number {
  try { return (db.query(sql).get() as any).c; } catch { return -1; }
}

// ------------------------------------------------------------ commands
function customersList() {
  const rows = db.query(`
    SELECT c.id, c.name, c.phone, c.segment, c.is_active,
           c.outstanding_balance AS stored_balance,
           (${CUSTOMER_OUTSTANDING_SQL}) AS computed_balance
    FROM customers c
    LEFT JOIN customer_payments p ON p.customer_id = c.id
    GROUP BY c.id ORDER BY computed_balance DESC`).all() as any[];

  out(rows.map(r => ({
    id: r.id, name: r.name, phone: maskPhone(r.phone), segment: r.segment,
    active: !!r.is_active, outstanding: r.computed_balance, direction: direction(r.computed_balance),
    stored_column: r.stored_balance, drift: Math.abs(r.computed_balance - (r.stored_balance ?? 0)) > 0.004,
  })), () => {
    console.log("CUSTOMERS (computed outstanding — green=RECEIVABLE lena hai, red=PAYABLE dena hai)\n");
    for (const r of rows) {
      const d = direction(r.computed_balance);
      const tag = d === "RECEIVABLE" ? "GREEN lena hai" : d === "PAYABLE" ? "RED dena hai" : "ZERO";
      const drift = Math.abs(r.computed_balance - (r.stored_balance ?? 0)) > 0.004 ? "  [!] stored-column drift" : "";
      console.log(`  #${r.id}  ${r.name}  (${maskPhone(r.phone)})  ${rs(r.computed_balance)}  [${tag}]${drift}`);
    }
    console.log(`\n  total: ${rows.length} customers`);
  });
}

function customersNet() {
  const rows = db.query(`
    SELECT c.id, c.name,
           (${CUSTOMER_OUTSTANDING_SQL}) AS bal
    FROM customers c
    LEFT JOIN customer_payments p ON p.customer_id = c.id
    GROUP BY c.id`).all() as any[];

  const receivable = rows.filter(r => r.bal > 0.004).sort((a, b) => b.bal - a.bal);
  const payable = rows.filter(r => r.bal < -0.004);
  const net = rows.reduce((s, r) => s + r.bal, 0);

  out({ net_outstanding: net, direction: direction(net), we_owe_count: payable.length,
        receivables: receivable.map(r => ({ id: r.id, name: r.name, amount: r.bal })),
        payables: payable.map(r => ({ id: r.id, name: r.name, amount: r.bal })) },
    () => {
      console.log("CUSTOMER KHATA — NET SUMMARY\n");
      console.log(`  Net outstanding : ${rs(net)}  [${direction(net) === "RECEIVABLE" ? "GREEN lena hai" : direction(net) === "PAYABLE" ? "RED dena hai" : "ZERO"}]`);
      console.log(`  We owe (count)  : ${payable.length}`);
      console.log(`\n  Receivables (customer -> HO):`);
      for (const r of receivable) console.log(`    #${r.id}  ${r.name}  ${rs(r.bal)}`);
      if (payable.length) {
        console.log(`\n  Payables (HO -> customer):`);
        for (const r of payable) console.log(`    #${r.id}  ${r.name}  ${rs(r.bal)}`);
      }
    });
}

function customersKhata(key: string) {
  const isId = /^\d+$/.test(key);
  const row = (isId
    ? db.query(`SELECT id, name, phone, location, notes, segment, is_active FROM customers WHERE id = ? LIMIT 2`).all(Number(key))
    : db.query(`SELECT id, name, phone, location, notes, segment, is_active FROM customers WHERE LOWER(name) LIKE '%' || LOWER(?) || '%' ORDER BY id LIMIT 2`).all(key)) as any[];
  if (row.length === 0) die(`customer not found: ${key}`);
  if (row.length > 1) die(`ambiguous match for "${key}" — ${row.map(r => `#${r.id} ${r.name}${r.phone ? ` (${maskPhone(r.phone)})` : ""}`).join(" | ")} — use id`);
  const c = row[0];

  const entries = db.query(`
    SELECT id, amount, entry_type, payment_date, notes, sale_id
    FROM customer_payments WHERE customer_id = ? ORDER BY payment_date, id`).all(c.id) as any[];

  let running = 0;
  const ledger = entries.map(e => {
    const delta = e.entry_type === "payment" ? -e.amount
      : e.entry_type === "opening_debit" ? e.amount
      : e.entry_type === "adjustment" ? e.amount : 0;
    running += delta;
    return { id: e.id, date: e.payment_date, type: e.entry_type, amount: e.amount,
             effect: delta, balance_after: running, notes: e.notes ?? "", sale_id: e.sale_id };
  });

  out({ customer: { id: c.id, name: c.name, phone: maskPhone(c.phone), location: c.location,
                    segment: c.segment, active: !!c.is_active },
        outstanding: running, direction: direction(running), entries: ledger },
    () => {
      console.log(`KHATA — ${c.name} (#${c.id})  ${maskPhone(c.phone)}  ${c.location ?? ""}\n`);
      console.log(`  ${"date".padEnd(11)} ${"type".padEnd(14)} ${"amount".padStart(12)} ${"effect".padStart(12)} ${"balance_after".padStart(14)}  notes`);
      for (const e of ledger) {
        console.log(`  ${String(e.date).padEnd(11)} ${e.type.padEnd(14)} ${rs(e.amount).padStart(12)} ${rs(e.effect).padStart(12)} ${rs(e.balance_after).padStart(14)}  ${e.notes}`);
      }
      console.log(`\n  OUTSTANDING: ${rs(running)}  [${direction(running) === "RECEIVABLE" ? "GREEN — lena hai" : direction(running) === "PAYABLE" ? "RED — dena hai" : "ZERO"}]`);
    });
}

function agentsList() {
  const rows = db.query(`
    SELECT a.id, a.agent_code, a.name, a.phone, a.city, a.is_active,
           (${AGENT_OUTSTANDING_SQL}) AS outstanding,
           (${AGENT_STOCK_UNITS_SQL}) AS stock_units
    FROM agents a
    LEFT JOIN agent_ledger_entries e ON e.agent_id = a.id
    GROUP BY a.id ORDER BY outstanding DESC`).all() as any[];

  out(rows.map(r => ({
    id: r.id, code: r.agent_code, name: r.name, phone: maskPhone(r.phone), city: r.city,
    active: !!r.is_active, outstanding: r.outstanding, direction: direction(r.outstanding),
    stock_units: r.stock_units,
  })), () => {
    console.log("AGENTS (outstanding — green=RECEIVABLE lena hai, red=PAYABLE dena hai)\n");
    for (const r of rows) {
      const d = direction(r.outstanding);
      const tag = d === "RECEIVABLE" ? "GREEN lena hai" : d === "PAYABLE" ? "RED dena hai" : "ZERO";
      console.log(`  #${r.id}  ${r.name} [${r.agent_code}]  ${rs(r.outstanding)}  [${tag}]  stock: ${r.stock_units} pcs`);
    }
    console.log(`\n  total: ${rows.length} agents`);
  });
}

function agentsLedger(key: string) {
  const isId = /^\d+$/.test(key);
  const rows = (isId
    ? db.query(`SELECT id, agent_code, name FROM agents WHERE id = ? LIMIT 2`).all(Number(key))
    : db.query(`SELECT id, agent_code, name FROM agents WHERE LOWER(agent_code) = LOWER(?) OR LOWER(name) LIKE '%' || LOWER(?) || '%' ORDER BY id LIMIT 2`).all(key, key)) as any[];
  if (rows.length === 0) die(`agent not found: ${key}`);
  if (rows.length > 1) die(`ambiguous match for "${key}" — ${rows.map(r => `#${r.id} ${r.name} [${r.agent_code}]`).join(" | ")} — use id or code`);
  const a = rows[0];

  const entries = db.query(`
    SELECT id, product_id, entry_type, qty, unit_price, amount, reference_code, notes, entry_date
    FROM agent_ledger_entries WHERE agent_id = ? ORDER BY entry_date, id`).all(a.id) as any[];

  let rsRunning = 0, unitRunning = 0;
  const ledger = entries.map(e => {
    let money = 0, units = 0;
    if (e.entry_type === "stock_sent") { money = e.amount; units = e.qty; }
    else if (e.entry_type === "stock_returned") { money = -e.amount; units = -e.qty; }
    else if (e.entry_type === "cash_received") { money = -e.amount; }
    else if (e.entry_type === "balance_adjustment") { money = -e.amount; } // stored negated
    else if (e.entry_type === "sale_reported") { units = -e.qty; }         // units only, no money
    rsRunning += money; unitRunning += units;
    return { id: e.id, date: e.entry_date, type: e.entry_type, qty: e.qty, amount: e.amount,
             money_effect: money, units_effect: units,
             rs_after: rsRunning, units_after: unitRunning,
             product_id: e.product_id, reference: e.reference_code ?? "", notes: e.notes ?? "" };
  });

  out({ agent: { id: a.id, code: a.agent_code, name: a.name }, outstanding: rsRunning,
        direction: direction(rsRunning), stock_units: unitRunning, entries: ledger },
    () => {
      console.log(`LEDGER — ${a.name} [${a.agent_code}] (#${a.id})\n`);
      console.log(`  ${"date".padEnd(11)} ${"type".padEnd(18)} ${"qty".padStart(4)} ${"amount".padStart(12)} ${"rs_after".padStart(12)} ${"units_after".padStart(7)}  notes`);
      for (const e of ledger) {
        console.log(`  ${String(e.date).padEnd(11)} ${e.type.padEnd(18)} ${String(e.qty).padStart(4)} ${rs(e.amount).padStart(12)} ${rs(e.rs_after).padStart(12)} ${String(e.units_after).padStart(7)}  ${e.notes}`);
      }
      console.log(`\n  OUTSTANDING: ${rs(rsRunning)}  [${direction(rsRunning) === "RECEIVABLE" ? "GREEN — lena hai" : direction(rsRunning) === "PAYABLE" ? "RED — dena hai" : "ZERO"}]   STOCK: ${unitRunning} pcs`);
    });
}

function productsList(lowStockOnly: boolean) {
  const rows = db.query(`
    SELECT id, sku, product_code, name, category, status,
           COALESCE(qty_in_head_office, stock_quantity, 0) AS qho,
           COALESCE(qty_with_agents, 0) AS qwa,
           COALESCE(qty_sold, 0) AS qs,
           COALESCE(profit_status, 'in_head_office') AS pstatus
    FROM products ${lowStockOnly ? "WHERE (COALESCE(qty_in_head_office, stock_quantity, 0) + COALESCE(qty_with_agents,0)) <= 2 AND COALESCE(profit_status,'in_head_office') != 'sold_out'" : ""}
    ORDER BY id`).all() as any[];

  out(rows, () => {
    console.log(lowStockOnly ? "PRODUCTS — LOW STOCK\n" : "PRODUCTS\n");
    for (const r of rows) {
      console.log(`  #${r.id}  ${String(r.product_code || r.sku).padEnd(14)} ${String(r.name).slice(0, 34).padEnd(34)} HO:${String(r.qho).padStart(3)} AG:${String(r.qwa).padStart(3)} SOLD:${String(r.qs).padStart(3)}  ${r.pstatus}`);
    }
    console.log(`\n  total: ${rows.length}`);
  });
}

function salesRecent(n: number) {
  const rows = db.query(`
    SELECT s.id, s.sale_date, s.sale_channel, s.qty, s.total_sale_amount, s.amount_paid,
           s.balance, s.customer_name, s.customer_id, s.agent_id, s.reversed, p.name AS product
    FROM sales s LEFT JOIN products p ON p.id = s.product_id
    WHERE s.reversed = 0 ORDER BY s.sale_date DESC, s.id DESC LIMIT ?`).all(n) as any[];

  const reversed = db.query(`SELECT COUNT(*) AS c FROM sales WHERE reversed = 1`).get() as any;

  out({ sales: rows, reversed_total: reversed?.c ?? 0 }, () => {
    console.log(`SALES — last ${n} (reversed excluded; ${reversed?.c ?? 0} reversed in DB)\n`);
    for (const r of rows) {
      const bal = r.balance > 0.004 ? `  [udhar ${rs(r.balance)}]` : "";
      console.log(`  #${r.id}  ${r.sale_date}  ${String(r.product ?? "?").slice(0, 26).padEnd(26)} ${rs(r.total_sale_amount)}${bal}`);
    }
  });
}

function dbHealth() {
  const integrity = (db.query(`PRAGMA integrity_check`).get() as any)?.integrity_check ?? "unknown";
  const count = (t: string): number => (db.query(`SELECT COUNT(*) AS c FROM ${t}`).get() as any).c;

  const custDrift = db.query(`
    SELECT c.id, c.name, c.outstanding_balance AS stored,
           (${CUSTOMER_OUTSTANDING_SQL}) AS computed
    FROM customers c
    LEFT JOIN customer_payments p ON p.customer_id = c.id
    GROUP BY c.id
    HAVING ABS(computed - COALESCE(c.outstanding_balance, 0)) > 0.004`).all() as any[];

  const orphanPayments = count_q(`SELECT COUNT(*) AS c FROM customer_payments p LEFT JOIN customers c ON c.id = p.customer_id WHERE c.id IS NULL`);
  const orphanLedger = count_q(`SELECT COUNT(*) AS c FROM agent_ledger_entries e LEFT JOIN agents a ON a.id = e.agent_id WHERE a.id IS NULL`);

  let walBytes = 0;
  try { walBytes = statSync(DB_PATH + "-wal").size; } catch {}

  const staleStock = db.query(`
    SELECT COUNT(*) AS c FROM products
    WHERE COALESCE(profit_status,'in_head_office') != 'sold_out'
      AND (COALESCE(qty_in_head_office, stock_quantity, 0) + COALESCE(qty_with_agents,0)) <= 0`).get() as any;

  const result = {
    db_path: DB_PATH,
    integrity_check: integrity,
    wal_bytes: walBytes,
    wal_note: walBytes > 4_000_000 ? "WAL file large — app checkpoint will shrink it on next clean close" : "ok",
    counts: {
      customers: count("customers"), customer_payments: count("customer_payments"),
      agents: count("agents"), agent_ledger_entries: count("agent_ledger_entries"),
      products: count("products"), sales: count("sales"),
    },
    customer_balance_drift_rows: custDrift.map(r => ({ id: r.id, name: r.name, stored: r.stored, computed: r.computed })),
    orphan_payments: orphanPayments, orphan_agent_ledger: orphanLedger,
    zero_stock_but_active_products: staleStock?.c ?? 0,
  };
  out(result, () => {
    console.log("DB HEALTH\n");
    console.log(`  path        : ${DB_PATH}`);
    console.log(`  integrity   : ${integrity}`);
    console.log(`  wal size    : ${(walBytes / 1024).toFixed(1)} KB`);
    console.log(`  counts      : customers=${result.counts.customers} payments=${result.counts.customer_payments} agents=${result.counts.agents} ledger=${result.counts.agent_ledger_entries} products=${result.counts.products} sales=${result.counts.sales}`);
    console.log(`  bal drift   : ${custDrift.length === 0 ? "none (stored == computed)" : custDrift.length + " rows — " + JSON.stringify(result.customer_balance_drift_rows)}`);
    console.log(`  orphans     : payments=${orphanPayments} agent_ledger=${orphanLedger}`);
    console.log(`  zero-stock-but-active: ${result.zero_stock_but_active_products}`);
  });
}

function dashboard() {
  const custNet = (db.query(`
    SELECT (${CUSTOMER_OUTSTANDING_SQL}) AS net FROM customer_payments`).get() as any)?.net ?? 0;
  const agentNet = (db.query(`
    SELECT ${AGENT_OUTSTANDING_SQL} AS net FROM agent_ledger_entries`).get() as any)?.net ?? 0;
  const lowStock = (db.query(`
    SELECT COUNT(*) AS c FROM products
    WHERE COALESCE(profit_status,'in_head_office') != 'sold_out'
      AND (COALESCE(qty_in_head_office, stock_quantity, 0) + COALESCE(qty_with_agents,0)) <= 2`).get() as any)?.c ?? 0;
  const soldOut = (db.query(`SELECT COUNT(*) AS c FROM products WHERE COALESCE(profit_status,'x') = 'sold_out'`).get() as any)?.c ?? 0;
  const recentSales = (db.query(`SELECT COUNT(*) AS c FROM sales WHERE reversed = 0 AND sale_date >= date('now','-30 days')`).get() as any)?.c ?? 0;

  out({ customers_net: custNet, customers_direction: direction(custNet),
        agents_net: agentNet, agents_direction: direction(agentNet),
        low_stock_count: lowStock, sold_out_count: soldOut, sales_last_30d: recentSales },
    () => {
      console.log("DASHBOARD SNAPSHOT\n");
      console.log(`  Customers net : ${rs(custNet)}  [${direction(custNet) === "RECEIVABLE" ? "GREEN lena hai" : direction(custNet) === "PAYABLE" ? "RED dena hai" : "ZERO"}]`);
      console.log(`  Agents net    : ${rs(agentNet)}  [${direction(agentNet) === "RECEIVABLE" ? "GREEN lena hai" : direction(agentNet) === "PAYABLE" ? "RED dena hai" : "ZERO"}]`);
      console.log(`  Low stock     : ${lowStock} products (<=2 pcs, not sold out)`);
      console.log(`  Sold out      : ${soldOut} products`);
      console.log(`  Sales 30d     : ${recentSales}`);
    });
}

function version() {
  let app = "unknown";
  try {
    const conf = JSON.parse(require("node:fs").readFileSync(join(process.cwd(), "src-tauri", "tauri.conf.json"), "utf8"));
    app = conf?.version ?? "unknown";
  } catch {}
  out({ cli_phase: "A (read-only)", app_version_in_repo: app, db_path: DB_PATH },
    () => {
      console.log("VERSION\n");
      console.log(`  CLI phase          : A (read-only) — writes ship in Phase B (acollectionho.exe, CI-built)`);
      console.log(`  app version (repo) : ${app}`);
      console.log(`  db path            : ${DB_PATH}`);
    });
}

// -------------------------------------------------------------- router
const [cmd, sub, ...rest] = args;

try {
  switch (cmd) {
    case "customers":
      if (sub === "list") customersList();
      else if (sub === "net") customersNet();
      else if (sub === "khata") customersKhata(rest[0] ?? die("usage: customers khata <id|name>"));
      else die("usage: customers list | net | khata <id|name>");
      break;
    case "agents":
      if (sub === "list") agentsList();
      else if (sub === "ledger") agentsLedger(rest[0] ?? die("usage: agents ledger <id|code|name>"));
      else die("usage: agents list | ledger <id|code|name>");
      break;
    case "products":
      if (sub === "list") productsList(rest.includes("--low-stock"));
      else die("usage: products list [--low-stock]");
      break;
    case "sales":
      if (sub === "recent") salesRecent(Number(rest[0]) || 10);
      else die("usage: sales recent [N]");
      break;
    case "db":
      if (sub === "health") dbHealth();
      else die("usage: db health");
      break;
    case "dashboard": dashboard(); break;
    case "version": version(); break;
    default:
      console.error(`A Collection Head Office — Operator CLI (Phase A, read-only)
DB: ${DB_PATH}

  bun cli/ac.ts [--json] [--db <path>] <command>

  customers list | net | khata <id|name>
  agents list | ledger <id|code|name>
  products list [--low-stock]
  sales recent [N]
  db health
  dashboard
  version`);
      process.exit(cmd ? 1 : 0);
  }
} finally {
  db.close();
}
