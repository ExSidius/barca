import { useEffect, useRef, useState } from 'react'
import { match } from 'ts-pattern'
import type { RunEvent, LogLine, StatusKind } from '@/lib/types'

export interface RunStream {
  /** True between run_started and run_finished. */
  running: boolean
  /** Captured log lines, in order. */
  logs: LogLine[]
  /** Per-node visual status derived from the event stream. */
  statuses: Record<string, StatusKind>
  /** Final outcome once finished, else null. */
  ok: boolean | null
}

const EMPTY: RunStream = { running: false, logs: [], statuses: {}, ok: null }

/**
 * Subscribe to a run's live event stream (`GET /api/events/{handle}`) via
 * Server-Sent Events. Returns accumulated logs + per-node status that update in
 * real time. Pass `null` to disconnect. The server replays the backlog on
 * connect, so a slightly-late subscribe still sees everything from the start.
 */
export function useRunStream(handle: string | null): RunStream {
  const [state, setState] = useState<RunStream>(EMPTY)
  const sourceRef = useRef<EventSource | null>(null)

  useEffect(() => {
    sourceRef.current?.close()
    if (!handle) {
      setState(EMPTY)
      return
    }
    setState({ ...EMPTY, running: true })

    const es = new EventSource(`/api/events/${encodeURIComponent(handle)}`)
    sourceRef.current = es

    es.onmessage = (e) => {
      let event: RunEvent
      try {
        event = JSON.parse(e.data) as RunEvent
      } catch {
        return
      }
      setState((prev) =>
        match(event)
          .with({ type: 'run_started' }, () => ({ ...prev, running: true }))
          .with({ type: 'log' }, (ev) => ({
            ...prev,
            logs: [...prev.logs, { nodeId: ev.node_id, text: ev.line }],
            statuses: { ...prev.statuses, [ev.node_id]: 'running' as StatusKind },
          }))
          .with({ type: 'step_finished' }, (ev) => ({
            ...prev,
            statuses: {
              ...prev.statuses,
              [ev.node_id]: (ev.ok ? 'success' : 'failed') as StatusKind,
            },
          }))
          .with({ type: 'run_finished' }, (ev) => ({
            ...prev,
            running: false,
            ok: ev.ok,
          }))
          .exhaustive(),
      )
      if (event.type === 'run_finished') es.close()
    }

    es.onerror = () => {
      // The browser auto-reconnects; if the run is already done the server has
      // dropped the channel and the stream simply ends.
    }

    return () => es.close()
  }, [handle])

  return state
}
