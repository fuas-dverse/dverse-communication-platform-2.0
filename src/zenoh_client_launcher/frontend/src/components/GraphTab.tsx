import { useMemo, useState, useCallback } from "react";
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
import dagre from "dagre";
import "@xyflow/react/dist/style.css";
import type { AppSnapshot, AgentStatus } from "../types";

/**
 * Live agent/topic graph, modelled on ROS2's `rqt_graph`.
 *
 *   agent ──publishes──▶ topic (key expression) ──subscribed-by──▶ agent
 *
 * Agents are oval-ish nodes coloured by liveness status; topics are
 * rectangular nodes. The graph is rebuilt from the polled `AppSnapshot`
 * on every render and laid out left-to-right with dagre, so it animates
 * as the snapshot updates (every 500ms in App.tsx).
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

// Fixed node footprints so dagre can reserve space.
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

// ── Graph construction + layout ──────────────────────────────────────────────

interface BuiltGraph {
  nodes: Node[];
  edges: Edge[];
}

function buildGraph(snapshot: AppSnapshot, showTopics: boolean): BuiltGraph {
  const g = new dagre.graphlib.Graph();
  g.setGraph({ rankdir: "LR", nodesep: 24, ranksep: 90 });
  g.setDefaultEdgeLabel(() => ({}));

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
    // pub edges land on a topic; sub edges leave a topic. Join them.
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

  // Register nodes with dagre.
  for (const [id] of agents) {
    g.setNode(id, { width: AGENT_W, height: AGENT_H });
  }
  if (showTopics) {
    for (const ke of topics) {
      g.setNode(`topic:${ke}`, { width: TOPIC_W, height: TOPIC_H });
    }
  }
  for (const e of renderEdges) {
    if (g.hasNode(e.source) && g.hasNode(e.target)) {
      g.setEdge(e.source, e.target);
    }
  }

  dagre.layout(g);

  const rfNodes: Node[] = [];
  for (const [id, data] of agents) {
    const pos = g.node(id);
    rfNodes.push({
      id,
      type: "agent",
      data,
      position: { x: (pos?.x ?? 0) - AGENT_W / 2, y: (pos?.y ?? 0) - AGENT_H / 2 },
    });
  }
  if (showTopics) {
    for (const ke of topics) {
      const id = `topic:${ke}`;
      const pos = g.node(id);
      rfNodes.push({
        id,
        type: "topic",
        data: { label: ke } satisfies TopicNodeData,
        position: { x: (pos?.x ?? 0) - TOPIC_W / 2, y: (pos?.y ?? 0) - TOPIC_H / 2 },
      });
    }
  }

  return { nodes: rfNodes, edges: renderEdges };
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function GraphTab({ snapshot }: Props) {
  const [showTopics, setShowTopics] = useState(true);

  const { nodes, edges } = useMemo(
    () => buildGraph(snapshot, showTopics),
    [snapshot, showTopics],
  );

  const miniMapColor = useCallback((n: Node) => {
    if (n.type === "topic") return "#0284c7";
    const status = (n.data as AgentNodeData).status;
    return status === "online" ? "#22c55e" : status === "degraded" ? "#eab308" : "#6b7280";
  }, []);

  const empty = nodes.length === 0;

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
      ) : (
        <ReactFlow
          nodes={nodes}
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
