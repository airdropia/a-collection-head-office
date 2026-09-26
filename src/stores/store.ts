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

interface AppState {
  // Navigation & UI
  currentTab: string;
  setCurrentTab: (tab: string) => void;

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
}

export const useAppStore = create<AppState>((set, get) => ({
  // Navigation & UI Defaults
  currentTab: 'dashboard',
  setCurrentTab: (tab) => set({ currentTab: tab }),

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
}));
