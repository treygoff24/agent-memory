import { Inspector } from '../../../inspector';
import { InboxHeader } from '../InboxHeader';
import { InboxList } from '../InboxList';
import type { InboxLayoutProps } from '../types';

export function TwoPaneLayout(props: InboxLayoutProps) {
    const selectedInspector = props.selected ? props.toInspectorItem(props.selected) : null;
    return (
        <div data-testid="inbox-layout-two-pane">
            <InboxHeader
                items={props.items}
                visibleCount={props.visible.length}
                activeFilter={props.activeFilter}
                onFilterChange={props.onFilterChange}
            />
            <div className="panes-2">
                <section className="pane left">
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
                            layout="wide"
                        />
                    </div>
                </section>
            </div>
        </div>
    );
}
