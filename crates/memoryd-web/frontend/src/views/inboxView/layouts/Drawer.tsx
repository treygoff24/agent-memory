import type { InboxLayoutProps } from '../types';

import { Inspector } from '../../../inspector';
import { InboxHeader } from '../InboxHeader';
import { InboxList } from '../InboxList';

export function DrawerLayout(props: InboxLayoutProps) {
    const selectedInspector = props.selected ? props.toInspectorItem(props.selected) : null;
    return (
        <div data-testid="inbox-layout-drawer">
            <InboxHeader
                items={props.items}
                visibleCount={props.visible.length}
                activeFilter={props.activeFilter}
                onFilterChange={props.onFilterChange}
                label="drawer"
            />
            <div className="panes-drawer">
                <section className="pane left">
                    <div className="pane-scroll">
                        <InboxList
                            items={props.visible}
                            selectedId={props.drawerOpen ? props.selectedId : ''}
                            focusedId={props.focusedId}
                            onFocus={props.onFocus}
                            onSelect={props.onSelect}
                        />
                    </div>
                </section>
                <aside
                    className={`drawer ${props.drawerOpen ? '' : 'closed'}`}
                    role="complementary"
                    aria-label="Inbox inspector drawer"
                >
                    <div
                        className="pane-scroll"
                        style={{ paddingTop: 0 }}
                    >
                        <div style={{ display: 'flex', justifyContent: 'flex-end', padding: '10px 14px 0' }}>
                            <button
                                className="icon-btn"
                                onClick={props.onCloseDrawer}
                                aria-label="Close drawer"
                                type="button"
                            >
                                ×
                            </button>
                        </div>
                        <Inspector
                            item={selectedInspector}
                            layout="narrow"
                        />
                    </div>
                </aside>
            </div>
        </div>
    );
}
