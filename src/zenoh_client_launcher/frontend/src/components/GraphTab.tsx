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
 * and laid out with ELK's `stress` algorithm — a force-directed-style
 * placement that minimises total edge stress (think Graphviz `neato`),
 * so nodes that share connections gravitate toward each other and the
 * spatial layout reveals structural relationships instead of imposing
 * a left-to-right pipeline.
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

// Handles on all four sides so edges can attach from whichever side is
// closest to the connected peer — stress layout places nodes by
// relationship, not on a left-to-right grid, so the natural arrow
// direction is "whichever way points at the target".
const HANDLE_POSITIONS: Position[] = [
  Position.Top,
  Position.Right,
  Position.Bottom,
  Position.Left,
];

function AgentNode({ data }: NodeProps) {
  const d = data as AgentNodeData;
  const s = STATUS_STYLES[d.status];
  return (
    <div
      className={`flex items-center gap-2 rounded-full border bg-gray-900 px-4 py-2 shadow-md ${s.ring}`}
      style={{ width: AGENT_W, height: AGENT_H }}
      title={`${d.label} (${d.status})`}
    >
      {HANDLE_POSITIONS.map((p) => (
        <Handle
          key={`t-${p}`}
          type="target"
          id={`t-${p}`}
          position={p}
          className="!w-0.5 !h-0.5 !min-w-0 !min-h-0 !border-0 !bg-transparent !opacity-0"
        />
      ))}
      <span className={`h-2 w-2 shrink-0 rounded-full ${s.dot}`} />
      <span className="truncate font-mono text-xs text-gray-100">{d.label}</span>
      {HANDLE_POSITIONS.map((p) => (
        <Handle
          key={`s-${p}`}
          type="source"
          id={`s-${p}`}
          position={p}
          className="!w-0.5 !h-0.5 !min-w-0 !min-h-0 !border-0 !bg-transparent !opacity-0"
        />
      ))}
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
      {HANDLE_POSITIONS.map((p) => (
        <Handle
          key={`t-${p}`}
          type="target"
          id={`t-${p}`}
          position={p}
          className="!w-0.5 !h-0.5 !min-w-0 !min-h-0 !border-0 !bg-transparent !opacity-0"
        />
      ))}
      <span className="truncate font-mono text-[11px] text-zenoh-100">{d.label}</span>
      {HANDLE_POSITIONS.map((p) => (
        <Handle
          key={`s-${p}`}
          type="source"
          id={`s-${p}`}
          position={p}
          className="!w-0.5 !h-0.5 !min-w-0 !min-h-0 !border-0 !bg-transparent !opacity-0"
        />
      ))}
    </div>
  );
}

const nodeTypes = { agent: AgentNode, topic: TopicNode };

// ── ELK setup ────────────────────────────────────────────────────────────────

// One instance per module — ELK is stateless across calls but the
// constructor spins up a worker, so we don't want to do it per render.
const elk = new ELK();

