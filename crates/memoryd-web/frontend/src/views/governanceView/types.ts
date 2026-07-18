// Shared view-model types for the Governance view. Extracted to a leaf module so the
// `Governance` container and its `ReviewQueue` child can both depend on them without
// forming an import cycle.

export type GovernanceDecision = 'redact_proposed' | 'review_required' | 'auto_approve' | 'consent_required';

interface PolicyTraceStep {
    rule: string;
    action: 'allow' | 'match' | 'deny' | 'quarantine';
    outcome: string;
    ms: number;
}

export interface GovernanceViewItem {
    id: string;
    title: string;
    severity: 'block' | 'warn' | 'info';
    decision: GovernanceDecision;
    namespace: string;
    sub: string[];
    meta: string;
    reason: string;
    rulePath: string;
    sensitivity: 'sensitive' | 'plain';
    encrypted: boolean;
    trace: PolicyTraceStep[];
}
