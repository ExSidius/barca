import { useState, useCallback } from "react";

interface UseActionResult<T> {
  execute: () => Promise<T | null>;
  data: T | null;
  loading: boolean;
  error: string | null;
}

export function useAction<T>(url: string, method = "POST"): UseActionResult<T> {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const execute = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(url, { method });
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
      const json = await res.json();
      setData(json);
      return json;
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Unknown error";
      setError(msg);
      return null;
    } finally {
      setLoading(false);
    }
  }, [url, method]);

  return { execute, data, loading, error };
}
