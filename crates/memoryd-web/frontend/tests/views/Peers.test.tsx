import { fireEvent, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Peers } from '../../src/views/Peers';
import { renderWithProviders } from '../support/render';

describe('peers view', () => {
    it('peers renders session-derived peer status without inferred trust or fake keys', async () => {
        renderWithProviders(<Peers />);
        await screen.findAllByText('MacBook Pro');

        expect(screen.getByText('Peers')).toBeInTheDocument();
        const pairButton = screen.getByRole('button', { name: /\+ pair new device/i });
        expect(pairButton).toBeDisabled();
        expect(pairButton).toHaveAttribute('aria-describedby', 'pairing-unavailable-copy');
        expect(screen.getByText(/Pairing API is not available in alpha/i)).toBeInTheDocument();
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

        expect(screen.getAllByText('local active').length).toBeGreaterThan(0);
        expect(screen.getAllByText('stale').length).toBeGreaterThan(0);
        expect(screen.queryByText('limited')).not.toBeInTheDocument();
        expect(screen.queryByText('fenced')).not.toBeInTheDocument();
        expect(screen.queryByText('revoked')).not.toBeInTheDocument();
        expect(screen.queryByText(/ed25519:/)).not.toBeInTheDocument();
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('peer detail');

        fireEvent.click(screen.getByRole('button', { name: /events 24h/i }));
        const firstRow = screen.getAllByTestId('peer-row')[0];
        expect(within(firstRow).getByText('MacBook Pro')).toBeInTheDocument();
    });
});
