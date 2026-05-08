import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Dreams } from '../../src/views/Dreams';

describe('dreams view', () => {
    it('dreams renders status pills and a distinct dream-run meta-entry', () => {
        render(<Dreams />);
        for (const label of ['all', 'proposed', 'queued', 'accepted', 'completed', 'dismissed', 'running']) {
            expect(screen.getByRole('tab', { name: new RegExp(label, 'i') })).toBeInTheDocument();
        }
        expect(screen.getAllByText('Nightly synthesis pass').length).toBeGreaterThan(0);
        expect(screen.getByText('dream run')).toBeInTheDocument();
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('dream output');
    });

    it('dreams filters by status and keeps the selected dream in the Inspector', () => {
        render(<Dreams />);
        fireEvent.click(screen.getByRole('tab', { name: /queued/i }));
        expect(screen.getByRole('tab', { name: /queued/i })).toHaveAttribute('aria-selected', 'true');
        expect(screen.getAllByText('Question: which laptop is primary now?').length).toBeGreaterThan(0);
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('Question: which laptop is primary now?');
    });
});
