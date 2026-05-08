import { StatusDot } from '../ui';
export function TopBar({ onPalette, onBell }: { onPalette(): void; onBell(): void }) {
    return (
        <header className="topbar">
            <div className="brand">
                <span className="sigil">◆</span>
                <span>memorum</span>
            </div>
            <div className="search">
                <input
                    aria-label="Search memories"
                    placeholder="Search memories, namespaces, ids…"
                />
            </div>
            <div className="topbar-right">
                <button
                    className="icon-btn"
                    onClick={onPalette}
                    aria-label="Command palette"
                >
                    :
                </button>
                <button
                    className="icon-btn"
                    onClick={onBell}
                    aria-label="Notifications"
                >
                    ●
                </button>
                <div className="status-cluster">
                    <span className="pair">
                        <StatusDot />
                        daemon
                    </span>
                    <span className="pair">
                        <StatusDot />
                        sync · 2
                    </span>
                </div>
            </div>
        </header>
    );
}
