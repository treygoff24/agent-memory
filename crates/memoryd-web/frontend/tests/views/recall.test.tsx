import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Recall, makeHeavyRecallEvents } from '../../src/views/Recall';
import { renderWithProviders } from '../support/render';

describe('recall ledger', () => {
  it('recall renders timeline strip, dense ledger headers, filters, and recall-event inspector', async () => {
    renderWithProviders(<Recall />);
    await screen.findAllByText('Project uses pnpm, never npm');
    expect(screen.getByText('Recall ledger')).toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'Timeline' })).toBeInTheDocument();
    for (const heading of [
      'time',
      'seq',
      'device',
      'agent',
      'memory',
      'namespace',
      'lat',
      'score',
    ]) {
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
    renderWithProviders(<Recall events={heavyEvents} heavy />);
    expect(screen.getByText(/9,000 events/)).toBeInTheDocument();
    expect(
      screen.getByTestId('recall-virtual-list').querySelectorAll('.rl-row').length,
    ).toBeLessThan(120);
    expect(screen.getByText(/scrolling backed by virtualization/i)).toBeInTheDocument();
  });
});
