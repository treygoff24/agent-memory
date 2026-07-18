import { describe, expect, it, vi } from 'vitest';

import { getNotificationsSnapshot, startNotificationsStream } from '../../src/api/notifications';

describe('notifications stream', () => {
    it('uses contextual copy when live notification updates disconnect', () => {
        let triggerError: () => void = () => undefined;
        class FakeEventSource {
            onerror: ((event: globalThis.Event) => void) | null = null;
            constructor(url: string) {
                void url;
                triggerError = () => {
                    this.onerror?.(new globalThis.Event('error'));
                };
            }
            addEventListener() {
                return undefined;
            }
            close() {
                return undefined;
            }
        }
        vi.stubGlobal('EventSource', FakeEventSource);

        const stop = startNotificationsStream();
        triggerError();

        expect(getNotificationsSnapshot()).toMatchObject({
            connected: false,
            error: 'Waiting for live notification updates. Dashboard data is still available.',
        });
        stop();
        vi.unstubAllGlobals();
    });

    it('deduplicates repeated daemon recent notifications by stable id', () => {
        let heartbeatListener: ((event: MessageEvent<string>) => void) | undefined;
        let triggerStreamError: () => void = () => undefined;
        class FakeEventSource {
            onerror: ((event: globalThis.Event) => void) | null = null;
            constructor(url: string) {
                void url;
                triggerStreamError = () => {
                    this.onerror?.(new globalThis.Event('error'));
                };
            }
            addEventListener(type: string, listener: (event: MessageEvent<string>) => void) {
                if (type === 'heartbeat') heartbeatListener = listener;
            }
            close() {
                return undefined;
            }
        }
        vi.stubGlobal('EventSource', FakeEventSource);

        const stop = startNotificationsStream();
        const heartbeat = {
            kind: 'heartbeat',
            notifications: [
                {
                    id: 'notif_review_threshold',
                    kind: 'passive',
                    message: 'Review queue has 7 items over threshold 5.',
                    created_at: '2026-05-25T12:00:00Z',
                },
                {
                    id: 'notif_review_threshold',
                    kind: 'passive',
                    message: 'Review queue has 7 items over threshold 5.',
                    created_at: '2026-05-25T12:00:00Z',
                },
            ],
        };

        heartbeatListener?.(new MessageEvent('heartbeat', { data: JSON.stringify(heartbeat) }));
        heartbeatListener?.(new MessageEvent('heartbeat', { data: JSON.stringify(heartbeat) }));

        expect(getNotificationsSnapshot().notifications).toHaveLength(1);
        expect(getNotificationsSnapshot().notifications[0]?.id).toBe('notif_review_threshold');
        expect(getNotificationsSnapshot().notifications[0]?.message).toBe('Review queue has 7 items over threshold 5.');
        triggerStreamError();
        expect(getNotificationsSnapshot()).toMatchObject({ connected: true });
        stop();
        vi.unstubAllGlobals();
    });
});
