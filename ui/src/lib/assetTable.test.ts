import { describe, expect, it } from 'vitest'
import type { NodeState } from './types'
import {
  buildRows,
  filterRows,
  formatAgo,
  formatSeconds,
  severityOf,
  summarize,
} from './assetTable'

const NOW = Date.parse('2026-10-02T12:00:00Z')

/** The single row of a one-node table. */
function only<T>(xs: T[]): T {
  expect(xs).toHaveLength(1)
  return xs[0]!
}

function node(over: Partial<NodeState> & { id: string }): NodeState {
  return {
    kind: 'asset',
    freshness: { type: 'Always' },
    cache: { state: 'fresh' },
    last: null,
    durations: null,
    next_run: null,
    ...over,
  }
}

const failedAttempt = {
  status: 'failed',
  created_at: '2026-10-02 11:24:51',
  elapsed_seconds: 0.2,
  error_message: 'AssertionError: 3 rows with negative units',
}

describe('severityOf', () => {
  it('maps every cache state', () => {
    expect(severityOf(node({ id: 'a', cache: { state: 'fresh' } }))).toBe('fresh')
    expect(severityOf(node({ id: 'a', cache: { state: 'stale', cause: 'code' } }))).toBe('stale')
    expect(severityOf(node({ id: 'a', cache: { state: 'missing' } }))).toBe('missing')
    expect(severityOf(node({ id: 'a', cache: { state: 'partial', cached: 1, total: 2 } }))).toBe(
      'partial',
    )
    expect(severityOf(node({ id: 'a', cache: { state: 'unknown' } }))).toBe('unknown')
    expect(severityOf(node({ id: 'a', cache: { state: 'always_runs' } }))).toBe('always_runs')
  })

  it('a failed latest attempt wins over any cache state', () => {
    // A task (always_runs) whose last run failed is the thing to look at…
    expect(
      severityOf(node({ id: 't', kind: 'task', cache: { state: 'always_runs' }, last: failedAttempt })),
    ).toBe('failed')
    // …and so is a fresh asset whose newer attempt failed without replacing it.
    expect(severityOf(node({ id: 'a', cache: { state: 'fresh' }, last: failedAttempt }))).toBe(
      'failed',
    )
  })
})

describe('buildRows', () => {
  it('sorts by severity, then name', () => {
    const rows = buildRows(
      [
        node({ id: 'p.py:zeta', cache: { state: 'fresh' } }),
        node({ id: 'p.py:alpha', cache: { state: 'fresh' } }),
        node({ id: 'p.py:missing_one', cache: { state: 'missing' } }),
        node({ id: 'p.py:broken', kind: 'task', cache: { state: 'always_runs' }, last: failedAttempt }),
        node({ id: 'p.py:edited', cache: { state: 'stale', cause: 'code' } }),
      ],
      NOW,
    )
    expect(rows.map((r) => r.name)).toEqual(['broken', 'edited', 'missing_one', 'alpha', 'zeta'])
  })

  it('describes state, last attempt, timing and schedule', () => {
    const row = only(buildRows(
      [
        node({
          id: 'utz/assets.py:ibp_model',
          cache: { state: 'stale', cause: 'upstream' },
          last: {
            status: 'success',
            created_at: '2026-10-02 11:55:00',
            elapsed_seconds: 3.2,
            error_message: null,
          },
          durations: { median_seconds: 3.04, p95_seconds: 9.5, samples: 12 },
          freshness: { type: 'Schedule', value: '0 6 * * *' },
          next_run: Date.parse('2026-10-03T06:00:00Z') / 1000,
        }),
      ],
      NOW))
    expect(row.name).toBe('ibp_model')
    expect(row.file).toBe('utz/assets.py')
    expect(row.stateLabel).toBe('stale · upstream')
    expect(row.stateHint).toMatch(/upstream/)
    expect(row.last).toEqual({ status: 'success', ago: '5m ago', error: null })
    expect(row.typical).toBe('3.0s')
    expect(row.p95).toBe('9.5s')
    expect(row.schedule).toBe('0 6 * * *')
    expect(row.nextRunMs).toBe(Date.parse('2026-10-03T06:00:00Z'))
  })

  it('reports partial progress and never-run nodes', () => {
    const rows = buildRows(
      [
        node({ id: 'p.py:fetch', cache: { state: 'partial', cached: 3, total: 5 } }),
        node({ id: 'p.py:new', cache: { state: 'missing' } }),
      ],
      NOW,
    )
    const fetch = rows.find((r) => r.name === 'fetch')!
    const fresh = rows.find((r) => r.name === 'new')!
    expect(fetch.stateLabel).toBe('partial · 3/5')
    expect(fresh.last).toBeNull()
    expect(fresh.typical).toBeNull()
  })

  it('surfaces the error of a failed attempt', () => {
    const row = only(buildRows([node({ id: 'p.py:v', kind: 'task', last: failedAttempt })], NOW))
    expect(row.severity).toBe('failed')
    expect(row.last?.error).toBe('AssertionError: 3 rows with negative units')
    expect(row.last?.ago).toBe('35m ago')
  })
})

describe('filterRows', () => {
  const rows = buildRows(
    [node({ id: 'utz/assets.py:ibp_model' }), node({ id: 'utz/assets.py:dim_ppg' })],
    NOW,
  )
  it('matches name or id, case-insensitively', () => {
    expect(filterRows(rows, 'IBP').map((r) => r.name)).toEqual(['ibp_model'])
    expect(filterRows(rows, 'utz/').length).toBe(2)
  })
  it('an empty query keeps everything', () => {
    expect(filterRows(rows, '  ').length).toBe(2)
  })
})

describe('summarize', () => {
  it('counts every severity, including zeroes', () => {
    const rows = buildRows(
      [
        node({ id: 'a', cache: { state: 'fresh' } }),
        node({ id: 'b', cache: { state: 'fresh' } }),
        node({ id: 'c', cache: { state: 'stale', cause: 'code' } }),
      ],
      NOW,
    )
    expect(summarize(rows)).toEqual({
      failed: 0,
      stale: 1,
      missing: 0,
      partial: 0,
      unknown: 0,
      always_runs: 0,
      fresh: 2,
    })
  })
})

describe('formatting', () => {
  it('formats seconds compactly', () => {
    expect(formatSeconds(0.04)).toBe('0.0s')
    expect(formatSeconds(12.34)).toBe('12.3s')
    expect(formatSeconds(125)).toBe('2.1m')
    expect(formatSeconds(7200)).toBe('2.0h')
  })
  it('formats UTC timestamps relative to now', () => {
    expect(formatAgo('2026-10-02 11:59:30', NOW)).toBe('just now')
    expect(formatAgo('2026-10-02 10:00:00', NOW)).toBe('2h ago')
    expect(formatAgo('2026-09-29 12:00:00', NOW)).toBe('3d ago')
  })
})
