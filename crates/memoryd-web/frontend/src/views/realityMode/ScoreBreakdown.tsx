import { useState } from 'react';

import type { RealityCheckMemory } from './types';

export function ScoreBreakdown({ memory, defaultOpen = false }: { memory: RealityCheckMemory; defaultOpen?: boolean }) {
    const [showScore, setShowScore] = useState(defaultOpen);
    return (
        <details
            className="rc-score"
            open={showScore}
            onToggle={(event) => setShowScore(event.currentTarget.open)}
        >
            <summary>
                <span>Score breakdown</span>
                <span className="s-meta">total {memory.score.toFixed(2)}</span>
            </summary>
            <div className="scorebars">
                {Object.entries(memory.component_scores).map(([key, value]) => (
                    <div
                        className="row"
                        key={key}
                    >
                        <span className="label">{key}</span>
                        <span className="track">
                            <span
                                className="fill"
                                style={{ width: `${value * 100}%` }}
                            />
                        </span>
                        <span className="val">{value.toFixed(2)}</span>
                    </div>
                ))}
            </div>
        </details>
    );
}
