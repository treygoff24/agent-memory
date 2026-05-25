import { fireEvent, screen, waitFor } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { describe, expect, it } from 'vitest';

import { Settings } from '../../src/views/Settings';
import { server } from '../msw/server';
import { renderWithProviders } from '../support/render';

describe('settings', () => {
    it('renders the six settings tabs required by the dashboard spec', () => {
        renderWithProviders(<Settings />);

        expect(screen.getByRole('tab', { name: 'Appearance' })).toBeInTheDocument();
        expect(screen.getByRole('tab', { name: 'Theme editor' })).toBeInTheDocument();
        expect(screen.getByRole('tab', { name: 'Keyboard' })).toBeInTheDocument();
        expect(screen.getByRole('tab', { name: 'Notifications' })).toBeInTheDocument();
        expect(screen.getByRole('tab', { name: 'Policies' })).toBeInTheDocument();
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

        for (const theme of ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast']) {
            fireEvent.click(screen.getByRole('button', { name: theme }));
            expect(document.documentElement.dataset.theme).toBe(theme);
            expect(localStorage.getItem('memorum.theme')).toBe(theme);
        }
    });

    it('enables daemon-backed policy saves and refreshes YAML after a successful POST', async () => {
        let savedYaml = 'name: project-standard\nversion: 2\nscope: project\nconfidence_floor: 0.7\n';
        server.use(
            http.get('/api/policy-editor', () =>
                HttpResponse.json({
                    source: 'disk',
                    raw_yaml: savedYaml,
                    writable: true,
                    files: ['project-standard.yaml'],
                    current_file: 'project-standard.yaml',
                    policies: [
                        {
                            scope: 'project',
                            selected_policy: savedYaml.includes('0.72')
                                ? 'project-standard@v2-updated'
                                : 'project-standard@v2',
                            policy_source: 'disk',
                        },
                    ],
                }),
            ),
            http.post('/api/policy-editor', async ({ request }) => {
                const payload = (await request.json()) as { raw_yaml: string; file_name?: string };
                savedYaml = payload.raw_yaml;
                return HttpResponse.json({
                    accepted: true,
                    file_name: payload.file_name ?? 'project-standard.yaml',
                    policies: [
                        { scope: 'project', selected_policy: 'project-standard@v2-updated', policy_source: 'disk' },
                    ],
                });
            }),
        );
        renderWithProviders(<Settings />);

        fireEvent.click(screen.getByRole('tab', { name: 'Policies' }));
        const save = await screen.findByRole('button', { name: 'Save policy' });
        await waitFor(() => expect(save).toBeEnabled());

        const editor = screen.getByLabelText('Policy YAML');
        fireEvent.change(editor, { target: { value: savedYaml.replace('0.7', '0.72') } });
        fireEvent.click(save);

        expect(await screen.findByText('Policy saved.')).toBeInTheDocument();
        await waitFor(() => expect(screen.getByText('project-standard@v2-updated')).toBeInTheDocument());
        expect((screen.getByLabelText('Policy YAML') as HTMLTextAreaElement).value).toContain('0.72');
    });
});
