import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Governance } from '../../src/views/Governance';

describe('governance view', () => {
    it('governance renders review filters, batch actions, and governance-decision inspector cards', () => {
        render(<Governance />);

        for (const label of ['all', 'blocks', 'warnings', 'info', 'consent', 'redactions']) {
            expect(screen.getByRole('tab', { name: new RegExp(label, 'i') })).toBeInTheDocument();
        }
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('governance');
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('Policy decision trace');

        fireEvent.click(screen.getByRole('tab', { name: /consent/i }));
        expect(screen.getByTestId('governance-view-consent_required')).toHaveTextContent('Family detail consent required');

        fireEvent.click(screen.getByLabelText(/select Family detail consent required/i));
        expect(screen.getByTestId('governance-batch-count')).toHaveTextContent('1 selected');
        expect(screen.getByRole('button', { name: /Approve selected/i })).toBeInTheDocument();
        expect(screen.getByRole('button', { name: /Reject selected/i })).toBeInTheDocument();
    });
});
