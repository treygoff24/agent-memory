import { AnswerCards } from './AnswerCards';
import { CorrectEditor } from './CorrectEditor';
import { ScoreBreakdown } from './ScoreBreakdown';
import type { RealityCheckActionName, RealityCheckMemory, RealityCheckVariant } from './types';

interface QuestionStageProps {
    memory: RealityCheckMemory;
    mode: 'answer' | 'correct';
    variant: RealityCheckVariant;
    onAction: (action: RealityCheckActionName, correction?: string) => void;
    onCorrectMode: () => void;
    onCancelCorrect: () => void;
}

export function QuestionStage({ memory, mode, variant, onAction, onCorrectMode, onCancelCorrect }: QuestionStageProps) {
    const encrypted = variant === 'encrypted' || memory.encrypted;
    const refused = variant === 'refused';
    return (
        <div className="rc-card">
            <div className="rc-scope-line">
                <span className="scope">{memory.namespace}</span>
                <span className="sep">·</span>
                <span>written {memory.written}</span>
                <span className="sep">·</span>
                <span>last verified {memory.last_verified_days}d</span>
            </div>
            <h2 className="rc-question">{memory.question}</h2>

            {mode === 'correct' ? (
                <CorrectEditor
                    initialBody={memory.think}
                    onCancel={onCancelCorrect}
                    onSubmit={(body) => onAction('correct', body)}
                />
            ) : encrypted ? (
                <div className="rc-think rc-think--encrypted">
                    <div className="head">What memorum thinks</div>
                    <div className="body">
                        <span className="enc-glyph">⌬</span>
                        <span>encrypted memory · score {memory.score.toFixed(2)}</span>
                    </div>
                    <div className="enc-help">reveal externally to confirm/correct — body is sealed in this surface</div>
                </div>
            ) : (
                <div className="rc-think">
                    <div className="head">What memorum thinks</div>
                    <div className="body">{memory.think}</div>
                    <div className="source">Source: {memory.source}</div>
                </div>
            )}

            {mode === 'answer' && refused ? (
                <div className="rc-refused">
                    <div className="head">Refused</div>
                    <div className="body">
                        This memory cannot be confirmed because{' '}
                        <span className="reason">
                            a tombstone in the personal/family namespace blocks mutations on entities tagged minor.
                        </span>
                    </div>
                    <div className="rc-refused-meta">policy_id family.minor.no_mutate · trace_id pdt_20260507_8f2a</div>
                    <button
                        className="rc-action"
                        onClick={() => onAction('not_relevant')}
                        type="button"
                    >
                        <span className="key">n</span>
                        <span>Next item</span>
                        <span className="desc">skip and continue</span>
                    </button>
                </div>
            ) : null}

            {mode === 'answer' && !refused ? (
                <AnswerCards
                    encrypted={Boolean(encrypted)}
                    onAction={onAction}
                    onCorrect={onCorrectMode}
                />
            ) : null}

            <ScoreBreakdown
                memory={memory}
                defaultOpen={variant === 'score-open'}
            />
        </div>
    );
}
