import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it } from 'vitest';

import { Entities } from '../../src/views/Entities';
import { server } from '../msw/server';
import { renderWithProviders } from '../support/render';

describe('entities view', () => {
    beforeEach(() => {
        window.history.replaceState(null, '', '/');
    });

    it('defaults to graph mode rendering the SVG entity layout', async () => {
        renderWithProviders(<Entities />);
        const graph = await screen.findByRole('group', { name: /entity relationship graph/i });
        expect(graph).toBeInTheDocument();
        // Mode toggle is visible in both modes; graph tab is the active one by default.
        expect(screen.getByRole('tab', { name: /^graph$/i })).toHaveAttribute('aria-selected', 'true');
    });

    it('allows keyboard users to select an entity from graph mode', async () => {
        renderWithProviders(<Entities />);

        const pnpmNode = await screen.findByRole('button', { name: /select entity pnpm/i });
        fireEvent.keyDown(pnpmNode, { key: 'Enter' });

        await waitFor(() => expect(window.location.hash).toBe('#/entities/ent_pnpm'));
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('pnpm');
    });

    it('caps large graph rendering and offers the table fallback before layout work scales unbounded', async () => {
        const nodes = Array.from({ length: 260 }, (_, index) => ({
            id: `ent_large_${String(index).padStart(3, '0')}`,
            label: `Large Entity ${index}`,
            kind: 'entity',
            namespace: index % 2 === 0 ? 'project:agent-memory' : 'work/clients/acme',
            memory_count: 300 - index,
        }));
        const edges = Array.from({ length: 259 }, (_, index) => ({
            source: nodes[index]!.id,
            target: nodes[index + 1]!.id,
            kind: 'co_mentioned',
            weight: index % 3 === 0 ? 0.2 : 0.8,
            temporal_from: null,
            temporal_to: null,
        }));
        server.use(http.get('/api/entity-graph', () => HttpResponse.json({ nodes, edges })));

        renderWithProviders(<Entities />);

        expect(await screen.findByText(/showing top 120 of 260 entities/i)).toBeInTheDocument();
        expect(screen.queryByRole('button', { name: /select entity large entity 180/i })).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole('button', { name: /switch to table mode/i }));
        await waitFor(() =>
            expect(screen.getByRole('tab', { name: /^table$/i })).toHaveAttribute('aria-selected', 'true'),
        );
    });

    it('normalizes daemon-shaped entity kinds before applying graph kind colors', async () => {
        server.use(
            http.get('/api/entity-graph', () =>
                HttpResponse.json({
                    nodes: [
                        {
                            id: 'ent_rust',
                            label: 'Rust',
                            kind: 'entity',
                            namespace: 'daemon',
                            memory_count: 31,
                        },
                    ],
                    edges: [],
                }),
            ),
        );

        renderWithProviders(<Entities />);

        const rustNode = await screen.findByRole('button', { name: /select entity rust/i });
        expect(rustNode.querySelector('circle')).toHaveAttribute('fill', 'var(--fg-2)');
    });

    it('table mode renders sortable table, kind filters, search, confidence bars, and entity-detail inspector', async () => {
        renderWithProviders(<Entities />);
        // Toggle to table mode for the table-shape assertions below.
        fireEvent.click(screen.getByRole('tab', { name: /^table$/i }));
        await screen.findAllByText('pnpm');

        for (const label of ['all', 'person', 'org', 'project', 'place', 'tool', 'language']) {
            expect(screen.getByRole('tab', { name: new RegExp(`^${label}\\s`, 'i') })).toBeInTheDocument();
        }
        for (const heading of ['name', 'kind', 'mentions', 'namespaces', 'last seen', 'first seen', 'confidence']) {
            expect(
                screen.getByRole('button', { name: new RegExp(`^sort by ${heading}(?: asc| desc)?$`, 'i') }),
            ).toBeInTheDocument();
        }
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('entity');

        fireEvent.click(screen.getByRole('tab', { name: /^tool\s/i }));
        fireEvent.change(screen.getByLabelText('Entity search'), { target: { value: 'pnpm' } });
        expect(screen.getByTestId('entities-view-tool')).toHaveTextContent('pnpm');

        fireEvent.click(screen.getByRole('button', { name: /sort by mentions/i }));
        const firstRow = screen.getAllByTestId('entity-row')[0];
        expect(within(firstRow).getByText(/pnpm|Rust|agent-memory/i)).toBeInTheDocument();
    });
});
