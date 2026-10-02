/**
 * Asset table presentation logic — pure, exhaustively matched.
 *
 * Turns `GET /state` rows into what the table shows: a severity per node (what
 * needs attention sorts first), a short state label with a longer hint, the
 * latest attempt, typical durations and the next scheduled run. Returns plain
 * descriptors; `AssetsPage` only maps them to elements.
 */

import { match } from 'ts-pattern'
import type { CacheState, NodeState, StatusKind } from './types'

/** How much a node needs attention, most urgent first. */
export type Severity =
  | 'failed'
  | 'stale'
  | 'missing'
  | 'partial'
  | 'unknown'
  | 'always_runs'
  | 'fresh'

export const SEVERITIES: readonly Severity[] = [
  'failed',
  'stale',
  'missing',
  'partial',
  'unknown',
  'always_runs',
  'fresh',
]

export interface AssetRow {
  id: string
  /** Function name — the last `:` segment of the id. */
  name: string
  /** Source file — everything before the last `:`. */
  file: string
  kind: NodeState['kind']
  severity: Severity
  /** Short label for the state column, e.g. `stale · code`. */
  stateLabel: string
  /** One sentence explaining the state (tooltip). */
  stateHint: string
  last: { status: string; ago: string; error: string | null } | null
  /** Median of recent successful runs, formatted; null if it never succeeded. */
  typical: string | null
  p95: string | null
  /** Cron expression, if scheduled. */
  schedule: string | null
  /** Next cron fire time (ms since epoch), if scheduled. */
  nextRunMs: number | null
}

/** A failed latest attempt outranks any cache state — it's the thing to look at. */
export function severityOf(node: NodeState): Severity {
  if (node.last?.status === 'failed') return 'failed'
  return match(node.cache)
    .with({ state: 'fresh' }, (): Severity => 'fresh')
    .with({ state: 'stale' }, (): Severity => 'stale')
    .with({ state: 'missing' }, (): Severity => 'missing')
    .with({ state: 'partial' }, (): Severity => 'partial')
    .with({ state: 'unknown' }, (): Severity => 'unknown')
    .with({ state: 'always_runs' }, (): Severity => 'always_runs')
    .exhaustive()
}

/** Severity → the design system's status vocabulary (colors). */
export function severityStatus(severity: Severity): StatusKind {
  return match(severity)
    .with('failed', (): StatusKind => 'failed')
    .with('stale', 'partial', (): StatusKind => 'warning')
    .with('missing', (): StatusKind => 'queued')
    .with('unknown', 'always_runs', (): StatusKind => 'skipped')
    .with('fresh', (): StatusKind => 'success')
    .exhaustive()
}

function describeCache(cache: CacheState): { label: string; hint: string } {
  return match(cache)
    .with({ state: 'fresh' }, () => ({
      label: 'fresh',
      hint: 'The cached result matches the current code and inputs; get reuses it.',
    }))
    .with({ state: 'stale', cause: 'code' }, () => ({
      label: 'stale · code',
      hint: 'Every upstream is fresh, so its own code (or code it calls) changed since it last ran.',
    }))
    .with({ state: 'stale', cause: 'upstream' }, () => ({
      label: 'stale · upstream',
      hint: 'Something upstream will recompute first.',
    }))
    .with({ state: 'missing' }, () => ({
      label: 'missing',
      hint: 'Never materialized successfully.',
    }))
    .with({ state: 'partial' }, ({ cached, total }) => ({
      label: `partial · ${cached}/${total}`,
      hint: `${cached} of ${total} partition keys are cached; the rest would run.`,
    }))
    .with({ state: 'always_runs' }, () => ({
      label: 'always runs',
      hint: 'Tasks and sensors are never cached; they run every time.',
    }))
    .with({ state: 'unknown' }, () => ({
      label: 'unknown',
      hint: "Depends on dynamic partitions whose source hasn't run, so its keys aren't known yet.",
    }))
    .exhaustive()
}

function splitId(id: string): { file: string; name: string } {
  const i = id.lastIndexOf(':')
  return i < 0 ? { file: '', name: id } : { file: id.slice(0, i), name: id.slice(i + 1) }
}

const RANK: Record<Severity, number> = Object.fromEntries(
  SEVERITIES.map((s, i) => [s, i]),
) as Record<Severity, number>

/** One row per node, sorted by severity, then name. */
export function buildRows(nodes: NodeState[], nowMs: number): AssetRow[] {
  return nodes
    .map((n): AssetRow => {
      const { file, name } = splitId(n.id)
      const { label, hint } = describeCache(n.cache)
      const severity = severityOf(n)
      return {
        id: n.id,
        name,
        file,
        kind: n.kind,
        severity,
        stateLabel: severity === 'failed' ? `failed · ${label}` : label,
        stateHint:
          severity === 'failed'
            ? `The latest attempt failed. Cache: ${label}. ${hint}`
            : hint,
        last: n.last
          ? {
              status: n.last.status,
              ago: formatAgo(n.last.created_at, nowMs),
              error: n.last.error_message,
            }
          : null,
        typical: n.durations ? formatSeconds(n.durations.median_seconds) : null,
        p95: n.durations ? formatSeconds(n.durations.p95_seconds) : null,
        schedule: n.freshness.type === 'Schedule' ? n.freshness.value : null,
        nextRunMs: n.next_run === null ? null : n.next_run * 1000,
      }
    })
    .sort((a, b) => RANK[a.severity] - RANK[b.severity] || a.name.localeCompare(b.name))
}

/** Case-insensitive substring match on name or full id; blank keeps all. */
export function filterRows(rows: AssetRow[], query: string): AssetRow[] {
  const q = query.trim().toLowerCase()
  if (!q) return rows
  return rows.filter((r) => r.name.toLowerCase().includes(q) || r.id.toLowerCase().includes(q))
}

/** Count of rows per severity (every severity present, zero or not). */
export function summarize(rows: AssetRow[]): Record<Severity, number> {
  const out = Object.fromEntries(SEVERITIES.map((s) => [s, 0])) as Record<Severity, number>
  for (const r of rows) out[r.severity] += 1
  return out
}

export function formatSeconds(s: number): string {
  if (s < 60) return `${s.toFixed(1)}s`
  if (s < 3600) return `${(s / 60).toFixed(1)}m`
  return `${(s / 3600).toFixed(1)}h`
}

/** `created_at` is UTC `YYYY-MM-DD HH:MM:SS` (SQLite `datetime('now')`). */
export function formatAgo(createdAt: string, nowMs: number): string {
  const t = Date.parse(`${createdAt.replace(' ', 'T')}Z`)
  if (Number.isNaN(t)) return createdAt
  const s = Math.max(0, (nowMs - t) / 1000)
  if (s < 60) return 'just now'
  if (s < 3600) return `${Math.floor(s / 60)}m ago`
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`
  return `${Math.floor(s / 86400)}d ago`
}
