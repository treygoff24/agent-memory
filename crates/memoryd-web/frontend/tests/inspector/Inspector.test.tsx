import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { Inspector } from '../../src/inspector';
import type { InspectorItem } from '../../src/inspector';

const base = {
    id: 'mem_20260508_a1b2c3d4e5f60718_000001',
    title: 'Project uses pnpm, never npm',
    namespace: 'coding/typescript',
    body: 'Use pnpm for every package-management command.',
    confidence: 0.84,
} satisfies Omit<InspectorItem, 'kind'>;

const items: InspectorItem[] = [
    { ...base, kind: 'inbox-review' },
    { ...base, kind: 'inbox-recall', title: 'Recall: Acme renewal date' },
    { ...base, kind: 'inbox-conflict', title: 'Editor preference disagreement' },
    { ...base, kind: 'inbox-due', title: "Daughter's school name (verify)" },
    { ...base, kind: 'inbox-dream', title: 'Pattern: prefers Rust over Go' },
    { ...base, kind: 'recall-event', title: 'Recall event: pnpm rule' },
    { ...base, kind: 'dream-output', title: 'Dream output: Rust pattern' },
    { ...base, kind: 'peer-detail', title: 'Peer: MacBook Pro' },
    { ...base, kind: 'governance-decision', title: 'Governance: low confidence candidate' },
    { ...base, kind: 'entity-detail', title: 'Entity: pnpm' },
];

describe('inspector composition', () => {
    it.each(items)('renders %s through the typed kind dispatcher', (item) => {
        render(<Inspector item={item} />);

        expect(screen.getByRole('region', { name: /inspector/i })).toBeInTheDocument();
        expect(screen.getAllByText(item.title).length).toBeGreaterThan(0);
        expect(screen.getAllByText(item.namespace).length).toBeGreaterThan(0);
    });

    it('renders an empty selection state', () => {
        render(<Inspector item={null} />);

        expect(screen.getByText('Nothing selected')).toBeInTheDocument();
    });

    it('dispatches inbox-review keyboard actions a/r/e/f outside text inputs', () => {
        const onAction = vi.fn();
        render(
            <>
                <input aria-label="typing target" />
                <Inspector
                    item={{ ...base, kind: 'inbox-review' }}
                    onAction={onAction}
                />
            </>,
        );

        for (const key of ['a', 'r', 'e', 'f']) {
            fireEvent.keyDown(window, { key });
        }
        expect(onAction.mock.calls.map(([action]) => action)).toEqual(['approve', 'reject', 'edit', 'forget']);

        screen.getByLabelText('typing target').focus();
        fireEvent.keyDown(screen.getByLabelText('typing target'), { key: 'a' });
        expect(onAction).toHaveBeenCalledTimes(4);
    });
});
