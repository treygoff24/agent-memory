import { useState } from 'react';

import { inboxItems } from '../data/fixtures';
import { Card } from '../ui';
export function RealityCheck() {
    const [index, setIndex] = useState(0);
    const item = inboxItems[index % inboxItems.length];
    const advance = () => setIndex((current) => current + 1);
    return (
        <div className="reality">
            <div className="reality-card">
                <p className="eyebrow">
                    Reality Check · {index + 1}/{inboxItems.length}
                </p>
                <h1>{item.title}</h1>
                <p>{item.body}</p>
                <div className="action-bar">
                    <button
                        className="btn primary"
                        onClick={advance}
                    >
                        y Confirm
                    </button>
                    <button
                        className="btn"
                        onClick={advance}
                    >
                        k Correct
                    </button>
                    <button
                        className="btn danger"
                        onClick={advance}
                    >
                        f Forget
                    </button>
                    <button
                        className="btn"
                        onClick={advance}
                    >
                        s Skip
                    </button>
                </div>
                <Card title="Progress">
                    <p>{Math.min(index, inboxItems.length)} reviewed · transition-ready focus mode.</p>
                </Card>
            </div>
        </div>
    );
}
