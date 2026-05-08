export function CompletionCard({ onExit }: { onExit: () => void }) {
    return (
        <div className="rc-card rc-complete">
            <div className="rc-complete-mark">✓</div>
            <h2 className="rc-complete-title">Reality Check complete.</h2>
            <div className="rc-complete-stats">
                <div>
                    <span className="n">11</span>
                    <span className="lbl">confirmed</span>
                </div>
                <div>
                    <span className="n bad">1</span>
                    <span className="lbl">forgotten</span>
                </div>
                <div>
                    <span className="n">0</span>
                    <span className="lbl">deferred</span>
                </div>
            </div>
            <div className="rc-complete-meta">Next session due in 7 days · session_id rc_20260507_001</div>
            <button
                className="rc-action primary"
                style={{ maxWidth: 280 }}
                onClick={onExit}
                type="button"
            >
                <span className="key">↵</span>
                <span>Dismiss</span>
                <span className="desc">return to inbox</span>
            </button>
        </div>
    );
}
