import { useEffect, useMemo, useState, useCallback } from "react";
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  type Node,
  type Edge,
  type NodeProps,
  Handle,
  Position,
  MarkerType,
} from "@xyflow/react";
import ELK, { type ElkNode } from "elkjs/lib/elk.bundled.js";
import "@xyflow/react/dist/style.css";
import type { AppSnapshot, AgentStatus } from "../types";

/**
 * Live agent/topic graph, modelled on ROS2's `rqt_graph`.
 *
 *   agent ──publishes──▶ topic (key expression) ──subscribed-by──▶ agent
 *
 * Agents are oval-ish nodes coloured by liveness status; topics are
 * rectangular nodes. The graph is rebuilt from the polled `AppSnapshot`
 * and laid out left-to-right with the Eclipse Layout Kernel (elkjs),
 * which produces a Sugiyama-style layered placement with orthogonal
 * edge routing — closer to the canonical `rqt_graph` look than dagre.
 *
 * Layout is asynchronous (ELK returns a Promise), so positions are
 * computed in an effect and the previous layout is kept on-screen while
 * the next one is in flight. A cancellation flag avoids stale results
 * from a slow tick overwriting a more recent fast one.
 *
 * This is a pure frontend feature — it consumes the existing
 * `connected_nodes[].agents[].publishes/subscribes` data and needs no
 * backend changes.
 */

interface Props {
  snapshot: AppSnapshot;
}

// ── Node data shapes ────────────────────────────────────────────────────────

interface AgentNodeData {
  label: string;
  status: AgentStatus;
  [key: string]: unknown;
}

interface TopicNodeData {
  label: string;
  [key: string]: unknown;
}

// Fixed node footprints so ELK can reserve space.
const AGENT_W = 180;
const AGENT_H = 48;
const TOPIC_W = 200;
const TOPIC_H = 36;

const STATUS_STYLES: Record<AgentStatus, { ring: string; dot: string }> = {
  online: { ring: "border-green-500/70", dot: "bg-green-400" },
  degraded: { ring: "border-yellow-500/70", dot: "bg-yellow-400" },
  offline: { ring: "border-gray-600/70", dot: "bg-gray-500" },
};

// ── Custom node renderers (Tailwind-styled to match the rest of the app) ──────

function AgentNode({ data }: NodeProps) {
  const d = data as AgentNodeData;
  const s = STATUS_STYLES[d.status];
  return (
    <div
      className={`flex items-center gap-2 rounded-full border bg-gray-900 px-4 py-2 shadow-md ${s.ring}`}
      style={{ width: AGENT_W, height: AGENT_H }}
      title={`${d.label} (${d.status})`}
    >
      <Handle type="target" position={Position.Left} className="!bg-gray-600" />
      <span className={`h-2 w-2 shrink-0 rounded-full ${s.dot}`} />
      <span className="truncate font-mono text-xs text-gray-100">{d.label}</span>
      <Handle type="source" position={Position.Right} className="!bg-gray-600" />
    </div>
  );
}

function TopicNode({ data }: NodeProps) {
  const d = data as TopicNodeData;
  return (
    <div
      className="flex items-center rounded-sm border border-zenoh-600/70 bg-gray-800 px-3 py-1.5 shadow"
      style={{ width: TOPIC_W, height: TOPIC_H }}
      title={d.label}
    >
      <Handle type="target" position={Position.Left} className="!bg-zenoh-700" />
      <span className="truncate font-mono text-[11px] text-zenoh-100">{d.label}</span>
      <Handle type="source" position={Position.Right} className="!bg-zenoh-700" />
    </div>
  );
}

const nodeTypes = { agent: AgentNode, topic: TopicNode };

// ── ELK setup ────────────────────────────────────────────────────────────────

// One instance per module — ELK is stateless across calls but the
// constructor spins up a worker, so we don't want to do it per render.
const elk = new ELK();

// Match the previous dagre tuning so the swap is a layout-quality
// improvement, not a visual surprise:
//   dagre rankdir "LR" ⇒ ELK direction "RIGHT"
//   dagre ranksep 90   ⇒ elk.layered.spacing.nodeNodeBetweenLayers
//   dagre nodesep 24   ⇒ elk.spacing.nodeNode
// ELK option values are strings.
const ELK_LAYOUT_OPTIONS: Record<string, string> = {
  "elk.algorithm": "layered",
  "elk.direction": "RIGHT",
  "elk.layered.spacing.nodeNodeBetweenLayers": "90",
  "elk.spacing.nodeNode": "24",
  "elk.edgeRouting": "ORTHOGONAL",
};

