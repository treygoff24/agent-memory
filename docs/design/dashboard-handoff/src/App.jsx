// App: top-level state, routing, tweaks integration
const { useState: useStateApp, useEffect: useEffectApp, useCallback: useCBApp, useMemo: useMemoApp } = React;

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/ {
    theme: 'warm-dark',
    density: 'comfortable',
    layout: 'two-pane',
    reducedMotion: 'respect-os',
    inspectorDensity: 'comfortable',
    dataVolume: 'typical',
    stateOverlay: 'none',
    rcVariant: 'default',
} /*EDITMODE-END*/;

function ThemeSwatchGrid({ value, onChange }) {
    const themes = [
        { id: 'warm-dark', bg: '#1f1c19', fg: '#e8d9c5', accent: '#f4a23a', label: 'warm-dark' },
        { id: 'warm-light', bg: '#f6f1ea', fg: '#2a2520', accent: '#c2680a', label: 'warm-light' },
        { id: 'high-contrast', bg: '#0a0907', fg: '#ffffff', accent: '#ffb547', label: 'hi-contrast' },
        { id: 'monochrome', bg: '#1a1a1a', fg: '#e6e6e6', accent: '#e6e6e6', label: 'monochrome' },
        { id: 'cool-dark', bg: '#161a1f', fg: '#d6dee8', accent: '#5fb3ff', label: 'cool-dark' },
        { id: 'cool-light', bg: '#eef2f6', fg: '#1d242c', accent: '#1f6fbf', label: 'cool-light' },
    ];
    return (
        <div
            className="tk-row"
            style={{ flexDirection: 'column', alignItems: 'stretch', gap: 6 }}
        >
            <div className="tk-label">Theme</div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(6, 1fr)', gap: 6 }}>
                {themes.map((th) => {
                    const sel = value === th.id;
                    return (
                        <button
                            key={th.id}
                            type="button"
                            title={th.label}
                            aria-label={th.label}
                            onClick={() => onChange(th.id)}
                            style={{
                                appearance: 'none',
                                border: 'none',
                                padding: 0,
                                cursor: 'pointer',
                                height: 30,
                                borderRadius: 4,
                                background: th.bg,
                                position: 'relative',
                                outline: sel ? `1.5px solid var(--accent)` : '1px solid var(--border-soft)',
                                outlineOffset: sel ? 1 : 0,
                            }}
                        >
                            <span
                                style={{
                                    position: 'absolute',
                                    left: 3,
                                    top: 3,
                                    width: 6,
                                    height: 6,
                                    background: th.fg,
                                    borderRadius: 1,
                                }}
                            />
                            <span
                                style={{
                                    position: 'absolute',
                                    right: 3,
                                    bottom: 3,
                                    width: 8,
                                    height: 8,
                                    background: th.accent,
                                    borderRadius: 1,
                                }}
                            />
                        </button>
                    );
                })}
            </div>
            <div
                className="tk-hint"
                style={{ fontSize: 10, color: 'var(--fg-4)', fontFamily: 'var(--font-mono)' }}
            >
                {value}
            </div>
        </div>
    );
}

