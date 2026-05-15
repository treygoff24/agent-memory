import type { RealityCheckSession } from './types';

export function FocusStrip({
    session,
    complete = false,
    onExit,
}: {
    session: RealityCheckSession;
    complete?: boolean;
    onExit: () => void;
}) {
    const progressCurrent = complete ? session.progress.total : session.progress.current;
    const progress = (progressCurrent / session.progress.total) * 100;
    return (
        <div className="rc-strip">
            <span className="brand">
                {/* Brand sigil stays Unicode per plan §5 invariant 6 — the brand
                   sigil string is the only Unicode-as-icon exception. */}
                <span className="sigil">◆</span>
                <span className="word">memorum</span>
            </span>
            <span className="sep">·</span>
            <span className="label">reality check</span>
            <span className="sep">·</span>
            <span className="scope">{session.current.namespace}</span>
            <div className="gauge">
                <i style={{ width: `${progress}%` }} />
            </div>
            <span className="progress-text">
                {progressCurrent} of {session.progress.total}
            </span>
            <button
                className="exit"
                onClick={onExit}
                type="button"
            >
                esc · pause
            </button>
        </div>
    );
}