// ── Graph construction ────────────────────────────────────────────────────────

interface BuiltGraph {
  /** Nodes without positions — positions are filled in by ELK. */
  unpositionedNodes: Node[];
  edges: Edge[];
}

function buildGraph(snapshot: AppSnapshot, showTopics: boolean): BuiltGraph {
  // Logical graph: agentId -> data, topic key -> set, plus pub/sub links.
  const agents = new Map<string, AgentNodeData>();
  const topics = new Set<string>();
  const edges: Edge[] = [];

  for (const node of snapshot.connected_nodes) {
    for (const [agentName, ag] of Object.entries(node.agents)) {
      const agentId = `agent:${node.cn}/${agentName}`;
      agents.set(agentId, {
        label: `${node.cn}/${agentName}`,
        status: ag.status,
      });

      for (const ke of ag.publishes) {
        topics.add(ke);
        edges.push({
          id: `pub:${agentId}->${ke}`,
          source: agentId,
          target: `topic:${ke}`,
          animated: ag.status === "online",
          markerEnd: { type: MarkerType.ArrowClosed },
          style: { stroke: "#0ea5e9" },
        });
      }
      for (const ke of ag.subscribes) {
        topics.add(ke);
        edges.push({
          id: `sub:${ke}->${agentId}`,
          source: `topic:${ke}`,
          target: agentId,
          animated: ag.status === "online",
          markerEnd: { type: MarkerType.ArrowClosed },
          style: { stroke: "#64748b" },
        });
      }
    }
  }

  // If topics are hidden, collapse agent -> topic -> agent into agent -> agent.
  let renderEdges = edges;
  if (!showTopics) {
    const collapsed = new Map<string, Edge>();
    const pubsByTopic = new Map<string, string[]>(); // topic -> [agentId]
    const subsByTopic = new Map<string, string[]>(); // topic -> [agentId]
    for (const e of edges) {
      if (e.source.startsWith("agent:") && e.target.startsWith("topic:")) {
        const topic = e.target.slice("topic:".length);
        (pubsByTopic.get(topic) ?? pubsByTopic.set(topic, []).get(topic)!).push(e.source);
      } else if (e.source.startsWith("topic:")) {
        const topic = e.source.slice("topic:".length);
        (subsByTopic.get(topic) ?? subsByTopic.set(topic, []).get(topic)!).push(e.target);
      }
    }
    for (const [topic, pubs] of pubsByTopic) {
      const subs = subsByTopic.get(topic) ?? [];
      for (const p of pubs) {
        for (const s of subs) {
          if (p === s) continue;
          const id = `direct:${p}->${s}`;
          if (!collapsed.has(id)) {
            collapsed.set(id, {
              id,
              source: p,
              target: s,
              label: topic,
              labelStyle: { fill: "#94a3b8", fontSize: 10 },
              markerEnd: { type: MarkerType.ArrowClosed },
              style: { stroke: "#0ea5e9" },
            });
          }
        }
      }
    }
    renderEdges = [...collapsed.values()];
  }

  // Build React Flow nodes with placeholder positions; ELK will fill in x/y.
  const unpositionedNodes: Node[] = [];
  for (const [id, data] of agents) {
    unpositionedNodes.push({
      id,
      type: "agent",
      data,
      position: { x: 0, y: 0 },
    });
  }
  if (showTopics) {
    for (const ke of topics) {
      unpositionedNodes.push({
        id: `topic:${ke}`,
        type: "topic",
        data: { label: ke } satisfies TopicNodeData,
        position: { x: 0, y: 0 },
      });
    }
  }

  return { unpositionedNodes, edges: renderEdges };
}

/**
 * Run ELK on `nodes`/`edges` and return a new array of React Flow nodes
 * with `position` populated from the ELK result. ELK x/y are already
 * top-left corners (unlike dagre, which gives centers), so they map
 * directly onto React Flow's `position` field.
 */
