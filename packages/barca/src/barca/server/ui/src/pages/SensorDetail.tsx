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
import { Badge } from "@/components/ui/badge";
import { SourceCode } from "@/components/SourceCode";
import { useAPI } from "@/hooks/useAPI";
import { useAction } from "@/hooks/useAction";
import { formatTime } from "@/lib/format";
import type { AssetDetail, SensorObservation } from "@/lib/types";

export function SensorDetail() {
  const { id } = useParams<{ id: string }>();
  const { data, loading, refetch } = useAPI<AssetDetail>(`/api/assets/${id}`);
  const observations = useAPI<SensorObservation[]>(
    `/api/sensors/${id}/observations?limit=20`
  );
  const trigger = useAction(`/api/sensors/${id}/trigger`);

  const handleTrigger = async () => {
    await trigger.execute();
    refetch();
    observations.refetch();
  };

  if (loading) {
    return <div className="text-sm text-muted-foreground">Loading…</div>;
  }
  if (!data) {
    return <div className="text-sm text-muted-foreground">Sensor not found</div>;
  }

  const { asset, latest_observation } = data;

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Link to="/ui/sensors" className="hover:text-foreground">
            Sensors
          </Link>
          <span>/</span>
          <span className="font-mono">{asset.logical_name}</span>
        </div>
        <div className="flex items-center justify-between">
          <h1 className="text-xl font-semibold text-foreground font-mono">
            {asset.logical_name}
          </h1>
          <Button
            variant="outline"
            size="sm"
            onClick={handleTrigger}
            disabled={trigger.loading}
          >
            {trigger.loading ? "Triggering…" : "Trigger"}
          </Button>
        </div>
      </div>

      {trigger.error && (
        <Card className="border-destructive/50 bg-destructive/10">
          <CardContent className="py-3 text-sm text-destructive">{trigger.error}</CardContent>
        </Card>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Definition</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            <Row label="File" value={asset.file_path} mono />
            <Row label="Function" value={asset.function_name} mono />
            <Row label="Python" value={asset.python_version} mono />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Latest Observation</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            {latest_observation ? (
              <>
                <Row
                  label="Update"
                  value={
                    <Badge
                      variant="outline"
                      className={
                        latest_observation.update_detected
                          ? "border-green-500/30 bg-green-500/10 text-green-400"
                          : "border-muted-foreground/30 text-muted-foreground"
                      }
                    >
                      {latest_observation.update_detected ? "detected" : "no change"}
                    </Badge>
                  }
                />
                <Row label="When" value={formatTime(latest_observation.created_at)} />
              </>
            ) : (
              <div className="text-muted-foreground">Never triggered</div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Source code */}
      {asset.source_text && <SourceCode code={asset.source_text} />}

      <Card className="overflow-hidden p-0">
        <CardHeader>
          <CardTitle className="text-sm font-medium">Observation History</CardTitle>
        </CardHeader>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[120px]">Update</TableHead>
              <TableHead>Output</TableHead>
              <TableHead className="w-[140px]">When</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {observations.data && observations.data.length > 0 ? (
              observations.data.map((o) => (
                <TableRow key={o.observation_id}>
                  <TableCell>
                    <Badge
                      variant="outline"
                      className={
                        o.update_detected
                          ? "border-green-500/30 bg-green-500/10 text-green-400"
                          : "text-muted-foreground"
                      }
                    >
                      {o.update_detected ? "detected" : "no change"}
                    </Badge>
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground truncate max-w-[400px]">
                    {o.output_json ?? "—"}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {formatTime(o.created_at)}
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell colSpan={3} className="text-center text-muted-foreground py-8">
                  No observations
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
    <div className="flex items-center justify-between gap-4">
      <span className="text-muted-foreground text-xs uppercase tracking-wider">{label}</span>
      <span className={`text-foreground ${mono ? "font-mono text-xs" : ""}`}>{value}</span>
    </div>
  );
}
