import { useState } from 'react';

import { governance } from '../data/fixtures';
import { Badge, Card } from '../ui';
export function Governance() {
    const [selected, setSelected] = useState(governance[0]);
    return (
        <>
            <div className="view-header">
                <span className="view-title">Governance</span>
                <span className="view-subtitle">· review queue</span>
                <span className="spacer" />
                <button className="btn primary">Approve selected</button>
                <button className="btn danger">Reject selected</button>
            </div>
            <div className="panes-2">
                <section className="pane left">
                    {governance.map((item) => (
                        <button
                            key={item.id}
                            className={`list-item ${selected.id === item.id ? 'selected' : ''}`}
                            onClick={() => setSelected(item)}
                        >
                            <span>{item.title}</span>
                            <Badge tone={item.severity === 'block' ? 'bad' : item.severity === 'warn' ? 'warn' : 'ok'}>
                                {item.decision}
                            </Badge>
                        </button>
                    ))}
                </section>
                <section className="pane">
                    <Card title="Policy decision trace">
                        <h2>{selected.title}</h2>
                        <p>{selected.namespace}</p>
                        <p className="mono">rule → match → {selected.decision}</p>
                    </Card>
                </section>
            </div>
        </>
    );
}
