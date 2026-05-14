import type { GovernanceDecisionItem, InspectorItem } from '../types';

import { CardFrame } from './CardFrame';

function TraceRows({ item }: { item: GovernanceDecisionItem }) {
    const steps = item.policyTrace;
    if (!steps || steps.length === 0) {
        return (
            <dl className="kv">
                <dt>decision</dt>
                <dd>{item.policy?.governance ?? 'default-v1'}</dd>
            </dl>
        );
    }
    return (
        <div className="trace">
            {steps.map((step) => (
                <div
                    key={step.step}
                    className={`trace-row ${step.action === 'deny' ? 'deny' : step.action === 'match' || step.action === 'quarantine' ? 'match' : ''}`}
                >
                    <span className="trace-step">{step.step}</span>
                    <span className="trace-rule">{step.rule}</span>
                    <span className="trace-action">{step.action}</span>
                    <span className="trace-outcome">{step.outcome}</span>
                    <span className="trace-ms">{step.ms.toFixed(1)}ms</span>
                </div>
            ))}
        </div>
    );
}

export function PolicyDecisionTraceCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame
            title="Policy decision trace"
            meta={item.kind === 'governance-decision' ? 'active' : undefined}
        >
            {item.kind === 'governance-decision' ? (
                <TraceRows item={item} />
            ) : (
                <dl className="kv">
                    <dt>decision</dt>
                    <dd>no manual decision</dd>
                    <dt>policy</dt>
                    <dd>{item.policy?.governance ?? 'default-v1'}</dd>
                </dl>
            )}
        </CardFrame>
    );
}
