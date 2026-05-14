import { fireEvent, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Dreams } from '../../src/views/Dreams';
import { renderWithProviders } from '../support/render';

describe('dreams view', () => {
    it('dreams renders sub-tabs, status pills, and a distinct dream-run meta-entry in the Journal tab', async () => {
        renderWithProviders(<Dreams />);
        await screen.findAllByText('Nightly synthesis pass');
        // Brief §View 4 mandates Journal / Questions / Cleanup sub-tabs.
        for (const tab of ['Journal', 'Questions', 'Cleanup']) {
            expect(screen.getByRole('tab', { name: new RegExp(tab, 'i') })).toBeInTheDocument();
        }
        // Status pills nest under the active tab.
        for (const label of ['all', 'proposed', 'queued', 'accepted', 'completed', 'dismissed', 'running']) {
            expect(screen.getByRole('tab', { name: new RegExp(label, 'i') })).toBeInTheDocument();
        }
        expect(screen.getAllByText('Nightly synthesis pass').length).toBeGreaterThan(0);
        expect(screen.getByText('dream run')).toBeInTheDocument();
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('dream output');
    });

    it('switching to the Questions tab surfaces the question dream', async () => {
        renderWithProviders(<Dreams />);
        await screen.findAllByText('Nightly synthesis pass');
        fireEvent.click(screen.getByRole('tab', { name: /Questions/i }));
        expect(screen.getByRole('tab', { name: /Questions/i })).toHaveAttribute('aria-selected', 'true');
        await screen.findAllByText('Question: which laptop is primary now?');
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent(
            'Question: which laptop is primary now?',
        );
    });
});
