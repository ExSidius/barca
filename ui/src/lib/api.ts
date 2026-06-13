/* ============================================================
   barca · API client
   Thin fetch wrappers over barca-server. In dev these go through
   the Vite proxy (/api → 127.0.0.1:8274); in production they are
   same-origin since the Rust server serves the built UI.
   ============================================================ */

import type {
  AssetSummary,
  AssetDetail,
  PlanResult,
  RunState,
  RunHandle,
  Health,
} from './types'

const BASE = '/api'

class ApiError extends Error {
  status: number

  constructor(status: number, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { 'content-type': 'application/json' },
    ...init,
  })
  if (!res.ok) {
    let message = res.statusText
    try {
      const body = (await res.json()) as { error?: string }
      if (body.error) message = body.error
    } catch {
      // non-JSON error body; keep statusText
    }
    throw new ApiError(res.status, message)
  }
  return res.json() as Promise<T>
}

export const api = {
  health: () => request<Health>('/health'),
  assets: () => request<AssetSummary[]>('/assets'),
  asset: (name: string) => request<AssetDetail>(`/assets/${encodeURIComponent(name)}`),
  plan: () => request<PlanResult>('/plan'),
  status: (runId: string) => request<RunState>(`/status/${encodeURIComponent(runId)}`),
  run: () => request<RunHandle>('/run', { method: 'POST' }),
  runTarget: (target: string) =>
    request<RunHandle>(`/run/${encodeURIComponent(target)}`, { method: 'POST' }),
  getTarget: (target: string) =>
    request<RunHandle>(`/get/${encodeURIComponent(target)}`, { method: 'POST' }),
}

export { ApiError }
