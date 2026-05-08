import { fireEvent, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Dreams } from '../../src/views/Dreams';
import { renderWithProviders } from '../support/render';

describe('dreams view', () => {
  it('dreams renders status pills and a distinct dream-run meta-entry', async () => {
    renderWithProviders(<Dreams />);
    await screen.findAllByText('Nightly synthesis pass');
    for (const label of [
      'all',
      'proposed',
      'queued',
      'accepted',
      'completed',
      'dismissed',
      'running',
    ]) {
      expect(screen.getByRole('tab', { name: new RegExp(label, 'i') })).toBeInTheDocument();
    }
    expect(screen.getAllByText('Nightly synthesis pass').length).toBeGreaterThan(0);
    expect(screen.getByText('dream run')).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('dream output');
  });

  it('dreams filters by status and keeps the selected dream in the Inspector', async () => {
    renderWithProviders(<Dreams />);
    await screen.findAllByText('Question: which laptop is primary now?');
    fireEvent.click(screen.getByRole('tab', { name: /queued/i }));
    expect(screen.getByRole('tab', { name: /queued/i })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getAllByText('Question: which laptop is primary now?').length).toBeGreaterThan(0);
    expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent(
      'Question: which laptop is primary now?',
    );
  });
});
