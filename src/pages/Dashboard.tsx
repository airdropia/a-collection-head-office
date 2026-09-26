import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '../stores/store'
import {
  Package, Layers, Users,
  ShoppingCart, RefreshCw, BookOpen, IndianRupee
} from 'lucide-react'
import { fmtMoney } from '../utils/format'

export default function Dashboard() {
  const { products, setCurrentTab, fetchProducts, fetchCustomers, customers } = useAppStore()
  const [loading, setLoading] = useState(true)
  // v0.39.0: Recent Sales panel (pi suggestion #3) — live activity signal
  const [recentSales, setRecentSales] = useState<any[]>([])

  const loadRecentSales = () => {
    invoke('get_recent_sales', { limit: 10 })
      .then((rows: any) => setRecentSales(rows ?? []))
      .catch(() => setRecentSales([]))
  }

  useEffect(() => {
    fetchProducts()
    // v0.34.0: load customers so the customer-khata net line can show on dashboard
    fetchCustomers()
    // v0.39.0: recent sales for the live activity panel
    loadRecentSales()
    setLoading(false)
  }, [])

  const refresh = async () => {
    setLoading(true)
    try {
      await Promise.all([fetchProducts(), fetchCustomers()])
      loadRecentSales()
    } finally {
      setLoading(false)
    }
  }

  // Compute stats from products
  const totalHOStock = products.reduce((s, p) => s + (p.qty_in_head_office ?? p.stock_quantity), 0)
  const totalSold = products.reduce((s, p) => s + (p.qty_sold ?? 0), 0)

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight text-white font-display">Dashboard</h1>
          <p className="text-sm text-gray-400 mt-1">Stock, customer khata, and sales at a glance.</p>
        </div>
        <button onClick={refresh} disabled={loading}
          className="flex items-center space-x-1 px-3 py-1.5 bg-slate-800 hover:bg-slate-700 text-gray-200 rounded-lg text-xs disabled:opacity-50">
          <RefreshCw size={12} className={loading ? 'animate-spin' : ''} />
          <span>Refresh</span>
        </button>
      </div>

      {/* === TOP STATS CARDS === */}
      <div className="grid grid-cols-3 gap-3">
        <button onClick={() => setCurrentTab('catalog')}
          className="glass-card p-4 text-left hover:border-violet-500/50 transition-colors">
          <Package size={16} className="text-violet-400 mb-1" />
          <div className="text-[10px] text-gray-500 uppercase">Active Products</div>
          <div className="text-xl font-bold text-white">{products.filter(p => p.status === 'active').length}</div>
        </button>

        <div className="glass-card p-4">
          <Layers size={16} className="text-emerald-400 mb-1" />
          <div className="text-[10px] text-gray-500 uppercase">Stock in HO</div>
          <div className="text-xl font-bold text-white">{totalHOStock} <span className="text-xs text-gray-500">units</span></div>
        </div>

        <div className="glass-card p-4">
          <ShoppingCart size={16} className="text-emerald-400 mb-1" />
          <div className="text-[10px] text-gray-500 uppercase">Sold (all-time)</div>
          <div className="text-xl font-bold text-white">{totalSold} <span className="text-xs text-gray-500">units</span></div>
        </div>
      </div>

      {/* Customer (khata) net outstanding — standalone card.
          Red/green direction: positive = lene hain, negative = dene hain. */}
      <div className="glass-card p-5">
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-semibold text-white flex items-center">
            <Users size={14} className="mr-2 text-amber-400" />
            Customer Khata (net)
          </h2>
          <button onClick={() => setCurrentTab('customers')}
            className="text-[10px] text-violet-400 hover:text-violet-300">View All →</button>
        </div>
        {(() => {
          const custNet = customers.reduce((s, c) => s + (c.outstanding_balance || 0), 0)
          if (custNet === 0) return (
            <p className="text-xs text-gray-500 py-6 text-center">All customer khatas settled — net zero.</p>
          )
          return (
            <button
              onClick={() => setCurrentTab('customers')}
              className={`w-full p-3 rounded-lg border flex items-center justify-between transition-colors ${
                custNet > 0 ? 'bg-emerald-900/10 border-emerald-700/30 hover:border-emerald-600/50'
                            : 'bg-red-900/10 border-red-700/30 hover:border-red-600/50'
              }`}>
              <div className="flex items-center gap-2">
                <Users size={14} className={custNet > 0 ? 'text-emerald-400' : 'text-red-400'} />
                <div className="text-left">
                  <div className={`text-xs font-semibold ${custNet > 0 ? 'text-emerald-300' : 'text-red-300'}`}>
                    {custNet > 0 ? 'HO ko lena hai' : 'HO ko dena hai'}
                  </div>
                  <div className="text-[10px] text-gray-500">net across {customers.length} customers</div>
                </div>
              </div>
              <div className={`text-sm font-bold ${custNet > 0 ? 'text-emerald-400' : 'text-red-400'}`}>
                {fmtMoney(Math.abs(custNet))}
              </div>
            </button>
          )
        })()}
      </div>

      {/* === RECENT SALES (last 10) — v0.39.0, pi suggestion #3 === */}
      <div className="glass-card p-5">
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-semibold text-white flex items-center">
            <ShoppingCart size={14} className="mr-2 text-emerald-400" />
            Recent Sales (last 10)
          </h2>
          <button onClick={() => setCurrentTab('reports')}
            className="text-[10px] text-violet-400 hover:text-violet-300">Reports →</button>
        </div>
        {recentSales.length === 0 ? (
          <p className="text-xs text-gray-500 py-6 text-center">No sales recorded yet — record one from the Catalog page.</p>
        ) : (
          <div className="space-y-1">
            {recentSales.map((s) => (
              <div key={s.id}
                className={`flex items-center justify-between px-3 py-2 rounded-lg text-xs ${
                  s.reversed ? 'bg-slate-800/40 text-gray-500' : 'bg-slate-800/60 text-gray-300'
                }`}>
                <div className="flex items-center gap-2 min-w-0">
                  <span className="text-gray-500 font-mono shrink-0">{String(s.sale_date).slice(0, 10)}</span>
                  <span className="truncate">{s.product ?? '?'}</span>
                  <span className="text-gray-500 shrink-0">x{s.qty}</span>
                  {s.reversed ? <span className="text-[9px] uppercase text-gray-600 shrink-0">undone</span> : null}
                </div>
                <div className="flex items-center gap-2 shrink-0">
                  {s.customer_name ? <span className="text-gray-500">{s.customer_name}</span> : null}
                  <span className={`font-semibold ${s.reversed ? 'text-gray-600 line-through' : 'text-white'}`}>
                    {fmtMoney(s.total_sale_amount)}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* === QUICK ACTIONS === */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
        <button onClick={() => setCurrentTab('catalog')}
          className="glass-card p-4 hover:border-violet-500/50 transition-colors text-center">
          <Package size={20} className="mx-auto text-violet-400 mb-1" />
          <div className="text-xs font-semibold text-white">Catalog</div>
          <div className="text-[10px] text-gray-500">{products.length} products</div>
        </button>
        <button onClick={() => setCurrentTab('customers')}
          className="glass-card p-4 hover:border-violet-500/50 transition-colors text-center">
          <BookOpen size={20} className="mx-auto text-amber-400 mb-1" />
          <div className="text-xs font-semibold text-white">Customers</div>
          <div className="text-[10px] text-gray-500">Khata, udhar, advance</div>
        </button>
        <button onClick={() => setCurrentTab('reports')}
          className="glass-card p-4 hover:border-violet-500/50 transition-colors text-center">
          <IndianRupee size={20} className="mx-auto text-emerald-400 mb-1" />
          <div className="text-xs font-semibold text-white">Sales & Reports</div>
          <div className="text-[10px] text-gray-500">Hisab and profit</div>
        </button>
      </div>
    </div>
  )
}
