import { useAppStore } from './stores/store'
import {
  LayoutDashboard,
  BookOpen,
  Users,
  Package,
  Bot,
  FileText,
  Settings as SettingsIcon
} from 'lucide-react'

import Dashboard from './pages/Dashboard'
import Catalog from './pages/Catalog'
import Customers from './pages/Customers'
import Inventory from './pages/Inventory'
import Automation from './pages/Automation'
import Reports from './pages/Reports'
import SettingsPage from './pages/Settings'

const tabs = [
  { id: 'dashboard', label: 'Dashboard', icon: LayoutDashboard },
  { id: 'catalog', label: 'Catalog', icon: BookOpen },
  { id: 'customers', label: 'Customers', icon: Users },
  { id: 'inventory', label: 'Inventory', icon: Package },
  { id: 'automation', label: 'Automation', icon: Bot },
  { id: 'reports', label: 'Reports', icon: FileText },
  { id: 'settings', label: 'Settings', icon: SettingsIcon },
]

function App() {
  const {
    currentTab,
    setCurrentTab,
  } = useAppStore()

  const renderPage = () => {
    switch (currentTab) {
      case 'dashboard': return <Dashboard />
      case 'catalog': return <Catalog />
      case 'customers': return <Customers />
      case 'inventory': return <Inventory />
      case 'automation': return <Automation />
      case 'reports': return <Reports />
      case 'settings': return <SettingsPage />
      default: return <Dashboard />
    }
  }

  return (
    <div className="h-screen flex overflow-hidden bg-[#030712]">
      {/* Sidebar */}
      <aside className="w-56 bg-slate-900/60 border-r border-gray-800/60 flex flex-col shrink-0">
        <div className="p-3 border-b border-gray-800/60 flex items-center space-x-3">
          <img src="/logo.png" alt="A Collection" className="w-9 h-9 rounded-lg object-cover ring-1 ring-violet-500/20" />
          <div>
            <h1 className="text-sm font-bold text-white font-display tracking-tight">A Collection</h1>
            <p className="text-[9px] text-gray-500 uppercase tracking-wider">Head Office</p>
          </div>
        </div>
        <nav className="flex-1 overflow-y-auto p-2 space-y-1">
          {tabs.map((tab) => {
            const Icon = tab.icon
            return (
              <button
                key={tab.id}
                onClick={() => setCurrentTab(tab.id)}
                className={`w-full flex items-center space-x-2.5 px-3 py-2.5 rounded-lg text-sm transition-all ${
                  currentTab === tab.id
                    ? 'bg-violet-600/15 text-violet-400 border border-violet-500/20'
                    : 'text-gray-400 hover:text-gray-200 hover:bg-slate-800/50 border border-transparent'
                }`}
              >
                <Icon size={17} />
                <span>{tab.label}</span>
              </button>
            )
          })}
        </nav>
      </aside>

      {/* Main Content */}
      <main className="flex-1 overflow-y-auto p-6 bg-[#030712]">
        {renderPage()}
      </main>
    </div>
  )
}

export default App
