import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'

export interface Product {
  id?: number;
  sku: string;
  name: string;
  category?: string;
  color?: string;
  design?: string;
  season?: string;
  cost_price: number;
  sale_price: number;
  purchase_price: number;
  description?: string;
  tags?: string;
  stock_quantity: number;
  status: string;
  images: string;
  supplier_id?: number;
  created_at?: string;
  updated_at?: string;
  // v0.11.0+ profit-mode fields (optional for backward compat)
  product_code?: string;
  brand?: string;
  fabric?: string;
  size_info?: string;
  base_unit_cost?: number;
  landed_unit_cost?: number;
  retail_price?: number;
  discount_price?: number;
  source_trip_id?: number;
  qty_in_head_office?: number;
  qty_with_agents?: number;
  qty_sold?: number;
  qty_reserved?: number;
  profit_status?: string;
}

export interface Customer {
  id?: number;
  name: string;
  phone?: string;
  location?: string;
  notes?: string;
  created_at?: string;
  // v0.26.0: Udhar/Credit tracking
  outstanding_balance?: number;
  segment?: string;
}

export interface OrderItemInput {
  product_id: number;
  quantity: number;
}

export interface OrderItemDetail {
  product_name: string;
  sku: string;
  quantity: number;
  sale_price: number;
}

export interface OrderHistory {
  order_id: number;
  order_date: string;
  total_amount: number;
  profit: number;
  items: OrderItemDetail[];
}

export interface ProductDraft {
  name?: string;
  sku?: string;
  category?: string;
  brand?: string;
  fabric?: string;
  color?: string;
  design?: string;
  season?: string;
  cost_price?: number;
  sale_price?: number;
  retail_price?: number;
  description?: string;
  tags?: string[];
  keywords?: string[];
  hashtags?: string[];
  images?: string[];
}

export interface LocalMatchResult {
  item_id: string;
  title: string;
  design_code?: string;
  confidence: number;
}

export interface CatalogDraft {
  title: string;
  brand?: string;
  fabric?: string;
  design_code?: string;
  notes?: string;
  web_evidence_count?: number;
  web_evidence_snippets?: string[];
  best_image_url?: string;
  // v0.13.8: Price fields for Edit Draft
  cost_price?: number;
  retail_price?: number;
  sale_price?: number;
  // v0.13.9: Locally saved image filename (from user upload)
  saved_image_filename?: string;
  // v0.14.5: Catalog metadata fields. AI generates these from the product
  // image + OCR + web evidence so they flow through to the products table
  // when the user clicks "Add to Catalog" — previously they were always
  // empty and the user had to fill them manually in the Catalog form.
  category?: string;
  season?: string;
  gender?: string;
  color?: string;
}

export type AssistantResult =
  | { type: "LocalMatchFound"; data: LocalMatchResult }
  | { type: "NewCatalogDraft"; data: CatalogDraft };

export interface AiResponse {
  text: string;
  detected_action?: string;
  action_data?: any;
  product_draft?: ProductDraft;
  confidence?: number;
  missing_fields?: string[];
  suggested_actions?: string[];
  fast_path_data?: AssistantResult;
}

export interface MarketingContent {
  platform: string;
  content: string;
  caption_type: string;
}

export interface MarketingPost {
  short_caption: string;
  long_caption: string;
  hashtags: string[];
}


interface AppState {
  // Navigation & UI
  currentTab: string;
  setCurrentTab: (tab: string) => void;
  showAiAssistant: boolean;
  setVectorAssistant: (show: boolean) => void;
  aiWorkspaceWidth: number;
  setAiWorkspaceWidth: (width: number) => void;

  // Products
  products: Product[];
  fetchProducts: () => Promise<void>;
  addProduct: (product: Product) => Promise<number>;
  updateProduct: (product: Product) => Promise<void>;
  deleteProduct: (id: number) => Promise<void>;
  exportProductsCsv: () => Promise<string>;
  importProductsCsv: (csvContent: string) => Promise<void>;
  uploadProductImage: (srcPath: string, formatType: string) => Promise<string>;

