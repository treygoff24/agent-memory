import { useState } from 'react';

const notificationChannels = ['Daemon health alerts', 'Reality-check pings', 'Governance review queue'] as const;

export function NotificationsTab() {
    const [enabled, setEnabled] = useState(() => new Set<string>(notificationChannels));
    const [threshold, setThreshold] = useState(3);

    return (
        <section
            className="card settings-card"
            aria-labelledby="notifications-heading"
        >
            <div className="card-head">
                <span id="notifications-heading">Notifications</span>
            </div>
            <p className="muted">Control which daemon events surface in the dashboard bell.</p>
            <div className="settings-form-grid">
                {notificationChannels.map((channel) => (
                    <label
                        key={channel}
                        className="settings-check"
                    >
                        <input
                            type="checkbox"
                            checked={enabled.has(channel)}
                            onChange={(event) => {
                                setEnabled((current) => {
                                    const next = new Set(current);
                                    if (event.target.checked) next.add(channel);
                                    else next.delete(channel);
                                    return next;
                                });
                            }}
                        />
                        <span>{channel}</span>
                    </label>
                ))}
                <label className="settings-field">
                    <span>Review queue threshold</span>
                    <input
                        type="number"
                        min="1"
                        max="20"
                        value={threshold}
                        onChange={(event) => setThreshold(Number(event.target.value))}
                    />
                </label>
            </div>
        </section>
    );
}
