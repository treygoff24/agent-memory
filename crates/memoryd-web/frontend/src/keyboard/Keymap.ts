import type { ViewId } from '../views';
export interface KeyCommand {
    key: string;
    label: string;
    view?: ViewId;
}
export const globalKeymap: KeyCommand[] = [
    { key: ':', label: 'Open command palette' },
    { key: '?', label: 'Open help' },
    { key: 'Esc', label: 'Close modal' },
    { key: 'gi', label: 'Go Inbox', view: 'inbox' },
    { key: 'gr', label: 'Go Reality Check', view: 'reality' },
    { key: 'gl', label: 'Go Recall', view: 'recall' },
];
