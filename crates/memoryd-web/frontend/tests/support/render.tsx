import { QueryClientProvider } from '@tanstack/react-query';
import { render, type RenderOptions } from '@testing-library/react';
import type { ReactElement, ReactNode } from 'react';

import { createDashboardQueryClient } from '../../src/api';

function Providers({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={createDashboardQueryClient()}>{children}</QueryClientProvider>
  );
}

export function renderWithProviders(ui: ReactElement, options?: Omit<RenderOptions, 'wrapper'>) {
  return render(ui, { wrapper: Providers, ...options });
}
