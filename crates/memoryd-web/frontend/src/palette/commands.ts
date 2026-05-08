import type { ViewId } from '../views';
import { views } from '../views';
import { themes, type Theme } from '../theme';

export interface Command {
    id: string;
    label: string;
    category: 'Navigate' | 'Theme' | 'Action' | 'Help';
    view?: ViewId;
    theme?: Theme;
    shortcut?: string;
    scope?: ViewId;
}

const navigationShortcuts: Record<ViewId, string> = {
    inbox: 'gi',
    reality: 'gr',
    recall: 'gl',
    dreams: 'gd',
    peers: 'gp',
    governance: 'gg',
    entities: 'ge',
    settings: 'gs',
};

export const commands: Command[] = [
    ...views.map((view) => ({
        id: `go-${view.id}`,
        label: view.id === 'settings' ? 'Open Settings' : `Go to ${view.label}`,
        category: 'Navigate' as const,
        view: view.id,
        shortcut: navigationShortcuts[view.id],
    })),
    ...themes.map((theme) => ({
        id: `theme-${theme}`,
        label: `Theme: ${theme}`,
        category: 'Theme' as const,
        theme,
    })),
    { id: 'help', label: 'Show keyboard help', category: 'Help', shortcut: '?' },
    { id: 'close-modals', label: 'Close modal or overlay', category: 'Action', shortcut: 'Escape' },
];
