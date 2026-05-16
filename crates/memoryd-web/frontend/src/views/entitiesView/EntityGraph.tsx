import {
    forceCenter,
    forceCollide,
    forceLink,
    forceManyBody,
    forceSimulation,
    forceX,
    forceY,
    type SimulationLinkDatum,
    type SimulationNodeDatum,
} from 'd3-force';
import { useMemo, useRef, useState, type ComponentRef, type KeyboardEvent } from 'react';

import type { EntityEdge, EntityNode } from '../../api';

// Color-by mode maps entity fields to CSS color tokens.
export type ColorBy = 'kind' | 'namespace' | 'confidence';
export type Density = 'sparse' | 'dense';

interface EntityGraphProps {
    nodes: EntityNode[];
    edges: EntityEdge[];
    colorBy: ColorBy;
    density: Density;
    focusId: string | null;
    /** Selecting an entity is owned by the parent so it can preserve any
     *  route-local hash query suffix (e.g. `?mode=table`) that calling the
     *  router's bare `navigate()` would clobber. */
    onSelect: (entityId: string) => void;
    onRequestTableMode: () => void;
}

// A simple LCG random source with a fixed seed — gives a deterministic layout
// that doesn't vary across mounts or React StrictMode double-invocations.
function seededLcg(seed = 1) {
    const a = 1664525;
    const c = 1013904223;
    const m = 4294967296; // 2^32
    let s = seed >>> 0;
    return () => {
        s = ((a * s + c) >>> 0) % m;
        return s / m;
    };
}

// Map entity kind to a CSS custom-property color token.
const KIND_COLORS: Record<string, string> = {
    person: 'var(--accent)',
    org: 'var(--info)',
    project: 'var(--warn)',
    language: 'var(--fg-2)',
    tool: 'var(--fg-3)',
    place: 'var(--bad)',
};

function kindColor(kind: string): string {
    return KIND_COLORS[kind] ?? 'var(--fg-3)';
}

function namespaceColor(namespace: string | undefined): string {
    if (!namespace) return 'var(--fg-3)';
    if (namespace.startsWith('personal/') || namespace.startsWith('me/')) return 'var(--accent)';
    if (namespace.startsWith('project:') || namespace.startsWith('coding/')) return 'var(--warn)';
    if (namespace.startsWith('work/')) return 'var(--info)';
    return 'var(--fg-2)';
}

function confidenceColor(memory_count: number): string {
    // Proxy confidence from memory_count for graph coloring.
    if (memory_count >= 30) return 'var(--accent)';
    if (memory_count >= 15) return 'var(--warn)';
    return 'var(--fg-3)';
}

function nodeColor(
    node: { kind: string; namespace?: string | undefined; memory_count: number },
    colorBy: ColorBy,
): string {
    if (colorBy === 'namespace') return namespaceColor(node.namespace);
    if (colorBy === 'confidence') return confidenceColor(node.memory_count);
    return kindColor(node.kind);
}

// Node radius is proportional to memory_count, clamped to 4–14 px.
function nodeRadius(count: number): number {
    return Math.min(14, Math.max(4, 4 + Math.sqrt(count) * 1.5));
}

// The types d3-force mutates into nodes during simulation setup.
interface SimNode extends SimulationNodeDatum {
    id: string;
    label: string;
    kind: string;
    namespace: string | undefined;
    memory_count: number;
}

interface SimLink extends SimulationLinkDatum<SimNode> {
    source: SimNode | string;
    target: SimNode | string;
    weight: number;
}

const WIDTH = 560;
const HEIGHT = 420;
const TICK_COUNT = 300;
const MAX_RENDERED_NODES = 120;
const MAX_RENDERED_EDGES = 240;
const MIN_RENDERED_EDGE_WEIGHT = 0.3;

function topGraphNodes(nodes: EntityNode[]): EntityNode[] {
    return [...nodes]
        .sort(
            (left, right) =>
                right.memory_count - left.memory_count ||
                left.label.localeCompare(right.label) ||
                left.id.localeCompare(right.id),
        )
        .slice(0, MAX_RENDERED_NODES);
}

