/** Waits one macrotask, so pending promises and mocked IPC calls settle. */
export const tick = () => new Promise((r) => setTimeout(r, 0))
