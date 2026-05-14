import { fireEvent, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { RealityCheck } from '../../src/views/RealityCheck';
import { renderWithProviders } from '../support/render';

describe('RealityCheck focus mode', () => {
    it('dispatches the four brief-mandated answer-card actions plus the keyboard-only not_relevant action', async () => {
        const onRespond = vi.fn();
        renderWithProviders(<RealityCheck onRespond={onRespond} />);

        // Brief §View 2 mandates four answer cards: Confirm / Correct / Forget /
        // Skip. The 'not_relevant' daemon action stays reachable as the 'n'
        // keyboard shortcut for power users — the visible stack matches the brief.
        await screen.findByRole('button', { name: /Confirm/i });
        fireEvent.click(screen.getByRole('button', { name: /Confirm/i }));
        fireEvent.click(screen.getByRole('button', { name: /Forget/i }));
        fireEvent.keyDown(window, { key: 'n' });
        fireEvent.click(screen.getByRole('button', { name: /Skip/i }));

        await waitFor(() => expect(onRespond).toHaveBeenCalledTimes(4));
        expect(onRespond.mock.calls.map(([payload]) => payload.action)).toEqual([
            'confirm',
            'forget',
            'not_relevant',
            'skip_this_week',
        ]);
        expect(onRespond.mock.calls[0][0]).toMatchObject({
            session_id: 'rc_20260507_001',
            memory_id: expect.any(String),
        });
    });

    it('opens an inline correction editor on k and submits Correct with new_body text', async () => {
        const onRespond = vi.fn();
        renderWithProviders(<RealityCheck onRespond={onRespond} />);

        await screen.findByRole('button', { name: /Confirm/i });
        fireEvent.keyDown(window, { key: 'k' });
        const editor = screen.getByRole('textbox', { name: 'Corrected memory body' });
        expect(editor).toBeInTheDocument();
        fireEvent.change(editor, { target: { value: 'Corrected body from operator.' } });
        fireEvent.click(screen.getByRole('button', { name: /Save correction/i }));

        await waitFor(() =>
            expect(onRespond).toHaveBeenCalledWith(
                expect.objectContaining({ action: 'correct', correction: 'Corrected body from operator.' }),
            ),
        );
    });

    it('renders focus-mode strip, sidebar, score, refused, encrypted, and complete variants', async () => {
        const { rerender } = renderWithProviders(<RealityCheck variant="score-open" />);
        await screen.findByText('reality check');
        expect(screen.getByText('Score breakdown')).toBeInTheDocument();
        expect(screen.getByRole('complementary', { name: 'Reality Check session' })).toBeInTheDocument();

        rerender(<RealityCheck variant="encrypted" />);
        await screen.findByText(/encrypted memory/i);

        rerender(<RealityCheck variant="refused" />);
        await screen.findByText('Refused');

        rerender(<RealityCheck variant="complete" />);
        await screen.findByText('Reality Check complete.');
    });
});
