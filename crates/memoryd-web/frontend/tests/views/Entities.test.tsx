import { fireEvent, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Entities } from '../../src/views/Entities';
import { renderWithProviders } from '../support/render';

describe('entities view', () => {
    it('entities renders sortable table, kind filters, search, confidence bars, and entity-detail inspector', async () => {
        renderWithProviders(<Entities />);
        await screen.findAllByText('pnpm');

        for (const label of ['all', 'person', 'org', 'project', 'place', 'tool', 'language']) {
            expect(screen.getByRole('tab', { name: new RegExp(label, 'i') })).toBeInTheDocument();
        }
        for (const heading of ['name', 'kind', 'mentions', 'namespaces', 'last seen', 'first seen', 'confidence']) {
            expect(
                screen.getByRole('button', { name: new RegExp(`^sort by ${heading}(?: asc| desc)?$`, 'i') }),
            ).toBeInTheDocument();
        }
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('entity');

        fireEvent.click(screen.getByRole('tab', { name: /tool/i }));
        fireEvent.change(screen.getByLabelText('Entity search'), { target: { value: 'pnpm' } });
        expect(screen.getByTestId('entities-view-tool')).toHaveTextContent('pnpm');

        fireEvent.click(screen.getByRole('button', { name: /sort by mentions/i }));
        const firstRow = screen.getAllByTestId('entity-row')[0];
        expect(within(firstRow).getByText(/pnpm|Rust|agent-memory/i)).toBeInTheDocument();
    });
});
