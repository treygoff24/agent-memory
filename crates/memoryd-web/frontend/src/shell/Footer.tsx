import { StatusDot } from '../ui';
export function Footer() {
    return (
        <footer className="footer">
            <span className="vital">
                <StatusDot /> daemon
            </span>
            <span className="vital">
                <StatusDot /> sync · 2 peers
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