  // Customers
  customers: Customer[];
  fetchCustomers: () => Promise<void>;
  addCustomer: (customer: Customer) => Promise<void>;
  updateCustomer: (customer: Customer) => Promise<void>;
  deleteCustomer: (id: number) => Promise<void>;
  createOrder: (customerId: number, items: OrderItemInput[]) => Promise<number>;
  getCustomerHistory: (customerId: number) => Promise<OrderHistory[]>;

  // Cart (for placing orders)
  cart: { product: Product; quantity: number }[];
  addToCart: (product: Product, quantity: number) => void;
  removeFromCart: (productId: number) => void;
  clearCart: () => void;

  // Settings
  settings: Record<string, string>;
  fetchSettings: () => Promise<void>;
  updateSetting: (key: string, value: string) => Promise<void>;
  backupDatabaseNow: () => Promise<string>;

  // AI Product Drafts
  aiProductDrafts: { draft: ProductDraft; confidence: number; missingFields: string[]; suggestedActions: string[] }[];
  addAiProductDraft: (draft: ProductDraft, confidence: number, missingFields: string[], suggestedActions: string[]) => void;
  removeAiProductDraft: (index: number) => void;
  updateAiProductDraft: (index: number, draft: ProductDraft) => void;
  addDraftToCatalog: (draft: ProductDraft) => Promise<number>;
  marketingForProduct: (productId: number) => Promise<void>;

  // Workspace assets

  // AI Assistant Chat
  aiMessages: { role: 'user' | 'assistant'; text: string; action?: string; product_draft?: ProductDraft; confidence?: number; missing_fields?: string[]; suggested_actions?: string[]; fast_path_data?: AssistantResult; social_post?: MarketingPost; image_data?: string }[];
  isAiLoading: boolean;
  sendAiMessage: (prompt: string, imageData?: string) => Promise<void>;
  clearAiChat: () => void;
  removeAiMessage: (index: number) => void;
}

