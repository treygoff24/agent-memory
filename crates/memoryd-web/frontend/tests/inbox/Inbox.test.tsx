import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Inbox, inboxFilters } from '../../src/views/Inbox';

describe('Inbox view', () => {
    it('renders six filter pills with 1-6 keyboard shortcuts', () => {
        render(<Inbox />);
        for (const filter of inboxFilters) {
            const tab = screen.getByRole('tab', { name: new RegExp(`^${filter.label}\\b.*${filter.key}`) });
            expect(tab).toBeInTheDocument();
        }
        fireEvent.keyDown(window, { key: '3' });
        expect(screen.getByRole('tab', { name: /conflicts.*3/i })).toHaveAttribute('aria-selected', 'true');
    });

    it('keeps keyboard focus separate from selection until enter is pressed', () => {
        render(<Inbox />);
        const list = screen.getByRole('listbox', { name: 'Inbox items' });
        expect(within(list).getByRole('option', { selected: true })).toHaveTextContent('Project uses pnpm');
        fireEvent.keyDown(window, { key: 'j' });
        const focused = within(list).getByRole('option', { current: true });
        expect(focused).toHaveTextContent('Editor preference disagreement');
        expect(within(list).getByRole('option', { selected: true })).toHaveTextContent('Project uses pnpm');
        fireEvent.keyDown(window, { key: 'Enter' });
        expect(within(list).getByRole('option', { selected: true })).toHaveTextContent('Editor preference disagreement');
    });

    it('renders the selected row through the shared Inspector', () => {
        render(<Inbox />);
        fireEvent.click(screen.getByRole('option', { name: /Editor preference disagreement/ }));
        expect(screen.getByRole('region', { name: 'Inspector' })).toHaveTextContent('Editor preference disagreement');
        expect(screen.getByText(/merge conflict/i)).toBeInTheDocument();
    });

    it('supports two-pane, three-pane, drawer, and modal layouts', () => {
        const { rerender } = render(<Inbox layout="two-pane" />);
        expect(screen.getByTestId('inbox-layout-two-pane')).toBeInTheDocument();

        rerender(<Inbox layout="three-pane" />);
        expect(screen.getByTestId('inbox-layout-three-pane')).toBeInTheDocument();

        rerender(<Inbox layout="drawer" />);
        expect(screen.getByTestId('inbox-layout-drawer')).toBeInTheDocument();
        expect(screen.getByRole('complementary', { name: 'Inbox inspector drawer' })).not.toHaveClass('closed');

        rerender(<Inbox layout="modal" />);
        fireEvent.click(screen.getByRole('option', { name: /Project uses pnpm/ }));
        expect(screen.getByRole('dialog', { name: 'Inbox inspector modal' })).toBeInTheDocument();
    });
});
