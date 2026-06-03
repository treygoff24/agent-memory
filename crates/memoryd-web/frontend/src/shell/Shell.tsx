import type { ReactNode } from 'react';

import type { ViewId } from '../views';

import { useStatusQuery } from '../api';
import { Footer } from './Footer';
import { Sidebar } from './Sidebar';
import { TopBar } from './TopBar';
import type { ShellStatus } from './types';

export type { ShellStatus } from './types';

function formatPendingChanges(ahead: number, behind: number): string {
    const count = ahead + behind;
    if (count === 0) return 'sync · clean';
    return `sync · ${count} pending`;
}

function formatPeerCount(count: number): string {
    return `peers · ${count} active`;
}

function daemonIndicator(socketState: string): ShellStatus['daemon'] {
    if (socketState === 'ok' || socketState === 'ready') return 'ok';
    if (socketState === 'loading') return 'idle';
    return 'warn';
}

function statusFromQuery(query: ReturnType<typeof useStatusQuery>): ShellStatus {
    if (query.isError) {
        return {
            daemon: 'bad',
            syncLabel: 'sync · unknown',
            peerLabel: 'sync · peers unknown',
        };
    }
    const status = query.data;
    if (!status) {
        return {
            daemon: 'idle',
            syncLabel: 'sync · loading',
            peerLabel: 'sync · peers loading',
        };
    }
    return {
        daemon: daemonIndicator(status.socket),
        syncLabel: formatPendingChanges(status.sync.ahead, status.sync.behind),
        peerLabel: formatPeerCount(status.active_sessions.length),
    };
}

export function Shell({
    active,
    children,
    onNav,
    onPalette,
    onBell,
}: {
    active: ViewId;
    children: ReactNode;
    onNav(id: ViewId): void;
    onPalette(): void;
    onBell(): void;
}) {
    const status = statusFromQuery(useStatusQuery());
    return (
        <div className="app">
            <TopBar
                onPalette={onPalette}
                onBell={onBell}
                status={status}
            />
            <Sidebar
                active={active}
                onNav={onNav}
            />
            <main className="main">{children}</main>
            <Footer status={status} />
        </div>
    );
}
