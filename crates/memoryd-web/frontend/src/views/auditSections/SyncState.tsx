import type { SyncStateResult } from '../../api';

interface Props {
    state: SyncStateResult;
}

/**
 * Trust Artifact §9 — sync state across known devices: which devices have a
 * copy, the merge driver's current status verdict (`in_sync` / `behind` /
 * `fenced` / `conflict`), and whether an advisory claim-lock is currently held.
 * Claim-lock status is optional on the daemon side; absent → muted "no lock".
 */
export function SyncState({ state }: Props) {
    return (
        <section
            className="audit-section audit-sync-state"
            aria-labelledby="audit-sync-state-heading"
        >
            <h3
                id="audit-sync-state-heading"
                className="audit-section-heading"
            >
                Sync state
            </h3>
            <dl className="audit-stat-grid">
                <div>
                    <dt>Devices</dt>
                    <dd>
                        {state.devices.length === 0 ? (
                            <span className="muted">no devices reported</span>
                        ) : (
                            <ul className="audit-sync-devices">
                                {state.devices.map((device) => (
                                    <li
                                        className="mono audit-sync-device"
                                        key={device}
                                    >
                                        {device}
                                    </li>
                                ))}
                            </ul>
                        )}
                    </dd>
                </div>
                <div>
                    <dt>Merge status</dt>
                    <dd>
                        <span className={`badge audit-merge-status audit-merge-${state.merge_status.toLowerCase()}`}>
                            {state.merge_status}
                        </span>
                    </dd>
                </div>
                <div>
                    <dt>Claim lock</dt>
                    <dd>
                        {state.claim_lock_status ? (
                            <span className="mono">{state.claim_lock_status}</span>
                        ) : (
                            <span className="muted">no lock</span>
                        )}
                    </dd>
                </div>
            </dl>
        </section>
    );
}
