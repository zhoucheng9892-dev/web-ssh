/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<{}, {}, any>
  export default component
}

// Injected by the backend into index.html (e.g. "/webssh"); "" for root deploy.
interface Window {
  __CONTEXT_PATH__?: string
}

