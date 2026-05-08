import type { ReactNode } from 'react';

export type InspectorKind =
    | 'inbox-review'
    | 'inbox-recall'
    | 'inbox-conflict'
    | 'inbox-due'
    | 'inbox-dream'
    | 'recall-event'
    | 'dream-output'
    | 'peer-detail'
    | 'governance-decision'
    | 'entity-detail';

export type InspectorAction =
    | 'approve'
    | 'reject'
    | 'edit'
    | 'forget'
    | 'keep-local'
    | 'keep-remote'
    | 'custom-merge'
    | 'verify-now'
    | 'skip'
    | 'promote'
    | 'queue-question'
    | 'dismiss';

export interface ProvenanceInfo {
    written?: string;
    session?: string;
    grounding?: string;
    confidence?: string;
    device?: string;
    peers?: string;
}

export interface PolicyInfo {
    privacy?: string;
    governance?: string;
    tombstone?: string;
}

export interface EvidenceItem {
    id: string;
    title: string;
    score?: number;
}

export interface RecallEventInfo {
    when: string;
    who: string;
    session: string;
}

export interface ConflictSide {
    device: string;
    body: string;
    written: string;
    session: string;
}

export interface InspectorBase {
    kind: InspectorKind;
    id: string;
    title: string;
    namespace: string;
    body?: string;
    confidence?: number;
    sensitivity?: string;
    encrypted?: boolean;
    meta?: string;
    sessionId?: string;
    memoryId?: string;
    summary?: string;
    provenance?: ProvenanceInfo;
    policy?: PolicyInfo;
    evidence?: EvidenceItem[];
    recalls?: RecallEventInfo[];
    recallCountTotal?: number;
    recallCount30d?: number;
    spark?: number[];
    diff?: {
        local: ConflictSide;
        remote: ConflictSide;
    };
    detail?: ReactNode;
}

export type InboxReviewItem = InspectorBase & { kind: 'inbox-review' };
export type InboxRecallItem = InspectorBase & { kind: 'inbox-recall' };
export type InboxConflictItem = InspectorBase & { kind: 'inbox-conflict' };
export type InboxDueItem = InspectorBase & { kind: 'inbox-due' };
export type InboxDreamItem = InspectorBase & { kind: 'inbox-dream' };
export type RecallEventItem = InspectorBase & { kind: 'recall-event' };
export type DreamOutputItem = InspectorBase & { kind: 'dream-output' };
export type PeerDetailItem = InspectorBase & { kind: 'peer-detail' };
export type GovernanceDecisionItem = InspectorBase & { kind: 'governance-decision' };
export type EntityDetailItem = InspectorBase & { kind: 'entity-detail' };

export type InspectorItem =
    | InboxReviewItem
    | InboxRecallItem
    | InboxConflictItem
    | InboxDueItem
    | InboxDreamItem
    | RecallEventItem
    | DreamOutputItem
    | PeerDetailItem
    | GovernanceDecisionItem
    | EntityDetailItem;

export type InspectorLayout = 'wide' | 'narrow';

export interface InspectorKindProps<TItem extends InspectorItem = InspectorItem> {
    item: TItem;
    layout: InspectorLayout;
    onAction?: ((action: InspectorAction, item: TItem) => void) | undefined;
}
