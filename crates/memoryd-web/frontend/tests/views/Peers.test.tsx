import { fireEvent, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Peers } from '../../src/views/Peers';
import { renderWithProviders } from '../support/render';

describe('peers view', () => {
    it('peers renders sortable trust ledger, paired badges, pair CTA, and peer-detail inspector', async () => {
        renderWithProviders(<Peers />);
        await screen.findAllByText('MacBook Pro');

        expect(screen.getByText('Peers')).toBeInTheDocument();
        expect(screen.getByRole('button', { name: /\+ pair new device/i })).toBeInTheDocument();
        const header = within(screen.getByTestId('peer-ledger-head'));
        for (const heading of [
            'device',
            'label',
            'trust',
            'sync',
            'pubkey',
            'last handshake',
            'locks h/p',
            'events 24h',
        ]) {
            expect(header.getByRole('button', { name: new RegExp(heading, 'i') })).toBeInTheDocument();
        }

        expect(screen.getByText('limited')).toBeInTheDocument();
        expect(screen.getByText('fenced')).toBeInTheDocument();
        expect(screen.getAllByText('revoked').length).toBeGreaterThan(0);
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('peer detail');

        fireEvent.click(screen.getByRole('button', { name: /events 24h/i }));
        const firstRow = screen.getAllByTestId('peer-row')[0];
        expect(within(firstRow).getByText('MacBook Pro')).toBeInTheDocument();
    });
});
