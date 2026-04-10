import { Link } from "react-router-dom";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useAPI } from "@/hooks/useAPI";
import { useSSE } from "@/hooks/useSSE";
import { StatusBadge, KindBadge } from "@/components/StatusBadge";
import type { AssetSummary, JobDetail } from "@/lib/types";

interface Health {
  status: string;
  scheduler_running: boolean;
}

interface SSEAssetsData {
  assets: AssetSummary[];
}

export function Dashboard() {
  const initialAssets = useAPI<AssetSummary[]>("/api/assets");
  const liveAssets = useSSE<SSEAssetsData>("/sse/assets");
  const assets = { data: liveAssets.data?.assets ?? initialAssets.data, loading: initialAssets.loading && !liveAssets.data };

  const jobs = useAPI<JobDetail[]>("/api/jobs");
  const health = useAPI<Health>("/api/health");

  const totalAssets = assets.data?.filter((a) => a.kind === "asset").length ?? 0;
  const totalSensors = assets.data?.filter((a) => a.kind === "sensor").length ?? 0;
  const totalEffects = assets.data?.filter((a) => a.kind === "effect").length ?? 0;

  return (
    <div className="flex flex-col gap-6">
      {/* Stat cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <StatCard label="Assets" value={totalAssets} />
        <StatCard label="Sensors" value={totalSensors} />
        <StatCard label="Effects" value={totalEffects} />
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
              Scheduler
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex items-center gap-2">
              {health.data?.scheduler_running ? (
                <>
                  <span className="h-2 w-2 rounded-full bg-green-500 animate-pulse" />
                  <span className="text-2xl font-semibold text-green-400">Running</span>
                </>
              ) : (
                <>
                  <span className="h-2 w-2 rounded-full bg-neutral-500" />
                  <span className="text-2xl font-semibold text-muted-foreground">Stopped</span>
                </>
              )}
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Recent jobs + Asset overview */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {/* Recent jobs */}
        <Card className="flex flex-col">
          <CardHeader className="flex-row items-center justify-between space-y-0">
            <CardTitle className="text-sm font-medium">Recent Jobs</CardTitle>
            <Link
              to="/ui/jobs"
              className="text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              View all →
            </Link>
          </CardHeader>
          <CardContent className="p-0">
            {jobs.loading ? (
              <div className="p-6 text-center text-sm text-muted-foreground">Loading…</div>
            ) : jobs.data && jobs.data.length > 0 ? (
              <div className="divide-y divide-border">
                {jobs.data.slice(0, 8).map((j) => (
                  <Link
                    key={j.job.materialization_id}
                    to={`/ui/jobs/${j.job.materialization_id}`}
                    className="flex items-center gap-3 px-6 py-3 hover:bg-accent transition-colors"
                  >
                    <StatusBadge status={j.job.status} />
                    <span className="font-mono text-xs text-muted-foreground truncate">
                      {j.asset.logical_name}
                    </span>
                  </Link>
                ))}
              </div>
            ) : (
              <div className="p-6 text-center text-sm text-muted-foreground">No jobs yet</div>
            )}
          </CardContent>
        </Card>

        {/* Asset overview */}
        <Card className="flex flex-col">
          <CardHeader className="flex-row items-center justify-between space-y-0">
            <CardTitle className="text-sm font-medium">Assets</CardTitle>
            <Link
              to="/ui/assets"
              className="text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              View all →
            </Link>
          </CardHeader>
          <CardContent className="p-0">
            {assets.loading ? (
              <div className="p-6 text-center text-sm text-muted-foreground">Loading…</div>
            ) : assets.data && assets.data.length > 0 ? (
              <div className="divide-y divide-border">
                {assets.data.slice(0, 8).map((a) => (
                  <Link
                    key={a.asset_id}
                    to={`/ui/${a.kind === "sensor" ? "sensors" : "assets"}/${a.asset_id}`}
                    className="flex items-center gap-3 px-6 py-3 hover:bg-accent transition-colors"
                  >
                    <KindBadge kind={a.kind} />
                    <span className="font-mono text-xs text-muted-foreground truncate flex-1">
                      {a.logical_name}
                    </span>
                    <StatusBadge status={a.materialization_status} />
                  </Link>
                ))}
              </div>
            ) : (
              <div className="p-6 text-center text-sm text-muted-foreground">
                No assets indexed yet
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function StatCard({ label, value }: { label: string; value: number | string }) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
          {label}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="text-3xl font-semibold text-foreground">{value}</div>
      </CardContent>
    </Card>
  );
}
