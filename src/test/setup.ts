import '@testing-library/jest-dom/vitest'
import { vi } from 'vitest'

// The Tauri runtime isn't available in tests.
vi.mock('@tauri-apps/api/app', () => ({ getVersion: vi.fn(() => Promise.resolve('0.1.0')) }))
