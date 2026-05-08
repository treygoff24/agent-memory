import type { ViewId } from '../views';
export const navItems: Array<{ id: ViewId; label: string; key: string }> = [
    { id: 'inbox', label: 'Inbox', key: 'i' },
    { id: 'reality', label: 'Reality Check', key: 'r' },
    { id: 'recall', label: 'Recall', key: 'l' },
    { id: 'dreams', label: 'Dreams', key: 'd' },
    { id: 'peers', label: 'Peers', key: 'p' },
    { id: 'governance', label: 'Governance', key: 'g' },
    { id: 'entities', label: 'Entities', key: 'e' },
    { id: 'settings', label: 'Settings', key: 's' },
];
export function Sidebar({ active, onNav }: { active: ViewId; onNav(id: ViewId): void }) {
    return (
        <nav
            className="sidebar"
            aria-label="Primary"
        >
            <div className="sidebar-section">Workspace</div>
            {navItems.map((item) => (
                <button
                    key={item.id}
                    className={`nav-item ${active === item.id ? 'active' : ''}`}
                    onClick={() => onNav(item.id)}
                >
                    <span className="ico">◆</span>
                    <span className="label">{item.label}</span>
                    <span className="count">{item.key}</span>
                </button>
            ))}
        </nav>
    );
}
