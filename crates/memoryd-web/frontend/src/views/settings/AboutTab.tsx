export function AboutTab() {
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
                    <dd>Memorum Dashboard embedded in memoryd-web</dd>
                </div>
                <div>
                    <dt>Frontend stack</dt>
                    <dd>React, Vite, TanStack Query, Playwright</dd>
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
