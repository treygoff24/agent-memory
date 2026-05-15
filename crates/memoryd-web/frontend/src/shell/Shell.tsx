import type { ReactNode } from 'react';

import type { ViewId } from '../views';

import { Footer } from './Footer';
import { Sidebar } from './Sidebar';
import { TopBar } from './TopBar';
export function Shell({
    active,
    children,
    fullbleed = false,
    chordPrefix,
    onNav,
    onPalette,
    onBell,
}: {
    active: ViewId;
    children: ReactNode;
    fullbleed?: boolean;
    chordPrefix?: string | null;
    onNav(id: ViewId): void;
    onPalette(): void;
    onBell(): void;
}) {
    return (
        <div className={fullbleed ? 'app fullbleed' : 'app'}>
            {/* Skip-to-main-content for keyboard + screen-reader users.
                Hidden until focused; lands focus on the main region. */}
            <a
                className="skip-link"
                href="#main"
            >
                Skip to main content
            </a>
            <TopBar
                onPalette={onPalette}
                onBell={onBell}
            />
            <Sidebar
                active={active}
                onNav={onNav}
            />
            <main
                className="main"
                id="main"
                tabIndex={-1}
            >
                {children}
            </main>
            <Footer chordPrefix={chordPrefix ?? null} />
        </div>
    );
}
