import type { ReactNode } from 'react';

interface CardFrameProps {
    title: string;
    meta?: string | undefined;
    children: ReactNode;
}

export function CardFrame({ title, meta, children }: CardFrameProps) {
    return (
        <div className="card">
            <div className="card-head">
                <span>{title}</span>
                {meta ? <span className="card-meta">{meta}</span> : null}
            </div>
            {children}
        </div>
    );
}
