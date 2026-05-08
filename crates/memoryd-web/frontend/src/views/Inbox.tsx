import { useCallback, useEffect, useMemo, useState } from 'react';

import { inboxItems, type InboxItem } from '../data/fixtures';
import { filterItems, inboxFilters, inspectorItemFromInbox, toInboxViewItem } from './inboxView/adapter';
import type { InboxFilterId, InboxLayout, InboxViewItem } from './inboxView';
import { DrawerLayout, ModalSheetLayout, ThreePaneLayout, TwoPaneLayout } from './inboxView/layouts';

export { inboxFilters };

interface InboxProps {
    layout?: InboxLayout;
    items?: InboxItem[];
}

const layouts = ['two-pane', 'three-pane', 'drawer', 'modal'] as const;

function layoutFromUrl(): InboxLayout {
    const raw = new URLSearchParams(window.location.search).get('layout');
    return layouts.find((candidate) => candidate === raw) ?? 'two-pane';
}

function isTextInputTarget(target: unknown): boolean {
    if (!(target instanceof HTMLElement)) return false;
    const tagName = target.tagName.toLowerCase();
    return tagName === 'input' || tagName === 'textarea' || target.isContentEditable;
}

function clampIndex(index: number, length: number) {
    if (length === 0) return 0;
    return Math.max(0, Math.min(index, length - 1));
}

function nextFilterForKey(key: string): InboxFilterId | undefined {
    return inboxFilters.find((filter) => filter.key === key)?.id;
}

export function Inbox({ layout, items: sourceItems = inboxItems }: InboxProps) {
    const resolvedLayout = layout ?? layoutFromUrl();
    const items = useMemo(() => sourceItems.map(toInboxViewItem), [sourceItems]);
    const [activeFilter, setActiveFilter] = useState<InboxFilterId>('all');
    const [selectedId, setSelectedId] = useState(items[0]?.id ?? '');
    const [focusedId, setFocusedId] = useState(items[0]?.id ?? '');
    const [drawerOpen, setDrawerOpen] = useState(true);
    const [modalOpen, setModalOpen] = useState(resolvedLayout === 'modal');

    const visible = useMemo(() => filterItems(items, activeFilter), [activeFilter, items]);
    const selected = visible.find((item) => item.id === selectedId) ?? visible[0];

    useEffect(() => {
        const firstVisible = visible[0];
        if (!firstVisible) {
            setSelectedId('');
            setFocusedId('');
            return;
        }
        if (!visible.some((item) => item.id === selectedId)) setSelectedId(firstVisible.id);
        if (!visible.some((item) => item.id === focusedId)) setFocusedId(firstVisible.id);
    }, [focusedId, selectedId, visible]);

    useEffect(() => {
        if (resolvedLayout === 'drawer') setDrawerOpen(true);
        if (resolvedLayout === 'modal') setModalOpen(true);
    }, [resolvedLayout]);

    const selectRow = useCallback(
        (id: string) => {
            setFocusedId(id);
            setSelectedId(id);
            if (resolvedLayout === 'drawer') setDrawerOpen(true);
            if (resolvedLayout === 'modal') setModalOpen(true);
        },
        [resolvedLayout],
    );

    const moveFocus = useCallback(
        (direction: 1 | -1) => {
            if (visible.length === 0) return;
            const currentIndex = Math.max(0, visible.findIndex((item) => item.id === focusedId));
            const next = visible[clampIndex(currentIndex + direction, visible.length)];
            if (next) setFocusedId(next.id);
        },
        [focusedId, visible],
    );

    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            if (isTextInputTarget(event.target) || isTextInputTarget(document.activeElement)) return;
            const filter = nextFilterForKey(event.key);
            if (filter) {
                event.preventDefault();
                setActiveFilter(filter);
                return;
            }
            if (event.key === 'j' || event.key === 'ArrowDown') {
                event.preventDefault();
                moveFocus(1);
                return;
            }
            if (event.key === 'k' || event.key === 'ArrowUp') {
                event.preventDefault();
                moveFocus(-1);
                return;
            }
            if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                if (focusedId) selectRow(focusedId);
            }
        };
        window.addEventListener('keydown', onKeyDown);
        return () => window.removeEventListener('keydown', onKeyDown);
    }, [focusedId, moveFocus, selectRow]);

    const layoutProps = {
        items,
        visible,
        selected: selected as InboxViewItem | undefined,
        selectedId: selected?.id ?? '',
        focusedId,
        activeFilter,
        drawerOpen,
        modalOpen,
        onFilterChange: setActiveFilter,
        onFocus: setFocusedId,
        onSelect: selectRow,
        onCloseDrawer: () => setDrawerOpen(false),
        onCloseModal: () => setModalOpen(false),
        toInspectorItem: inspectorItemFromInbox,
    };

    if (resolvedLayout === 'three-pane') return <ThreePaneLayout {...layoutProps} />;
    if (resolvedLayout === 'drawer') return <DrawerLayout {...layoutProps} />;
    if (resolvedLayout === 'modal') return <ModalSheetLayout {...layoutProps} />;
    return <TwoPaneLayout {...layoutProps} />;
}
