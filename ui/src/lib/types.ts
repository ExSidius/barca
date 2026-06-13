/* ============================================================
   barca · API types
   TypeScript mirrors of the Rust serde output from barca-server.
   Enum-like types are discriminated unions so ts-pattern can
   match them exhaustively — keeping the UI aligned with the
   Rust source of truth.
   ============================================================ */

/** Rust: NodeKind (serde rename_all = "snake_case") */
export type NodeKind = 'asset' | 'sensor' | 'task'

/**
 * Rust: Freshness — `#[serde(tag = "type", content = "value")]` with
 * verbatim (PascalCase) variant names. Schedule carries a cron string.
 */
export type Freshness =
  | { type: 'Always' }
  | { type: 'Manual' }
  | { type: 'Schedule'; value: string }

/** GET /assets — one entry per node */
export interface AssetSummary {
  id: string
  kind: NodeKind
  freshness: Freshness
  inputs: string[]
}

/** GET /plan */
export interface PlanResult {
  total_steps: number
  phases: PlanPhase[]
}

export interface PlanPhase {
  reason: string
  streams: PlanStream[]
}

export interface PlanStream {
  stream_id: string
  steps: string[]
}

/** Rust: RunStatus (in-flight run lifecycle) */
export type RunStatus = 'pending' | 'running' | 'complete' | 'failed'

/** GET /status/{run_id} */
export interface RunState {
  handle: string
  status: RunStatus
  result: GetResult | null
  error: string | null
  started_at: number
  finished_at: number | null
}

export interface GetResult {
  run_id: string
  elapsed_seconds: number
  steps_executed: number
  phases: number
  final_output: OutputRef | null
}

export interface OutputRef {
  path: string
  format: string
  size_bytes: number
  elapsed_seconds?: number
}

/** GET /assets/{name} */
export interface AssetDetail {
  asset: AssetSummary
  stats: AssetStats
}

export interface AssetStats {
  node_id: string
  total_runs: number
  avg_elapsed_seconds: number | null
  median_elapsed_seconds: number | null
  max_elapsed_seconds: number | null
  p95_elapsed_seconds: number | null
  cache_hit_rate: number
  recent_runs: AssetRunEntry[]
}

export interface AssetRunEntry {
  elapsed_seconds: number | null
  status: string
  created_at: string
  error_message: string | null
  attempts: number
}

/** GET /health */
export interface Health {
  status: string
  version: string
}

/**
 * Live run events streamed over SSE from `GET /events/{run_id}`. Mirrors Rust's
 * `RunEvent` (serde tag = "type", snake_case) — see events.rs tests for the
 * exact shapes.
 */
export type RunEvent =
  | { type: 'run_started'; run_id: string }
  | { type: 'log'; node_id: string; line: string }
  | {
      type: 'step_finished'
      node_id: string
      ok: boolean
      elapsed_seconds?: number
      error?: string
    }
  | { type: 'run_finished'; run_id: string; ok: boolean }

/** A single captured log line, as rendered by the LogViewer. */
export interface LogLine {
  nodeId: string
  text: string
}

/** GET /logs/{run_id} */
export interface LogEntry {
  node_id: string
  seq: number
  line: string
}

/** POST /run, /run/{target}, /get/{target} */
export interface RunHandle {
  run_id: string
}

/**
 * Visual run state used across status components. Superset of the
 * server's lifecycle plus the asset-level states the design system
 * defines (queued, skipped, warning, success).
 */
export type StatusKind =
  | 'success'
  | 'running'
  | 'failed'
  | 'warning'
  | 'queued'
  | 'skipped'
  | 'pending'
