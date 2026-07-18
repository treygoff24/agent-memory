import { useEffect } from 'react';

import type { InspectorAction, InspectorItem, InspectorLayout } from './types';

import { isTextInputTarget } from '../keyboard/useKeymap';
import { DreamOutputInspector } from './kinds/dreamOutput';
import { EntityDetailInspector } from './kinds/entityDetail';
import { GovernanceDecisionInspector } from './kinds/governanceDecision';
import { InboxConflictInspector } from './kinds/inboxConflict';
import { InboxDreamInspector } from './kinds/inboxDream';
import { InboxDueInspector } from './kinds/inboxDue';
import { InboxRecallInspector } from './kinds/inboxRecall';
import { InboxReviewInspector } from './kinds/inboxReview';
import { PeerDetailInspector } from './kinds/peerDetail';
import { RecallEventInspector } from './kinds/recallEvent';

export interface InspectorProps {
    item: InspectorItem | null;
    layout?: InspectorLayout;
    onAction?: (action: InspectorAction, item: InspectorItem) => void;
}

const inboxReviewKeys = new Map<string, InspectorAction>([
    ['a', 'approve'],
    ['r', 'reject'],
    ['e', 'edit'],
    ['f', 'forget'],
]);

export function Inspector({ item, layout = 'wide', onAction }: InspectorProps) {
    useEffect(() => {
        if (!item || item.kind !== 'inbox-review' || !onAction) return undefined;
        const onKeyDown = (event: KeyboardEvent) => {
            if (isTextInputTarget(event.target) || isTextInputTarget(document.activeElement)) return;
            const action = inboxReviewKeys.get(event.key.toLowerCase());
            if (!action) return;
            event.preventDefault();
            onAction(action, item);
        };
        window.addEventListener('keydown', onKeyDown);
        return () => window.removeEventListener('keydown', onKeyDown);
    }, [item, onAction]);

    if (!item) {
        return (
            <div
                className="empty"
                role="region"
                aria-label="Inspector"
            >
                <span className="ico">○</span>
                <h3>Nothing selected</h3>
                <p>Pick a row to inspect it. Use ↑↓ to navigate, enter to focus.</p>
            </div>
        );
    }

    switch (item.kind) {
        case 'inbox-review':
            return (
                <InboxReviewInspector
                    item={item}
                    layout={layout}
                    onAction={onAction}
                />
            );
        case 'inbox-recall':
            return (
                <InboxRecallInspector
                    item={item}
                    layout={layout}
                    onAction={onAction}
                />
            );
        case 'inbox-conflict':
            return (
                <InboxConflictInspector
                    item={item}
                    layout={layout}
                    onAction={onAction}
                />
            );
        case 'inbox-due':
            return (
                <InboxDueInspector
                    item={item}
                    layout={layout}
                    onAction={onAction}
                />
            );
        case 'inbox-dream':
            return (
                <InboxDreamInspector
                    item={item}
                    layout={layout}
                    onAction={onAction}
                />
            );
        case 'recall-event':
            return (
                <RecallEventInspector
                    item={item}
                    layout={layout}
                    onAction={onAction}
                />
            );
        case 'dream-output':
            return (
                <DreamOutputInspector
                    item={item}
                    layout={layout}
                    onAction={onAction}
                />
            );
        case 'peer-detail':
            return (
                <PeerDetailInspector
                    item={item}
                    layout={layout}
                    onAction={onAction}
                />
            );
        case 'governance-decision':
            return (
                <GovernanceDecisionInspector
                    item={item}
                    layout={layout}
                    onAction={onAction}
                />
            );
        case 'entity-detail':
            return (
                <EntityDetailInspector
                    item={item}
                    layout={layout}
                    onAction={onAction}
                />
            );
    }
}
