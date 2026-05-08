import type { ReactNode } from 'react';

import type { ViewId } from '../views';

import { Footer } from './Footer';
import { Sidebar } from './Sidebar';
import { TopBar } from './TopBar';
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
    return (
        <div className="app">
            <TopBar
                onPalette={onPalette}
                onBell={onBell}
            />
            <Sidebar
                active={active}
                onNav={onNav}
            />
            <main className="main">{children}</main>
            <Footer />
        </div>
    );
}
