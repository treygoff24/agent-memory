import { QueryClientProvider } from '@tanstack/react-query';
import { useCallback, useEffect, useMemo, useState } from 'react';

import { createDashboardQueryClient, startNotificationsStream, useNotifications } from './api';
import { HelpOverlay } from './help/HelpOverlay';
import { navigationCommands } from './keyboard/Keymap';
import { useKeymap } from './keyboard/useKeymap';
import { CommandPalette, type Command } from './palette';
import { Shell } from './shell';
import { ThemeProvider, useTheme } from './theme';
import { Toast } from './ui';
import { viewFor, type ViewId } from './views';

const queryClient = createDashboardQueryClient();

function initialDashboardView(): ViewId {
    const params = new URLSearchParams(window.location.search);
    if (params.get('tweaks') === '1') return 'settings';
    return viewFor((params.get('view') as ViewId | null) ?? 'inbox').id;
}

function DashboardApp() {
    const [view, setView] = useState<ViewId>(() => initialDashboardView());
    const [paletteOpen, setPaletteOpen] = useState(false);
    const [helpOpen, setHelpOpen] = useState(false);
    const [bellOpen, setBellOpen] = useState(false);
    const [toast, setToast] = useState<{ title: string; body: string } | null>(null);
    const { setTheme } = useTheme();
    const notifications = useNotifications();
    const ActiveView = useMemo(() => viewFor(view).component, [view]);

    useEffect(() => startNotificationsStream(), []);

    const runCommand = useCallback(
        (command: Command) => {
            if (command.view) setView(command.view);
            if (command.theme) setTheme(command.theme);
            if (command.id === 'help') setHelpOpen(true);
            if (command.id === 'close-modals') {
                setHelpOpen(false);
                setBellOpen(false);
            }
            setPaletteOpen(false);
        },
        [setTheme],
    );

    useKeymap(
        useCallback((key: string) => {
            if (key === ':') setPaletteOpen(true);
            if (key === '?') setHelpOpen(true);
            const navigationCommand = navigationCommands.find((command) => command.key === key);
            if (navigationCommand?.view) setView(navigationCommand.view);
            if (key === 'Escape') {
                setPaletteOpen(false);
                setHelpOpen(false);
                setBellOpen(false);
            }
        }, []),
    );

    const notificationRows = notifications.notifications;
    const notificationSummary =
        notificationRows.length > 0
            ? notificationRows.map((item) => item.title).join('; ')
            : (notifications.error ?? 'No new notifications.');

    return (
        <Shell
            active={view}
            onNav={setView}
            onPalette={() => setPaletteOpen(true)}
            onBell={() => {
                setBellOpen((open) => !open);
                setToast({ title: 'Notifications', body: notificationSummary });
            }}
        >
            <h1 className="sr-only">Memorum Dashboard</h1>
            <ActiveView />
            {bellOpen && (
                <div className="notif">
                    <div className="notif-head">Notifications · {notificationRows.length}</div>
                    {notificationRows.length > 0 ? (
                        notificationRows.map((item) => (
                            <div
                                className="notif-row"
                                key={item.id}
                            >
                                {item.title}
                            </div>
                        ))
                    ) : (
                        <div className="notif-row">{notifications.error ?? 'No new notifications.'}</div>
                    )}
                </div>
            )}
            <CommandPalette
                open={paletteOpen}
                activeView={view}
                onClose={() => setPaletteOpen(false)}
                onRun={runCommand}
            />
            <HelpOverlay
                open={helpOpen}
                onClose={() => setHelpOpen(false)}
            />
            {toast && (
                <div className="toast-stack">
                    <Toast
                        title={toast.title}
                        body={toast.body}
                        onDismiss={() => setToast(null)}
                    />
                </div>
            )}
        </Shell>
    );
}

export function App() {
    return (
        <QueryClientProvider client={queryClient}>
            <ThemeProvider>
                <DashboardApp />
            </ThemeProvider>
        </QueryClientProvider>
    );
}
