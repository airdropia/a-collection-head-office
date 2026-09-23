import React, { useEffect, useState } from 'react'
import { useAppStore, Customer, OrderHistory } from '../stores/store'
import { invoke } from '@tauri-apps/api/core'
import { 
  Search, 
  UserPlus, 
  Phone, 
  Calendar, 
  ShoppingBag, 
  User, 
  Trash2,
  X,
  Wallet,
  Banknote,
  BookOpen,
  Sliders,
  Pencil,
} from 'lucide-react'
import { fmtMoney } from '../utils/format'

export default function Customers() {
  const { 
    customers, 
    products,
    fetchCustomers, 
    fetchProducts,
    addCustomer, 
    updateCustomer, 
    deleteCustomer,
    cart,
    addToCart,
    removeFromCart,
    clearCart,
    createOrder,
    getCustomerHistory
  } = useAppStore()

  const [searchTerm, setSearchTerm] = useState('')
  const [selectedCustomerId, setSelectedCustomerId] = useState<number | null>(null)
  const [purchaseHistory, setPurchaseHistory] = useState<OrderHistory[]>([])
  
  // Customer Modals
  const [showCustModal, setShowCustModal] = useState(false)
  const [editCustomer, setEditCustomer] = useState<Customer | null>(null)
  const [custName, setCustName] = useState('')
  const [custPhone, setCustPhone] = useState('')
  const [custLocation, setCustLocation] = useState('')
  const [custNotes, setCustNotes] = useState('')

  // Order Placement Modal
  const [showOrderModal, setShowOrderModal] = useState(false)
  const [orderSearch, setOrderSearch] = useState('')

  // v0.26.0: Payment modal state
  const [showPaymentModal, setShowPaymentModal] = useState(false)
  const [paymentCustomer, setPaymentCustomer] = useState<Customer | null>(null)
  const [paymentAmount, setPaymentAmount] = useState(0)
  const [paymentNotes, setPaymentNotes] = useState('')
  const [balanceHistory, setBalanceHistory] = useState<any[]>([])
  const [showBalanceHistory, setShowBalanceHistory] = useState(false)

  const [showLedgerEntryModal, setShowLedgerEntryModal] = useState(false)
  // v0.34.0: Direction of ledger entry — 'udhaar' (customer owes us, positive) or
  // 'advance' (we owe customer / they paid in advance, negative). Sign is derived
  // from direction so the user never types a minus sign.
  const [ledgerDirection, setLedgerDirection] = useState<'udhaar' | 'advance'>('udhaar')
  const [ledgerEntryType, setLedgerEntryType] = useState<'opening_debit' | 'adjustment'>('opening_debit')
  const [ledgerEntryAmount, setLedgerEntryAmount] = useState(0)
  const [ledgerEntryNotes, setLedgerEntryNotes] = useState('')
  const [ledgerEntryDate, setLedgerEntryDate] = useState('')

  // v0.29.0: Edit existing entry modal
  const [showEditEntryModal, setShowEditEntryModal] = useState(false)
  const [editEntry, setEditEntry] = useState<any | null>(null)
  const [editEntryAmount, setEditEntryAmount] = useState(0)
  const [editEntryNotes, setEditEntryNotes] = useState('')
  const [editEntryDate, setEditEntryDate] = useState('')

  useEffect(() => {
    fetchCustomers()
    fetchProducts()
  }, [])

  useEffect(() => {
    if (selectedCustomerId) {
      loadHistory(selectedCustomerId)
    } else {
      setPurchaseHistory([])
    }
  }, [selectedCustomerId])

  const loadHistory = async (id: number) => {
    try {
      const history = await getCustomerHistory(id)
      setPurchaseHistory(history)
    } catch (err) {
      console.error(err)
    }
  }

  const handleOpenAddCust = () => {
    setEditCustomer(null)
    setCustName('')
    setCustPhone('')
    setCustLocation('')
    setCustNotes('')
    setShowCustModal(true)
  }

  const handleOpenEditCust = (c: Customer) => {
    setEditCustomer(c)
    setCustName(c.name)
    setCustPhone(c.phone || '')
    setCustLocation(c.location || '')
    setCustNotes(c.notes || '')
    setShowCustModal(true)
  }

  const handleSaveCustomer = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!custName) return

    const data: Customer = {
      id: editCustomer?.id,
      name: custName,
      phone: custPhone,
      location: custLocation,
      notes: custNotes
    }

    try {
      if (editCustomer) {
        await updateCustomer(data)
      } else {
        await addCustomer(data)
      }
      setShowCustModal(false)
    } catch (err) {
      alert(`Error saving customer: ${err}`)
    }
  }

  const handleDeleteCustomer = async (id: number) => {
    if (confirm('Are you sure you want to delete this customer? This will also delete their order history.')) {
      try {
        await deleteCustomer(id)
        if (selectedCustomerId === id) setSelectedCustomerId(null)
      } catch (err) {
        alert(err)
      }
    }
  }

  // v0.26.0: Record payment against customer's outstanding balance
  // v0.34.0: negative balance → settle our debt via negative adjustment instead,
  // because record_customer_payment rejects amounts exceeding a positive balance.
  const handleOpenPaymentModal = (customer: Customer) => {
    setPaymentCustomer(customer)
    setPaymentAmount(Math.abs(customer.outstanding_balance || 0))
    setPaymentNotes('')
    setShowPaymentModal(true)
  }

  const handleRecordPayment = async () => {
    if (!paymentCustomer?.id) return
    if (paymentAmount <= 0) { alert('Payment amount must be positive.'); return }
    try {
      if ((paymentCustomer.outstanding_balance || 0) >= 0) {
        // Normal flow: customer pays us
        await invoke('record_customer_payment', {
          customerId: paymentCustomer.id,
          amount: paymentAmount,
          notes: paymentNotes || null,
          saleId: null,
        })
      } else {
        // We owe the customer — record a negative adjustment to settle our debt
        await invoke('add_customer_ledger_entry', {
          customerId: paymentCustomer.id,
          entryType: 'adjustment',
          amount: -Math.abs(paymentAmount),
          entryDate: null,
          notes: paymentNotes || 'Settled advance debt (hum ne wapas diya)',
        })
      }
      setShowPaymentModal(false)
      await fetchCustomers()
      alert(`Entry recorded! Rs. ${paymentAmount.toFixed(0)} — ${paymentCustomer.name}`)
    } catch (err) {
      alert(`Error: ${err}`)
    }
  }

  // v0.26.0: Show customer's balance history (sales + payments timeline)
  const handleShowBalanceHistory = async (customer: Customer) => {
    if (!customer.id) return
    try {
      const history = await invoke<any[]>('get_customer_balance_history', { customerId: customer.id })
      setBalanceHistory(history)
      setPaymentCustomer(customer)
      setShowBalanceHistory(true)
    } catch (err) {
      alert(`Error: ${err}`)
    }
  }

  // v0.29.0: Open modal for adding a manual ledger entry (opening_debit or adjustment)
  // v0.34.0: direction defaults to 'udhaar'; caller can preset 'advance' for negative balances
  const handleOpenLedgerEntryModal = (customer: Customer, type: 'opening_debit' | 'adjustment', direction: 'udhaar' | 'advance' = 'udhaar') => {
    setPaymentCustomer(customer)
    setLedgerEntryType(type)
    setLedgerDirection(direction)
    setLedgerEntryAmount(0)
    setLedgerEntryNotes('')
    // Default date = today (YYYY-MM-DD for <input type="date">)
    setLedgerEntryDate(new Date().toISOString().split('T')[0])
    setShowLedgerEntryModal(true)
  }

  // v0.34.0: Save the manual ledger entry. Sign comes from direction, not user input:
  //   udhaar  → customer owes us → stored positive (opening_debit) or +adjustment
  //   advance → we owe customer (advance payment / maal wapas) → adjustment stays
  //             negative. opening_debit cannot be negative per backend invariant, so
  //             an advance opening balance is stored as a negative 'adjustment'.
  const handleSaveLedgerEntry = async () => {
    if (!paymentCustomer?.id) return
    const signedAmount = ledgerDirection === 'udhaar' ? Math.abs(ledgerEntryAmount) : -Math.abs(ledgerEntryAmount)
    if (ledgerEntryAmount <= 0) {
      alert('Amount must be greater than zero.')
      return
    }
    // Backend invariant: opening_debit must be positive. An advance-direction
    // entry is therefore always recorded as an 'adjustment' (signed amount).
    const backendType = ledgerDirection === 'advance' ? 'adjustment' : ledgerEntryType
    try {
      // Convert date to ISO 8601 (with time)
      const isoDate = ledgerEntryDate
        ? new Date(ledgerEntryDate + 'T12:00:00').toISOString()
        : null
      await invoke('add_customer_ledger_entry', {
        customerId: paymentCustomer.id,
        entryType: backendType,
        amount: signedAmount,
        entryDate: isoDate,
        notes: ledgerEntryNotes || null,
      })
      setShowLedgerEntryModal(false)
      await fetchCustomers()
      alert(`Entry added for ${paymentCustomer.name}`)
    } catch (err) {
      alert(`Error: ${err}`)
    }
  }

  // v0.29.0: Open modal for editing an existing ledger entry
  const handleOpenEditEntry = (entry: any) => {
    setEditEntry(entry)
    setEditEntryAmount(entry.amount)
    setEditEntryNotes('')
    // Parse the entry's date for the date input
    try {
      const d = new Date(entry.date)
      setEditEntryDate(d.toISOString().split('T')[0])
    } catch {
      setEditEntryDate(new Date().toISOString().split('T')[0])
    }
    setShowEditEntryModal(true)
  }

  // v0.29.0: Save edits to an existing ledger entry
  const handleSaveEditEntry = async () => {
    if (!editEntry) return
    if (editEntry.entry_type === 'opening_debit' && editEntryAmount <= 0) {
      alert('Opening debit amount must be positive.')
      return
    }
    if (editEntry.entry_type === 'adjustment' && editEntryAmount === 0) {
      alert('Adjustment amount cannot be zero.')
      return
    }
    try {
      const isoDate = editEntryDate
        ? new Date(editEntryDate + 'T12:00:00').toISOString()
        : null
      await invoke('update_customer_ledger_entry', {
        entryId: editEntry.id,
        amount: editEntryAmount,
        notes: editEntryNotes || null,
        entryDate: isoDate,
      })
      setShowEditEntryModal(false)
      // Refresh balance history + customer list
      if (paymentCustomer?.id) {
        const history = await invoke<any[]>('get_customer_balance_history', { customerId: paymentCustomer.id })
        setBalanceHistory(history)
      }
      await fetchCustomers()
      alert('Entry updated.')
    } catch (err) {
      alert(`Error: ${err}`)
    }
  }

  // v0.29.0: Delete a ledger entry (only opening_debit + adjustment + payment)
  const handleDeleteLedgerEntry = async (entryId: number) => {
    if (!confirm('Delete this entry? Customer balance will be recalculated.')) return
    try {
      await invoke('delete_customer_ledger_entry', { entryId })
      // Refresh balance history + customer list
      if (paymentCustomer?.id) {
        const history = await invoke<any[]>('get_customer_balance_history', { customerId: paymentCustomer.id })
        setBalanceHistory(history)
      }
      await fetchCustomers()
      alert('Entry deleted.')
    } catch (err) {
      alert(`Error: ${err}`)
    }
  }

  // v0.26.0: Total outstanding across all customers (net — can be negative if
  // we owe customers more than they owe us)
  const totalOutstanding = customers.reduce((s, c) => s + (c.outstanding_balance || 0), 0)
  const customersWithUdhar = customers.filter(c => (c.outstanding_balance || 0) > 0).length
  // v0.34.0: customers with negative balance — we owe them (advance/maal wapas)
  const customersWeOwe = customers.filter(c => (c.outstanding_balance || 0) < 0).length

  const handlePlaceOrder = async () => {
    if (!selectedCustomerId) return
    if (cart.length === 0) {
      alert('Your cart is empty. Please add products first.')
      return
    }

    const items = cart.map(item => ({
      product_id: item.product.id!,
      quantity: item.quantity
    }))

    try {
      await createOrder(selectedCustomerId, items)
      clearCart()
      setShowOrderModal(false)
      loadHistory(selectedCustomerId)
      alert('Order placed successfully! Stock levels updated.')
    } catch (err) {
      alert(`Failed to place order: ${err}`)
    }
  }

  // Filters
  const filteredCustomers = customers.filter(c => 
    c.name.toLowerCase().includes(searchTerm.toLowerCase()) || 
    (c.phone && c.phone.includes(searchTerm)) ||
    (c.location && c.location.toLowerCase().includes(searchTerm.toLowerCase()))
  )

  const activeCustomer = customers.find(c => c.id === selectedCustomerId)

  // Products filter for order modal
  const filteredProductsForOrder = products.filter(p => 
    p.status === 'active' && 
    p.stock_quantity > 0 &&
    (p.name.toLowerCase().includes(orderSearch.toLowerCase()) || p.sku.toLowerCase().includes(orderSearch.toLowerCase()))
  )

  const totalCartValue = cart.reduce((acc, item) => acc + item.product.sale_price * item.quantity, 0)

  return (
    <div className="space-y-6">
      <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold tracking-tight text-white font-display">Customer Management</h1>
          <p className="text-sm text-gray-400 mt-1">Manage customer profiles and place sales orders.</p>
        </div>
        <button 
          onClick={handleOpenAddCust}
          className="flex items-center space-x-1 px-4 py-2 bg-violet-600 hover:bg-violet-700 text-white rounded-lg text-sm font-medium transition-colors self-start"
        >
          <UserPlus size={16} />
          <span>New Customer</span>
        </button>
      </div>

      {/* v0.34.0: Udhar Summary Bar — NET balance with red/green direction.
          Positive net  = customers owe us  (green, HO ko lena hai)
          Negative net  = we owe customers  (red, HO ko dena hai)
          Shown whenever net is non-zero, in either direction. */}
      {totalOutstanding !== 0 && (
        <div className={`glass-card p-4 mb-4 flex items-center justify-between ${(
          totalOutstanding > 0
            ? 'bg-emerald-900/10 border-emerald-700/30'
            : 'bg-red-900/10 border-red-700/30'
        )}`}>
          <div className="flex items-center gap-3">
            <Wallet className={totalOutstanding > 0 ? 'text-emerald-400' : 'text-red-400'} size={24} />
            <div>
              {totalOutstanding > 0 ? (
                <>
                  <p className="text-sm text-emerald-300">HO ko lena hai (Udhar)</p>
                  <p className="text-3xl font-bold text-emerald-400">{fmtMoney(totalOutstanding)}</p>
                </>
              ) : (
                <>
                  <p className="text-sm text-red-300">HO ko dena hai (Advance/Wapas)</p>
                  <p className="text-3xl font-bold text-red-400">{fmtMoney(-totalOutstanding)}</p>
                </>
              )}
            </div>
          </div>
          <div className="text-right">
            <p className="text-xs text-gray-500">{customersWithUdhar} customer{customersWithUdhar !== 1 ? 's' : ''} with udhar</p>
            {customersWeOwe > 0 && (
              <p className="text-xs text-gray-500">{customersWeOwe} customer{customersWeOwe !== 1 ? 's' : ''} we owe</p>
            )}
          </div>
        </div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Customers List (Left 1/3) */}
        <div className="glass-card p-5 flex flex-col h-[550px]">
          <div className="relative mb-4">
            <Search className="absolute left-3 top-2.5 text-gray-500" size={18} />
            <input 
              type="text"
              placeholder="Search customers..."
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              className="w-full bg-slate-950 border border-gray-800 rounded-lg pl-10 pr-4 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500 transition-colors"
            />
          </div>

          <div className="flex-1 overflow-y-auto space-y-2 pr-1">
            {filteredCustomers.length === 0 ? (
              <p className="text-sm text-gray-500 text-center py-8">No customers found.</p>
            ) : (
              filteredCustomers.map(c => (
                <div 
                  key={c.id}
                  onClick={() => c.id && setSelectedCustomerId(c.id)}
                  className={`p-3 rounded-lg border cursor-pointer transition-all flex justify-between items-center ${
                    selectedCustomerId === c.id 
                      ? 'bg-violet-600/10 border-violet-500/50' 
                      : 'bg-slate-950 border-gray-800 hover:border-gray-700'
                  }`}
                >
                  <div className="space-y-1 flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <p className="text-sm font-semibold text-white">{c.name}</p>
                      {/* v0.34.0: balance badge — green udhar, red advance (we owe them).
                          Previously negative balances showed nothing (hidden debt). */}
                      {(c.outstanding_balance || 0) > 0 && (
                        <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-900/50 text-emerald-300 border border-emerald-700/50 font-bold whitespace-nowrap">
                          Udhar: {fmtMoney(c.outstanding_balance || 0)}
                        </span>
                      )}
                      {(c.outstanding_balance || 0) < 0 && (
                        <span className="text-[10px] px-1.5 py-0.5 rounded bg-red-900/50 text-red-300 border border-red-700/50 font-bold whitespace-nowrap">
                          Dene hain: {fmtMoney(-(c.outstanding_balance || 0))}
                        </span>
                      )}
                    </div>
                    <p className="text-xs text-gray-400 flex items-center"><Phone size={10} className="mr-1" />{c.phone || '-'}</p>
                  </div>
                  <div className="flex items-center gap-1 shrink-0">
                    {/* v0.34.0: Record Payment — green icon when they owe us (udhar),
                        red icon when we owe them (settle advance debt) */}
                    {(c.outstanding_balance || 0) !== 0 && c.id && (
                      <button 
                        onClick={(e) => { e.stopPropagation(); handleOpenPaymentModal(c); }}
                        className={`${(c.outstanding_balance || 0) > 0 ? 'text-emerald-400 hover:text-emerald-300' : 'text-red-400 hover:text-red-300'} transition-colors p-1`}
                        title={(c.outstanding_balance || 0) > 0 ? 'Record Payment (lena hai)' : 'Settle Debt (dena hai)'}
                      >
                        <Banknote size={14} />
                      </button>
                    )}
                    {/* v0.29.0: Manual ledger entry buttons — always visible */}
                    {c.id && (
                      <>
                        <button 
                          onClick={(e) => { e.stopPropagation(); handleOpenLedgerEntryModal(c, 'opening_debit', (c.outstanding_balance || 0) < 0 ? 'advance' : 'udhaar'); }}
                          className="text-amber-400 hover:text-amber-300 transition-colors p-1"
                          title="Add Opening Balance (purana udhar / advance)"
                        >
                          <BookOpen size={14} />
                        </button>
                        <button 
                          onClick={(e) => { e.stopPropagation(); handleOpenLedgerEntryModal(c, 'adjustment', (c.outstanding_balance || 0) < 0 ? 'advance' : 'udhaar'); }}
                          className="text-violet-400 hover:text-violet-300 transition-colors p-1"
                          title="Add Adjustment (maal wapas / correction)"
                        >
                          <Sliders size={14} />
                        </button>
                        {/* History button — always visible (even if balance=0) */}
                        {(c.outstanding_balance || 0) === 0 && (
                          <button 
                            onClick={(e) => { e.stopPropagation(); handleShowBalanceHistory(c); }}
                            className="text-blue-400 hover:text-blue-300 transition-colors p-1"
                            title="View Khata History"
                          >
                            <Wallet size={14} />
                          </button>
                        )}
                      </>
                    )}
                    <button 
                      onClick={(e) => { e.stopPropagation(); c.id && handleDeleteCustomer(c.id); }}
                      className="text-gray-500 hover:text-red-400 transition-colors p-1"
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>

        {/* Customer Details & Order History (Right 2/3) */}
        <div className="lg:col-span-2 space-y-6">
          {activeCustomer ? (
            <>
              {/* Customer Profile Card */}
              <div className="glass-card p-5 space-y-4">
                <div className="flex justify-between items-start">
                  <div className="flex items-center space-x-3">
                    <div className="w-12 h-12 bg-violet-600/10 text-violet-400 rounded-full flex items-center justify-center">
                      <User size={24} />
                    </div>
                    <div>
                      <h2 className="text-xl font-bold text-white">{activeCustomer.name}</h2>
                      <p className="text-xs text-gray-400">Created: {new Date(activeCustomer.created_at || '').toLocaleDateString()}</p>
                    </div>
                  </div>
                  <div className="flex space-x-2">
                    <button 
                      onClick={() => handleOpenEditCust(activeCustomer)}
                      className="px-3 py-1.5 bg-slate-800 hover:bg-slate-700 border border-gray-700 text-gray-300 rounded-lg text-xs font-medium transition-colors"
                    >
                      Edit Profile
                    </button>
                    <button 
                      onClick={() => setShowOrderModal(true)}
                      className="px-3 py-1.5 bg-violet-600 hover:bg-violet-700 text-white rounded-lg text-xs font-medium transition-colors glow-btn"
                    >
                      New Order
                    </button>
                  </div>
                </div>

                <div className="grid grid-cols-2 gap-4 text-sm border-t border-gray-800 pt-4">
                  <div className="space-y-1">
                    <p className="text-xs text-gray-400 uppercase font-semibold">Phone</p>
                    <p className="text-gray-200">{activeCustomer.phone || 'N/A'}</p>
                  </div>
                  <div className="space-y-1">
                    <p className="text-xs text-gray-400 uppercase font-semibold">Location</p>
                    <p className="text-gray-200">{activeCustomer.location || 'N/A'}</p>
                  </div>
                </div>

                {activeCustomer.notes && (
                  <div className="bg-slate-950 p-3 rounded-lg border border-gray-800 text-xs text-gray-400">
                    <strong>Notes:</strong> {activeCustomer.notes}
                  </div>
                )}
              </div>

              {/* Purchase History */}
              <div className="glass-card p-5">
                <h3 className="text-lg font-semibold text-white mb-4 flex items-center">
                  <ShoppingBag className="mr-2 text-violet-500" size={18} /> Purchase History
                </h3>
                
                <div className="space-y-3 overflow-y-auto max-h-[250px] pr-1">
                  {purchaseHistory.length === 0 ? (
                    <p className="text-sm text-gray-500 text-center py-6">No previous orders found for this customer.</p>
                  ) : (
                    purchaseHistory.map(order => (
                      <div key={order.order_id} className="p-4 bg-slate-950 border border-gray-800 rounded-xl space-y-3">
                        <div className="flex justify-between items-center text-xs">
                          <div className="space-y-0.5">
                            <p className="text-gray-400">Order ID: #{order.order_id}</p>
                            <p className="text-gray-500 flex items-center"><Calendar size={10} className="mr-1" />{new Date(order.order_date).toLocaleString()}</p>
                          </div>
                          <div className="text-right">
                            <p className="text-sm font-bold text-violet-400">Rs. {order.total_amount.toFixed(2)}</p>
                            <p className="text-[10px] text-emerald-400">Est. Profit: Rs. {order.profit.toFixed(2)}</p>
                          </div>
                        </div>

                        <div className="border-t border-gray-900 pt-2 space-y-1">
                          {order.items.map((item, idx) => (
                            <div key={idx} className="flex justify-between text-xs text-gray-400">
                              <span>{item.product_name} <span className="text-gray-600">x{item.quantity}</span></span>
                              <span className="font-mono">${(item.sale_price * item.quantity).toFixed(2)}</span>
                            </div>
                          ))}
                        </div>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </>
          ) : (
            <div className="glass-card p-12 text-center text-gray-500 flex flex-col items-center justify-center h-[350px]">
              <User size={48} className="text-gray-700 mb-4" />
              <p className="text-sm">Select a customer from the list to view profile, order history, and place new orders.</p>
            </div>
          )}
        </div>
      </div>

      {/* Customer Add/Edit Modal */}
      {showCustModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4">
          <div className="bg-slate-900 border border-gray-800 rounded-2xl w-full max-w-md overflow-hidden shadow-2xl animate-in fade-in zoom-in-95 duration-150">
            <div className="flex items-center justify-between p-4 border-b border-gray-800 bg-slate-950/40">
              <h3 className="text-lg font-bold text-white font-display">
                {editCustomer ? 'Edit Customer' : 'Add New Customer'}
              </h3>
              <button onClick={() => setShowCustModal(false)} className="text-gray-400 hover:text-white transition-colors">
                <X size={20} />
              </button>
            </div>

            <form onSubmit={handleSaveCustomer} className="p-6 space-y-4">
              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Full Name *</label>
                <input 
                  type="text" required value={custName} onChange={(e) => setCustName(e.target.value)}
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                  placeholder="E.g. Yasir Ali"
                />
              </div>

              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Phone Number</label>
                <input 
                  type="text" value={custPhone} onChange={(e) => setCustPhone(e.target.value)}
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                  placeholder="E.g. +923001234567"
                />
              </div>

              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Location / Address</label>
                <input 
                  type="text" value={custLocation} onChange={(e) => setCustLocation(e.target.value)}
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                  placeholder="E.g. Lahore, Pakistan"
                />
              </div>

              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Notes</label>
                <textarea 
                  value={custNotes} onChange={(e) => setCustNotes(e.target.value)} rows={3}
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                  placeholder="Preferences, sizing, favorite categories..."
                />
              </div>

              <div className="flex justify-end space-x-2 pt-4 border-t border-gray-800">
                <button
                  type="button" onClick={() => setShowCustModal(false)}
                  className="px-4 py-2 bg-slate-800 hover:bg-slate-700 text-gray-200 rounded-lg text-sm"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="px-4 py-2 bg-violet-600 hover:bg-violet-700 text-white rounded-lg text-sm font-medium"
                >
                  Save Customer
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Order Placement Modal */}
      {showOrderModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4">
          <div className="bg-slate-900 border border-gray-800 rounded-2xl w-full max-w-2xl overflow-hidden shadow-2xl animate-in fade-in zoom-in-95 duration-150">
            <div className="flex items-center justify-between p-4 border-b border-gray-800 bg-slate-950/40">
              <h3 className="text-lg font-bold text-white font-display">
                Place Sales Order for {activeCustomer?.name}
              </h3>
              <button onClick={() => setShowOrderModal(false)} className="text-gray-400 hover:text-white transition-colors">
                <X size={20} />
              </button>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4 p-6">
              {/* Product Selector */}
              <div className="space-y-3 flex flex-col h-[350px]">
                <p className="text-xs font-semibold uppercase text-gray-400">Select Products</p>
                <div className="relative">
                  <Search className="absolute left-2.5 top-2 text-gray-500" size={14} />
                  <input 
                    type="text"
                    placeholder="Search product to add..."
                    value={orderSearch}
                    onChange={(e) => setOrderSearch(e.target.value)}
                    className="w-full bg-slate-950 border border-gray-800 rounded-lg pl-8 pr-3 py-1.5 text-xs text-gray-200 focus:outline-none focus:border-violet-500"
                  />
                </div>

                <div className="flex-1 overflow-y-auto space-y-2 pr-1">
                  {filteredProductsForOrder.map(product => (
                    <div key={product.id} className="p-2 bg-slate-950 border border-gray-850 rounded-lg flex justify-between items-center text-xs">
                      <div>
                        <p className="font-semibold text-white">{product.name}</p>
                        <p className="text-[10px] text-gray-500">SKU: {product.sku} | Stock: {product.stock_quantity}</p>
                      </div>
                      <div className="flex items-center space-x-2">
                        <span className="font-mono text-violet-400 font-bold">${product.sale_price.toFixed(2)}</span>
                        <button
                          onClick={() => addToCart(product, 1)}
                          className="bg-violet-600 hover:bg-violet-700 text-white rounded px-2 py-1 font-medium transition-colors"
                        >
                          Add
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              </div>

              {/* Order Cart */}
              <div className="space-y-3 flex flex-col h-[350px] border-l border-gray-800 pl-4">
                <p className="text-xs font-semibold uppercase text-gray-400">Order Cart</p>
                <div className="flex-1 overflow-y-auto space-y-2 pr-1">
                  {cart.length === 0 ? (
                    <p className="text-xs text-gray-500 text-center py-12">No items in cart.</p>
                  ) : (
                    cart.map(item => (
                      <div key={item.product.id} className="p-2 bg-slate-950 border border-gray-850 rounded-lg flex justify-between items-center text-xs">
                        <div>
                          <p className="font-semibold text-white">{item.product.name}</p>
                          <p className="text-[10px] text-gray-500">${item.product.sale_price.toFixed(2)} x {item.quantity}</p>
                        </div>
                        <button
                          onClick={() => item.product.id && removeFromCart(item.product.id)}
                          className="text-gray-500 hover:text-red-400 p-1"
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                    ))
                  )}
                </div>

                {/* Subtotal */}
                <div className="border-t border-gray-800 pt-3 space-y-2">
                  <div className="flex justify-between items-center text-sm font-bold text-white">
                    <span>Total Amount:</span>
                    <span className="font-mono text-violet-400">${totalCartValue.toFixed(2)}</span>
                  </div>
                  <div className="flex space-x-2">
                    <button
                      onClick={clearCart}
                      className="flex-1 py-2 bg-slate-800 hover:bg-slate-700 text-gray-300 rounded-lg text-xs"
                    >
                      Clear
                    </button>
                    <button
                      onClick={handlePlaceOrder}
                      disabled={cart.length === 0}
                      className="flex-1 py-2 bg-violet-600 hover:bg-violet-700 text-white rounded-lg text-xs font-medium disabled:opacity-50"
                    >
                      Confirm Order
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* v0.34.0: Payment Modal — works both ways now.
          Positive balance: customer pays us (green, existing flow).
          Negative balance: we pay/settle the customer (red). The backend
          `record_customer_payment` only handles positive balances, so settling
          our debt to a customer goes through the ledger as a negative adjustment. */}
      {showPaymentModal && paymentCustomer && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4">
          <div className="bg-slate-900 border border-gray-800 rounded-2xl w-full max-w-md overflow-hidden shadow-2xl">
            <div className="flex items-center justify-between p-4 border-b border-gray-800 bg-slate-950/40">
              <h3 className="text-lg font-bold text-white">
                {(paymentCustomer.outstanding_balance || 0) > 0 ? 'Record Payment' : 'Settle Debt (Hum ko dena hai)'}
              </h3>
              <button onClick={() => setShowPaymentModal(false)} className="text-gray-400 hover:text-white"><X size={20} /></button>
            </div>
            <div className="p-5 space-y-3">
              <div className="bg-slate-950/50 border border-gray-800 rounded-lg p-3">
                <p className="text-sm text-gray-400">Customer</p>
                <p className="text-base font-semibold text-white">{paymentCustomer.name}</p>
                <p className="text-xs text-gray-500">{paymentCustomer.phone || 'No phone'}</p>
              </div>
              <div className={`rounded-lg p-3 flex justify-between items-center border ${
                (paymentCustomer.outstanding_balance || 0) >= 0
                  ? 'bg-amber-900/20 border-amber-700/50'
                  : 'bg-red-900/20 border-red-700/50'
              }`}>
                <span className={`text-sm ${(paymentCustomer.outstanding_balance || 0) >= 0 ? 'text-amber-300' : 'text-red-300'}`}>
                  {(paymentCustomer.outstanding_balance || 0) >= 0 ? 'Outstanding Balance' : 'Hum in ko dete hain'}
                </span>
                <span className={`text-lg font-bold ${(paymentCustomer.outstanding_balance || 0) >= 0 ? 'text-amber-400' : 'text-red-400'}`}>
                  {fmtMoney(Math.abs(paymentCustomer.outstanding_balance || 0))}
                </span>
              </div>
              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Payment Amount</label>
                <input
                  type="number"
                  min={0}
                  step={0.01}
                  value={paymentAmount}
                  onChange={e => setPaymentAmount(Math.max(0, Number(e.target.value)))}
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                />
                <p className="text-[10px] text-gray-600 mt-1">
                  New balance after payment: <span className="text-gray-400">{fmtMoney((paymentCustomer.outstanding_balance || 0) + ((paymentCustomer.outstanding_balance || 0) >= 0 ? -paymentAmount : paymentAmount))}</span>
                </p>
              </div>
              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Notes (optional)</label>
                <input
                  type="text"
                  value={paymentNotes}
                  onChange={e => setPaymentNotes(e.target.value)}
                  placeholder="e.g. Cash, Online transfer, Partial payment..."
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                />
              </div>
              <div className="flex gap-2 pt-2">
                <button onClick={() => setShowPaymentModal(false)} className="flex-1 px-4 py-2 bg-slate-800 hover:bg-slate-700 text-gray-200 rounded-lg text-sm">Cancel</button>
                <button onClick={handleRecordPayment} className="flex-1 px-4 py-2 bg-emerald-600 hover:bg-emerald-700 text-white rounded-lg text-sm font-medium">Save Payment</button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* v0.26.0: Balance History (Khata) Modal — v0.29.0 enhanced with edit/delete + new entry types */}
      {showBalanceHistory && paymentCustomer && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4">
          <div className="bg-slate-900 border border-gray-800 rounded-2xl w-full max-w-2xl max-h-[80vh] overflow-hidden shadow-2xl flex flex-col">
            <div className="flex items-center justify-between p-4 border-b border-gray-800 bg-slate-950/40">
              <div>
                <h3 className="text-lg font-bold text-white">Khata History — {paymentCustomer.name}</h3>
                <p className="text-xs text-gray-500">{paymentCustomer.phone || 'No phone'}</p>
              </div>
              <button onClick={() => setShowBalanceHistory(false)} className="text-gray-400 hover:text-white"><X size={20} /></button>
            </div>
            <div className="overflow-y-auto p-4 space-y-2">
              {balanceHistory.length === 0 ? (
                <p className="text-sm text-gray-500 text-center py-8">No transactions yet.</p>
              ) : (
                balanceHistory.map((entry, i) => {
                  // v0.29.0: Color code by entry type
                  const isDebit = entry.amount > 0  // sale, opening_debit, +adjustment
                  const isManualEntry = entry.entry_type === 'opening_debit' || entry.entry_type === 'adjustment' || entry.entry_type === 'payment'
                  return (
                    <div key={i} className={`flex items-start gap-3 p-3 rounded-lg border ${
                      entry.entry_type === 'sale' ? 'bg-slate-950/50 border-gray-800' :
                      entry.entry_type === 'payment' ? 'bg-emerald-950/30 border-emerald-800/50' :
                      entry.entry_type === 'opening_debit' ? 'bg-amber-950/30 border-amber-800/50' :
                      entry.entry_type === 'adjustment' ? 'bg-violet-950/30 border-violet-800/50' :
                      'bg-slate-950/50 border-gray-800'
                    }`}>
                      <div className={`mt-1 w-2 h-2 rounded-full ${
                        entry.entry_type === 'sale' ? 'bg-amber-400' :
                        entry.entry_type === 'payment' ? 'bg-emerald-400' :
                        entry.entry_type === 'opening_debit' ? 'bg-amber-500' :
                        entry.entry_type === 'adjustment' ? 'bg-violet-400' :
                        'bg-gray-400'
                      }`}></div>
                      <div className="flex-1 min-w-0">
                        <p className="text-sm text-gray-200">{entry.description}</p>
                        <p className="text-[10px] text-gray-500">{new Date(entry.date).toLocaleDateString('en-PK', { day: '2-digit', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' })}</p>
                      </div>
                      <div className="text-right shrink-0 flex items-start gap-2">
                        <div>
                          <p className={`text-sm font-bold ${isDebit ? 'text-amber-400' : 'text-emerald-400'}`}>
                            {isDebit ? '+' : ''}{fmtMoney(Math.abs(entry.amount))}
                          </p>
                          <p className="text-[10px] text-gray-500">Bal: {fmtMoney(entry.balance_after)}</p>
                        </div>
                        {/* v0.29.0: Edit + Delete buttons (only for manual entries — not sales) */}
                        {isManualEntry && (
                          <div className="flex gap-1">
                            <button
                              onClick={() => handleOpenEditEntry(entry)}
                              className="text-gray-500 hover:text-blue-400 transition-colors"
                              title="Edit entry"
                            >
                              <Pencil size={12} />
                            </button>
                            <button
                              onClick={() => handleDeleteLedgerEntry(entry.id)}
                              className="text-gray-500 hover:text-red-400 transition-colors"
                              title="Delete entry"
                            >
                              <Trash2 size={12} />
                            </button>
                          </div>
                        )}
                      </div>
                    </div>
                  )
                })
              )}
            </div>
            <div className="p-4 border-t border-gray-800 bg-slate-950/40 flex justify-between items-center">
              <span className="text-sm text-gray-400">Current Outstanding:</span>
              <span className="text-lg font-bold text-amber-400">{fmtMoney(paymentCustomer.outstanding_balance || 0)}</span>
            </div>
          </div>
        </div>
      )}

      {/* v0.29.0: Manual Ledger Entry Modal (opening_debit + adjustment) */}
      {showLedgerEntryModal && paymentCustomer && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4">
          <div className="bg-slate-900 border border-gray-800 rounded-2xl w-full max-w-md overflow-hidden shadow-2xl">
            <div className="flex items-center justify-between p-4 border-b border-gray-800 bg-slate-950/40">
              <h3 className="text-lg font-bold text-white">
                {ledgerEntryType === 'opening_debit' ? 'Add Opening Balance' : 'Add Adjustment'}
              </h3>
              <button onClick={() => setShowLedgerEntryModal(false)} className="text-gray-400 hover:text-white"><X size={20} /></button>
            </div>
            <div className="p-5 space-y-3">
              <div className="bg-slate-950/50 border border-gray-800 rounded-lg p-3">
                <p className="text-sm text-gray-400">Customer</p>
                <p className="text-base font-semibold text-white">{paymentCustomer.name}</p>
                <p className="text-xs text-gray-500">{paymentCustomer.phone || 'No phone'}</p>
              </div>
              <div className="bg-amber-900/20 border border-amber-700/50 rounded-lg p-3 flex justify-between items-center">
                <span className="text-sm text-amber-300">Current Outstanding</span>
                <span className="text-lg font-bold text-amber-400">{fmtMoney(paymentCustomer.outstanding_balance || 0)}</span>
              </div>
              {/* v0.34.0: Direction chooser — the core fix. Which way does the money go?
                  Sign is derived from this, user never types a minus. */}
              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Kis taraf ka hisaab hai?</label>
                <div className="grid grid-cols-2 gap-2">
                  <button
                    type="button"
                    onClick={() => setLedgerDirection('udhaar')}
                    className={`p-3 rounded-lg border text-left transition-all ${
                      ledgerDirection === 'udhaar'
                        ? 'bg-emerald-900/30 border-emerald-600/60'
                        : 'bg-slate-950 border-gray-800 hover:border-gray-600'
                    }`}>
                    <p className={`text-sm font-bold ${ledgerDirection === 'udhaar' ? 'text-emerald-300' : 'text-gray-300'}`}>Customer hum se deta hai</p>
                    <p className="text-[10px] text-gray-500">Udhaar / purana udhaar — HO ko lena hai</p>
                  </button>
                  <button
                    type="button"
                    onClick={() => setLedgerDirection('advance')}
                    className={`rounded-lg border text-left transition-all p-3 ${
                      ledgerDirection === 'advance'
                        ? 'bg-red-900/30 border-red-600/60'
                        : 'bg-slate-950 border-gray-800 hover:border-gray-600'
                    }`}>
                    <p className={`text-sm font-bold ${ledgerDirection === 'advance' ? 'text-red-300' : 'text-gray-300'}`}>Hum customer ko dete hain</p>
                    <p className="text-[10px] text-gray-500">Advance payment / maal wapas — HO ko dena hai</p>
                  </button>
                </div>
              </div>
              {ledgerEntryType === 'opening_debit' && (
                <p className="text-xs text-gray-400 bg-slate-950/50 border border-gray-800 rounded p-2">
                  Use this to record old/purana hisaab that wasn't tracked before.
                </p>
              )}
              {ledgerEntryType === 'adjustment' && (
                <p className="text-xs text-gray-400 bg-slate-950/50 border border-gray-800 rounded p-2">
                  For corrections, maal wapas, or error fixes. Direction above decides the sign.
                </p>
              )}
              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">
                  Amount
                </label>
                <input
                  type="number"
                  step={0.01}
                  min={0}
                  value={ledgerEntryAmount}
                  onChange={e => setLedgerEntryAmount(Number(e.target.value))}
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                />
                <p className="text-[10px] text-gray-600 mt-1">
                  New balance: <span className="text-gray-400">{fmtMoney((paymentCustomer.outstanding_balance || 0) + (ledgerDirection === 'udhaar' ? Math.abs(ledgerEntryAmount) : -Math.abs(ledgerEntryAmount)))}</span>
                </p>
              </div>
              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Date</label>
                <input
                  type="date"
                  value={ledgerEntryDate}
                  onChange={e => setLedgerEntryDate(e.target.value)}
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                />
              </div>
              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Notes (optional)</label>
                <input
                  type="text"
                  value={ledgerEntryNotes}
                  onChange={e => setLedgerEntryNotes(e.target.value)}
                  placeholder={ledgerEntryType === 'opening_debit' ? 'e.g. Purana udhar from before app' : 'e.g. Discount, error correction...'}
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                />
              </div>
              <div className="flex gap-2 pt-2">
                <button onClick={() => setShowLedgerEntryModal(false)} className="flex-1 px-4 py-2 bg-slate-800 hover:bg-slate-700 text-gray-200 rounded-lg text-sm">Cancel</button>
                <button onClick={handleSaveLedgerEntry} className="flex-1 px-4 py-2 bg-violet-600 hover:bg-violet-700 text-white rounded-lg text-sm font-medium">Save Entry</button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* v0.29.0: Edit Entry Modal */}
      {showEditEntryModal && editEntry && (
        <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 backdrop-blur-sm p-4">
          <div className="bg-slate-900 border border-gray-800 rounded-2xl w-full max-w-md overflow-hidden shadow-2xl">
            <div className="flex items-center justify-between p-4 border-b border-gray-800 bg-slate-950/40">
              <h3 className="text-lg font-bold text-white">Edit Entry</h3>
              <button onClick={() => setShowEditEntryModal(false)} className="text-gray-400 hover:text-white"><X size={20} /></button>
            </div>
            <div className="p-5 space-y-3">
              <div className="bg-slate-950/50 border border-gray-800 rounded-lg p-3">
                <p className="text-xs text-gray-500">Type</p>
                <p className="text-sm font-semibold capitalize text-white">{editEntry.entry_type.replace('_', ' ')}</p>
                <p className="text-xs text-gray-500 mt-2">Current Description</p>
                <p className="text-sm text-gray-300">{editEntry.description}</p>
              </div>
              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">
                  Amount {editEntry.entry_type === 'adjustment' && '(use minus for discount)'}
                </label>
                <input
                  type="number"
                  step={0.01}
                  value={editEntryAmount}
                  onChange={e => setEditEntryAmount(Number(e.target.value))}
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                />
              </div>
              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Date</label>
                <input
                  type="date"
                  value={editEntryDate}
                  onChange={e => setEditEntryDate(e.target.value)}
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                />
              </div>
              <div>
                <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Notes (leave blank to keep existing)</label>
                <input
                  type="text"
                  value={editEntryNotes}
                  onChange={e => setEditEntryNotes(e.target.value)}
                  placeholder="New notes (optional)"
                  className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
                />
              </div>
              <div className="flex gap-2 pt-2">
                <button onClick={() => setShowEditEntryModal(false)} className="flex-1 px-4 py-2 bg-slate-800 hover:bg-slate-700 text-gray-200 rounded-lg text-sm">Cancel</button>
                <button onClick={handleSaveEditEntry} className="flex-1 px-4 py-2 bg-violet-600 hover:bg-violet-700 text-white rounded-lg text-sm font-medium">Update Entry</button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
