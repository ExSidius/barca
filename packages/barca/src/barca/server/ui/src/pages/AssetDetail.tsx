import { useEffect } from "react";
import { useParams, Link } from "react-router-dom";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { StatusBadge, KindBadge } from "@/components/StatusBadge";
import { SourceCode } from "@/components/SourceCode";
import { useAPI } from "@/hooks/useAPI";
import { useSSE } from "@/hooks/useSSE";
import { useAction } from "@/hooks/useAction";
import { formatTime } from "@/lib/format";
import type { AssetDetail as AssetDetailType, MaterializationRecord } from "@/lib/types";

interface LiveData {
  detail: AssetDetailType;
  materializations: MaterializationRecord[];
}

export function AssetDetail() {
  const { id } = useParams<{ id: string }>();
  const initial = useAPI<AssetDetailType>(`/api/assets/${id}`);
  const initialHistory = useAPI<MaterializationRecord[]>(
    `/api/assets/${id}/materializations?limit=20`
  );
  // Live updates: server pushes a new JSON payload whenever this asset changes
  const live = useSSE<LiveData>(`/sse/assets/${id}`);
  const refresh = useAction(`/api/assets/${id}/refresh`);

  // If live SSE sends an update, override the initial fetch
  const detail = live.data?.detail ?? initial.data;
  const history = live.data?.materializations ?? initialHistory.data;

  useEffect(() => {
    if (refresh.data) {
      // After a manual refresh, re-fetch initial state (SSE will also update)
      initial.refetch();
      initialHistory.refetch();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refresh.data]);

  if (initial.loading && !detail) {
    return <div className="text-sm text-muted-foreground">Loading…</div>;
  }
  if (!detail) {
    return <div className="text-sm text-muted-foreground">Asset not found</div>;
  }

  const { asset, latest_materialization } = detail;

  return (
    <div className="flex flex-col gap-6">
      {/* Header */}
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Link to="/ui/assets" className="hover:text-foreground">
            Assets
          </Link>
          <span>/</span>
          <span className="font-mono">{asset.logical_name}</span>
        </div>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <KindBadge kind={asset.kind} />
            <h1 className="text-xl font-semibold text-foreground font-mono">
              {asset.logical_name}
            </h1>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => refresh.execute()}
            disabled={refresh.loading}
          >
            {refresh.loading ? "Running…" : "Refresh"}
          </Button>
        </div>
      </div>

      {refresh.error && (
        <Card className="border-destructive/50 bg-destructive/10">
          <CardContent className="py-3 text-sm text-destructive">
            {refresh.error}
          </CardContent>
        </Card>
      )}

      {/* Info + Status */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Definition</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            <Row label="File" value={asset.file_path} mono />
            <Row label="Function" value={asset.function_name} mono />
            <Row label="Python" value={asset.python_version} mono />
            <Row
              label="Definition Hash"
              value={asset.definition_hash.slice(0, 16) + "…"}
              mono
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Latest Run</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            {latest_materialization ? (
              <>
                <Row
                  label="Status"
                  value={<StatusBadge status={latest_materialization.status} />}
                />
                <Row
                  label="When"
                  value={formatTime(latest_materialization.created_at)}
                />
                <Row
                  label="Run Hash"
                  value={latest_materialization.run_hash.slice(0, 16) + "…"}
                  mono
                />
                {latest_materialization.artifact_path && (
                  <Row
                    label="Artifact"
                    value={latest_materialization.artifact_path}
                    mono
                  />
                )}
                {latest_materialization.partition_key_json && (
                  <Row
                    label="Partition"
                    value={latest_materialization.partition_key_json}
                    mono
                  />
                )}
                {latest_materialization.last_error && (
                  <Row
                    label="Error"
                    value={
                      <span className="text-destructive text-xs">
                        {latest_materialization.last_error}
                      </span>
                    }
                  />
                )}
              </>
            ) : (
              <div className="text-muted-foreground">Never materialized</div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Source code */}
      {asset.source_text && <SourceCode code={asset.source_text} />}

      {/* History */}
      <Card className="overflow-hidden p-0">
        <CardHeader>
          <CardTitle className="text-sm font-medium">Materialization History</CardTitle>
        </CardHeader>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[100px]">Status</TableHead>
              <TableHead>Run Hash</TableHead>
              <TableHead>Artifact</TableHead>
              <TableHead className="w-[140px]">When</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {history && history.length > 0 ? (
              history.map((m) => (
                <TableRow key={m.materialization_id}>
                  <TableCell>
                    <StatusBadge status={m.status} />
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">
                    {m.run_hash.slice(0, 24)}…
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground truncate max-w-[200px]">
                    {m.artifact_path ?? "—"}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {formatTime(m.created_at)}
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell colSpan={4} className="text-center text-muted-foreground py-8">
                  No history
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </Card>
    </div>
  );
}

function Row({
  label,
  value,
  mono,
}: {
  label: string;
  value: React.ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="flex items-start justify-between gap-4">
      <span className="text-muted-foreground text-xs uppercase tracking-wider shrink-0">
        {label}
      </span>
      <span
        className={`text-foreground text-right ${mono ? "font-mono text-xs break-all" : ""}`}
      >
        {value}
      </span>
    </div>
  );
}
