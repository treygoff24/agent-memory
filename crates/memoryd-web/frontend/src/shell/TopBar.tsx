import { useState, type FormEvent } from 'react';

import type { ShellStatus } from './Shell';

import { searchMemories, type SearchHitSummary } from '../api';
import { StatusDot } from '../ui';

export function TopBar({ onPalette, onBell, status }: { onPalette(): void; onBell(): void; status: ShellStatus }) {
    const [query, setQuery] = useState('');
    const [hits, setHits] = useState<SearchHitSummary[]>([]);
    const [searchState, setSearchState] = useState<'idle' | 'searching' | 'error'>('idle');

    async function submitSearch(event: FormEvent<HTMLFormElement>) {
        event.preventDefault();
        const trimmed = query.trim();
        if (!trimmed) {
            setHits([]);
            setSearchState('idle');
            return;
        }
        setSearchState('searching');
        try {
            const response = await searchMemories(trimmed);
            setHits(response.hits);
            setSearchState('idle');
        } catch {
            setHits([]);
            setSearchState('error');
        }
    }

    return (
        <header className="topbar">
            <div className="brand">
                <span className="sigil">◆</span>
                <span>memorum</span>
            </div>
            <form
                className="search"
                role="search"
                onSubmit={submitSearch}
            >
                <input
                    aria-label="Search memories"
                    name="q"
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                    placeholder="Search memories"
                />
                <button
                    className="search-state"
                    type="submit"
                    disabled={searchState === 'searching'}
                >
                    {searchState === 'searching' ? 'searching' : hits.length > 0 ? `${hits.length} result` : 'search'}
                </button>
                {hits.length > 0 || searchState === 'error' ? (
                    <div
                        className="search-results"
                        role="listbox"
                        aria-label="Search results"
                    >
                        {searchState === 'error' ? (
                            <div
                                className="search-result"
                                role="option"
                                aria-selected="false"
                            >
                                Search unavailable
                            </div>
                        ) : (
                            hits.map((hit) => (
                                <a
                                    key={hit.id}
                                    className="search-result"
                                    role="option"
                                    aria-selected="false"
                                    href={`/?view=recall&memory=${encodeURIComponent(hit.id)}`}
                                >
                                    <span>{hit.summary}</span>
                                    <small>{hit.snippet}</small>
                                </a>
                            ))
                        )}
                    </div>
                ) : null}
            </form>
            <div className="topbar-right">
                <button
                    className="icon-btn"
                    onClick={onPalette}
                    aria-label="Command palette"
                >
                    :
                </button>
                <button
                    className="icon-btn"
                    onClick={onBell}
                    aria-label="Notifications"
                >
                    ●
                </button>
                <div className="status-cluster">
                    <span className="pair">
                        <StatusDot kind={status.daemon} />
                        daemon
                    </span>
                    <span className="pair">
                        <StatusDot kind={status.daemon === 'bad' ? 'idle' : 'ok'} />
                        {status.syncLabel}
                    </span>
                </div>
            </div>
        </header>
    );
}
