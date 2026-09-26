/**
 * Social sharing utility — reusable helper used by Catalog.tsx and
 * ShareCenter.tsx to share product content to social platforms.
 *
 * All sharing is done via platform-specific share URLs (no API keys, no OAuth).
 * - WhatsApp: wa.me/?text=... (works on web + mobile, opens WhatsApp app)
 * - Facebook: facebook.com/sharer/sharer.php?quote=... (opens FB share dialog)
 * - Twitter/X: twitter.com/intent/tweet?text=... (opens Tweet composer)
 * - Instagram: no web share URL — copy to clipboard + alert user (IG requires
 *   in-app paste because Instagram does not support third-party image/text
 *   share intents on web)
 *
 * CRITICAL (Tauri desktop context):
 * In a Tauri webview, window.open() does NOT open the URL in the user's
 * default system browser — it tries to navigate the webview itself, which
 * fails for cross-origin URLs due to CSP/security restrictions. To open
 * external links in the user's actual browser (Chrome, Firefox, Edge),
 * we must use @tauri-apps/plugin-shell's open() function.
 *
 * We detect the Tauri environment at runtime and dispatch accordingly:
 *   - Tauri desktop: use shell.open(url)
 *   - Plain web (Firebase preview): use window.open(url, '_blank')
 *
 * This keeps the share utility working in BOTH contexts without any
 * conditional imports or build-time flags.
 */

// Detect Tauri environment at module load. The Tauri webview injects a
// `window.__TAURI_INTERNALS__` global; in plain browsers this is undefined.
// We also check window.__TAURI__ for older Tauri versions.
const IS_TAURI = typeof window !== 'undefined'
  && (Boolean((window as any).__TAURI_INTERNALS__) || Boolean((window as any).__TAURI__));

/**
 * Lazy-load the Tauri shell open() function. We use dynamic import so that
 * the @tauri-apps/plugin-shell package is NOT bundled when running in a
 * plain web context (e.g., the Firebase preview). This avoids runtime
 * errors when the Tauri runtime is not present.
 */
async function openExternalUrl(url: string): Promise<void> {
  if (IS_TAURI) {
    try {
      const { open } = await import('@tauri-apps/plugin-shell');
      await open(url);
      return;
    } catch (err) {
      // Fallback to window.open() if the shell plugin fails for any reason
      // (e.g., permission denied, plugin not registered, scope mismatch).
      console.warn('[share] Tauri shell.open() failed, falling back to window.open():', err);
    }
  }
  // Plain web context OR Tauri fallback — use window.open()
  window.open(url, '_blank', 'noopener,noreferrer');
}

export type SharePlatform = 'whatsapp' | 'facebook' | 'twitter/x' | 'instagram'

export const ALL_SHARE_PLATFORMS: SharePlatform[] = ['whatsapp', 'facebook', 'twitter/x', 'instagram']

export const PLATFORM_LABELS: Record<SharePlatform, string> = {
  'whatsapp': 'WhatsApp',
  'facebook': 'Facebook',
  'twitter/x': 'Twitter/X',
  'instagram': 'Instagram',
}

/**
 * Build a shareable text block for a product.
 * Format: "<name>\n<design>\nRs.<sale_price>\n\n<description>"
 * Truncated to 1000 chars to stay within WhatsApp/Twitter limits.
 */
export function buildProductShareText(opts: {
  name: string
  design?: string | null
  salePrice?: number | null
  retailPrice?: number | null
  description?: string | null
  hashtags?: string[] | null
  includeHashtags?: boolean
}): string {
  const parts: string[] = []
  if (opts.name) parts.push(opts.name)
  if (opts.design) parts.push(opts.design)
  if (opts.salePrice != null) {
    parts.push(`Rs. ${opts.salePrice.toFixed(0)}`)
    if (opts.retailPrice != null && opts.retailPrice > opts.salePrice) {
      const discount = Math.round(((opts.retailPrice - opts.salePrice) / opts.retailPrice) * 100)
      parts.push(`(${discount}% off from Rs. ${opts.retailPrice.toFixed(0)})`)
    }
  }
  if (opts.description) parts.push('', opts.description)
  if (opts.includeHashtags && opts.hashtags && opts.hashtags.length > 0) {
    parts.push('', opts.hashtags.map(h => h.startsWith('#') ? h : `#${h}`).join(' '))
  }
  let text = parts.join('\n')
  if (text.length > 1000) text = text.slice(0, 997) + '...'
  return text
}

