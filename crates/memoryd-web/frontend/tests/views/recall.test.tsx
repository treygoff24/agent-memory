import { fireEvent, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { Recall, makeHeavyRecallEvents } from '../../src/views/Recall';
import { renderWithProviders } from '../support/render';

describe('recall ledger', () => {
    it('recall renders timeline strip, dense ledger headers, filters, and recall-event inspector', async () => {
        renderWithProviders(<Recall />);
        await screen.findAllByText('Project uses pnpm, never npm');
        expect(screen.getByText('Recall ledger')).toBeInTheDocument();
        expect(screen.getByRole('group', { name: 'Timeline' })).toBeInTheDocument();
        for (const heading of ['time', 'seq', 'device', 'agent', 'memory', 'namespace', 'lat', 'score']) {
            expect(screen.getByText(heading)).toBeInTheDocument();
        }
        expect(screen.getByLabelText('Agent filter')).toBeInTheDocument();
        expect(screen.getByLabelText('Device filter')).toBeInTheDocument();
        expect(screen.getByLabelText('Recall search')).toBeInTheDocument();
        expect(screen.getByRole('button', { name: /export csv/i })).toBeInTheDocument();
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('recall event');
    });

    it('recall keeps the 9k-event heavy state virtualized instead of rendering every row', () => {
        const heavyEvents = makeHeavyRecallEvents(9000);
        renderWithProviders(
            <Recall
                events={heavyEvents}
                heavy
            />,
        );
        expect(screen.getByText(/9,000 events/)).toBeInTheDocument();
        expect(screen.getByTestId('recall-virtual-list').querySelectorAll('.rl-row').length).toBeLessThan(120);
        expect(screen.getByText(/scrolling backed by virtualization/i)).toBeInTheDocument();
    });

    it('exports a CSV containing the currently visible recall rows', async () => {
        let downloadedHref = '';
        let downloadedName = '';
        const click = vi
            .spyOn(HTMLAnchorElement.prototype, 'click')
            .mockImplementation(function captureDownload(this: HTMLAnchorElement) {
                downloadedHref = this.href;
                downloadedName = this.download;
            });

        renderWithProviders(
            <Recall
                events={[
                    {
                        id: 'row_keep',
                        seq: 1,
                        isoTime: '2026-05-01T00:00:00Z',
                        time: '00:00:00',
                        device: 'mbp',
                        agent: 'codex',
                        memory: 'Visible pnpm memory',
                        namespace: 'coding/typescript',
                        score: 0.91,
                        latencyMs: 18,
                        session: 'session_keep',
                    },
                    {
                        id: 'row_filtered',
                        seq: 2,
                        isoTime: '2026-05-01T01:00:00Z',
                        time: '01:00:00',
                        device: 'mini',
                        agent: 'claude-code',
                        memory: 'Hidden rust memory',
                        namespace: 'coding/rust',
                        score: 0.82,
                        latencyMs: 22,
                        session: 'session_filtered',
                    },
                ]}
            />,
        );

        fireEvent.change(screen.getByLabelText('Recall search'), { target: { value: 'pnpm' } });
        fireEvent.click(screen.getByRole('button', { name: /export csv/i }));

        const csv = decodeURIComponent(downloadedHref.split(',', 2)[1] ?? '');
        expect(csv).toContain('Visible pnpm memory');
        expect(csv).not.toContain('Hidden rust memory');
        expect(downloadedName).toBe('memorum-recall-visible.csv');
        expect(click).toHaveBeenCalled();
        click.mockRestore();
    });

    it('renders daemon recall hits without inferred agent namespace score latency or session telemetry', async () => {
        let downloadedHref = '';
        const click = vi
            .spyOn(HTMLAnchorElement.prototype, 'click')
            .mockImplementation(function captureDownload(this: HTMLAnchorElement) {
                downloadedHref = this.href;
            });

        renderWithProviders(<Recall />);
        await screen.findAllByText('Project uses pnpm, never npm');

        expect(screen.getAllByText('unknown').length).toBeGreaterThan(0);
        expect(screen.queryByText('coding/typescript')).not.toBeInTheDocument();
        fireEvent.change(screen.getByLabelText('Recall search'), { target: { value: 'pnpm' } });
        fireEvent.click(screen.getByRole('button', { name: /export csv/i }));

        const csv = decodeURIComponent(downloadedHref.split(',', 2)[1] ?? '');
        expect(csv).toContain('Project uses pnpm, never npm');
        expect(csv).toContain('unknown');
        expect(csv).not.toContain('coding/typescript');
        expect(csv).not.toContain('session_');
        expect(csv).not.toContain('codex');
        click.mockRestore();
    });
});
