import type { InspectorAction, InspectorItem } from '../../inspector';

export type InboxLayout = 'two-pane' | 'three-pane' | 'drawer' | 'modal';

export type InboxFilterId = 'all' | 'review' | 'conflicts' | 'recall' | 'dreams' | 'due';

export type InboxKind = 'review' | 'recall' | 'conflict' | 'dream' | 'due';

export interface InboxItem {
    id: string;
    kind: InboxKind;
    title: string;
    namespace: string;
    meta: string;
    body: string;
    confidence: number;
}

export interface InboxFilterDefinition {
    id: InboxFilterId;
    label: string;
    key: string;
}

export interface InboxViewItem extends InboxItem {
    /** Glyph kind drives a Phosphor icon in InboxList per brief §3.1.
     *  Kept as a kind tag (not a Unicode string) so list rendering can pick
     *  the right `<Icon weight="..." color="..." />` from `src/ui/icons.ts`. */
    glyphKind: 'review' | 'conflict' | 'recall' | 'dream' | 'due';
    sub: string[];
}

export interface InboxLayoutProps {
    items: InboxViewItem[];
    visible: InboxViewItem[];
    selected: InboxViewItem | undefined;
    selectedId: string;
    focusedId: string;
    activeFilter: InboxFilterId;
    drawerOpen: boolean;
    modalOpen: boolean;
    onFilterChange: (filter: InboxFilterId) => void;
    onFocus: (id: string) => void;
    onSelect: (id: string) => void;
    onCloseDrawer: () => void;
    onCloseModal: () => void;
    onAction: (action: InspectorAction, item: InspectorItem) => void;
    toInspectorItem: (item: InboxViewItem) => InspectorItem;
    onRunAnyway?: (() => void) | undefined;
}
