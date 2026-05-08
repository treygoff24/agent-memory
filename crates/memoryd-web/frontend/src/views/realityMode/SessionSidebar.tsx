import type { RealityCheckSession } from './types';

export function SessionSidebar({ session, complete = false }: { session: RealityCheckSession; complete?: boolean }) {
    return (
        <aside
            className="rc-side"
            role="complementary"
            aria-label="Reality Check session"
        >
            <h3>Session</h3>
            <ul>
                {session.items.slice(0, 8).map((item) => {
                    const status = complete || item.status === 'done' ? 'done' : item.status;
                    return (
                        <li
                            key={item.id}
                            className={status}
                        >
                            <span className="mark">{status === 'done' ? '✓' : status === 'now' ? '▸' : '·'}</span>
                            <span>{item.title}</span>
                        </li>
                    );
                })}
                {session.items.length > 8 ? (
                    <li>
                        <span className="mark">·</span>
                        <span>+ {session.items.length - 8} more</span>
                    </li>
                ) : null}
            </ul>
        </aside>
    );
}
