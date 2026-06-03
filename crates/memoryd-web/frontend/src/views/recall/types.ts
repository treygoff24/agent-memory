export interface RecallLedgerEvent {
    id: string;
    seq: number;
    isoTime: string;
    time: string;
    device: string;
    agent: string;
    memory: string;
    namespace: string;
    score: number | null;
    latencyMs: number | null;
    session: string;
}
