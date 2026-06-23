// The context path the app is served under, injected by the backend into
// index.html (e.g. "/webssh"). Empty string means root deployment.
// All API/WS/router base URLs are built off this value.
export const CONTEXT_PATH: string = window.__CONTEXT_PATH__ ?? ''

/** Build an absolute app path, ensuring a leading slash. e.g. path('api') → '/webssh/api'. */
export function appPath(p: string): string {
  const prefix = CONTEXT_PATH
  const suffix = p.startsWith('/') ? p : `/${p}`
  return prefix === '' ? suffix : `${prefix}${suffix}`
}
