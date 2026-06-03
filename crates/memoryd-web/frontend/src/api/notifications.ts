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

function notificationKey(notification: NotificationSnapshotItem): string {
    return notification.id || [notification.created_at ?? '', notification.kind, notification.message].join('|');
}

function dedupeNotifications(notifications: NotificationSnapshotItem[]): NotificationSnapshotItem[] {
    const seen = new Set<string>();
    const deduped: NotificationSnapshotItem[] = [];
    for (const notification of notifications) {
        const key = notificationKey(notification);
        if (seen.has(key)) continue;
        seen.add(key);
        deduped.push(notification);
    }
    return deduped.slice(0, 100);
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
        const notifications = dedupeNotifications([...heartbeat.notifications, ...snapshot.notifications]);
        emit({
            connected: true,
            notifications,
            lastHeartbeat: new Date().toISOString(),
            ...(heartbeat.error ? { error: heartbeat.error.message } : {}),
        });
    });
    events.onerror = () => {
        if (snapshot.lastHeartbeat) return;
        emit({
            ...snapshot,
            connected: false,
            error: 'Waiting for live notification updates. Dashboard data is still available.',
        });
    };
    return () => {
        events?.close();
        events = null;
        emit({ ...snapshot, connected: false });
    };
}

function subscribeNotifications(listener: () => void): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
}

export function getNotificationsSnapshot(): NotificationStoreSnapshot {
    return snapshot;
}

export function useNotifications(): NotificationStoreSnapshot {
    return useSyncExternalStore(subscribeNotifications, getNotificationsSnapshot, getNotificationsSnapshot);
}
