import { chromeIcons } from '../ui/icons';
import { StatusDot } from '../ui';
export function TopBar({ onPalette, onBell }: { onPalette(): void; onBell(): void }) {
    const Palette = chromeIcons.palette;
    const BellIcon = chromeIcons.bell;
    return (
        <header className="topbar">
            <div className="brand">
                {/* Brand sigil stays Unicode per plan §5 invariant 6 — the brand
                   sigil string is the only Unicode-as-icon exception. */}
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
                    <Palette
                        size={16}
                        weight="regular"
                    />
                </button>
                <button
                    className="icon-btn"
                    onClick={onBell}
                    aria-label="Notifications"
                >
                    <BellIcon
                        size={16}
                        weight="regular"
                    />
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
