import type { ViewId } from '../views';
export interface Command {
    id: string;
    label: string;
    category: 'Navigate' | 'Theme' | 'Action' | 'Help';
    view?: ViewId;
}
export const commands: Command[] = [
    { id: 'go-inbox', label: 'Go to Inbox', category: 'Navigate', view: 'inbox' },
    { id: 'go-reality', label: 'Go to Reality Check', category: 'Navigate', view: 'reality' },
    { id: 'go-recall', label: 'Go to Recall', category: 'Navigate', view: 'recall' },
    { id: 'go-settings', label: 'Open Settings', category: 'Navigate', view: 'settings' },
    { id: 'theme-warm-dark', label: 'Theme: warm dark', category: 'Theme' },
    { id: 'help', label: 'Show keyboard help', category: 'Help' },
];