function App() {
    const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
    const [view, setView] = useStateApp('inbox');
    // Per-view selection state — local to each view, no leakage
    const [selectedByView, setSelectedByView] = useStateApp({
        inbox: 'mem_20260507_a1b2c3d4e5f60718_000010',
        recall: null,
        dreams: null,
        peers: null,
        governance: null,
        entities: null,
    });
    const selectedId = selectedByView[view];
    function setSelectedId(id) {
        setSelectedByView((s) => ({ ...s, [view]: id }));
    }
    const [paletteOpen, setPaletteOpen] = useStateApp(false);
    const [bellOpen, setBellOpen] = useStateApp(false);
    const [drawerOpen, setDrawerOpen] = useStateApp(false);
    const [modalOpen, setModalOpen] = useStateApp(false);
    const [toasts, setToasts] = useStateApp([]);

    // Apply theme + density + reduced motion to <html>
    useEffectApp(() => {
        const r = document.documentElement;
        r.dataset.theme = t.theme;
        r.dataset.density = t.density;
        r.dataset.inspDensity = t.inspectorDensity;
        if (t.reducedMotion === 'force-on') r.dataset.reducedMotion = 'on';
        else if (t.reducedMotion === 'force-off') r.dataset.reducedMotion = 'off';
        else
            r.dataset.reducedMotion =
                window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'on' : 'off';
    }, [t.theme, t.density, t.inspectorDensity, t.reducedMotion]);

    // Items based on volume
    const items = t.dataVolume === 'heavy' ? MEMORUM_DATA.inboxItemsHeavy : MEMORUM_DATA.inboxItemsTypical;
    const empty = t.stateOverlay === 'empty-inbox';
    const daemonDown = t.stateOverlay === 'daemon-down';
    const csrfToast = t.stateOverlay === 'csrf';

    useEffectApp(() => {
        if (t.stateOverlay === 'palette') setPaletteOpen(true);
        else if (t.stateOverlay === 'bell') setBellOpen(true);
        else if (t.stateOverlay === 'csrf') {
            pushToast({
                kind: 'warn',
                title: 'Mutation refused (403)',
                msg: 'CSRF token expired. Refresh and retry — your selection is preserved.',
                action: { label: 'Refresh token', onClick: () => dismissToast() },
            });
        } else {
            setPaletteOpen(false);
            setBellOpen(false);
        }
    }, [t.stateOverlay]);

    function pushToast(tt) {
        const id = Date.now() + Math.random();
        setToasts((ts) => [...ts, { ...tt, id }]);
        setTimeout(() => setToasts((ts) => ts.filter((x) => x.id !== id)), 6000);
    }
    function dismissToast(id) {
        setToasts((ts) => (id == null ? ts.slice(1) : ts.filter((x) => x.id !== id)));
    }

    function selectItem(id) {
        setSelectedId(id);
        if (t.layout === 'drawer') setDrawerOpen(true);
        if (t.layout === 'modal') setModalOpen(true);
    }

    function navTo(v) {
        setView(v);
    }

    // Keyboard
    useEffectApp(() => {
        function k(e) {
            const tag = (e.target?.tagName || '').toLowerCase();
            if (tag === 'input' || tag === 'textarea') return;
            if (e.key === ':') {
                e.preventDefault();
                setPaletteOpen(true);
            } else if (e.key === 'Escape') {
                setPaletteOpen(false);
                setBellOpen(false);
                setDrawerOpen(false);
                setModalOpen(false);
                if (view === 'reality') setView('inbox');
            } else if (e.key === '?') {
                /* could open help */
            }
        }
        window.addEventListener('keydown', k);
        return () => window.removeEventListener('keydown', k);
    }, [view]);

    function runCommand(c) {
        setPaletteOpen(false);
        if (c.id.startsWith('go-')) {
            const map = {
                'go-inbox': 'inbox',
                'go-reality': 'reality',
                'go-recall': 'recall',
                'go-dreams': 'dreams',
                'go-peers': 'peers',
            };
            setView(map[c.id] || 'inbox');
        } else if (c.id.startsWith('theme-')) {
            const map = {
                'theme-wd': 'warm-dark',
                'theme-wl': 'warm-light',
                'theme-hc': 'high-contrast',
                'theme-mono': 'monochrome',
            };
            setTweak('theme', map[c.id]);
        } else if (c.id === 'den-com') setTweak('density', 'comfortable');
        else if (c.id === 'den-cpt') setTweak('density', 'compact');
        else if (c.id === 'act-approve')
            pushToast({ kind: 'ok', title: 'Memory accepted', msg: 'Promoted to active. Provenance chain extended.' });
        else if (c.id === 'act-reject')
            pushToast({ kind: 'warn', title: 'Memory rejected', msg: 'Quarantined for manual review.' });
    }

    function actOnSelected(action) {
        if (action === 'approve') pushToast({ kind: 'ok', title: 'Memory accepted', msg: 'Promoted to active.' });
        else if (action === 'reject') pushToast({ kind: 'warn', title: 'Memory rejected', msg: 'Sent to quarantine.' });
        else if (action === 'forget')
            pushToast({ kind: 'warn', title: 'Memory forgotten', msg: 'Tombstoned. No further recall.' });
        else if (action === 'edit')
            pushToast({ title: 'Editor opened', msg: 'Body opened in $EDITOR — return when done.' });
    }

    const fullbleed = view === 'reality';

    return (
        <div className={'app' + (fullbleed ? ' fullbleed' : '')}>
            <TopBar
                view={view}
                onPalette={() => setPaletteOpen(true)}
                onBell={() => setBellOpen((b) => !b)}
                bellOpen={bellOpen}
                daemon={daemonDown ? 'down' : 'ok'}
                fullbleed={fullbleed}
            />
            {!fullbleed && (
                <Sidebar
                    active={view}
                    onNav={navTo}
                />
            )}
            <div className="main">
                {daemonDown && (
                    <Banner
                        label="daemon down"
                        msg="memoryd unreachable on 127.0.0.1:7137. Mutations disabled. Retrying every 5s."
                        actions={[{ label: 'Retry now', onClick: () => {} }]}
                    />
                )}

                {view === 'inbox' && !empty && (
                    <InboxView
                        items={items}
                        layout={
                            t.layout === 'two-pane'
                                ? 'two'
                                : t.layout === 'three-pane'
                                  ? 'three'
                                  : t.layout === 'modal'
                                    ? 'modal'
                                    : 'drawer'
                        }
                        selectedId={selectedId}
                        onSelect={selectItem}
                        drawerOpen={drawerOpen}
                        onCloseDrawer={() => setDrawerOpen(false)}
                        modalOpen={false}
                        onCloseModal={() => setModalOpen(false)}
                        onAct={actOnSelected}
                    />
                )}

                {view === 'inbox' && empty && (
                    <>
                        <div className="view-header">
                            <span className="view-title">Inbox</span>
                            <span className="view-subtitle">· 0 items</span>
                            <span className="spacer" />
                        </div>
                        <div
                            className="empty"
                            style={{ paddingTop: 120 }}
                        >
                            <span className="ico">○</span>
                            <h3>Inbox is clear.</h3>
                            <p>
                                All review items processed. Last activity: 2 hours ago. Reality Check next due in 3
                                days.
                            </p>
                        </div>
                    </>
                )}

                {view === 'reality' && (
                    <RealityCheck
                        session={MEMORUM_DATA.realityCheckSession}
                        variant={t.rcVariant}
                        onExit={() => setView('inbox')}
                        onRespond={(action) => {
                            pushToast({
                                kind: action === 'confirm' ? 'ok' : 'warn',
                                title: 'Reality check · ' + action,
                                msg: "Maeve's school name — recorded. 9 items remaining.",
                            });
                        }}
                    />
                )}

                {view === 'recall' && (
                    <RecallView
                        events={MEMORUM_DATA.recallEventsTypical}
                        dayBuckets={MEMORUM_DATA.recallDayBuckets}
                        hourBuckets={MEMORUM_DATA.recallHourBuckets}
                        heavy={t.dataVolume === 'heavy'}
                        selectedId={selectedId}
                        onSelect={(id) => setSelectedId(id)}
                    />
                )}
                {view === 'dreams' && (
                    <DreamsView
                        items={MEMORUM_DATA.dreamItems}
                        selectedId={selectedId}
                        onSelect={(id) => setSelectedId(id)}
                        onAct={actOnSelected}
                    />
                )}
                {view === 'peers' && (
                    <PeersView
                        peers={MEMORUM_DATA.peers}
                        selectedId={selectedId}
                        onSelect={(id) => setSelectedId(id)}
                    />
                )}
                {view === 'governance' && (
                    <GovernanceView
                        items={MEMORUM_DATA.governanceItems}
                        selectedId={selectedId}
                        onSelect={(id) => setSelectedId(id)}
                        onAct={actOnSelected}
                    />
                )}
                {view === 'entities' && (
                    <EntitiesView
                        entities={MEMORUM_DATA.entities}
                        selectedId={selectedId}
                        onSelect={(id) => setSelectedId(id)}
                    />
                )}
                {view === 'settings' && (
                    <>
                        <div className="view-header">
                            <span className="view-title">Settings</span>
                            <span className="view-subtitle">· tweaks panel made permanent</span>
                        </div>
                        <div
                            className="empty"
                            style={{ paddingTop: 80 }}
                        >
                            <span className="ico">○</span>
                            <h3>Settings inherits the Tweaks panel</h3>
                            <p>Same controls, made permanent. Use the floating Tweaks panel for now.</p>
                        </div>
                    </>
                )}
            </div>
            <Footer
                view={view}
                daemon={daemonDown ? 'down' : 'ok'}
            />

            <CommandPalette
                open={paletteOpen}
                onClose={() => setPaletteOpen(false)}
                commands={MEMORUM_DATA.commands}
                onRun={runCommand}
            />

            <NotificationDropdown
                open={bellOpen}
                onClose={() => setBellOpen(false)}
                items={MEMORUM_DATA.notifications}
                onAction={(n) => {
                    setBellOpen(false);
                    if (n.action.route === 'reality') setView('reality');
                    else if (n.action.route === 'inbox') setView('inbox');
                }}
            />

            <div className="toast-stack">
                {toasts.map((tt) => (
                    <Toast
                        key={tt.id}
                        toast={tt}
                        onDismiss={() => dismissToast(tt.id)}
                    />
                ))}
            </div>

            <TweaksRoot
                t={t}
                setTweak={setTweak}
            />
        </div>
    );
}

