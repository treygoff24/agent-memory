import { useMemo, useState } from 'react';

import { useTheme } from '../theme';
import { AboutTab } from './settings/AboutTab';
import { AppearanceTab } from './settings/AppearanceTab';
import { KeyboardTab } from './settings/KeyboardTab';
import { NotificationsTab } from './settings/NotificationsTab';
import { PolicyEditorTab } from './settings/PolicyEditorTab';
import { ThemeEditorTab } from './settings/ThemeEditorTab';

const settingsTabs = [
    { id: 'appearance', label: 'Appearance' },
    { id: 'theme-editor', label: 'Theme editor' },
    { id: 'keyboard', label: 'Keyboard' },
    { id: 'notifications', label: 'Notifications' },
    { id: 'policies', label: 'Policies' },
    { id: 'about', label: 'About' },
] as const;

type SettingsTabId = (typeof settingsTabs)[number]['id'];

function tabFromSearchParams(): SettingsTabId {
    const value = new URLSearchParams(window.location.search).get('settingsTab');
    return settingsTabs.some((tab) => tab.id === value) ? (value as SettingsTabId) : 'appearance';
}

export function Settings() {
    const { preferences, setTheme, setDensity, setReducedMotion, setFontSize } = useTheme();
    const tweaksEnabled = new URLSearchParams(window.location.search).get('tweaks') === '1';
    const [activeTab, setActiveTab] = useState<SettingsTabId>(() => tabFromSearchParams());
    const panelId = useMemo(() => `settings-panel-${activeTab}`, [activeTab]);

    return (
        <>
            <div className="view-header">
                <span className="view-title">Settings</span>
                <span className="view-subtitle">· appearance · keyboard · notifications · policies</span>
            </div>
            <div className="settings-layout">
                {tweaksEnabled && (
                    <section
                        className="card settings-dev-panel"
                        aria-label="Dev tweaks"
                    >
                        <div className="card-head">
                            <span>Dev tweaks</span>
                        </div>
                        <p className="muted">Experimental dashboard controls are enabled by ?tweaks=1.</p>
                    </section>
                )}
                <div
                    className="settings-tabs"
                    role="tablist"
                    aria-label="Settings sections"
                >
                    {settingsTabs.map((tab) => (
                        <button
                            key={tab.id}
                            type="button"
                            role="tab"
                            aria-selected={activeTab === tab.id}
                            aria-controls={activeTab === tab.id ? panelId : undefined}
                            className={`settings-tab ${activeTab === tab.id ? 'active' : ''}`}
                            onClick={() => setActiveTab(tab.id)}
                        >
                            {tab.label}
                        </button>
                    ))}
                </div>
                <div
                    id={panelId}
                    role="tabpanel"
                    className="settings-panel"
                >
                    {activeTab === 'appearance' && (
                        <AppearanceTab
                            preferences={preferences}
                            setTheme={setTheme}
                            setDensity={setDensity}
                            setReducedMotion={setReducedMotion}
                            setFontSize={setFontSize}
                        />
                    )}
                    {activeTab === 'theme-editor' && <ThemeEditorTab />}
                    {activeTab === 'keyboard' && <KeyboardTab />}
                    {activeTab === 'notifications' && <NotificationsTab />}
                    {activeTab === 'policies' && <PolicyEditorTab />}
                    {activeTab === 'about' && <AboutTab />}
                </div>
            </div>
        </>
    );
}
