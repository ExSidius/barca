import { useState, useEffect, useRef } from "react";

interface UseSSEResult<T> {
  data: T | null;
  connected: boolean;
  error: string | null;
}

export function useSSE<T>(url: string): UseSSEResult<T> {
  const [data, setData] = useState<T | null>(null);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const esRef = useRef<EventSource | null>(null);

  useEffect(() => {
    const es = new EventSource(url);
    esRef.current = es;

    es.onopen = () => {
      setConnected(true);
      setError(null);
    };

    es.onmessage = (event) => {
      try {
        const parsed = JSON.parse(event.data);
        setData(parsed);
      } catch {
        // Non-JSON message, ignore
      }
    };

    es.onerror = () => {
      setConnected(false);
      setError("SSE connection lost");
    };

    return () => {
      es.close();
      esRef.current = null;
    };
  }, [url]);

  return { data, connected, error };
}
