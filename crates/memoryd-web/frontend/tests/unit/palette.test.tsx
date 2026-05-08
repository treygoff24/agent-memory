import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { CommandPalette } from '../../src/palette';

describe('palette', () => {
  it('runs the first Fuse-matched command with Enter', () => {
    const onRun = vi.fn();
    render(<CommandPalette open onClose={vi.fn()} onRun={onRun} />);

    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: 'settngs' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(onRun).toHaveBeenCalledTimes(1);
    expect(onRun).toHaveBeenCalledWith(expect.objectContaining({ id: 'go-settings' }));
  });

  it('closes from the input with Escape', () => {
    const onClose = vi.fn();
    render(<CommandPalette open onClose={onClose} onRun={vi.fn()} />);

    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Escape' });

    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
