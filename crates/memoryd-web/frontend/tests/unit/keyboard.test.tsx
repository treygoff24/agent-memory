import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { isTextInputTarget, useKeymap } from '../../src/keyboard/useKeymap';

function KeymapHarness({ onKey }: { onKey(key: string): void }) {
    useKeymap(onKey);
    return (
        <div>
            <button type="button">Focusable</button>
            <input aria-label="Search memories" />
        </div>
    );
}

describe('keyboard', () => {
    it('treats form controls as text targets', () => {
        const input = document.createElement('input');
        const textarea = document.createElement('textarea');
        const select = document.createElement('select');
        const button = document.createElement('button');

        expect(isTextInputTarget(input)).toBe(true);
        expect(isTextInputTarget(textarea)).toBe(true);
        expect(isTextInputTarget(select)).toBe(true);
        expect(isTextInputTarget(button)).toBe(false);
        expect(isTextInputTarget(null)).toBe(false);
    });

    it('dispatches g-prefix navigation as a single key command', () => {
        const onKey = vi.fn();
        render(<KeymapHarness onKey={onKey} />);

        fireEvent.keyDown(window, { key: 'g' });
        fireEvent.keyDown(window, { key: 's' });

        expect(onKey).toHaveBeenCalledTimes(1);
        expect(onKey).toHaveBeenCalledWith('gs');
    });

    it('does not dispatch global commands while focus is in a text input', () => {
        const onKey = vi.fn();
        render(<KeymapHarness onKey={onKey} />);

        const input = screen.getByRole('textbox', { name: 'Search memories' });
        input.focus();
        fireEvent.keyDown(input, { key: ':' });

        expect(onKey).not.toHaveBeenCalled();
    });
});
