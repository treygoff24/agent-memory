import { useEffect, useRef, useState } from 'react';

const sequenceTimeoutMs = 900;

export function isTextInputTarget(target: unknown): boolean {
    if (!(target instanceof HTMLElement)) return false;
    if (target.isContentEditable) return true;
    return ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName);
}

function sequenceKey(event: KeyboardEvent): string {
    return event.key.length === 1 ? event.key.toLowerCase() : event.key;
}

export function useKeymap(handler: (key: string) => void): { chordPrefix: string | null } {
    const prefixRef = useRef<string | null>(null);
    const timeoutRef = useRef<number | null>(null);
    // Mirror the prefix-ref into render state so the Footer can render a
    // "g …" indicator while the chord is armed. Internal `prefixRef` stays
    // as the source of truth for the keyboard handler.
    const [chordPrefix, setChordPrefix] = useState<string | null>(null);

    useEffect(() => {
        const clearPrefix = () => {
            const wasArmed = prefixRef.current !== null;
            prefixRef.current = null;
            if (timeoutRef.current !== null) window.clearTimeout(timeoutRef.current);
            timeoutRef.current = null;
            // Avoid a setter call (and the React 18 bail-out path) when the
            // chord was already idle; every non-chord keypress hits this path.
            if (wasArmed) setChordPrefix(null);
        };

        const listener = (event: KeyboardEvent) => {
            if (isTextInputTarget(event.target)) return;
            if (event.metaKey || event.ctrlKey || event.altKey) return;

            const key = sequenceKey(event);
            if (prefixRef.current === 'g') {
                event.preventDefault();
                clearPrefix();
                handler(`g${key}`);
                return;
            }

            if (key === 'g') {
                event.preventDefault();
                prefixRef.current = 'g';
                setChordPrefix('g');
                if (timeoutRef.current !== null) window.clearTimeout(timeoutRef.current);
                timeoutRef.current = window.setTimeout(clearPrefix, sequenceTimeoutMs);
                return;
            }

            clearPrefix();
            if (key === ':' || key === '?' || key === 'Escape') event.preventDefault();
            handler(event.key);
        };
        window.addEventListener('keydown', listener);
        return () => {
            clearPrefix();
            window.removeEventListener('keydown', listener);
        };
    }, [handler]);

    return { chordPrefix };
}
