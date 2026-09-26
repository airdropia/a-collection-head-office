import { useAppStore } from './stores/store'
import {
  LayoutDashboard,
  BookOpen,
  Users,
  Package,
  Bot,
  FileText,
  Settings as SettingsIcon,
  Megaphone,
  Truck,
  ChevronLeft,
  Sparkles,
  MessageSquare
} from 'lucide-react'

import Dashboard from './pages/Dashboard'
import Catalog from './pages/Catalog'
import Customers from './pages/Customers'
import Inventory from './pages/Inventory'
import Automation from './pages/Automation'
import Reports from './pages/Reports'
import SettingsPage from './pages/Settings'
import ShareCenter from './pages/ShareCenter'
import PurchaseTripsPage from './pages/PurchaseTrips'
import AiWorkspace from './components/AiWorkspace'

const tabs = [
  { id: 'dashboard', label: 'Dashboard', icon: LayoutDashboard },
  { id: 'catalog', label: 'Catalog', icon: BookOpen },
  { id: 'share_center', label: 'Share Center', icon: Megaphone },
  { id: 'customers', label: 'Customers', icon: Users },
  { id: 'inventory', label: 'Inventory', icon: Package },
  { id: 'purchase_trips', label: 'Trips', icon: Truck },
  { id: 'automation', label: 'Automation', icon: Bot },
  { id: 'reports', label: 'Reports', icon: FileText },
  { id: 'settings', label: 'Settings', icon: SettingsIcon },
]

function App() {
  const {
    currentTab,
    setCurrentTab,
    showAiAssistant,
    setVectorAssistant,
    aiProductDrafts,
  } = useAppStore()

  const renderPage = () => {
    switch (currentTab) {
      case 'dashboard': return <Dashboard />
      case 'catalog': return <Catalog />
      case 'share_center': return <ShareCenter />
      case 'customers': return <Customers />
      case 'inventory': return <Inventory />
      case 'automation': return <Automation />
      case 'purchase_trips': return <PurchaseTripsPage />
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
        <div className="p-3 border-t border-gray-800/60 space-y-2">
          <button
            onClick={() => setVectorAssistant(!showAiAssistant)}
            className="w-full flex items-center justify-between px-3 py-2 rounded-lg text-xs bg-violet-600/10 text-violet-400 border border-violet-500/10 hover:bg-violet-600/20 transition-colors"
          >
            <span className="flex items-center space-x-1.5">
              <Sparkles size={14} />
              <span>AI Workspace</span>
            </span>
            <div className="flex items-center space-x-1">
              {aiProductDrafts.length > 0 && (
                <span className="bg-violet-500 text-white text-[9px] rounded-full w-4 h-4 flex items-center justify-center font-bold">
                  {aiProductDrafts.length}
                </span>
              )}
              <ChevronLeft size={14} className={`transition-transform ${showAiAssistant ? 'rotate-180' : ''}`} />
            </div>
          </button>
          {!showAiAssistant && (
            <button
              onClick={() => setVectorAssistant(true)}
              className="w-full flex items-center space-x-2 px-3 py-2 rounded-lg text-xs text-gray-500 hover:text-gray-300 hover:bg-slate-800/50 transition-colors"
            >
              <MessageSquare size={12} />
              <span>Open AI Workspace</span>
            </button>
          )}
        </div>
      </aside>

      {/* Main Content */}
      <main className="flex-1 overflow-y-auto p-6 bg-[#030712]">
        {renderPage()}
      </main>

      {/* AI Workspace */}
      <AiWorkspace />
    </div>
  )
}

export default App
