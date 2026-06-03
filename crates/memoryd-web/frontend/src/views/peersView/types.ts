// Shared view-model types for the Peers view. Extracted to a leaf module so the
// `Peers` container and its `TrustLedger` child can both depend on them without
// forming an import cycle.

export type PeerTrust = 'local active' | 'stale' | 'unknown';
export type PeerSync = 'active' | 'stale' | 'unknown';

export interface PeerViewItem {
    id: string;
    label: string;
    locksHeld: number;
    /** null when the daemon does not supply a pending-lock count for this peer */
    locksPending: number | null;
    /** null when the daemon does not supply a 24h event count for this peer */
    events24h: number | null;
    device: string;
    trust: PeerTrust;
    sync: PeerSync;
    reachable: boolean;
    devicePubkeyShort: string;
    lastHandshake: string;
    lastHandshakeTs: string;
    sessionsOpen: number;
    /** null when the daemon does not supply inbound event counts for this peer */
    eventsIn24h: number | null;
    /** null when the daemon does not supply outbound event counts for this peer */
    eventsOut24h: number | null;
    fenceReason?: string;
    revocation?: {
        at: string;
        by: string;
        reason: string;
    };
}

export type PeerSortKey =
    | 'device'
    | 'label'
    | 'trust'
    | 'sync'
    | 'devicePubkeyShort'
    | 'lastHandshake'
    | 'locks'
    | 'events24h';
