import type { ReactElement, ReactNode } from 'react';

import { QueryClientProvider } from '@tanstack/react-query';
import { render, type RenderOptions } from '@testing-library/react';

import { createDashboardQueryClient } from '../../src/api';
import { ThemeProvider } from '../../src/theme';

function Providers({ children }: { children: ReactNode }) {
    return (
        <QueryClientProvider client={createDashboardQueryClient()}>
            <ThemeProvider>{children}</ThemeProvider>
        </QueryClientProvider>
    );
}

export function renderWithProviders(ui: ReactElement, options?: Omit<RenderOptions, 'wrapper'>) {
    return render(ui, { wrapper: Providers, ...options });
}
