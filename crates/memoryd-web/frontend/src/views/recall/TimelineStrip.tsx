export interface TimelineBucket {
    key: number;
    label: string;
    count: number;
}

export function TimelineStrip({ buckets, mode, selected, onPick }: { buckets: TimelineBucket[]; mode: '24h' | '30d'; selected: number | null; onPick: (key: number) => void }) {
    const max = Math.max(1, ...buckets.map((bucket) => bucket.count));
    return (
        <div className="rl-strip">
            <div className="rl-strip-head">
                <span className="section-label">{mode === '24h' ? '24-hour scrubber' : '30-day scrubber'}</span>
                <span className="rl-strip-meta">peak {max}/{mode === '24h' ? 'hr' : 'day'}</span>
            </div>
            <div
                className="rl-strip-bars"
                role="group"
                aria-label="Timeline"
            >
                {buckets.map((bucket) => {
                    const height = Math.max(2, Math.round((bucket.count / max) * 56));
                    return (
                        <button
                            key={bucket.key}
                            className={`rl-bar ${selected === bucket.key ? 'selected' : ''}`}
                            style={{ height }}
                            title={`${bucket.label} — ${bucket.count} recalls`}
                            onClick={() => onPick(bucket.key)}
                            type="button"
                        >
                            <span className="rl-bar-cap" />
                        </button>
                    );
                })}
            </div>
            <div className="rl-strip-axis">
                {mode === '24h' ? (
                    <>
                        <span>00</span>
                        <span>06</span>
                        <span>12</span>
                        <span>18</span>
                        <span>23</span>
                    </>
                ) : (
                    <>
                        <span>30d ago</span>
                        <span>21d</span>
                        <span>14d</span>
                        <span>7d</span>
                        <span>today</span>
                    </>
                )}
            </div>
        </div>
    );
}
