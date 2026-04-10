import { useState, useMemo } from "react";
import { Link } from "react-router-dom";
import { Card } from "@/components/ui/card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
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
import type { JobDetail } from "@/lib/types";

export function Jobs() {
  const { data, loading } = useAPI<JobDetail[]>("/api/jobs");
  const [statusFilter, setStatusFilter] = useState("all");

  const filtered = useMemo(() => {
    if (!data) return [];
    if (statusFilter === "all") return data;
    return data.filter((j) => j.job.status === statusFilter);
  }, [data, statusFilter]);

  return (
    <div className="flex flex-col gap-4">
      <Tabs value={statusFilter} onValueChange={setStatusFilter}>
        <TabsList>
          <TabsTrigger value="all">All</TabsTrigger>
          <TabsTrigger value="success">Success</TabsTrigger>
          <TabsTrigger value="failed">Failed</TabsTrigger>
          <TabsTrigger value="running">Running</TabsTrigger>
        </TabsList>
      </Tabs>

      <Card className="overflow-hidden p-0">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[100px]">Status</TableHead>
              <TableHead>Asset</TableHead>
              <TableHead className="w-[140px]">When</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <TableRow>
                <TableCell colSpan={3} className="text-center text-muted-foreground py-8">
                  Loading…
                </TableCell>
              </TableRow>
            ) : filtered.length === 0 ? (
              <TableRow>
                <TableCell colSpan={3} className="text-center text-muted-foreground py-8">
                  No jobs
                </TableCell>
              </TableRow>
            ) : (
              filtered.map((j) => (
                <TableRow key={j.job.materialization_id}>
                  <TableCell>
                    <StatusBadge status={j.job.status} />
                  </TableCell>
                  <TableCell>
                    <Link
                      to={`/ui/jobs/${j.job.materialization_id}`}
                      className="font-mono text-xs hover:underline"
                    >
                      {j.asset.logical_name}
                    </Link>
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {formatTime(j.job.created_at)}
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
