import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { useKeymap } from '../../src/keyboard/useKeymap';

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
