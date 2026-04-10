import { Badge } from "@/components/ui/badge";

interface StatusBadgeProps {
  status: string | null;
}

export function StatusBadge({ status }: StatusBadgeProps) {
  if (!status) {
    return (
      <Badge variant="outline" className="text-muted-foreground font-mono text-[10px]">
        never
      </Badge>
    );
  }

  const colorMap: Record<string, string> = {
    success: "border-green-500/30 bg-green-500/10 text-green-400",
    failed: "border-red-500/30 bg-red-500/10 text-red-400",
    running: "border-blue-500/30 bg-blue-500/10 text-blue-400",
    queued: "border-yellow-500/30 bg-yellow-500/10 text-yellow-400",
  };

  return (
    <Badge
      variant="outline"
      className={`font-mono text-[10px] ${colorMap[status] ?? "text-muted-foreground"}`}
    >
      {status}
    </Badge>
  );
}

interface KindBadgeProps {
  kind: string;
}

export function KindBadge({ kind }: KindBadgeProps) {
  const colorMap: Record<string, string> = {
    asset: "border-blue-500/30 bg-blue-500/10 text-blue-400",
    sensor: "border-purple-500/30 bg-purple-500/10 text-purple-400",
    effect: "border-amber-500/30 bg-amber-500/10 text-amber-400",
  };

  return (
    <Badge variant="outline" className={`text-[10px] ${colorMap[kind] ?? ""}`}>
      {kind}
    </Badge>
  );
}
