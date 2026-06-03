import type { ShellStatus } from './types';

import { StatusDot } from '../ui';

export function Footer({ status }: { status: ShellStatus }) {
    return (
        <footer className="footer">
            <span className="vital">
                <StatusDot kind={status.daemon} /> daemon
            </span>
            <span className="vital">
                <StatusDot kind={status.daemon === 'bad' ? 'idle' : 'ok'} /> {status.peerLabel}
            </span>
            <div className="right">
                <span>
                    <kbd>:</kbd>palette
                </span>
                <span>
                    <kbd>?</kbd>help
                </span>
            </div>
        </footer>
    );
}
