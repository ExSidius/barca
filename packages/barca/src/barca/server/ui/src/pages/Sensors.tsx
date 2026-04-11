import { Link } from "react-router-dom";
import { Card } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { StatusBadge } from "@/components/StatusBadge";
import { useAPI } from "@/hooks/useAPI";
import { formatTime } from "@/lib/format";
import type { AssetSummary } from "@/lib/types";

function formatFreshness(f: string): string {
  if (f === "always") return "always";
  if (f === "manual") return "manual";
  if (f.startsWith("schedule:")) return `schedule(${f.slice("schedule:".length)})`;
  return f;
}

export function Sensors() {
  const { data, loading } = useAPI<AssetSummary[]>("/api/sensors");

  return (
    <div className="flex flex-col gap-4">
      <Card className="overflow-hidden p-0">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead className="w-[160px]">Freshness</TableHead>
              <TableHead className="w-[120px]">Status</TableHead>
              <TableHead className="w-[140px]">Last Run</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <TableRow>
                <TableCell colSpan={4} className="text-center text-muted-foreground py-8">
                  Loading…
                </TableCell>
              </TableRow>
            ) : !data || data.length === 0 ? (
              <TableRow>
                <TableCell colSpan={4} className="text-center text-muted-foreground py-8">
                  No sensors indexed yet
                </TableCell>
              </TableRow>
            ) : (
              data.map((s) => (
                <TableRow key={s.asset_id}>
                  <TableCell>
                    <Link
                      to={`/ui/sensors/${s.asset_id}`}
                      className="font-mono text-xs hover:underline"
                    >
                      {s.logical_name}
                    </Link>
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">
                    {formatFreshness(s.freshness)}
                  </TableCell>
                  <TableCell>
                    <StatusBadge status={s.materialization_status} />
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {formatTime(s.materialization_created_at)}
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </Card>
    </div>
  );
}
