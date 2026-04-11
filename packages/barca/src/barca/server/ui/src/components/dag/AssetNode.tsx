import { memo } from "react";
import { Handle, Position } from "@xyflow/react";
import { useNavigate } from "react-router-dom";
import type { GraphNode } from "@/lib/types";

const KIND_COLORS: Record<string, { border: string; dot: string; text: string }> = {
  asset: {
    border: "border-l-blue-500",
    dot: "bg-blue-500",
    text: "text-blue-400",
  },
  sensor: {
    border: "border-l-purple-500",
    dot: "bg-purple-500",
    text: "text-purple-400",
  },
  effect: {
    border: "border-l-amber-500",
    dot: "bg-amber-500",
    text: "text-amber-400",
  },
  sink: {
    border: "border-l-cyan-500",
    dot: "bg-cyan-500",
    text: "text-cyan-400",
  },
};

const STATUS_DOT: Record<string, string> = {
  success: "bg-green-500",
  failed: "bg-red-500",
  running: "bg-blue-400 animate-pulse",
  queued: "bg-yellow-400",
};

interface AssetNodeData extends GraphNode {
  label?: string;
}

interface AssetNodeProps {
  data: AssetNodeData;
  selected?: boolean;
}

export const AssetNode = memo(function AssetNode({ data, selected }: AssetNodeProps) {
  const navigate = useNavigate();
  const colors = KIND_COLORS[data.kind] ?? KIND_COLORS.asset;
  const statusColor = data.materialization_status
    ? (STATUS_DOT[data.materialization_status] ?? "bg-neutral-600")
    : "bg-neutral-700";

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => navigate(`/ui/assets/${data.asset_id}`)}
      onKeyDown={(e) => e.key === "Enter" && navigate(`/ui/assets/${data.asset_id}`)}
      className={`
        relative w-[220px] rounded-md border border-border bg-card
        border-l-4 ${colors.border}
        px-3 py-2.5 cursor-pointer select-none
        transition-shadow duration-150
        ${selected ? "ring-1 ring-ring shadow-lg" : "hover:shadow-md hover:border-neutral-600"}
      `}
    >
      {/* Target handle — left side (receives inputs) */}
      <Handle
        type="target"
        position={Position.Left}
        className="!w-2 !h-2 !border-border !bg-neutral-600"
      />

      {/* Node body */}
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium text-foreground leading-tight">
            {data.logical_name}
          </p>
          <p className={`mt-0.5 text-[10px] ${colors.text} truncate`}>
            {data.kind}
            {data.purity === "unsafe" && (
              <span className="ml-1 text-amber-500/70">unsafe</span>
            )}
          </p>
        </div>
        {/* Status dot */}
        <div className="mt-0.5 shrink-0">
          <span
            className={`block w-2 h-2 rounded-full ${statusColor}`}
            title={data.materialization_status ?? "never run"}
          />
        </div>
      </div>

      {data.freshness !== "always" && (
        <p className="mt-1.5 text-[10px] text-muted-foreground truncate">
          {data.freshness === "manual" ? "manual" : data.freshness.replace("schedule:", "")}
        </p>
      )}

      {/* Source handle — right side (feeds outputs) */}
      <Handle
        type="source"
        position={Position.Right}
        className="!w-2 !h-2 !border-border !bg-neutral-600"
      />
    </div>
  );
});
