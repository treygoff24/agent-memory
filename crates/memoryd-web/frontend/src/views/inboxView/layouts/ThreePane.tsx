import type { InboxLayoutProps } from '../types';

import { Inspector } from '../../../inspector';
import { inboxFilters } from '../adapter';
import { InboxHeader } from '../InboxHeader';
import { InboxList } from '../InboxList';

export function ThreePaneLayout(props: InboxLayoutProps) {
    const selectedInspector = props.selected ? props.toInspectorItem(props.selected) : null;
    return (
        <div data-testid="inbox-layout-three-pane">
            <InboxHeader
                items={props.items}
                visibleCount={props.visible.length}
                activeFilter={props.activeFilter}
                onFilterChange={props.onFilterChange}
                label="three-pane"
            />
            <div className="panes-3">
                <aside className="pane left">
                    <div className="pane-scroll">
                        <div className="list-section">Filters</div>
                        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                            {inboxFilters.map((filter) => (
                                <button
                                    key={filter.id}
                                    className={`nav-item ${props.activeFilter === filter.id ? 'active' : ''}`}
                                    onClick={() => props.onFilterChange(filter.id)}
                                    type="button"
                                >
                                    <span className="ico">{filter.key}</span>
                                    <span className="label">{filter.label}</span>
                                </button>
                            ))}
                        </div>
                        <div className="list-section">Namespaces</div>
                        {Array.from(new Set(props.items.map((item) => item.namespace))).map((namespace) => (
                            <div
                                key={namespace}
                                className="meta mono"
                                style={{ padding: '4px 10px' }}
                            >
                                {namespace}
                            </div>
                        ))}
                    </div>
                </aside>
                <section className="pane mid">
                    <div className="pane-scroll">
                        <InboxList
                            items={props.visible}
                            selectedId={props.selectedId}
                            focusedId={props.focusedId}
                            onFocus={props.onFocus}
                            onSelect={props.onSelect}
                        />
                    </div>
                </section>
                <section className="pane">
                    <div className="pane-scroll">
                        <Inspector
                            item={selectedInspector}
                            layout="narrow"
                            onAction={props.onInspectorAction}
                        />
                    </div>
                </section>
            </div>
        </div>
    );
}
