import { useParams, Link } from "react-router-dom";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { StatusBadge } from "@/components/StatusBadge";
import { useAPI } from "@/hooks/useAPI";
import { formatTime } from "@/lib/format";
import type { JobDetail as JobDetailType } from "@/lib/types";

export function JobDetail() {
  const { id } = useParams<{ id: string }>();
  const { data, loading } = useAPI<JobDetailType>(`/api/jobs/${id}`);

  if (loading) {
    return <div className="text-sm text-muted-foreground">Loading…</div>;
  }
  if (!data) {
    return <div className="text-sm text-muted-foreground">Job not found</div>;
  }

  const { job, asset } = data;

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Link to="/ui/jobs" className="hover:text-foreground">
            Jobs
          </Link>
          <span>/</span>
          <span className="font-mono">#{job.materialization_id}</span>
        </div>
        <div className="flex items-center gap-3">
          <StatusBadge status={job.status} />
          <h1 className="text-xl font-semibold text-foreground font-mono">
            {asset.logical_name}
          </h1>
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">Details</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <Row label="Job ID" value={`#${job.materialization_id}`} mono />
          <Row label="Asset" value={asset.logical_name} mono />
          <Row label="Run Hash" value={job.run_hash} mono />
          <Row label="Created" value={formatTime(job.created_at)} />
          {job.artifact_path && <Row label="Artifact" value={job.artifact_path} mono />}
          {job.artifact_format && (
            <Row label="Format" value={job.artifact_format} mono />
          )}
          {job.artifact_checksum && (
            <Row
              label="Checksum"
              value={job.artifact_checksum.slice(0, 24) + "…"}
              mono
            />
          )}
          {job.partition_key_json && (
            <Row label="Partition" value={job.partition_key_json} mono />
          )}
          {job.last_error && (
            <Row
              label="Error"
              value={<span className="text-destructive text-xs">{job.last_error}</span>}
            />
          )}
        </CardContent>
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
