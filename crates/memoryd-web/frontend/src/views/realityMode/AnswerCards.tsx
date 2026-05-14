import type { RealityCheckActionName } from './types';

interface AnswerCardsProps {
    encrypted: boolean;
    onAction: (action: RealityCheckActionName) => void;
    onCorrect: () => void;
}

// Brief §View 2 mandates four answer cards: Confirm / Correct / Forget / Skip.
// The daemon also supports `not_relevant` (permanent dismissal, distinct from
// `skip_this_week`'s "defer to next cycle"). The 'n' keyboard shortcut in
// RealityCheck.tsx keeps that action reachable for power users; the visible
// stack stays at the brief-mandated four.
export function AnswerCards({ encrypted, onAction, onCorrect }: AnswerCardsProps) {
    return (
        <div className="rc-actions">
            <button
                className={`rc-action primary ${encrypted ? 'disabled' : ''}`}
                onClick={() => !encrypted && onAction('confirm')}
                title={
                    encrypted ? 'Cannot confirm encrypted memories from this surface — reveal externally first.' : ''
                }
                disabled={encrypted}
                type="button"
            >
                <span className="key">y</span>
                <span>Confirm — still true</span>
                <span className="desc">{encrypted ? 'requires external reveal' : 'keep, refresh verified-at'}</span>
            </button>
            <button
                className="rc-action"
                onClick={onCorrect}
                type="button"
            >
                <span className="key">k</span>
                <span>Correct — replace with…</span>
                <span className="desc">opens text input</span>
            </button>
            <button
                className="rc-action danger"
                onClick={() => onAction('forget')}
                type="button"
            >
                <span className="key">f</span>
                <span>Forget</span>
                <span className="desc">tombstone, no recall</span>
            </button>
            <button
                className="rc-action"
                onClick={() => onAction('skip_this_week')}
                type="button"
            >
                <span className="key">s</span>
                <span>Skip — ask later</span>
                <span className="desc">defer until next cycle</span>
            </button>
        </div>
    );
}
