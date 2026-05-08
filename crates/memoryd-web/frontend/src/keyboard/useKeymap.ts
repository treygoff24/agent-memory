import { useEffect } from 'react';
export function useKeymap(handler: (key: string) => void) {
    useEffect(() => {
        const listener = (event: KeyboardEvent) => {
            const target = event.target as HTMLElement | null;
            if (target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)) return;
            handler(event.key);
        };
        window.addEventListener('keydown', listener);
        return () => window.removeEventListener('keydown', listener);
    }, [handler]);
}