function graphEdgesForNodes(edges: EntityEdge[], nodes: EntityNode[]): EntityEdge[] {
    const ids = new Set(nodes.map((node) => node.id));
    return edges
        .filter((edge) => ids.has(edge.source) && ids.has(edge.target) && edge.weight >= MIN_RENDERED_EDGE_WEIGHT)
        .sort((left, right) => right.weight - left.weight || left.source.localeCompare(right.source))
        .slice(0, MAX_RENDERED_EDGES);
}

function buildLayout(
    apiNodes: EntityNode[],
    apiEdges: EntityEdge[],
    density: Density,
): { nodes: SimNode[]; links: Array<{ source: SimNode; target: SimNode; weight: number }> } {
    const simNodes: SimNode[] = apiNodes.map((n) => ({
        id: n.id,
        label: n.label,
        kind: n.kind,
        namespace: n.namespace,
        memory_count: n.memory_count,
    }));

    const idSet = new Set(simNodes.map((n) => n.id));
    const simLinks: SimLink[] = apiEdges
        .filter((e) => idSet.has(e.source) && idSet.has(e.target))
        .map((e) => ({ source: e.source, target: e.target, weight: e.weight }));

    const chargeStrength = density === 'dense' ? -60 : -120;
    const linkDistance = density === 'dense' ? 50 : 100;

    const sim = forceSimulation<SimNode>(simNodes)
        .randomSource(seededLcg(42))
        .force('charge', forceManyBody<SimNode>().strength(chargeStrength))
        .force('center', forceCenter(WIDTH / 2, HEIGHT / 2))
        .force(
            'link',
            forceLink<SimNode, SimLink>(simLinks)
                .id((d) => d.id)
                .distance(linkDistance),
        )
        .force(
            'collide',
            forceCollide<SimNode>().radius((d) => nodeRadius(d.memory_count) + 4),
        )
        .force('x', forceX(WIDTH / 2).strength(0.05))
        .force('y', forceY(HEIGHT / 2).strength(0.05))
        .stop();

    sim.tick(TICK_COUNT);

    // After ticking, d3 has mutated source/target from string → SimNode ref.
    const resolvedLinks = (sim.force('link') as ReturnType<typeof forceLink> | undefined)?.links() as
        | Array<{ source: SimNode; target: SimNode; weight: number }>
        | undefined;

    return { nodes: sim.nodes(), links: resolvedLinks ?? [] };
}

