import { fireEvent, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Settings } from '../../src/views/Settings';
import { renderWithProviders } from '../support/render';

describe('settings', () => {
  it('renders the five settings tabs required by the dashboard spec', () => {
    renderWithProviders(<Settings />);

    expect(screen.getByRole('tab', { name: 'Appearance' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Theme editor' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Keyboard' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Notifications' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'About' })).toBeInTheDocument();
  });

  it('updates the dashboard base font size from the appearance tab', () => {
    renderWithProviders(<Settings />);

    const slider = screen.getByRole('slider', { name: 'Base font size' });
    fireEvent.change(slider, { target: { value: '17' } });

    expect(document.documentElement.style.getPropertyValue('--text-base')).toBe('17px');
    expect(localStorage.getItem('memorum.fontSize')).toBe('17');
  });

  it('selects each of the six theme presets', () => {
    renderWithProviders(<Settings />);

    for (const theme of [
      'warm-dark',
      'warm-light',
      'cool-dark',
      'cool-light',
      'monochrome',
      'high-contrast',
    ]) {
      fireEvent.click(screen.getByRole('button', { name: theme }));
      expect(document.documentElement.dataset.theme).toBe(theme);
      expect(localStorage.getItem('memorum.theme')).toBe(theme);
    }
  });
});
