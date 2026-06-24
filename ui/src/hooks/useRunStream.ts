import { useEffect, useRef, useState } from 'react'
import {
  reduceRunEvent,
  EMPTY_RUN_STATE,
  type RunStreamState,
} from '@/lib/runStream'
import type { RunEvent } from '@/lib/types'

/**
 * Thin presentation wrapper: opens the SSE connection and pipes each live
 * `RunEvent` through the pure `reduceRunEvent` reducer. All the folding logic
 * lives in `lib/runStream.ts` so it can be tested without a browser.
 *
 * Pass `null` to disconnect. The server replays the backlog on connect, so a
 * slightly-late subscribe still sees everything from the start.
 */
export function useRunStream(handle: string | null): RunStreamState {
  const [state, setState] = useState<RunStreamState>(EMPTY_RUN_STATE)
  const sourceRef = useRef<EventSource | null>(null)

  useEffect(() => {
    sourceRef.current?.close()
    if (!handle) {
      setState(EMPTY_RUN_STATE)
      return
    }
    setState({ ...EMPTY_RUN_STATE, running: true })

    const es = new EventSource(`/api/events/${encodeURIComponent(handle)}`)
    sourceRef.current = es

    es.onmessage = (e) => {
      let event: RunEvent
      try {
        event = JSON.parse(e.data) as RunEvent
      } catch {
        return
      }
      setState((prev) => reduceRunEvent(prev, event))
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
