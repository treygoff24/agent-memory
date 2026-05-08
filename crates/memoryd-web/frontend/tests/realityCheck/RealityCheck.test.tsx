import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { RealityCheck } from '../../src/views/RealityCheck';

describe('RealityCheck focus mode', () => {
    it('dispatches all RealityCheckRequest::Respond action variants', async () => {
        const onRespond = vi.fn();
        render(<RealityCheck onRespond={onRespond} />);

        fireEvent.click(screen.getByRole('button', { name: /Confirm/i }));
        fireEvent.click(screen.getByRole('button', { name: /Forget/i }));
        fireEvent.click(screen.getByRole('button', { name: /Not relevant/i }));
        fireEvent.click(screen.getByRole('button', { name: /Skip this week/i }));

        await waitFor(() => expect(onRespond).toHaveBeenCalledTimes(4));
        expect(onRespond.mock.calls.map(([payload]) => payload.action)).toEqual([
            'confirm',
            'forget',
            'not_relevant',
            'skip_this_week',
        ]);
        expect(onRespond.mock.calls[0][0]).toMatchObject({ session_id: 'rc_20260507_001', memory_id: expect.any(String) });
    });

    it('opens an inline correction editor on k and submits Correct with new_body text', async () => {
        const onRespond = vi.fn();
        render(<RealityCheck onRespond={onRespond} />);

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

    it('renders focus-mode strip, sidebar, score, refused, encrypted, and complete variants', () => {
        const { rerender } = render(<RealityCheck variant="score-open" />);
        expect(screen.getByText('reality check')).toBeInTheDocument();
        expect(screen.getByText('Score breakdown')).toBeInTheDocument();
        expect(screen.getByRole('complementary', { name: 'Reality Check session' })).toBeInTheDocument();

        rerender(<RealityCheck variant="encrypted" />);
        expect(screen.getByText(/encrypted memory/i)).toBeInTheDocument();

        rerender(<RealityCheck variant="refused" />);
        expect(screen.getByText('Refused')).toBeInTheDocument();

        rerender(<RealityCheck variant="complete" />);
        expect(screen.getByText('Reality Check complete.')).toBeInTheDocument();
    });
});
