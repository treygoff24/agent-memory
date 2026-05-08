import type { ReactNode } from 'react';

export function Badge({
    children,
    tone = 'neutral',
}: {
    children: ReactNode;
    tone?: 'neutral' | 'ok' | 'warn' | 'bad';
}) {
    return <span className={`badge ${tone}`}>{children}</span>;
}
export function Card({ title, children }: { title: string; children: ReactNode }) {
    return (
        <section className="card">
            <div className="card-head">
                <span>{title}</span>
            </div>
            {children}
        </section>
    );
}
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
export function Modal({ title, children, onClose }: { title: string; children: ReactNode; onClose(): void }) {
    return (
        <div
            className="modal-veil"
            onClick={onClose}
        >
            <div
                className="modal"
                onClick={(event) => event.stopPropagation()}
            >
                <div className="card-head">
                    <span>{title}</span>
                    <button
                        className="icon-btn"
                        onClick={onClose}
                    >
                        ×
                    </button>
                </div>
                {children}
            </div>
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