// Force-directed placement via ELK's `force` algorithm (Eades model):
// connected nodes attract along a spring, every node pair repels via a
// Coulomb-style force. Unlike `stress`, which can let nodes overlap when
// the desired-spring-length is shorter than the node geometry allows,
// `force` keeps non-connected nodes apart and so produces the
// Graphviz-`neato` look without bodies overlapping each other.
//
// Deterministic given identical input — important so that the
// per-topology cache below doesn't relayout on every snapshot tick.
// ELK option values are strings.
const ELK_LAYOUT_OPTIONS: Record<string, string> = {
  "elk.algorithm": "force",
  // Spacing between any two nodes (Coulomb repulsion magnitude).
  // Just enough that bodies don't overlap; the curved edges fill the
  // visual breathing room without needing wide empty space.
  "elk.spacing.nodeNode": "60",
  // Number of force iterations. Default is 300; raise if the layout
  // looks under-relaxed (nodes still drifting visibly when you reload).
  "elk.force.iterations": "400",
  // Repulsion power scales Coulomb force; default 1.0.
  "elk.force.repulsivePower": "1",
  // Fixed seed so the algorithm produces identical positions for
  // identical input — required for the topology cache to be useful.
  "elk.randomSeed": "1",
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

  // Sort the input deterministically — the backend HashMap<String,
  // AgentInfo> serialises with randomised key order, which would feed
  // ELK a different node ordering every poll and let ELK's layered
  // tie-breaker flip ping/pong between calls.
  const nodesByCn = [...snapshot.connected_nodes].sort((a, b) =>
    a.cn.localeCompare(b.cn),
  );
  for (const node of nodesByCn) {
    const agentsByName = Object.entries(node.agents).sort(([a], [b]) =>
      a.localeCompare(b),
    );
    for (const [agentName, ag] of agentsByName) {
      const agentId = `agent:${node.cn}/${agentName}`;
      agents.set(agentId, {
        label: `${node.cn}/${agentName}`,
        status: ag.status,
      });

      const sortedPublishes = [...ag.publishes].sort();
      const sortedSubscribes = [...ag.subscribes].sort();
      for (const ke of sortedPublishes) {
        topics.add(ke);
        edges.push({
          id: `pub:${agentId}->${ke}`,
          source: agentId,
          target: `topic:${ke}`,
          // Default bezier curves — match the Graphviz `neato` look that
          // pairs with force-directed placement. The orthogonal
          // smoothstep aesthetic was a leftover from the layered
          // algorithm and felt rigid here.
          animated: ag.status === "online",
          markerEnd: { type: MarkerType.ArrowClosed },
          style: { stroke: "#0ea5e9" },
        });
      }
      for (const ke of sortedSubscribes) {
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

interface LayoutResult {
  positions: Record<string, { x: number; y: number }>;
  edgeHandles: Record<string, { sourceHandle: string; targetHandle: string }>;
}

/**
 * Run ELK's stress algorithm and return positions plus per-edge handle
 * assignments. With stress placement nodes can sit on any side of each
 * other, so the right "where does the arrow leave / arrive" answer is
 * "whichever side of the source faces the target." We compute that for
 * each edge from the laid-out coordinates.
 */
async function layoutWithElk(nodes: Node[], edges: Edge[]): Promise<LayoutResult> {
  const nodeIds = new Set(nodes.map((n) => n.id));
  const nodeSize = (n: Node) =>
    n.type === "topic"
      ? { w: TOPIC_W, h: TOPIC_H }
      : { w: AGENT_W, h: AGENT_H };

  const elkGraph: ElkNode = {
    id: "root",
    layoutOptions: ELK_LAYOUT_OPTIONS,
    children: nodes.map((n) => ({
      id: n.id,
      width: nodeSize(n).w,
      height: nodeSize(n).h,
    })),
    edges: edges
      .filter((e) => nodeIds.has(e.source) && nodeIds.has(e.target))
      .map((e) => ({
        id: e.id,
        sources: [e.source],
        targets: [e.target],
      })),
  };

  const result = await elk.layout(elkGraph);
  const positions: Record<string, { x: number; y: number }> = {};
  for (const c of result.children ?? []) {
    positions[c.id] = { x: c.x ?? 0, y: c.y ?? 0 };
  }

  // Index nodes by id once so the per-edge handle picker is O(1).
  const nodeById = new Map<string, Node>(nodes.map((n) => [n.id, n]));

  // Bidirectional detection: pre-build a set of "source|target" keys so
  // we can spot when an edge has a partner in the opposite direction
  // (typical for ping/pong, where each agent publishes one topic and
  // subscribes to the other — in the collapsed-topics view these
  // become two edges between the same two nodes, each labelled with
  // its topic).
  const edgeKeys = new Set(edges.map((e) => `${e.source}|${e.target}`));

  // Dominant-axis rule for the simple (single-direction) case: pick the
  // source / target handle pair on the side that faces the other
  // endpoint. For bidirectional pairs the two curves would overlap on
  // those handles, so we use the *perpendicular* axis instead and split
  // the two curves to opposite sides (one above, one below) so both
  // arrows and both labels remain visible.
  const edgeHandles: Record<string, { sourceHandle: string; targetHandle: string }> = {};
  for (const e of edges) {
    const s = nodeById.get(e.source);
    const t = nodeById.get(e.target);
    const sp = positions[e.source];
    const tp = positions[e.target];
    if (!s || !t || !sp || !tp) continue;
    const ss = nodeSize(s);
    const ts = nodeSize(t);
    const sx = sp.x + ss.w / 2;
    const sy = sp.y + ss.h / 2;
    const tx = tp.x + ts.w / 2;
    const ty = tp.y + ts.h / 2;
    const dx = tx - sx;
    const dy = ty - sy;
    const dominantHorizontal = Math.abs(dx) > Math.abs(dy);
    const hasReverse = edgeKeys.has(`${e.target}|${e.source}`);

    if (hasReverse) {
      // Stable assignment: the lexicographically smaller source picks
      // the "first" side (top if horizontal-dominant, left if
      // vertical-dominant). The reverse direction takes the opposite.
      const isFirst = e.source < e.target;
      if (dominantHorizontal) {
        edgeHandles[e.id] = isFirst
          ? { sourceHandle: "s-top", targetHandle: "t-top" }
          : { sourceHandle: "s-bottom", targetHandle: "t-bottom" };
      } else {
        edgeHandles[e.id] = isFirst
          ? { sourceHandle: "s-left", targetHandle: "t-left" }
          : { sourceHandle: "s-right", targetHandle: "t-right" };
      }
    } else if (dominantHorizontal) {
      edgeHandles[e.id] =
        dx > 0
          ? { sourceHandle: "s-right", targetHandle: "t-left" }
          : { sourceHandle: "s-left", targetHandle: "t-right" };
    } else {
      edgeHandles[e.id] =
        dy > 0
          ? { sourceHandle: "s-bottom", targetHandle: "t-top" }
          : { sourceHandle: "s-top", targetHandle: "t-bottom" };
    }
  }

  return { positions, edgeHandles };
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function GraphTab({ snapshot }: Props) {
  const [showTopics, setShowTopics] = useState(true);

  // Pure data construction — synchronous and cheap.
  const { unpositionedNodes, edges } = useMemo(
    () => buildGraph(snapshot, showTopics),
    [snapshot, showTopics],
  );

  // Topology fingerprint: only the node set + edge endpoints, not the
  // node data (which changes every 500 ms tick as last_seen_secs_ago
  // ticks and status flickers between online/degraded). Layout depends
  // only on topology, so the effect should only fire when this string
  // actually changes.
  const topologyKey = useMemo(() => {
    const nodeIds = unpositionedNodes
      .map((n) => `${n.type}:${n.id}`)
      .sort()
      .join("|");
    const edgeKeys = edges.map((e) => `${e.source}->${e.target}`).sort().join("|");
    return `${nodeIds}::${edgeKeys}`;
  }, [unpositionedNodes, edges]);

  // ELK is async. We cache positions (by node id) and edge handle
  // assignments (by edge id) keyed by topology, so status / heartbeat
  // ticks don't trigger relayout and a slow layout from tick N can't
  // overwrite a fresh status update from tick N+1.
  const [layout, setLayout] = useState<LayoutResult>({
    positions: {},
    edgeHandles: {},
  });

  useEffect(() => {
    if (unpositionedNodes.length === 0) {
      setLayout({ positions: {}, edgeHandles: {} });
      return;
    }
    let cancelled = false;
    layoutWithElk(unpositionedNodes, edges)
      .then((result) => {
        if (cancelled) return;
        setLayout(result);
      })
      .catch((err) => {
        // eslint-disable-next-line no-console
        console.error("ELK layout failed:", err);
      });
    return () => {
      cancelled = true;
    };
    // Depend on topologyKey, not on unpositionedNodes/edges, so the
    // layout only re-runs when the graph's structure changes (new
    // agent, removed topic, showTopics toggle) — not on every 500 ms
    // snapshot tick.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [topologyKey]);

  // Merge current data with cached positions. New nodes (no cached
  // position yet) render at (0, 0) for the single frame between
  // topology change and the next layout result; usually invisible.
  const positionedNodes = useMemo<Node[]>(
    () =>
      unpositionedNodes.map((n) => ({
        ...n,
        position: layout.positions[n.id] ?? { x: 0, y: 0 },
      })),
    [unpositionedNodes, layout.positions],
  );

  // Merge cached handle assignments onto the current edge set so each
  // arrow leaves / arrives from the side facing the other node.
  const renderedEdges = useMemo<Edge[]>(
    () =>
      edges.map((e) => {
        const h = layout.edgeHandles[e.id];
        return h ? { ...e, sourceHandle: h.sourceHandle, targetHandle: h.targetHandle } : e;
      }),
    [edges, layout.edgeHandles],
  );

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
          edges={renderedEdges}
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
