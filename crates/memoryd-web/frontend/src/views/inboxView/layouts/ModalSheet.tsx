import type { InboxLayoutProps } from '../types';

import { Inspector } from '../../../inspector';
import { InboxHeader } from '../InboxHeader';
import { InboxList } from '../InboxList';

export function ModalSheetLayout(props: InboxLayoutProps) {
    const selectedInspector = props.selected ? props.toInspectorItem(props.selected) : null;
    return (
        <div data-testid="inbox-layout-modal">
            <InboxHeader
                items={props.items}
                visibleCount={props.visible.length}
                activeFilter={props.activeFilter}
                onFilterChange={props.onFilterChange}
                label="modal"
            />
            <div className="panes-single">
                <section className="pane">
                    <div className="pane-scroll">
                        <InboxList
                            items={props.visible}
                            selectedId={props.modalOpen ? props.selectedId : ''}
                            focusedId={props.focusedId}
                            onFocus={props.onFocus}
                            onSelect={props.onSelect}
                        />
                    </div>
                </section>
            </div>
            {props.modalOpen && props.selected ? (
                <div
                    className="modal-veil"
                    onClick={props.onCloseModal}
                >
                    <div
                        className="modal"
                        role="dialog"
                        aria-label="Inbox inspector modal"
                        style={{ width: 760, maxHeight: '76vh', overflow: 'auto' }}
                        onClick={(event) => event.stopPropagation()}
                    >
                        <div style={{ display: 'flex', justifyContent: 'flex-end', padding: '10px 14px 0' }}>
                            <button
                                className="icon-btn"
                                onClick={props.onCloseModal}
                                aria-label="Close modal"
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
                </div>
            ) : null}
        </div>
    );
}
