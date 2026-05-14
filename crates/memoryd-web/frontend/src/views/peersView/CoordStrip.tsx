interface CoordStripProps {
    level: number;
}

interface CoordInfo {
    label: string;
    tone: 'ok' | 'info' | 'warn';
    description: string;
}

function coordInfo(level: number): CoordInfo {
    if (level <= 1) {
        return {
            label: 'observe-only',
            tone: 'warn',
            description: 'Level 1 — promoted memories surface via recall; no peer updates or claim locks.',
        };
    }
    if (level === 2) {
        return {
            label: 'soft-claims',
            tone: 'info',
            description:
                'Level 2 — in-flight candidates, notes, and observe fragments are shared; claim locks expire on completion.',
        };
    }
    return {
        label: 'strict-claims',
        tone: 'ok',
        description: 'Level 3 — presence heartbeats active; claim locks renewable; full peer-update stream.',
    };
}

export function CoordStrip({ level }: CoordStripProps) {
    const info = coordInfo(level);
    return (
        <div
            aria-label="Coordination mode"
            className="coord-strip"
            role="note"
        >
            <span className={`badge ${info.tone}`}>{info.label}</span>
            <span className="coord-strip-desc">{info.description}</span>
            <span className="coord-strip-level mono">L{level}</span>
        </div>
    );
}
