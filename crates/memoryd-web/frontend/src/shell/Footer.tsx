import { StatusDot } from '../ui';
export function Footer({ chordPrefix }: { chordPrefix?: string | null }) {
    return (
        <footer className="footer">
            <span className="vital">
                <StatusDot /> daemon
            </span>
            <span className="vital">
                <StatusDot /> sync · 2 peers
            </span>
            <div className="right">
                {chordPrefix ? (
                    <span
                        className="chord-indicator mono"
                        aria-live="polite"
                    >
                        <kbd>{chordPrefix}</kbd> …
                    </span>
                ) : null}
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