export function EntityGraph({
    nodes: apiNodes,
    edges: apiEdges,
    colorBy,
    density,
    focusId,
    onSelect,
    onRequestTableMode,
}: EntityGraphProps) {
    const [hoveredId, setHoveredId] = useState<string | null>(null);
    const [tooltipPos, setTooltipPos] = useState<{ x: number; y: number } | null>(null);
    const svgRef = useRef<ComponentRef<'svg'>>(null);

    const renderedNodes = useMemo(() => topGraphNodes(apiNodes), [apiNodes]);
    const renderedEdges = useMemo(() => graphEdgesForNodes(apiEdges, renderedNodes), [apiEdges, renderedNodes]);
    const omittedNodeCount = Math.max(0, apiNodes.length - renderedNodes.length);

    // Re-run the layout whenever input data or density changes. The seeded RNG
    // ensures identical inputs always produce the same positions. The graph is
    // capped before layout so large corpora cannot lock the main thread.
    const { nodes, links } = useMemo(
        () => buildLayout(renderedNodes, renderedEdges, density),
        [renderedNodes, renderedEdges, density],
    );

    if (apiNodes.length === 0) {
        return (
            <div className="graph-empty">
                <span className="fg-3">No entities mapped for this namespace yet.</span>
            </div>
        );
    }

    function handleNodeClick(id: string) {
        onSelect(id);
    }

    function handleNodeKeyDown(event: KeyboardEvent, id: string) {
        if (event.key !== 'Enter' && event.key !== ' ') return;
        event.preventDefault();
        onSelect(id);
    }

    function handleNodeMouseEnter(id: string, svgX: number, svgY: number) {
        setHoveredId(id);
        setTooltipPos({ x: svgX, y: svgY });
    }

    function handleNodeMouseLeave() {
        setHoveredId(null);
        setTooltipPos(null);
    }

    const hoveredNode = hoveredId ? nodes.find((n) => n.id === hoveredId) : null;

    return (
        <div className="graph-container">
            {omittedNodeCount > 0 ? (
                <div
                    className="graph-limit"
                    role="status"
                >
                    <span>
                        Showing top {MAX_RENDERED_NODES.toLocaleString()} of {apiNodes.length.toLocaleString()}{' '}
                        entities.
                    </span>
                    <button
                        type="button"
                        className="btn graph-table-fallback"
                        onClick={onRequestTableMode}
                    >
                        Switch to table mode
                    </button>
                </div>
            ) : null}
            <svg
                ref={svgRef}
                className="graph-svg"
                viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
                aria-label="Entity relationship graph"
                role="group"
            >
                <title>Entity relationship graph</title>
                <g className="graph-links">
                    {links.map((link) => {
                        const src = link.source as SimNode;
                        const tgt = link.target as SimNode;
                        if (src.x == null || src.y == null || tgt.x == null || tgt.y == null) return null;
                        const isFaded = focusId !== null && src.id !== focusId && tgt.id !== focusId;
                        return (
                            <line
                                key={`${src.id}-${tgt.id}`}
                                x1={src.x}
                                y1={src.y}
                                x2={tgt.x}
                                y2={tgt.y}
                                className="graph-edge"
                                opacity={isFaded ? 0.15 : Math.min(1, link.weight * 0.6 + 0.2)}
                            />
                        );
                    })}
                </g>
                <g className="graph-nodes">
                    {nodes.map((node) => {
                        if (node.x == null || node.y == null) return null;
                        const r = nodeRadius(node.memory_count);
                        const color = nodeColor(node, colorBy);
                        const isFocused = focusId === null || node.id === focusId;
                        const isHovered = node.id === hoveredId;
                        return (
                            <g
                                key={node.id}
                                className="graph-node-group"
                                transform={`translate(${node.x},${node.y})`}
                                role="button"
                                tabIndex={0}
                                aria-label={`Select entity ${node.label}`}
                                onClick={() => handleNodeClick(node.id)}
                                onKeyDown={(event) => handleNodeKeyDown(event, node.id)}
                                onMouseEnter={(e) => {
                                    const rect = svgRef.current?.getBoundingClientRect();
                                    const svgX = rect ? e.clientX - rect.left : node.x!;
                                    const svgY = rect ? e.clientY - rect.top : node.y!;
                                    handleNodeMouseEnter(node.id, svgX, svgY);
                                }}
                                onMouseLeave={handleNodeMouseLeave}
                                style={{ cursor: 'pointer', opacity: isFocused ? 1 : 0.3 }}
                            >
                                <title>{node.label}</title>
                                <circle
                                    r={isHovered ? r + 2 : r}
                                    fill={color}
                                    className="graph-node-circle"
                                    style={{ transition: 'r 0.1s ease' }}
                                />
                                {r >= 8 || isHovered ? (
                                    <text
                                        className="graph-node-label"
                                        dy={r + 11}
                                        textAnchor="middle"
                                    >
                                        {node.label}
                                    </text>
                                ) : null}
                            </g>
                        );
                    })}
                </g>
            </svg>
            {hoveredNode && tooltipPos ? (
                <div
                    className="graph-tooltip"
                    style={{ left: tooltipPos.x + 12, top: tooltipPos.y - 8 }}
                    aria-hidden="true"
                >
                    <span className="graph-tooltip-name">{hoveredNode.label}</span>
                    <span className="graph-tooltip-meta">
                        {hoveredNode.kind} · {hoveredNode.memory_count} memories
                    </span>
                    {hoveredNode.namespace ? <span className="graph-tooltip-ns">{hoveredNode.namespace}</span> : null}
                </div>
            ) : null}
        </div>
    );
}
