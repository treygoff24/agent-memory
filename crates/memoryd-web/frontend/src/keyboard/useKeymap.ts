import { useEffect, useRef } from 'react';

const sequenceTimeoutMs = 900;

export function isTextInputTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    if (target.isContentEditable) return true;
    return ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName);
}

function sequenceKey(event: KeyboardEvent): string {
    return event.key.length === 1 ? event.key.toLowerCase() : event.key;
}

export function useKeymap(handler: (key: string) => void) {
    const prefixRef = useRef<string | null>(null);
    const timeoutRef = useRef<number | null>(null);

    useEffect(() => {
        const clearPrefix = () => {
            prefixRef.current = null;
            if (timeoutRef.current !== null) window.clearTimeout(timeoutRef.current);
            timeoutRef.current = null;
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
}
