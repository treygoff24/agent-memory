import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Peers } from '../../src/views/Peers';
import { renderWithProviders } from '../support/render';

describe('peers view', () => {
    it('peers renders card layout with badges, pair CTA, coord strip, and peer-detail inspector', async () => {
        renderWithProviders(<Peers />);
        await screen.findAllByText('MacBook Pro');

        // View header
        expect(screen.getByText('Peers')).toBeInTheDocument();
        expect(screen.getByRole('button', { name: /\+ pair new device/i })).toBeInTheDocument();

        // Cards are rendered (default layout)
        const cards = screen.getAllByTestId('peer-card');
        expect(cards.length).toBeGreaterThan(0);

        // Trust/sync badges appear in cards
        expect(screen.getByText('limited')).toBeInTheDocument();
        expect(screen.getAllByText('fenced').length).toBeGreaterThan(0);
        expect(screen.getAllByText('revoked').length).toBeGreaterThan(0);

        // Coordination strip is rendered
        expect(screen.getByRole('note', { name: 'Coordination mode' })).toBeInTheDocument();

        // Inspector opens for first card
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('peer detail');

        // Layout toggle link is present
        expect(screen.getByRole('link', { name: /switch to table layout/i })).toBeInTheDocument();
    });
});