async function layoutWithElk(nodes: Node[], edges: Edge[]): Promise<Node[]> {
  const nodeIds = new Set(nodes.map((n) => n.id));
  const elkGraph: ElkNode = {
    id: "root",
    layoutOptions: ELK_LAYOUT_OPTIONS,
    children: nodes.map((n) => ({
      id: n.id,
      width: n.type === "topic" ? TOPIC_W : AGENT_W,
      height: n.type === "topic" ? TOPIC_H : AGENT_H,
    })),
    // Only include edges whose endpoints exist as nodes — when topics
    // are hidden, collapsed edges already satisfy this, but be defensive.
    edges: edges
      .filter((e) => nodeIds.has(e.source) && nodeIds.has(e.target))
      .map((e) => ({
        id: e.id,
        sources: [e.source],
        targets: [e.target],
      })),
  };

  const result = await elk.layout(elkGraph);
  const positions = new Map<string, { x: number; y: number }>();
  for (const c of result.children ?? []) {
    positions.set(c.id, { x: c.x ?? 0, y: c.y ?? 0 });
  }

  return nodes.map((n) => ({
    ...n,
    position: positions.get(n.id) ?? { x: 0, y: 0 },
  }));
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function GraphTab({ snapshot }: Props) {
  const [showTopics, setShowTopics] = useState(true);

  // Pure data construction — synchronous and cheap.
  const { unpositionedNodes, edges } = useMemo(
    () => buildGraph(snapshot, showTopics),
    [snapshot, showTopics],
  );

  // ELK is async. We hold the most recent positioned snapshot in state so
  // that the previous layout stays on-screen while the next one computes,
  // avoiding a flicker on every 500ms snapshot tick.
  const [positionedNodes, setPositionedNodes] = useState<Node[]>([]);

  useEffect(() => {
    if (unpositionedNodes.length === 0) {
      setPositionedNodes([]);
      return;
    }
    let cancelled = false;
    layoutWithElk(unpositionedNodes, edges)
      .then((nodes) => {
        if (!cancelled) setPositionedNodes(nodes);
      })
      .catch((err) => {
        // eslint-disable-next-line no-console
        console.error("ELK layout failed:", err);
      });
    return () => {
      cancelled = true;
    };
  }, [unpositionedNodes, edges]);

  const miniMapColor = useCallback((n: Node) => {
    if (n.type === "topic") return "#0284c7";
    const status = (n.data as AgentNodeData).status;
    return status === "online" ? "#22c55e" : status === "degraded" ? "#eab308" : "#6b7280";
  }, []);

  const empty = unpositionedNodes.length === 0;

  return (
    <div className="relative h-full w-full bg-gray-950">
      {/* Toolbar */}
      <div className="absolute left-3 top-3 z-10 flex items-center gap-3 rounded-md border border-gray-800 bg-gray-900/90 px-3 py-1.5 text-xs text-gray-300 backdrop-blur">
        <span className="font-semibold text-gray-200">Agent graph</span>
        <label className="flex cursor-pointer items-center gap-1.5">
          <input
            type="checkbox"
            checked={showTopics}
            onChange={(e) => setShowTopics(e.target.checked)}
            className="accent-zenoh-600"
          />
          Show topics
        </label>
        <span className="text-gray-500">
          {snapshot.connected_nodes.reduce((a, n) => a + Object.keys(n.agents).length, 0)} agents
        </span>
      </div>

      {empty ? (
        <div className="flex h-full items-center justify-center text-sm text-gray-600">
          Waiting for agents to connect…
        </div>
      ) : positionedNodes.length === 0 ? (
        <div className="flex h-full items-center justify-center text-sm text-gray-600">
          Laying out graph…
        </div>
      ) : (
        <ReactFlow
          nodes={positionedNodes}
          edges={edges}
          nodeTypes={nodeTypes}
          fitView
          fitViewOptions={{ padding: 0.2 }}
          minZoom={0.1}
          proOptions={{ hideAttribution: true }}
          nodesDraggable
          nodesConnectable={false}
          elementsSelectable
        >
          <Background color="#1f2937" gap={20} />
          <Controls className="!bg-gray-800 !text-gray-200" showInteractive={false} />
          <MiniMap
            pannable
            zoomable
            nodeColor={miniMapColor}
            maskColor="rgba(0,0,0,0.6)"
            className="!bg-gray-900"
          />
        </ReactFlow>
      )}
    </div>
  );
}