/**
 * Share `text` to the given platform.
 *
 * v0.14.7: HONEST SIMPLIFICATION.
 *
 * Technical reality: browsers (and Tauri's webview) CANNOT auto-attach
 * images to social media posts via URL clicks. This is a hard security
 * restriction imposed by the platforms (FB, IG, WhatsApp). The only ways
 * to truly automate image sharing are paid APIs (WhatsApp Business API,
 * Facebook Graph API) which the user has ruled out.
 *
 * Best realistic approach: OPEN the image in the OS default viewer
 * (Windows Photos) so it's VISIBLE on screen, AND open the social
 * platform in the browser. User drags from Photos to the browser.
 * No folder-hunting required — image is right there on screen.
 *
 * @param platform    — 'whatsapp' | 'facebook' | 'twitter/x' | 'instagram'
 * @param text        — caption text to share
 * @param imageData   — base64-encoded JPEG image (optional)
 * @param productName — product name for the saved filename (optional)
 */
export async function shareToPlatform(
  platform: SharePlatform,
  text: string,
  imageData?: string | null,
  productName?: string | null,
): Promise<boolean> {
  const encoded = encodeURIComponent(text)
  let url = ''

  // v0.14.8: Save image to a dedicated folder AND open Windows Explorer
  // with the file highlighted (done in Rust via explorer.exe /select).
  // The user sees Explorer window with their image already selected —
  // they just drag it to the browser. No folder-hunting, no Photos app
  // dependency (which was unreliable on Windows 10).
  let savedImagePath: string | null = null
  let explorerOpened = false
  if (imageData && IS_TAURI) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      savedImagePath = await invoke<string>('save_image_for_share', {
        base64Data: imageData,
        productName: productName || 'product',
      })
      // If invoke succeeded, the Rust side already opened Explorer.
      // (explorer.exe /select,<path> is called from Rust on Windows)
      explorerOpened = true
    } catch (err) {
      console.warn('[share] Could not save image / open Explorer:', err)
    }
  }

  // Copy caption text to clipboard — user pastes it into the platform's
  // text box with Ctrl+V.
  let textOnClipboard = false
  try {
    await navigator.clipboard.writeText(text)
    textOnClipboard = true
  } catch {
    // Clipboard writeText might fail in some Tauri contexts
  }

  // Build a SHORT, clear alert. v0.14.8: Explorer is already open with
  // the image selected — user just drags from Explorer to browser.
  const buildAlertMsg = (platformName: string): string => {
    let msg = `${platformName} opening now!\n\n`
    if (explorerOpened && savedImagePath) {
      // Extract just the filename for display
      const filename = savedImagePath.split(/[\\/]/).pop() || savedImagePath
      msg += `📁 Windows Explorer open ho gaya hai — image select hai:\n`
      msg += `   ${filename}\n\n`
      msg += `📋 Caption clipboard pe copy ho gaya hai.\n\n`
      msg += `${platformName} pe:\n`
      msg += `1. Text box me Ctrl+V → caption paste ho jayegi\n`
      msg += `2. Explorer me select ki hui image ko drag karke post me drop karein\n\n`
      msg += `(Dono windows screen pe hain — Explorer se browser me drag karein)\n`
    } else if (savedImagePath) {
      // Image saved but Explorer didn't open — fall back to folder path
      const filename = savedImagePath.split(/[\\/]/).pop() || savedImagePath
      const folder = savedImagePath.split(/[\\/]/).slice(-2, -1)[0] || 'A-Collection-Share'
      msg += `📷 Image saved: ${folder}\\${filename}\n`
      msg += `📋 Caption clipboard pe hai.\n\n`
      msg += `${platformName} pe:\n`
      msg += `1. Ctrl+V se caption paste karein\n`
      msg += `2. Image ${folder} folder se drag karein\n\n`
    } else if (textOnClipboard) {
      msg += `📋 Caption clipboard pe copy ho gaya hai.\n`
      msg += `Ctrl+V se paste karein.\n\n`
      msg += `(Image save nahi ho payi — manual attach karein.)\n`
    } else {
      msg += `Caption neeche box me paste karein.\n`
    }
    return msg
  }

  switch (platform) {
    case 'whatsapp':
      url = `https://wa.me/?text=${encoded}`
      alert(buildAlertMsg('WhatsApp'))
      break
    case 'facebook':
      url = 'https://www.facebook.com/'
      alert(buildAlertMsg('Facebook'))
      await openExternalUrl(url)
      return true
    case 'twitter/x':
      url = `https://twitter.com/intent/tweet?text=${encoded}`
      alert(buildAlertMsg('Twitter/X'))
      break
    case 'instagram':
      alert(buildAlertMsg('Instagram'))
      return true
    default:
      return false
  }
  await openExternalUrl(url)
  return true
}
