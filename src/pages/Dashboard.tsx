import { useEffect, useState } from 'react'
import { useAppStore } from '../stores/store'
import { invoke } from '@tauri-apps/api/core'
import {
  Package, Layers, AlertTriangle, Users,
  ShoppingCart, RefreshCw, Sparkles
} from 'lucide-react'
import { Product } from '../stores/store'
import { fmtMoney } from '../utils/format'

interface ShareLog {
  id: number
  product_id: number | null
  platform: string
  share_angle: string
  caption_text: string
  shared_by: string
  shared_at: string
  notes: string
  product_name: string
}

export default function Dashboard() {
  const { products, setCurrentTab, fetchProducts, fetchCustomers, customers } = useAppStore()
  const [shareLogs, setShareLogs] = useState<ShareLog[]>([])
  const [staleProducts, setStaleProducts] = useState<Product[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    fetchProducts()
    // v0.34.0: load customers so the customer-khata net line can show on dashboard
    fetchCustomers()
    loadProfitModeData()
  }, [])

  const loadProfitModeData = async () => {
    setLoading(true)
    try {
      const [logData, staleData] = await Promise.all([
        invoke<ShareLog[]>('get_share_logs', { limit: 10 }).catch(() => []),
        invoke<Product[]>('get_stale_products', { days: 7 }).catch(() => []),
      ])
      setShareLogs(logData)
      setStaleProducts(staleData)
    } catch (err) {
      console.error('Failed to load profit-mode dashboard data:', err)
    } finally {
      setLoading(false)
    }
  }

  // Compute stats from products
  const totalHOStock = products.reduce((s, p) => s + (p.qty_in_head_office ?? p.stock_quantity), 0)
  const totalSold = products.reduce((s, p) => s + (p.qty_sold ?? 0), 0)

  const fmtDate = (iso: string) => new Date(iso).toLocaleDateString('en-PK', { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' })

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight text-white font-display">Dashboard</h1>
          <p className="text-sm text-gray-400 mt-1">Stock, customer khata, and shares at a glance.</p>
        </div>
        <button onClick={loadProfitModeData} disabled={loading}
          className="flex items-center space-x-1 px-3 py-1.5 bg-slate-800 hover:bg-slate-700 text-gray-200 rounded-lg text-xs disabled:opacity-50">
          <RefreshCw size={12} className={loading ? 'animate-spin' : ''} />
          <span>Refresh</span>
        </button>
      </div>

      {/* === TOP STATS CARDS (4 cards) === */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
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

        <div className="glass-card p-4 border-red-500/20">
          <AlertTriangle size={16} className="text-red-400 mb-1" />
          <div className="text-[10px] text-gray-500 uppercase">Stale Stock</div>
          <div className="text-xl font-bold text-red-400">{staleProducts.length}</div>
        </div>
      </div>

      {/* === TWO-COLUMN LAYOUT === */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* LEFT: Customer (khata) net outstanding — v0.34.0, now standalone card (v0.36.0: agents panel removed).
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

        {/* RIGHT: Recent share activity */}
        <div className="glass-card p-5">
          <div className="flex items-center justify-between mb-3">
            <h2 className="text-sm font-semibold text-white flex items-center">
              <Sparkles size={14} className="mr-2 text-violet-400" />
              Recent Shares (last 10)
            </h2>
            <button onClick={() => setCurrentTab('share_center')}
              className="text-[10px] text-violet-400 hover:text-violet-300">Share Center →</button>
          </div>
          {shareLogs.length === 0 ? (
            <p className="text-xs text-gray-500 py-6 text-center">No shares logged yet. Visit Share Center to push products.</p>
          ) : (
            <div className="space-y-2 max-h-64 overflow-y-auto">
              {shareLogs.map(log => (
                <div key={log.id} className="flex items-start justify-between p-2 bg-slate-950/50 rounded-lg">
                  <div className="flex-1 min-w-0">
                    <div className="text-xs font-semibold text-white truncate">{log.product_name}</div>
                    <div className="text-[10px] text-gray-500">
                      {log.platform.replace(/_/g, ' ')} • {log.share_angle.replace(/_/g, ' ') || '—'}
                    </div>
                  </div>
                  <div className="text-[10px] text-gray-500 ml-2 shrink-0">{fmtDate(log.shared_at)}</div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* === STALE STOCK ALERT === */}
      {staleProducts.length > 0 && (
        <div className="glass-card p-5 border-amber-500/30">
          <div className="flex items-center justify-between mb-3">
            <h2 className="text-sm font-semibold text-white flex items-center">
              <AlertTriangle size={14} className="mr-2 text-amber-400" />
              Stale Stock — Not shared in 7+ days ({staleProducts.length})
            </h2>
            <button onClick={() => setCurrentTab('share_center')}
              className="text-[10px] px-2 py-1 bg-amber-600/20 hover:bg-amber-600/40 text-amber-300 rounded font-medium">
              Share Now →
            </button>
          </div>
          <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-2">
            {staleProducts.slice(0, 10).map(p => (
              <div key={p.id} className="bg-slate-950/50 rounded-lg p-2">
                <div className="text-xs font-semibold text-white truncate">{p.name}</div>
                <div className="text-[10px] text-gray-500">Rs. {p.sale_price.toFixed(0)} • {p.stock_quantity} in stock</div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* === QUICK ACTIONS === */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
        <button onClick={() => setCurrentTab('catalog')}
          className="glass-card p-4 hover:border-violet-500/50 transition-colors text-center">
          <Package size={20} className="mx-auto text-violet-400 mb-1" />
          <div className="text-xs font-semibold text-white">Catalog</div>
          <div className="text-[10px] text-gray-500">{products.length} products</div>
        </button>
        <button onClick={() => setCurrentTab('purchase_trips')}
          className="glass-card p-4 hover:border-violet-500/50 transition-colors text-center">
          <Sparkles size={20} className="mx-auto text-emerald-400 mb-1" />
          <div className="text-xs font-semibold text-white">Purchase Trips</div>
          <div className="text-[10px] text-gray-500">Record buying trips</div>
        </button>
        <button onClick={() => setCurrentTab('share_center')}
          className="glass-card p-4 hover:border-violet-500/50 transition-colors text-center">
          <Sparkles size={20} className="mx-auto text-pink-400 mb-1" />
          <div className="text-xs font-semibold text-white">Share Center</div>
          <div className="text-[10px] text-gray-500">Push to social</div>
        </button>
      </div>
    </div>
  )
}
