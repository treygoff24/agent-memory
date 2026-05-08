import { useMemo, useState } from 'react';

import { governance, type GovernanceItem } from '../data/fixtures';
import { Inspector, type InspectorItem } from '../inspector';
import { ReviewQueue } from './governanceView';

export type GovernanceFilter = 'all' | 'block' | 'warn' | 'info' | 'consent_required' | 'redact_proposed';
export type GovernanceDecision = 'redact_proposed' | 'review_required' | 'auto_approve' | 'consent_required';

export interface PolicyTraceStep {
    rule: string;
    action: 'allow' | 'match' | 'deny' | 'quarantine';
    outcome: string;
    ms: number;
}

export interface GovernanceViewItem extends GovernanceItem {
    decision: GovernanceDecision;
    sub: string[];
    meta: string;
    reason: string;
    rulePath: string;
    sensitivity: 'sensitive' | 'plain';
    encrypted: boolean;
    trace: PolicyTraceStep[];
}

const filters: Array<{ id: GovernanceFilter; label: string }> = [
    { id: 'all', label: 'all' },
    { id: 'block', label: 'blocks' },
    { id: 'warn', label: 'warnings' },
    { id: 'info', label: 'info' },
    { id: 'consent_required', label: 'consent' },
    { id: 'redact_proposed', label: 'redactions' },
];

function toGovernanceViewItem(item: GovernanceItem, index: number): GovernanceViewItem {
    const decision = item.decision as GovernanceDecision;
    return {
        ...item,
        decision,
        sub: [item.severity, decision.replace('_', ' '), `rule ${index + 1}`],
        meta: index === 0 ? '2m' : `${index + 1}h`,
        reason:
            item.severity === 'block'
                ? 'A privacy rule matched content that should not be broad-surfaced.'
                : item.severity === 'warn'
                  ? 'Confidence or consent policy requires a human review before write promotion.'
                  : 'The item is included as an explainability breadcrumb.',
        rulePath: item.severity === 'block' ? 'privacy.source.redaction' : item.severity === 'warn' ? 'governance.review.human_required' : 'governance.review.info',
        sensitivity: item.severity === 'block' ? 'sensitive' : 'plain',
        encrypted: item.severity === 'block',
        trace: [
            { rule: 'capture.classify', action: item.severity === 'block' ? 'match' : 'allow', outcome: item.severity === 'block' ? 'sensitive candidate' : 'no hard block', ms: 1.1 + index },
            { rule: 'governance.policy', action: item.severity === 'block' ? 'quarantine' : item.severity === 'warn' ? 'match' : 'allow', outcome: decision, ms: 0.8 + index / 10 },
            { rule: 'review.queue', action: item.severity === 'info' ? 'allow' : 'match', outcome: 'surface in review queue', ms: 0.3 },
        ],
    };
}

function governanceItems(): GovernanceViewItem[] {
    const base = governance.map(toGovernanceViewItem);
    return [
        ...base,
        toGovernanceViewItem(
            {
                id: 'gov_consent_family',
                title: 'Family detail consent required',
                severity: 'warn',
                decision: 'consent_required',
                namespace: 'personal/family',
            },
            base.length,
        ),
    ];
}

function inspectorItemFromGovernance(item: GovernanceViewItem | undefined): InspectorItem | null {
    if (!item) return null;
    return {
        kind: 'governance-decision',
        id: item.id,
        title: item.title,
        namespace: item.namespace,
        body: `${item.reason} Policy decision trace: ${item.trace.map((step) => `${step.rule}=${step.outcome}`).join(' → ')}.`,
        sensitivity: item.sensitivity,
        encrypted: item.encrypted,
        meta: item.meta,
        policy: {
            privacy: item.sensitivity ?? 'plain',
            governance: item.decision,
            tombstone: item.decision === 'redact_proposed' ? 'proposed redaction' : 'none',
        },
        provenance: {
            written: item.meta,
            session: 'review-queue',
            grounding: item.rulePath,
            confidence: item.severity,
            device: 'mbp',
        },
        evidence: item.trace.map((step, index) => ({
            id: `${item.id}_trace_${index}`,
            title: `${step.rule}: ${step.outcome}`,
            score: Math.max(0.1, 1 - step.ms / 10),
        })),
        summary: item.decision,
    };
}

