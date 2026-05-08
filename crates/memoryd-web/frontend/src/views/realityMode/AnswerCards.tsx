import type { RealityCheckActionName } from './types';

interface AnswerCardsProps {
    encrypted: boolean;
    onAction: (action: RealityCheckActionName) => void;
    onCorrect: () => void;
}

export function AnswerCards({ encrypted, onAction, onCorrect }: AnswerCardsProps) {
    return (
        <div className="rc-actions">
            <button
                className={`rc-action primary ${encrypted ? 'disabled' : ''}`}
                onClick={() => !encrypted && onAction('confirm')}
                title={encrypted ? 'Cannot confirm encrypted memories from this surface — reveal externally first.' : ''}
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
                className="rc-action"
                onClick={() => onAction('forget')}
                type="button"
            >
                <span className="key">f</span>
                <span>Forget</span>
                <span className="desc">tombstone, no recall</span>
            </button>
            <button
                className="rc-action"
                onClick={() => onAction('not_relevant')}
                type="button"
            >
                <span className="key">n</span>
                <span>Not relevant</span>
                <span className="desc">remove from future checks</span>
            </button>
            <button
                className="rc-action"
                onClick={() => onAction('skip_this_week')}
                type="button"
            >
                <span className="key">s</span>
                <span>Skip this week</span>
                <span className="desc">defer until next cycle</span>
            </button>
        </div>
    );
}
