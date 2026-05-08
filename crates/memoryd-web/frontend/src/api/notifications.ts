export type NotificationHandler = (event: MessageEvent<string>) => void;

export function subscribeNotifications(handler: NotificationHandler): () => void {
    const events = new EventSource('/api/notifications/stream');
    events.addEventListener('heartbeat', handler);
    events.onerror = () => events.close();
    return () => events.close();
}
