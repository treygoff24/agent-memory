import { useStatusQuery } from '../../api';

export function AboutTab() {
    const status = useStatusQuery();

    const daemonVersion = status.data?.daemon.version ?? '—';
    const socketPath = status.data?.socket ?? '—';

    return (
        <section
            className="card settings-card"
            aria-labelledby="about-heading"
        >
            <div className="card-head">
                <span id="about-heading">About</span>
            </div>
            <dl className="settings-about">
                <div>
                    <dt>Product</dt>
                    <dd>Memorum</dd>
                </div>
                <div>
                    <dt>Dashboard</dt>
                    <dd>memoryd-web · React + Vite + TanStack Query</dd>
                </div>
                <div>
                    <dt>Daemon version</dt>
                    <dd className="mono">{daemonVersion}</dd>
                </div>
                <div>
                    <dt>Daemon socket</dt>
                    <dd className="mono">{socketPath}</dd>
                </div>
                <div>
                    <dt>Docs</dt>
                    <dd>
                        <a
                            href="/docs"
                            target="_blank"
                            rel="noreferrer"
                        >
                            Local operator docs
                        </a>
                    </dd>
                </div>
            </dl>
        </section>
    );
}
