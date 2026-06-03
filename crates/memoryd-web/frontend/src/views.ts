import type { ComponentType } from 'react';

import type { ViewId } from './views/registry';

import { Dreams } from './views/Dreams';
import { Entities } from './views/Entities';
import { Governance } from './views/Governance';
import { Inbox } from './views/Inbox';
import { Peers } from './views/Peers';
import { RealityCheck } from './views/RealityCheck';
import { Recall } from './views/Recall';
import { Settings } from './views/Settings';

export type { ViewId } from './views/registry';

const views = [
    { id: 'inbox', label: 'Inbox', component: Inbox },
    { id: 'reality', label: 'Reality Check', component: RealityCheck },
    { id: 'recall', label: 'Recall', component: Recall },
    { id: 'dreams', label: 'Dreams', component: Dreams },
    { id: 'peers', label: 'Peers', component: Peers },
    { id: 'governance', label: 'Governance', component: Governance },
    { id: 'entities', label: 'Entities', component: Entities },
    { id: 'settings', label: 'Settings', component: Settings },
] as const satisfies ReadonlyArray<{ id: ViewId; label: string; component: ComponentType }>;
export function viewFor(id: ViewId) {
    return views.find((view) => view.id === id) ?? views[0];
}
