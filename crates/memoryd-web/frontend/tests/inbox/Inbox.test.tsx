import { fireEvent, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Inbox, inboxFilters } from '../../src/views/Inbox';
import { renderWithProviders } from '../support/render';

describe('Inbox view', () => {
  it('renders six filter pills with 1-6 keyboard shortcuts', async () => {
    renderWithProviders(<Inbox />);
    await screen.findAllByText('Project uses pnpm, never npm');
    for (const filter of inboxFilters) {
      const tab = screen.getByRole('tab', {
        name: new RegExp(`^${filter.label}\\s+\\d+\\s+${filter.key}$`),
      });
      expect(tab).toBeInTheDocument();
    }
    fireEvent.keyDown(window, { key: '3' });
    expect(screen.getByRole('tab', { name: /conflicts.*3/i })).toHaveAttribute(
      'aria-selected',
      'true',
    );
  });

  it('keeps keyboard focus separate from selection until enter is pressed', async () => {
    renderWithProviders(<Inbox />);
    const list = await screen.findByRole('listbox', { name: 'Inbox items' });
    expect(within(list).getByRole('option', { selected: true })).toHaveTextContent(
      'Project uses pnpm',
    );
    fireEvent.keyDown(window, { key: 'j' });
    const focused = within(list).getByRole('option', { current: true });
    expect(focused).toHaveTextContent('Editor preference disagreement');
    expect(within(list).getByRole('option', { selected: true })).toHaveTextContent(
      'Project uses pnpm',
    );
    fireEvent.keyDown(window, { key: 'Enter' });
    expect(within(list).getByRole('option', { selected: true })).toHaveTextContent(
      'Editor preference disagreement',
    );
  });

  it('renders the selected row through the shared Inspector', async () => {
    renderWithProviders(<Inbox />);
    await screen.findAllByText('Project uses pnpm, never npm');
    fireEvent.click(screen.getByRole('option', { name: /Editor preference disagreement/ }));
    expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent(
      'Editor preference disagreement',
    );
    expect(screen.getAllByText(/merge conflict/i).length).toBeGreaterThan(0);
  });

  it('supports two-pane, three-pane, drawer, and modal layouts', async () => {
    const { rerender } = renderWithProviders(<Inbox layout="two-pane" />);
    await screen.findAllByText('Project uses pnpm, never npm');
    expect(screen.getByTestId('inbox-layout-two-pane')).toBeInTheDocument();

    rerender(<Inbox layout="three-pane" />);
    expect(screen.getByTestId('inbox-layout-three-pane')).toBeInTheDocument();

    rerender(<Inbox layout="drawer" />);
    expect(screen.getByTestId('inbox-layout-drawer')).toBeInTheDocument();
    expect(screen.getByRole('complementary', { name: 'Inbox inspector drawer' })).not.toHaveClass(
      'closed',
    );

    rerender(<Inbox layout="modal" />);
    fireEvent.click(screen.getByRole('option', { name: /Project uses pnpm/ }));
    expect(screen.getByRole('dialog', { name: 'Inbox inspector modal' })).toBeInTheDocument();
  });
});
