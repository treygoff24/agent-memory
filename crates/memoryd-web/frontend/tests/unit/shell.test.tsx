import { fireEvent, screen, waitFor } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { describe, expect, it } from 'vitest';

import { Shell } from '../../src/shell';
import { scenarioHandlers } from '../msw/handlers';
import { server } from '../msw/server';
import { renderWithProviders } from '../support/render';

describe('Shell readiness states', () => {
    it('renders sync and peer counts from /api/status instead of static demo copy', async () => {
        server.use(...scenarioHandlers.empty);

        renderWithProviders(
            <Shell
                active="inbox"
                onNav={() => undefined}
                onPalette={() => undefined}
                onBell={() => undefined}
            >
                <div />
            </Shell>,
        );

        expect(await screen.findByText('sync · 2 pending')).toBeInTheDocument();
        expect(screen.getByText('peers · 0 active')).toBeInTheDocument();
        expect(screen.queryByText('sync · 2 peers')).not.toBeInTheDocument();
    });

    it('surfaces search API failures in the search panel', async () => {
        server.use(
            http.get('/api/search', () =>
                HttpResponse.json({ error: 'daemon_unavailable', message: 'memoryd socket closed' }, { status: 503 }),
            ),
        );

        renderWithProviders(
            <Shell
                active="inbox"
                onNav={() => undefined}
                onPalette={() => undefined}
                onBell={() => undefined}
            >
                <div />
            </Shell>,
        );

        const search = screen.getByRole('textbox', { name: 'Search memories' });
        fireEvent.change(search, { target: { value: 'pnpm' } });
        fireEvent.submit(search.closest('form')!);

        expect(await screen.findByRole('alert')).toHaveTextContent('memoryd socket closed');
    });

    it('submits global memory search and renders results from /api/search', async () => {
        renderWithProviders(
            <Shell
                active="inbox"
                onNav={() => undefined}
                onPalette={() => undefined}
                onBell={() => undefined}
            >
                <div />
            </Shell>,
        );

        const search = screen.getByRole('textbox', { name: 'Search memories' });
        fireEvent.change(search, { target: { value: 'pnpm' } });
        fireEvent.submit(search.closest('form')!);

        expect(await screen.findByRole('listbox', { name: 'Search results' })).toBeInTheDocument();
        const result = screen.getByRole('option', { name: /Project uses pnpm, never npm/i });
        expect(result).toBeInTheDocument();
        expect(result.getAttribute('href')).toBe('/?view=recall&memory=mem_20260507_a1b2c3d4e5f60718_000010');
        await waitFor(() => expect(screen.getByText('1 result')).toBeInTheDocument());
    });

    it('treats daemon state ready as healthy even when dashboard fields are incomplete', async () => {
        server.use(
            http.get('/api/status', () =>
                HttpResponse.json({
                    degraded: true,
                    warnings: ['sync_status_unavailable'],
                    daemon: { version: '0.1.0-test', pid: 7137, uptime_seconds: 12 },
                    socket: 'ready',
                    index: { active_memories: 0, last_reindex: null },
                    sync: { ahead: 0, behind: 0, remote: 'unknown', last_push: null },
                    review: { candidate: 0, quarantined: 0, dream_low_confidence: 0 },
                    conflicts: 0,
                    active_sessions: [],
                    dreaming: {
                        status: 'unknown',
                        next_run: null,
                        last_run: { at: null, promoted: null, queued: null, dropped: null },
                    },
                    recall: { startup_total: 0, delta_total: 0, peer_update_snapshot_count: 0 },
                }),
            ),
        );

        renderWithProviders(
            <Shell
                active="inbox"
                onNav={() => undefined}
                onPalette={() => undefined}
                onBell={() => undefined}
            >
                <div />
            </Shell>,
        );

        await screen.findAllByText('daemon');
        expect(document.querySelector('.status-cluster .pair .status-dot.ok')).toBeTruthy();
    });
});
