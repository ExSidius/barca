import { useCallback, useEffect, useMemo, useRef } from "react";
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  MarkerType,
  useNodesState,
  useEdgesState,
  useReactFlow,
  ReactFlowProvider,
  type Node,
  type Edge,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import * as dagre from "@dagrejs/dagre";
import { useAPI } from "@/hooks/useAPI";
import { useSSE } from "@/hooks/useSSE";
import { AssetNode } from "@/components/dag/AssetNode";
import type { GraphResponse, GraphNode } from "@/lib/types";

const NODE_WIDTH = 220;
const NODE_HEIGHT = 72;

// React Flow requires node data to be Record<string, unknown>.
// We cast our typed GraphNode through unknown to satisfy that constraint.
type RFNode = Node<Record<string, unknown>>;
type RFEdge = Edge;

const nodeTypes = { assetNode: AssetNode };

function buildLayout(graphData: GraphResponse): { nodes: RFNode[]; edges: RFEdge[] } {
  const g = new dagre.graphlib.Graph();
  g.setGraph({
    rankdir: "LR",
    ranksep: 120,
    nodesep: 50,
    marginx: 40,
    marginy: 40,
  });
  g.setDefaultEdgeLabel(() => ({}));

  for (const node of graphData.nodes) {
    g.setNode(String(node.asset_id), { width: NODE_WIDTH, height: NODE_HEIGHT });
  }
  for (const edge of graphData.edges) {
    g.setEdge(String(edge.source_asset_id), String(edge.target_asset_id));
  }

  dagre.layout(g);

  const nodes: RFNode[] = graphData.nodes.map((n: GraphNode) => {
    const pos = g.node(String(n.asset_id));
    return {
      id: String(n.asset_id),
      type: "assetNode",
      position: {
        x: pos ? pos.x - NODE_WIDTH / 2 : 0,
        y: pos ? pos.y - NODE_HEIGHT / 2 : 0,
      },
      data: n as unknown as Record<string, unknown>,
    };
  });

  const edges: RFEdge[] = graphData.edges.map((e, i) => ({
    id: `e-${e.source_asset_id}-${e.target_asset_id}-${i}`,
    source: String(e.source_asset_id),
    target: String(e.target_asset_id),
    label: e.parameter_name,
    labelStyle: { fontSize: 10, fill: "#737373" },
    labelBgStyle: { fill: "#111111", fillOpacity: 0.85 },
    style: {
      stroke: e.is_partition_source ? "#737373" : "#525252",
      strokeWidth: 1.5,
      strokeDasharray: e.is_partition_source ? "5,4" : undefined,
    },
    markerEnd: {
      type: MarkerType.ArrowClosed,
      color: e.is_partition_source ? "#737373" : "#525252",
      width: 14,
      height: 14,
    },
  }));

  return { nodes, edges };
}

function DagCanvas() {
  const { data, loading, error, refetch } = useAPI<GraphResponse>("/api/graph");
  const sseData = useSSE<unknown>("/sse/assets");
  const [nodes, setNodes, onNodesChange] = useNodesState<RFNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<RFEdge>([]);
  const { fitView } = useReactFlow();
  const fittedRef = useRef(false);

  const layout = useMemo(() => {
    if (!data) return null;
    return buildLayout(data);
  }, [data]);

  useEffect(() => {
    if (!layout) return;
    setNodes(layout.nodes);
    setEdges(layout.edges);
    if (!fittedRef.current) {
      fittedRef.current = true;
      setTimeout(() => fitView({ padding: 0.12, duration: 400 }), 50);
    }
  }, [layout, setNodes, setEdges, fitView]);

  // Refetch whenever SSE signals a DB change
  const prevSseRef = useRef<unknown>(null);
  useEffect(() => {
    if (sseData.data && sseData.data !== prevSseRef.current) {
      prevSseRef.current = sseData.data;
      refetch();
    }
  }, [sseData.data, refetch]);

  const onFitView = useCallback(() => {
    fitView({ padding: 0.12, duration: 400 });
  }, [fitView]);

  if (loading && nodes.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        Loading graph…
      </div>
    );
  }

  if (error && nodes.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-red-400">
        {error}
      </div>
    );
  }

  if (!loading && nodes.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        No assets indexed yet. Run{" "}
        <code className="mx-1 rounded bg-muted px-1 py-0.5 text-xs">barca reindex</code>{" "}
        to discover assets.
      </div>
    );
  }

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      nodeTypes={nodeTypes}
      colorMode="dark"
      fitView
      fitViewOptions={{ padding: 0.12 }}
      minZoom={0.1}
      maxZoom={2}
      proOptions={{ hideAttribution: true }}
    >
      <Background color="#262626" gap={20} size={1} />
      <Controls
        onFitView={onFitView}
        showInteractive={false}
        className="[&>button]:bg-card [&>button]:border-border [&>button]:text-foreground [&>button:hover]:bg-accent"
      />
      <MiniMap
        nodeColor={(node) => {
          const kind = (node.data as unknown as GraphNode).kind;
          const colors: Record<string, string> = {
            asset: "#3b82f6",
            sensor: "#a855f7",
            effect: "#f59e0b",
            sink: "#06b6d4",
          };
          return colors[kind] ?? "#525252";
        }}
        maskColor="rgba(0,0,0,0.6)"
        className="!bg-card !border !border-border"
      />
    </ReactFlow>
  );
}

export function Dag() {
  return (
    <div className="h-full w-full">
      <ReactFlowProvider>
        <DagCanvas />
      </ReactFlowProvider>
    </div>
  );
}
