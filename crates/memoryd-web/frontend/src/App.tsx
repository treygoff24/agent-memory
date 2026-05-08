import { useCallback, useMemo, useState } from 'react';

import { HelpOverlay } from './help/HelpOverlay';
import { useKeymap } from './keyboard/useKeymap';
import { CommandPalette, type Command } from './palette';
import { Shell } from './shell';
import { ThemeProvider, useTheme } from './theme';
import { Toast } from './ui';
import { viewFor, type ViewId } from './views';

function DashboardApp() {
    const initialView = (new URLSearchParams(window.location.search).get('view') as ViewId | null) ?? 'inbox';
    const [view, setView] = useState<ViewId>(() => viewFor(initialView).id);
    const [paletteOpen, setPaletteOpen] = useState(false);
    const [helpOpen, setHelpOpen] = useState(false);
    const [bellOpen, setBellOpen] = useState(false);
    const [toast, setToast] = useState<{ title: string; body: string } | null>(null);
    const { setTheme } = useTheme();
    const ActiveView = useMemo(() => viewFor(view).component, [view]);

    const runCommand = useCallback(
        (command: Command) => {
            if (command.view) setView(command.view);
            if (command.id === 'theme-warm-dark') setTheme('warm-dark');
            if (command.id === 'help') setHelpOpen(true);
            setPaletteOpen(false);
        },
        [setTheme],
    );

    useKeymap(
        useCallback((key: string) => {
            if (key === ':') setPaletteOpen(true);
            if (key === '?') setHelpOpen(true);
            if (key === 'Escape') {
                setPaletteOpen(false);
                setHelpOpen(false);
                setBellOpen(false);
            }
        }, []),
    );

    return (
        <Shell
            active={view}
            onNav={setView}
            onPalette={() => setPaletteOpen(true)}
            onBell={() => {
                setBellOpen((open) => !open);
                setToast({ title: 'Notifications', body: 'Review queue over threshold; dream run scheduled.' });
            }}
        >
            <h1 className="sr-only">Memorum Dashboard</h1>
            <ActiveView />
            {bellOpen && (
                <div className="notif">
                    <div className="notif-head">Notifications · 2</div>
                    <div className="notif-row">Review queue over threshold</div>
                    <div className="notif-row">Dream run scheduled for 03:00</div>
                </div>
            )}
            <CommandPalette
                open={paletteOpen}
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
        <ThemeProvider>
            <DashboardApp />
        </ThemeProvider>
    );
}
