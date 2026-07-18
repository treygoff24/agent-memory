import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { App } from '../../src/App';

describe('App', () => {
    it('renders the bootstrap shell', () => {
        render(<App />);
        expect(screen.getByRole('heading', { name: 'Memorum Dashboard' })).toBeTruthy();
    });

    it('renders daemon-shaped notification messages in the bell drawer and toast', async () => {
        let heartbeatListener: ((event: MessageEvent<string>) => void) | undefined;
        class FakeEventSource {
            onerror: ((event: globalThis.Event) => void) | null = null;
            constructor(url: string) {
                void url;
            }
            addEventListener(type: string, listener: (event: MessageEvent<string>) => void) {
                if (type === 'heartbeat') heartbeatListener = listener;
            }
            close() {
                return undefined;
            }
        }
        vi.stubGlobal('EventSource', FakeEventSource);

        const { unmount } = render(<App />);
        act(() => {
            heartbeatListener?.(
                new MessageEvent('heartbeat', {
                    data: JSON.stringify({
                        kind: 'heartbeat',
                        notifications: [
                            {
                                id: 'notif_review_threshold',
                                kind: 'passive',
                                message: 'Review queue has 7 items over threshold 5.',
                                created_at: '2026-05-25T12:00:00Z',
                            },
                        ],
                    }),
                }),
            );
        });

        fireEvent.click(screen.getByRole('button', { name: 'Notifications' }));

        expect(screen.getByText('Notifications · 1')).toBeTruthy();
        expect(screen.getAllByText('Review queue has 7 items over threshold 5.').length).toBeGreaterThanOrEqual(2);
        unmount();
        vi.unstubAllGlobals();
    });
});