export function Governance() {
    const items = useMemo(governanceItems, []);
    const [filter, setFilter] = useState<GovernanceFilter>('all');
    const [selectedId, setSelectedId] = useState(items[0]?.id ?? '');
    const [checked, setChecked] = useState<Set<string>>(() => new Set());
    const visible = useMemo(
        () => (filter === 'all' ? items : items.filter((item) => item.severity === filter || item.decision === filter)),
        [filter, items],
    );
    const selected = visible.find((item) => item.id === selectedId) ?? visible[0];
    const counts = filters.reduce<Record<GovernanceFilter, number>>(
        (acc, filterOption) => {
            acc[filterOption.id] =
                filterOption.id === 'all'
                    ? items.length
                    : items.filter((item) => item.severity === filterOption.id || item.decision === filterOption.id).length;
            return acc;
        },
        { all: 0, block: 0, warn: 0, info: 0, consent_required: 0, redact_proposed: 0 },
    );

    function updateFilter(next: GovernanceFilter) {
        setFilter(next);
        const nextSelected = next === 'all' ? items[0] : items.find((item) => item.severity === next || item.decision === next);
        setSelectedId(nextSelected?.id ?? '');
        setChecked(new Set());
    }

    function toggleChecked(id: string) {
        setChecked((current) => {
            const next = new Set(current);
            if (next.has(id)) next.delete(id);
            else next.add(id);
            return next;
        });
    }

    function toggleAll() {
        setChecked((current) => (current.size === visible.length ? new Set() : new Set(visible.map((item) => item.id))));
    }

    return (
        <div data-testid={`governance-view-${filter}`}>
            <div className="view-header">
                <span className="view-title">Governance</span>
                <span className="view-subtitle">
                    · review queue · {items.length} items · {counts.block} blocks
                </span>
                <span className="spacer" />
                <div
                    className="filter-pills"
                    role="tablist"
                    aria-label="Governance filters"
                >
                    {filters.map((filterOption, index) => (
                        <button
                            key={filterOption.id}
                            className={`pill ${filter === filterOption.id ? 'active' : ''}`}
                            onClick={() => updateFilter(filterOption.id)}
                            role="tab"
                            aria-selected={filter === filterOption.id}
                            type="button"
                        >
                            <span>{filterOption.label}</span>
                            <span className="count">{counts[filterOption.id]}</span>
                            <span className="kbd-hint">{index + 1}</span>
                        </button>
                    ))}
                </div>
            </div>
            {checked.size > 0 ? (
                <div className="batch-bar">
                    <span
                        className="batch-count"
                        data-testid="governance-batch-count"
                    >
                        <span className="mono">{checked.size}</span> selected
                    </span>
                    <span className="sep">·</span>
                    <button
                        className="btn-link"
                        onClick={toggleAll}
                        type="button"
                    >
                        {checked.size === visible.length ? 'deselect all' : `select all ${visible.length}`}
                    </button>
                    <span className="spacer" />
                    <button
                        className="btn primary"
                        type="button"
                    >
                        Approve selected
                    </button>
                    <button
                        className="btn"
                        type="button"
                    >
                        Reject selected
                    </button>
                </div>
            ) : null}
            <div className="panes-2">
                <div className="pane left">
                    <div className="pane-scroll">
                        <ReviewQueue
                            items={visible}
                            selectedId={selected?.id ?? ''}
                            checked={checked}
                            onSelect={setSelectedId}
                            onCheck={toggleChecked}
                        />
                    </div>
                </div>
                <div className="pane">
                    <div className="pane-scroll">
                        <Inspector
                            item={inspectorItemFromGovernance(selected)}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </div>
    );
}
