import type { ViewId } from '../views';
export interface KeyCommand {
    key: string;
    label: string;
    view?: ViewId;
}
export const globalKeymap: KeyCommand[] = [
    { key: ':', label: 'Open command palette' },
    { key: '?', label: 'Open help' },
    { key: 'Escape', label: 'Close modal' },
    { key: 'gi', label: 'Go Inbox', view: 'inbox' },
    { key: 'gr', label: 'Go Reality Check', view: 'reality' },
    { key: 'gl', label: 'Go Recall', view: 'recall' },
    { key: 'gd', label: 'Go Dreams', view: 'dreams' },
    { key: 'gp', label: 'Go Peers', view: 'peers' },
    { key: 'gg', label: 'Go Governance', view: 'governance' },
    { key: 'ge', label: 'Go Entities', view: 'entities' },
    { key: 'gs', label: 'Go Settings', view: 'settings' },
];

export const navigationCommands = globalKeymap.filter((command) => command.view);
