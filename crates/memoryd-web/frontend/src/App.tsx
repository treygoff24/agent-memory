import { QueryClientProvider } from '@tanstack/react-query';
import { useCallback, useEffect, useMemo, useState } from 'react';

import { createDashboardQueryClient, startNotificationsStream, useNotifications } from './api';
import { HelpOverlay } from './help/HelpOverlay';
import { useKeymap } from './keyboard/useKeymap';
import { commands, CommandPalette, type Command } from './palette';
import { hashFor, useRoute, type Route } from './router';
import { Shell } from './shell';
import { ThemeProvider, useTheme } from './theme';
import { Toast } from './ui';
import { viewFor, type ViewId } from './views';

const queryClient = createDashboardQueryClient();

// Map every Route kind to a top-level ViewId for the existing `views.ts`
// component registry. Audit + entities/:id both reuse the same renderer; the
// view itself reads the route via `useRoute()` to render the right detail.
function routeToView(route: Route): ViewId {
    if (route.kind === 'audit') return 'audit';
    if (route.kind === 'entities') return 'entities';
    return route.kind;
}

function DashboardApp() {
    const { route, navigate } = useRoute();
    const view = routeToView(route);
    const [paletteOpen, setPaletteOpen] = useState(false);
    const [helpOpen, setHelpOpen] = useState(false);
    const [bellOpen, setBellOpen] = useState(false);
    const [toast, setToast] = useState<{ title: string; body: string } | null>(null);
    const { setTheme } = useTheme();
    const notifications = useNotifications();
    const ActiveView = useMemo(() => viewFor(view).component, [view]);

    useEffect(() => startNotificationsStream(), []);

    const navigateTo = useCallback(
        (id: ViewId) => {
            // ViewId → Route conversion: only the non-parameterized views are
            // reachable from the sidebar; audit + entities/:id arrive via
            // memory-id / entity-id links.
            if (id === 'audit') return;
            navigate({ kind: id } as Route);
        },
        [navigate],
    );

    const runCommand = useCallback(
        (command: Command) => {
            if (command.view) navigateTo(command.view);
            if (command.theme) setTheme(command.theme);
            if (command.id === 'help') setHelpOpen(true);
            if (command.id === 'close-modals') {
                setHelpOpen(false);
                setBellOpen(false);
            }
            setPaletteOpen(false);
        },
        [navigateTo, setTheme],
    );

    useKeymap(
        useCallback(
            (key: string) => {
                if (key === ':') setPaletteOpen(true);
                if (key === '?') setHelpOpen(true);
                if (key === 'Escape') {
                    setPaletteOpen(false);
                    setHelpOpen(false);
                    setBellOpen(false);
                }
                // `g <letter>` chord: useKeymap fires this composite key after
                // the 1s window. We look up the matching nav command (gi/gr/gd
                // ...) and route to it. Anything not in the command catalog is
                // a no-op so misfired chords stay quiet.
                if (key.length === 2 && key.startsWith('g')) {
                    const cmd = commands.find((c) => c.shortcut === key);
                    if (cmd?.view) navigateTo(cmd.view);
                }
            },
            [navigateTo],
        ),
    );

    const notificationRows = notifications.notifications;
    const notificationSummary =
        notificationRows.length > 0
            ? notificationRows.map((item) => item.title).join('; ')
            : (notifications.error ?? 'No new notifications.');

    return (
        <Shell
            active={view}
            fullbleed={view === 'reality'}
            onNav={navigateTo}
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

export { hashFor };
