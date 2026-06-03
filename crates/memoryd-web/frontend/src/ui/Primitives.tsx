export function EmptyState({ title, body }: { title: string; body: string }) {
    return (
        <div className="empty">
            <span className="ico">○</span>
            <h3>{title}</h3>
            <p>{body}</p>
        </div>
    );
}
export function StatusDot({ kind = 'ok' }: { kind?: 'ok' | 'warn' | 'bad' | 'idle' }) {
    return (
        <span
            aria-hidden
            className={`status-dot ${kind}`}
        />
    );
}
export function Banner({ title, body, tone = 'bad' }: { title: string; body: string; tone?: 'bad' | 'warn' | 'ok' }) {
    return (
        <div className={`banner ${tone}`}>
            <span className="label">{title}</span>
            <span className="msg">{body}</span>
        </div>
    );
}
export function Toast({ title, body, onDismiss }: { title: string; body: string; onDismiss(): void }) {
    return (
        <div className="toast">
            <div>
                <div className="t-title">{title}</div>
                <div>{body}</div>
            </div>
            <button
                className="icon-btn"
                onClick={onDismiss}
            >
                ×
            </button>
        </div>
    );
}
