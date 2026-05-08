import { useState } from 'react';

export function CorrectEditor({ initialBody, onCancel, onSubmit }: { initialBody: string; onCancel: () => void; onSubmit: (body: string) => void }) {
    const [body, setBody] = useState(initialBody);
    return (
        <div className="rc-think">
            <div className="head">Correct memory</div>
            <label htmlFor="corrected-memory-body" className="sr-only">
                Corrected memory body
            </label>
            <textarea
                id="corrected-memory-body"
                aria-label="Corrected memory body"
                value={body}
                onChange={(event) => setBody(event.target.value)}
                rows={6}
                style={{ width: '100%', resize: 'vertical' }}
            />
            <div className="rc-actions">
                <button
                    className="rc-action primary"
                    onClick={() => onSubmit(body)}
                    type="button"
                >
                    <span className="key">↵</span>
                    <span>Save correction</span>
                    <span className="desc">dispatch Correct new_body</span>
                </button>
                <button
                    className="rc-action"
                    onClick={onCancel}
                    type="button"
                >
                    <span className="key">esc</span>
                    <span>Cancel</span>
                    <span className="desc">return to choices</span>
                </button>
            </div>
        </div>
    );
}