export const useAppStore = create<AppState>((set, get) => ({
  // Navigation & UI Defaults
  currentTab: 'dashboard',
  setCurrentTab: (tab) => set({ currentTab: tab }),
  showAiAssistant: true,
  setVectorAssistant: (show) => set({ showAiAssistant: show }),
  aiWorkspaceWidth: 35,
  setAiWorkspaceWidth: (width) => set({ aiWorkspaceWidth: width }),

  // Products
  products: [],
  fetchProducts: async () => {
    try {
      const products: Product[] = await invoke('get_products');
      set({ products });
    } catch (err) {
      console.error(err);
    }
  },
  addProduct: async (product) => {
    try {
      const id: number = await invoke('add_product', { product });
      await get().fetchProducts();
      return id;
    } catch (err) {
      throw new Error(String(err));
    }
  },
  updateProduct: async (product) => {
    try {
      await invoke('update_product', { product });
      await get().fetchProducts();
    } catch (err) {
      throw new Error(String(err));
    }
  },
  deleteProduct: async (id) => {
    try {
      await invoke('delete_product', { id });
      await get().fetchProducts();
    } catch (err) {
      throw new Error(String(err));
    }
  },
  exportProductsCsv: async () => {
    try {
      return await invoke<string>('export_products_csv');
    } catch (err) {
      throw new Error(String(err));
    }
  },
  importProductsCsv: async (csvContent) => {
    try {
      await invoke('import_products_csv', { csvContent });
      await get().fetchProducts();
    } catch (err) {
      throw new Error(String(err));
    }
  },
  uploadProductImage: async (srcPath, formatType) => {
    try {
      return await invoke<string>('upload_product_image', { srcPath, formatType });
    } catch (err) {
      throw new Error(String(err));
    }
  },

  // Customers
  customers: [],
  fetchCustomers: async () => {
    try {
      const customers: Customer[] = await invoke('get_customers');
      set({ customers });
    } catch (err) {
      console.error(err);
    }
  },
  addCustomer: async (customer) => {
    try {
      await invoke('add_customer', { customer });
      await get().fetchCustomers();
    } catch (err) {
      throw new Error(String(err));
    }
  },
  updateCustomer: async (customer) => {
    try {
      await invoke('update_customer', { customer });
      await get().fetchCustomers();
    } catch (err) {
      throw new Error(String(err));
    }
  },
  deleteCustomer: async (id) => {
    try {
      await invoke('delete_customer', { id });
      await get().fetchCustomers();
    } catch (err) {
      throw new Error(String(err));
    }
  },
  createOrder: async (customerId, items) => {
    try {
      const orderId = await invoke<number>('create_order', { customerId, items });
      await get().fetchProducts(); // Refresh stock
      return orderId;
    } catch (err) {
      throw new Error(String(err));
    }
  },
  getCustomerHistory: async (customerId) => {
    try {
      return await invoke<OrderHistory[]>('get_customer_history', { customerId });
    } catch (err) {
      throw new Error(String(err));
    }
  },

  // Cart
  cart: [],
  addToCart: (product, quantity) => {
    const cart = get().cart;
    const existing = cart.find((item) => item.product.id === product.id);
    if (existing) {
      set({
        cart: cart.map((item) =>
          item.product.id === product.id
            ? { ...item, quantity: item.quantity + quantity }
            : item
        ),
      });
    } else {
      set({ cart: [...cart, { product, quantity }] });
    }
  },
  removeFromCart: (productId) => {
    set({ cart: get().cart.filter((item) => item.product.id !== productId) });
  },
  clearCart: () => set({ cart: [] }),

  // Settings
  settings: {},
  fetchSettings: async () => {
    try {
      const settings: Record<string, string> = await invoke('get_settings');
      set({ settings });
    } catch (err) {
      console.error(err);
    }
  },
  updateSetting: async (key, value) => {
    try {
      await invoke('update_setting', { key, value });
      await get().fetchSettings();
    } catch (err) {
      throw new Error(String(err));
    }
  },
  backupDatabaseNow: async () => {
    try {
      return await invoke<string>('backup_database_now');
    } catch (err) {
      throw new Error(String(err));
    }
  },

  // AI Product Drafts
  aiProductDrafts: [],
  addAiProductDraft: (draft, confidence, missingFields, suggestedActions) => set((state) => {
    // Deduplicate by SKU (preferred) or name (fallback). If a draft with the
    // same SKU already exists in the in-memory list, replace it instead of
    // pushing a duplicate. This is a defensive layer — the backend
    // `ask_ai` command in v0.9.5+ should already short-circuit duplicate
    // drafts, but this guard prevents any future regression from surfacing
    // as a duplicate UI card.
    const sku = (draft.sku || '').trim().toLowerCase();
    const name = (draft.name || '').trim().toLowerCase();
    const key = sku || name;
    let list = [...state.aiProductDrafts];
    if (key) {
      list = list.filter((d) => {
        const dSku = (d.draft.sku || '').trim().toLowerCase();
        const dName = (d.draft.name || '').trim().toLowerCase();
        return (dSku || dName) !== key;
      });
    }
    list.push({ draft, confidence, missingFields, suggestedActions });
    return { aiProductDrafts: list };
  }),
  removeAiProductDraft: (index) => set((state) => ({
    aiProductDrafts: state.aiProductDrafts.filter((_, i) => i !== index),
  })),
  updateAiProductDraft: (index, draft) => set((state) => ({
    aiProductDrafts: state.aiProductDrafts.map((d, i) => i === index ? { ...d, draft } : d),
  })),
  addDraftToCatalog: async (draft) => {
    try {
      const id: number = await invoke('save_product_draft_to_catalog', { draft });
      await get().fetchProducts();
      // Auto-generate marketing content
      try {
        await get().marketingForProduct(id);
      } catch {}
      return id;
    } catch (err) {
      throw new Error(String(err));
    }
  },
  marketingForProduct: async (productId) => {
    try {
      await invoke<MarketingContent[]>('generate_marketing', { productId });
    } catch (err) {
      console.error('Marketing generation failed:', err);
    }
  },

  // Workspace assets

  // AI Assistant Chat
  aiMessages: [
    {
      role: 'assistant',
      text: '👋 Hello! I am your AI Business Assistant.\n\nI can help you with:\n• **Product Intake** — Drop an image, link, or paste product code\n• **Catalog Management** — Create and edit product drafts\n• **Marketing Content** — Generate posts for Facebook, WhatsApp, Instagram\n• **Inventory Insights** — Check stock, low items, dead stock\n• **Business Advice** — Purchasing decisions, sales analysis\n\nTry sending a product image or paste a product link!',
    },
  ],
  isAiLoading: false,
  sendAiMessage: async (prompt, imageData) => {
    if (!prompt.trim() && !imageData) return;

    // Add user message to chat
    const userMsg: any = { role: 'user', text: prompt || (imageData ? '[Image uploaded]' : ''), image_data: imageData || undefined };
    set((state) => ({
      aiMessages: [...state.aiMessages, userMsg],
      isAiLoading: true,
    }));

    try {
      // v0.12.4: Pass conversation history (last 10 messages) to ask_ai so
      // the AI can maintain context across turns. We exclude the initial
      // greeting message (role=assistant with no preceding user message)
      // and skip empty/system messages. We also skip the user message we
      // just added above (it's already in aiMessages at this point, but we
      // want history BEFORE the current prompt).
      const currentMessages = get().aiMessages
      // Take all messages except the last one (which is the user message we
      // just added) and the very first greeting. Limit to last 10 to keep
      // the payload small.
      const history = currentMessages
        .slice(1, -1) // skip greeting (index 0) and current user msg (last)
        .slice(-10)   // last 10 messages
        .filter(m => m.text && m.text.trim().length > 0)
        .map(m => ({
          role: m.role === 'user' ? 'user' : 'assistant',
          content: m.text,
        }))
      const response: AiResponse = await invoke('ask_ai', {
        prompt,
        imageData: imageData || null,
        history: history.length > 0 ? history : null,
      });

      // v0.13.9: Save user-uploaded image for BOTH code paths:
      // - product_draft (fallback path) 
      // - fast_path_data (fast path — THIS WAS THE BUG! Image was never saved
      //   because only product_draft was checked, but fast_path_data is the
      //   one that's populated when AI generates a catalog draft from image)
      if (imageData) {
        try {
          const savedName = await invoke<string>('save_base64_image', {
            base64Data: imageData,
            formatType: 'thumbnail',
          })
          // Save image filename to product_draft if it exists
          if (response.product_draft) {
            response.product_draft.images = [savedName]
          }
          // Save image filename to fast_path_data draft if it exists
          if (response.fast_path_data && response.fast_path_data.type === 'NewCatalogDraft') {
            // Store the saved image filename on the message so AiWorkspace
            // can pass it to save_catalog_draft when user clicks Add to Catalog
            // We'll store it as a separate field that AiWorkspace reads
          }
          // Store savedName on the response so frontend can use it
          ;(response as any)._saved_image = savedName
        } catch (err) {
          console.error('Failed to save image from AI:', err)
        }
      }

      const assistantMsg: any = {
        role: 'assistant',
        text: response.text,
        action: response.detected_action,
        product_draft: response.product_draft,
        confidence: response.confidence,
        missing_fields: response.missing_fields,
        suggested_actions: response.suggested_actions,
        fast_path_data: response.fast_path_data,
        image_data: imageData || undefined,
        // v0.13.9: Pass saved image filename so AiWorkspace can inject it
        // into the draft when user clicks "Add to Catalog"
        saved_image: (response as any)._saved_image || undefined,
      };

      set((state) => ({
        // v0.14.4: Cap aiMessages at 50 entries (was unbounded — after 100+
        // interactions every set() re-diffed an ever-growing array, slowing
        // the chat panel). Keep the initial greeting + last 49 messages.
        aiMessages: [...state.aiMessages, assistantMsg].slice(-50),
        isAiLoading: false,
      }));

      // Add draft to drafts store if detected
      if (response.product_draft && response.confidence) {
        get().addAiProductDraft(
          response.product_draft,
          response.confidence,
          response.missing_fields || [],
          response.suggested_actions || []
        );
      }

      if (response.detected_action === 'low_stock') {
        await get().fetchProducts();
      }
    } catch (err) {
      set((state) => ({
        aiMessages: [
          ...state.aiMessages,
          {
            role: 'assistant',
            text: `Error: ${String(err)}`,
          },
        ],
        isAiLoading: false,
      }));
    }
  },
  clearAiChat: () =>
    set({
      aiMessages: [
        {
          role: 'assistant',
          text: 'Chat history cleared. I\'m ready to help with product intake, marketing, or business questions.',
        },
      ],
    }),
  removeAiMessage: (index) => set((state) => ({
    aiMessages: state.aiMessages.filter((_, i) => i !== index),
  })),
}));
