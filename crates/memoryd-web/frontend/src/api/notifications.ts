import { useSyncExternalStore } from 'react';

import type { NotificationSnapshotItem, NotificationsHeartbeat } from './types';

export interface NotificationStoreSnapshot {
    connected: boolean;
    notifications: NotificationSnapshotItem[];
    lastHeartbeat?: string;
    error?: string;
}

const listeners = new Set<() => void>();
let snapshot: NotificationStoreSnapshot = { connected: false, notifications: [] };
let events: EventSource | null = null;

function emit(next: NotificationStoreSnapshot) {
    snapshot = next;
    for (const listener of listeners) listener();
}

function parseHeartbeat(event: MessageEvent<string>): NotificationsHeartbeat | null {
    try {
        const parsed = JSON.parse(event.data) as NotificationsHeartbeat;
        return parsed.kind === 'heartbeat' && Array.isArray(parsed.notifications) ? parsed : null;
    } catch {
        return null;
    }
}

export function startNotificationsStream(): () => void {
    if (events) return () => undefined;
    if (typeof EventSource === 'undefined') return () => undefined;

    events = new EventSource('/api/notifications/stream');
    emit({
        connected: true,
        notifications: snapshot.notifications,
        ...(snapshot.lastHeartbeat ? { lastHeartbeat: snapshot.lastHeartbeat } : {}),
    });
    events.addEventListener('heartbeat', (event) => {
        const heartbeat = parseHeartbeat(event as MessageEvent<string>);
        if (!heartbeat) return;
        emit({
            connected: true,
            notifications: heartbeat.notifications,
            lastHeartbeat: new Date().toISOString(),
        });
    });
    events.onerror = () => {
        emit({ ...snapshot, connected: false, error: 'Notification stream disconnected.' });
        events?.close();
        events = null;
    };
    return () => {
        events?.close();
        events = null;
        emit({ ...snapshot, connected: false });
    };
}

export function subscribeNotifications(listener: () => void): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
}

export function getNotificationsSnapshot(): NotificationStoreSnapshot {
    return snapshot;
}

export function useNotifications(): NotificationStoreSnapshot {
    return useSyncExternalStore(subscribeNotifications, getNotificationsSnapshot, getNotificationsSnapshot);
}
