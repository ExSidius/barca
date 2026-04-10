import { useState, useMemo } from "react";
import { Link } from "react-router-dom";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { StatusBadge, KindBadge } from "@/components/StatusBadge";
import { useAPI } from "@/hooks/useAPI";
import { useSSE } from "@/hooks/useSSE";
import { formatTime } from "@/lib/format";
import type { AssetSummary } from "@/lib/types";

interface SSEAssetsData {
  assets: AssetSummary[];
}

export function Assets() {
  const initial = useAPI<AssetSummary[]>("/api/assets");
  // Live updates: server pushes a new JSON payload whenever the asset list changes
  const live = useSSE<SSEAssetsData>("/sse/assets");
  // SSE data wins over initial fetch once connected
  const data = live.data?.assets ?? initial.data;
  const loading = initial.loading && !data;

  const [kindFilter, setKindFilter] = useState("all");
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    if (!data) return [];
    return data.filter((a) => {
      if (kindFilter !== "all" && a.kind !== kindFilter) return false;
      if (search && !a.logical_name.toLowerCase().includes(search.toLowerCase())) return false;
      return true;
    });
  }, [data, kindFilter, search]);

  return (
    <div className="flex flex-col gap-4">
      {/* Filters */}
      <div className="flex flex-col sm:flex-row sm:items-center gap-3">
        <Tabs value={kindFilter} onValueChange={setKindFilter}>
          <TabsList>
            <TabsTrigger value="all">All</TabsTrigger>
            <TabsTrigger value="asset">Assets</TabsTrigger>
            <TabsTrigger value="sensor">Sensors</TabsTrigger>
            <TabsTrigger value="effect">Effects</TabsTrigger>
          </TabsList>
        </Tabs>

        <Input
          placeholder="Search assets…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="sm:ml-auto sm:w-64"
        />
      </div>

      {/* Table */}
      <Card className="overflow-hidden p-0">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[90px]">Kind</TableHead>
              <TableHead>Name</TableHead>
              <TableHead className="w-[120px]">Schedule</TableHead>
              <TableHead className="w-[120px]">Status</TableHead>
              <TableHead className="w-[140px]">Last Run</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <TableRow>
                <TableCell colSpan={5} className="text-center text-muted-foreground py-8">
                  Loading…
                </TableCell>
              </TableRow>
            ) : filtered.length === 0 ? (
              <TableRow>
                <TableCell colSpan={5} className="text-center text-muted-foreground py-8">
                  No assets match your filters
                </TableCell>
              </TableRow>
            ) : (
              filtered.map((a) => (
                <TableRow key={a.asset_id}>
                  <TableCell>
                    <KindBadge kind={a.kind} />
                  </TableCell>
                  <TableCell>
                    <Link
                      to={`/ui/${a.kind === "sensor" ? "sensors" : "assets"}/${a.asset_id}`}
                      className="font-mono text-xs text-foreground hover:underline"
                    >
                      {a.logical_name}
                    </Link>
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">
                    {a.schedule}
                  </TableCell>
                  <TableCell>
                    <StatusBadge status={a.materialization_status} />
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {formatTime(a.materialization_created_at)}
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
