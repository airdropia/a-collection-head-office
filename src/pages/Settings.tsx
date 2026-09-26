import { useEffect, useState } from 'react'
import { useAppStore } from '../stores/store'
import { open } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'
import { Shield, Key, HardDrive, RefreshCw, Globe, History } from 'lucide-react'

export default function Settings() {
  const {
    settings,
    fetchSettings,
    updateSetting,
    backupDatabaseNow
  } = useAppStore()

  const [backupPath, setBackupPath] = useState('')
  const [backupInterval, setBackupInterval] = useState('7')
  const [backupResult, setBackupResult] = useState('')
  // v0.22.0: Restore backup + Import from Catalog state
  const [isRestoring, setIsRestoring] = useState(false)
  const [isImporting, setIsImporting] = useState(false)
  const [restoreResult, setRestoreResult] = useState('')
  const [importResult, setImportResult] = useState('')
  const [isBackingUp, setIsBackingUp] = useState(false)
  // v0.15.0: Public catalog config
  const [catalogRepo, setCatalogRepo] = useState('xpunjabi/a-collection-catalog')
  const [catalogBrand, setCatalogBrand] = useState('A Collection Narowal')
  const [catalogWhatsapp, setCatalogWhatsapp] = useState('923420830995')
  const [catalogGithubToken, setCatalogGithubToken] = useState('')
  // v0.16.0: Publish history
  const [publishHistory, setPublishHistory] = useState<any[]>([])

  useEffect(() => {
    fetchSettings()
    // v0.16.0: Load publish history on mount
    loadPublishHistory()
  }, [])

  // v0.16.0: Load publish history from local DB
  const loadPublishHistory = async () => {
    try {
      const history = await invoke<any[]>('get_catalog_publish_history', { limit: 10 })
      setPublishHistory(Array.isArray(history) ? history : [])
    } catch (err) {
      console.warn('Failed to load publish history:', err)
    }
  }

  useEffect(() => {
    if (settings.backup_path) setBackupPath(settings.backup_path)
    if (settings.backup_interval_days) setBackupInterval(settings.backup_interval_days)
    // v0.15.0: Load catalog settings
    if (settings.catalog_repo) setCatalogRepo(settings.catalog_repo)
    if (settings.catalog_brand) setCatalogBrand(settings.catalog_brand)
    if (settings.catalog_whatsapp) setCatalogWhatsapp(settings.catalog_whatsapp)
    if (settings.catalog_github_token) setCatalogGithubToken(settings.catalog_github_token)
  }, [settings])

  const handleSelectBackupPath = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select Backup Folder'
      })
      if (selected && typeof selected === 'string') {
        setBackupPath(selected)
        await updateSetting('backup_path', selected)
      }
    } catch (err) {
      console.error(err)
    }
  }

  const handleSaveBackupSettings = async () => {
    try {
      await updateSetting('backup_path', backupPath)
      await updateSetting('backup_interval_days', backupInterval)
      alert('Backup settings saved!')
    } catch (err) {
      alert(`Failed to save backup settings: ${err}`)
    }
  }

  const handleBackupNow = async () => {
    setIsBackingUp(true)
    setBackupResult('')
    try {
      const dest = await backupDatabaseNow()
      setBackupResult(`Backup successful! Saved to:\n${dest}`)
    } catch (err) {
      setBackupResult(`Backup failed: ${err}`)
    } finally {
      setIsBackingUp(false)
    }
  }

  // v0.22.0: Restore from latest valid backup (auto-picks newest non-empty file)
  const handleRestoreBackup = async () => {
    if (!confirm('This will overwrite your current database with the latest backup.\n\nA safety backup of your current DB will be created first.\n\nContinue?')) {
      return
    }
    setIsRestoring(true)
    setRestoreResult('')
    try {
      // List all backups, pick latest valid
      const backups = await invoke<any[]>('list_backups')
      if (!backups || backups.length === 0) {
        setRestoreResult('No backups found in backup folder.')
        return
      }
      const valid = backups.filter(b => b.is_valid)
      if (valid.length === 0) {
        setRestoreResult('No valid (non-empty) backups found.')
        return
      }
      // Prefer ZIP (full backup) over DB-only
      const zips = valid.filter(b => b.is_zip)
      const target = zips.length > 0 ? zips[0] : valid[0]
      const result = await invoke<string>('restore_backup', { filename: target.name })
      setRestoreResult(`✓ ${result}\n\nRestored from: ${target.name}\n\nPlease RESTART the app for changes to take effect.`)
    } catch (err) {
      setRestoreResult(`Restore failed: ${err}`)
    } finally {
      setIsRestoring(false)
    }
  }

  // v0.22.0: Import products from live catalog.json (recovery feature)
  const handleImportFromCatalog = async () => {
    if (!confirm('This will import products from the live catalog (frontend).\n\nProducts with existing SKUs will be skipped.\n\nPrivate data (cost_price, supplier, sales history) is NOT in catalog.json and cannot be recovered.\n\nContinue?')) {
      return
    }
    setIsImporting(true)
    setImportResult('')
    try {
      const result = await invoke<any>('import_from_catalog_json', { catalogUrl: null })
      setImportResult(
        `✓ Import complete!\n\n` +
        `Catalog products: ${result.total_in_catalog}\n` +
        `Imported: ${result.imported}\n` +
        `Skipped (already exist): ${result.skipped}\n` +
        `Failed: ${result.failed}` +
        (result.errors && result.errors.length > 0 ? `\n\nErrors:\n${result.errors.join('\n')}` : '')
      )
    } catch (err) {
      setImportResult(`Import failed: ${err}`)
    } finally {
      setIsImporting(false)
    }
  }

  const handleSaveCatalogSettings = async () => {
    try {
      await updateSetting('catalog_repo', catalogRepo)
      await updateSetting('catalog_brand', catalogBrand)
      // Normalize WhatsApp number: strip + and spaces, keep digits only
      const cleanWhatsapp = catalogWhatsapp.replace(/[^\d]/g, '')
      await updateSetting('catalog_whatsapp', cleanWhatsapp)
      setCatalogWhatsapp(cleanWhatsapp)
      await updateSetting('catalog_github_token', catalogGithubToken)
      alert('Catalog settings saved!')
    } catch (err) {
      alert(`Failed to save catalog settings: ${err}`)
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight text-white font-display">Settings</h1>
        <p className="text-sm text-gray-400 mt-1">Configure backups, catalog publishing, and system options.</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* AI Provider Settings — REMOVED in v0.37.0 (AI system removed) */}

        {/* Backup Configuration */}
        <div className="glass-card p-5 space-y-4">
          <h2 className="text-lg font-semibold text-white flex items-center">
            <Shield className="mr-2 text-violet-500" size={20} /> Database Backup
          </h2>

          <div>
            <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Backup Location</label>
            <div className="flex space-x-2">
              <input
                type="text"
                value={backupPath}
                readOnly
                placeholder="Select a backup folder..."
                className="flex-1 bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-400 focus:outline-none"
              />
              <button
                onClick={handleSelectBackupPath}
                className="px-3 py-2 bg-slate-800 hover:bg-slate-700 text-gray-200 border border-gray-700 rounded-lg text-sm transition-colors"
              >
                <HardDrive size={16} />
              </button>
            </div>
            {/* Google Drive backup tip */}
            <div className="mt-2 bg-emerald-950/30 border border-emerald-800/40 rounded-lg p-3">
              <p className="text-xs text-emerald-300 font-semibold flex items-center">
                ☁️ Google Drive Auto-Backup Tip
              </p>
              <p className="text-xs text-gray-400 mt-1">
                Install <strong>Google Drive desktop app</strong> on your PC. It creates a folder like <code className="text-emerald-400">C:\Users\YourName\Google Drive</code>. Select that folder as your backup location above — your database will automatically sync to the cloud whenever a backup runs.
              </p>
              <p className="text-xs text-gray-500 mt-1">
                No extra app configuration needed — Drive syncs the file in the background.
              </p>
            </div>
          </div>

          <div>
            <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Backup Interval (Days)</label>
            <select
              value={backupInterval}
              onChange={(e) => setBackupInterval(e.target.value)}
              className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
            >
              <option value="1">Every Day</option>
              <option value="3">Every 3 Days</option>
              <option value="7">Every Week</option>
              <option value="14">Every 2 Weeks</option>
              <option value="30">Every Month</option>
            </select>
          </div>

          <div className="flex space-x-2">
            <button
              onClick={handleSaveBackupSettings}
              className="px-4 py-2 bg-slate-800 hover:bg-slate-700 text-gray-200 border border-gray-700 rounded-lg text-sm transition-colors"
            >
              Save Backup Settings
            </button>
            <button
              onClick={handleBackupNow}
              disabled={isBackingUp}
              className="flex items-center space-x-1 px-4 py-2 bg-violet-600 hover:bg-violet-700 text-white rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
            >
              <RefreshCw size={16} className={isBackingUp ? 'animate-spin' : ''} />
              <span>Backup Now</span>
            </button>
          </div>

          {/* v0.22.0: Restore + Import buttons */}
          <div className="flex space-x-2">
            <button
              onClick={handleRestoreBackup}
              disabled={isRestoring}
              className="flex items-center space-x-1 px-4 py-2 bg-emerald-700 hover:bg-emerald-600 text-white rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
            >
              <History size={16} className={isRestoring ? 'animate-spin' : ''} />
              <span>{isRestoring ? 'Restoring...' : 'Restore Backup'}</span>
            </button>
            <button
              onClick={handleImportFromCatalog}
              disabled={isImporting}
              className="flex items-center space-x-1 px-4 py-2 bg-amber-700 hover:bg-amber-600 text-white rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
              title="Recover product data from live catalog (use if local DB is lost)"
            >
              <Globe size={16} className={isImporting ? 'animate-spin' : ''} />
              <span>{isImporting ? 'Importing...' : 'Import from Catalog'}</span>
            </button>
          </div>

          {restoreResult && (
            <div className="bg-emerald-950/40 border border-emerald-700/50 rounded-lg p-3 text-xs font-mono text-emerald-300 whitespace-pre-wrap">
              {restoreResult}
            </div>
          )}

          {importResult && (
            <div className="bg-amber-950/40 border border-amber-700/50 rounded-lg p-3 text-xs font-mono text-amber-300 whitespace-pre-wrap">
              {importResult}
            </div>
          )}

          {backupResult && (
            <div className="bg-slate-950 border border-gray-800 rounded-lg p-3 text-xs font-mono text-gray-400 whitespace-pre-wrap">
              {backupResult}
            </div>
          )}
        </div>
      </div>

      {/* v0.15.0: Public Catalog Configuration — full width below grid */}
      <div className="glass-card p-5 space-y-4">
        <h2 className="text-lg font-semibold text-white flex items-center">
          <Globe className="mr-2 text-violet-500" size={20} /> Public Catalog (PWA)
        </h2>
        <p className="text-xs text-gray-400">
          Configure the public catalog that customers browse. Head Office publishes products to a separate GitHub repo which is served as a PWA on GitHub Pages. Only public fields (name, price, images, etc.) are published — cost, supplier, and profit stay private.
        </p>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          <div>
            <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">GitHub Repo</label>
            <input
              type="text"
              value={catalogRepo}
              onChange={(e) => setCatalogRepo(e.target.value)}
              placeholder="username/repo-name"
              className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
            />
            <p className="text-[10px] text-gray-600 mt-1">Format: username/repo-name</p>
          </div>
          <div>
            <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">Brand Name</label>
            <input
              type="text"
              value={catalogBrand}
              onChange={(e) => setCatalogBrand(e.target.value)}
              placeholder="A Collection Narowal"
              className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
            />
          </div>
          <div>
            <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">WhatsApp Number</label>
            <input
              type="text"
              value={catalogWhatsapp}
              onChange={(e) => setCatalogWhatsapp(e.target.value)}
              placeholder="923420830995"
              className="w-full bg-slate-950 border border-gray-800 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
            />
            <p className="text-[10px] text-gray-600 mt-1">Digits only, with country code (no +)</p>
          </div>
        </div>

        {/* GitHub Token — full width */}
        <div>
          <label className="block text-xs font-semibold uppercase text-gray-400 mb-1">GitHub Personal Access Token (PAT)</label>
          <div className="relative">
            <Key className="absolute left-3 top-2.5 text-gray-500" size={16} />
            <input
              type="password"
              value={catalogGithubToken}
              onChange={(e) => setCatalogGithubToken(e.target.value)}
              placeholder="ghp_... or github_pat_..."
              className="w-full bg-slate-950 border border-gray-800 rounded-lg pl-10 pr-4 py-2 text-sm text-gray-200 focus:outline-none focus:border-violet-500"
            />
          </div>
          <p className="text-[10px] text-gray-600 mt-1">
            Used to publish catalog updates to the GitHub repo. Create at{' '}
            <a href="https://github.com/settings/tokens" target="_blank" rel="noopener noreferrer" className="text-violet-400 hover:underline">
              github.com/settings/tokens
            </a>{' '}
            with <code className="text-violet-400">repo</code> scope.
          </p>
        </div>

        <div className="bg-violet-950/30 border border-violet-800/40 rounded-lg p-3">
          <p className="text-xs text-violet-300 font-semibold">📡 Catalog URL</p>
          <p className="text-xs text-gray-400 mt-1">
            Once published, your catalog will be live at:{' '}
            <code className="text-violet-400">
              https://{catalogRepo.split('/')[0]}.github.io/{catalogRepo.split('/')[1] || ''}/
            </code>
          </p>
          <p className="text-[10px] text-gray-500 mt-2">
            Go to <strong>Catalog</strong> tab → click <strong>“Publish to Catalog”</strong> button to push your products live.
          </p>
        </div>

        <button
          onClick={handleSaveCatalogSettings}
          className="px-4 py-2 bg-violet-600 hover:bg-violet-700 text-white rounded-lg text-sm font-medium transition-colors"
        >
          Save Catalog Settings
        </button>
      </div>

      {/* v0.16.0: Publish History — shows last 10 publish attempts */}
      <div className="glass-card p-5 space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold text-white flex items-center">
            <History className="mr-2 text-violet-500" size={20} /> Publish History
          </h2>
          <button
            onClick={loadPublishHistory}
            className="p-1.5 text-gray-400 hover:text-white rounded-lg hover:bg-slate-800"
            title="Refresh"
          >
            <RefreshCw size={14} />
          </button>
        </div>
        {publishHistory.length === 0 ? (
          <p className="text-xs text-gray-500 italic py-4 text-center">
            No publish history yet. Click "Publish" in the Catalog tab to push products live.
          </p>
        ) : (
          <div className="space-y-2 max-h-64 overflow-y-auto">
            {publishHistory.map((entry: any) => (
              <div
                key={entry.id}
                className={`flex items-start gap-3 p-3 rounded-lg border ${
                  entry.success
                    ? 'bg-emerald-950/20 border-emerald-800/40'
                    : 'bg-red-950/20 border-red-800/40'
                }`}
              >
                <div className={`shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold ${
                  entry.success ? 'bg-emerald-600 text-white' : 'bg-red-600 text-white'
                }`}>
                  {entry.success ? '✓' : '✕'}
                </div>
                <div className="flex-1 min-w-0 text-xs space-y-1">
                  <div className="flex flex-wrap gap-x-3 gap-y-0.5 text-gray-300">
                    <span className="font-mono">{new Date(entry.published_at).toLocaleString()}</span>
                    <span className="text-gray-500">·</span>
                    <span>{entry.products_count} products</span>
                    <span className="text-gray-500">·</span>
                    <span>{entry.images_uploaded} images up</span>
                    {entry.images_deleted > 0 && (
                      <>
                        <span className="text-gray-500">·</span>
                        <span className="text-red-400">{entry.images_deleted} deleted</span>
                      </>
                    )}
                    <span className="text-gray-500">·</span>
                    <span>{(entry.duration_ms / 1000).toFixed(1)}s</span>
                  </div>
                  {(entry.warnings_count > 0 || entry.errors_count > 0) && (
                    <div className="flex gap-3 text-[10px]">
                      {entry.warnings_count > 0 && (
                        <span className="text-amber-400">⚠ {entry.warnings_count} warnings</span>
                      )}
                      {entry.errors_count > 0 && (
                        <span className="text-red-400">❌ {entry.errors_count} errors</span>
                      )}
                    </div>
                  )}
                  {entry.error_message && (
                    <div className="text-[10px] text-red-400 font-mono break-all">
                      {entry.error_message}
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
