import { useEffect, useMemo, useState } from 'react';

import { apiErrorBody } from '../../api/errorPresentation';
import { usePolicyEditorMutation } from '../../api/mutations';
import { usePolicyEditorQuery } from '../../api/queries';
import { QueryErrorBanner } from '../QueryFeedback';

export function PolicyEditorTab() {
    const policyQuery = usePolicyEditorQuery();
    const policyMutation = usePolicyEditorMutation();
    const [rawYaml, setRawYaml] = useState('');
    const [fileName, setFileName] = useState('');
    const [message, setMessage] = useState<string | null>(null);

    useEffect(() => {
        if (!policyQuery.data) return;
        setRawYaml(policyQuery.data.raw_yaml);
        setFileName(policyQuery.data.current_file ?? policyQuery.data.files[0] ?? 'project-standard.yaml');
    }, [policyQuery.data]);

    const summaries = policyQuery.data?.policies ?? [];
    const saveDisabled = !policyQuery.data?.writable || rawYaml.trim().length === 0 || policyMutation.isPending;
    const sourceLabel = useMemo(() => {
        if (!policyQuery.data) return 'loading';
        return policyQuery.data.writable
            ? `${policyQuery.data.source} · writable`
            : `${policyQuery.data.source} · read-only`;
    }, [policyQuery.data]);

    async function savePolicy() {
        setMessage(null);
        try {
            await policyMutation.mutateAsync({ raw_yaml: rawYaml, file_name: fileName });
            setMessage('Policy saved.');
        } catch (error) {
            setMessage(apiErrorBody(error));
        }
    }

    return (
        <section
            className="card settings-card"
            aria-labelledby="policies-heading"
        >
            <div className="card-head">
                <span id="policies-heading">Policies</span>
                <span className="muted">{sourceLabel}</span>
            </div>
            <p className="muted">
                Edit daemon-owned governance YAML. Saves validate the complete policy set before replacing a file.
            </p>
            <QueryErrorBanner
                error={policyQuery.error}
                label="Policy editor"
            />
            <div className="settings-form-grid">
                <label className="settings-field">
                    <span>Policy file</span>
                    <input
                        aria-label="Policy file"
                        value={fileName}
                        list="policy-files"
                        onChange={(event) => setFileName(event.target.value)}
                    />
                    <datalist id="policy-files">
                        {(policyQuery.data?.files ?? []).map((file) => (
                            <option
                                key={file}
                                value={file}
                            />
                        ))}
                    </datalist>
                </label>
                <label className="settings-field">
                    <span>Policy YAML</span>
                    <textarea
                        aria-label="Policy YAML"
                        rows={14}
                        value={rawYaml}
                        onChange={(event) => setRawYaml(event.target.value)}
                    />
                </label>
            </div>
            <div className="action-bar">
                <button
                    type="button"
                    className="btn primary"
                    disabled={saveDisabled}
                    onClick={savePolicy}
                >
                    {policyMutation.isPending ? 'Saving…' : 'Save policy'}
                </button>
                {message && <span role="status">{message}</span>}
            </div>
            <div
                className="settings-table"
                role="table"
                aria-label="Policy summaries"
            >
                {summaries.map((policy) => (
                    <div
                        className="settings-table-row"
                        role="row"
                        key={`${policy.scope}-${policy.selected_policy}`}
                    >
                        <span role="cell">{policy.scope}</span>
                        <span role="cell">{policy.selected_policy}</span>
                        <span role="cell">{policy.policy_source ?? 'unknown'}</span>
                    </div>
                ))}
            </div>
        </section>
    );
}