function TweaksRoot({ t, setTweak }) {
    return (
        <TweaksPanel
            title="Tweaks"
            defaultPosition="bottom-right"
        >
            <TweakSection label="Theme">
                <ThemeSwatchGrid
                    value={t.theme}
                    onChange={(v) => setTweak('theme', v)}
                />
                <div style={{ display: 'none' }}>
                    <TweakSelect
                        label="Theme"
                        value={t.theme}
                        options={[
                            { value: 'warm-dark', label: 'warm-dark (default)' },
                            { value: 'warm-light', label: 'warm-light' },
                            { value: 'high-contrast', label: 'high-contrast' },
                            { value: 'monochrome', label: 'monochrome' },
                            { value: 'cool-dark', label: 'cool-dark' },
                            { value: 'cool-light', label: 'cool-light' },
                        ]}
                        onChange={(v) => setTweak('theme', v)}
                    />
                </div>
            </TweakSection>

            <TweakSection label="Layout">
                <TweakRadio
                    label="Density"
                    value={t.density}
                    options={[
                        { value: 'comfortable', label: 'comfy' },
                        { value: 'compact', label: 'compact' },
                    ]}
                    onChange={(v) => setTweak('density', v)}
                />
                <TweakRadio
                    label="Inspector density"
                    value={t.inspectorDensity}
                    options={[
                        { value: 'comfortable', label: 'comfy' },
                        { value: 'dense', label: 'dense' },
                    ]}
                    onChange={(v) => setTweak('inspectorDensity', v)}
                />
                <TweakSelect
                    label="Inbox layout"
                    value={t.layout}
                    options={[
                        { value: 'two-pane', label: 'two-pane (list + rail)' },
                        { value: 'three-pane', label: 'three-pane (filters + list + rail)' },
                        { value: 'drawer', label: 'drawer (list full · drawer overlay)' },
                        { value: 'modal', label: 'modal (list full · modal sheet)' },
                    ]}
                    onChange={(v) => setTweak('layout', v)}
                />
                <TweakRadio
                    label="Reduced motion"
                    value={t.reducedMotion}
                    options={[
                        { value: 'respect-os', label: 'OS' },
                        { value: 'force-on', label: 'on' },
                        { value: 'force-off', label: 'off' },
                    ]}
                    onChange={(v) => setTweak('reducedMotion', v)}
                />
            </TweakSection>

            <TweakSection label="State">
                <TweakSelect
                    label="Data volume"
                    value={t.dataVolume}
                    options={[
                        { value: 'typical', label: 'typical (12 items)' },
                        { value: 'heavy', label: 'heavy (29 items)' },
                    ]}
                    onChange={(v) => setTweak('dataVolume', v)}
                />
                <TweakSelect
                    label="Surface state"
                    value={t.stateOverlay}
                    options={[
                        { value: 'none', label: 'happy path' },
                        { value: 'empty-inbox', label: 'empty inbox' },
                        { value: 'daemon-down', label: 'daemon-down banner' },
                        { value: 'csrf', label: 'CSRF / 403 toast' },
                        { value: 'palette', label: 'command palette open' },
                        { value: 'bell', label: 'notification bell open' },
                    ]}
                    onChange={(v) => setTweak('stateOverlay', v)}
                />
                <TweakSelect
                    label="Reality Check variant"
                    value={t.rcVariant}
                    options={[
                        { value: 'default', label: 'default · score collapsed' },
                        { value: 'score-open', label: 'score breakdown expanded' },
                        { value: 'encrypted', label: 'encrypted memory · confirm disabled' },
                        { value: 'refused', label: 'refused by policy / tombstone' },
                        { value: 'complete', label: 'session complete · 12 of 12' },
                    ]}
                    onChange={(v) => setTweak('rcVariant', v)}
                />
            </TweakSection>
        </TweaksPanel>
    );
}

window.App = App;
