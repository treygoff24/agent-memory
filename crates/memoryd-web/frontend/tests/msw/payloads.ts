export const apiScenarioNames = [
    'happy',
    'empty',
    'heavy',
    'error',
    'forbidden403',
    'conflict409',
    'unavailable503',
] as const;
export type ApiScenario = (typeof apiScenarioNames)[number];

export const apiRouteIds = [
    'GET /api/status',
    'GET /api/entity-graph',
    'GET /api/entity-graph/:entity_id',
    'GET /api/roi',
    'GET /api/reality-check',
    'GET /api/reality-check/history',
    'POST /api/reality-check/respond',
    'GET /api/recall-hits',
    'GET /api/audit/:id',
    'GET /api/audit/:id/walk',
    'GET /api/audit/:id/temporal',
    'GET /api/review',
    'POST /api/review/action',
    'GET /api/notifications/stream',
    'GET /api/policy-editor',
    'POST /api/policy-editor',
    'GET /api/sync-dashboard',
] as const;

export interface ApiMockResponse {
    status: number;
    contentType: string;
    body: string;
}

const now = '2026-05-08T14:00:00Z';
const earlier = '2026-05-08T03:04:00Z';

function jsonResponse(body: unknown, status = 200): ApiMockResponse {
    return { status, contentType: 'application/json', body: JSON.stringify(body) };
}

function scenarioError(scenario: ApiScenario, route: string): ApiMockResponse | null {
    if (scenario === 'error')
        return jsonResponse({ error: 'mock_error', route, message: `${route} failed in error scenario` }, 500);
    if (scenario === 'forbidden403')
        return jsonResponse({ error: 'forbidden', route, message: 'Dashboard policy forbids this request.' }, 403);
    if (scenario === 'conflict409')
        return jsonResponse(
            {
                error: 'memory_not_in_review_state',
                route,
                message: 'The item changed state before the dashboard action landed.',
            },
            409,
        );
    if (scenario === 'unavailable503')
        return jsonResponse({ error: 'backend_unavailable', route, message: 'memoryd daemon is not reachable.' }, 503);
    return null;
}

function reviewItems(scenario: ApiScenario) {
    if (scenario === 'empty') return [];
    const base = [
        {
            id: 'mem_20260507_a1b2c3d4e5f60718_000010',
            summary: 'Project uses pnpm, never npm',
            status: 'candidate',
            namespace: 'coding/typescript',
            policy_applied: 'project-standard@v2',
            reason: 'review_required',
            next_actions: ['approve', 'reject', 'forget', 'quarantine'],
        },
        {
            id: 'mem_conflict_editor',
            summary: 'Editor preference disagreement',
            status: 'conflict',
            namespace: 'prefs/editor',
            policy_applied: 'merge-conflict@v1',
            reason: 'merge_conflict',
            next_actions: ['approve', 'reject'],
        },
        {
            id: 'mem_recall_evt_28f1a4',
            summary: 'Acme renewal date — Mar 14, 2026',
            status: 'recall',
            namespace: 'work/clients/acme',
            policy_applied: 'recall-ledger@v1',
            reason: 'recall_hit',
            next_actions: ['approve'],
        },
        {
            id: 'mem_due_school',
            summary: "Daughter's school name (verify)",
            status: 'due',
            namespace: 'personal/family',
            policy_applied: 'reality-check@v1',
            reason: 'stale_due',
            next_actions: ['approve', 'skip_this_week'],
        },
        {
            id: 'dream_run_1',
            summary: 'Nightly synthesis pass',
            status: 'running',
            namespace: 'dreams/runs',
            policy_applied: 'dreaming@v1',
            reason: 'dream_run',
            next_actions: ['approve'],
        },
        {
            id: 'dream_pattern_1',
            summary: 'Pattern: favors evidence-first reviews',
            status: 'proposed',
            namespace: 'dreams/patterns',
            policy_applied: 'dreaming@v1',
            reason: 'dream_pattern',
            next_actions: ['approve', 'reject'],
        },
        {
            id: 'dream_question_1',
            summary: 'Question: which laptop is primary now?',
            status: 'queued',
            namespace: 'dreams/questions',
            policy_applied: 'dreaming@v1',
            reason: 'dream_question',
            next_actions: ['approve', 'reject'],
        },
        {
            id: 'dream_cleanup_1',
            summary: 'Cleanup stale fragments older than 45d',
            status: 'completed',
            namespace: 'dreams/cleanup',
            policy_applied: 'dreaming@v1',
            reason: 'dream_cleanup',
            next_actions: ['approve'],
        },
        {
            id: 'dream_accepted_1',
            summary: 'Accepted: prefers evidence-first reviews',
            status: 'accepted',
            namespace: 'dreams/patterns',
            policy_applied: 'dreaming@v1',
            reason: 'dream_pattern',
            next_actions: ['approve'],
        },
        {
            id: 'dream_dismissed_1',
            summary: 'Dismissed: stale editor inference',
            status: 'dismissed',
            namespace: 'dreams/patterns',
            policy_applied: 'dreaming@v1',
            reason: 'dream_pattern',
            next_actions: ['reject'],
        },
        {
            id: 'gov_secret',
            summary: 'Possible secret in source capture',
            status: 'blocked',
            namespace: 'privacy/source',
            policy_applied: 'privacy.source.redaction',
            reason: 'redact_proposed',
            next_actions: ['reject', 'quarantine'],
        },
        {
            id: 'gov_low_conf',
            summary: 'Low confidence dream candidate',
            status: 'candidate',
            namespace: 'dreams/patterns',
            policy_applied: 'governance.review.human_required',
            reason: 'low_confidence',
            next_actions: ['approve', 'reject'],
        },
        {
            id: 'gov_info',
            summary: 'Grounding evidence attached',
            status: 'info',
            namespace: 'project:agent-memory',
            policy_applied: 'governance.review.info',
            reason: 'grounding_attached',
            next_actions: ['approve'],
        },
        {
            id: 'gov_consent_family',
            summary: 'Family detail consent required',
            status: 'candidate',
            namespace: 'personal/family',
            policy_applied: 'privacy.consent.family',
            reason: 'consent_required',
            next_actions: ['approve', 'reject'],
        },
    ];
    if (scenario !== 'heavy') return base;
    return [
        ...base,
        ...Array.from({ length: 160 }, (_, index) => ({
            id: `heavy_review_${index}`,
            summary: `Heavy review candidate ${index}`,
            status: index % 7 === 0 ? 'conflict' : 'candidate',
            namespace: index % 4 === 0 ? 'dreams/patterns' : 'project:agent-memory',
            policy_applied: 'project-standard@v2',
            reason: index % 4 === 0 ? 'dream_pattern' : 'review_required',
            next_actions: ['approve', 'reject'],
        })),
    ];
}

function statusPayload(scenario: ApiScenario) {
    return {
        daemon: { version: '0.1.0-test', pid: 7137, uptime_seconds: 302440 },
        socket: 'ok',
        index: { active_memories: scenario === 'empty' ? 0 : 1204, last_reindex: now },
        sync: {
            ahead: 2,
            behind: scenario === 'empty' ? 0 : 1,
            remote: 'git@github.com:trey/memory.git',
            last_push: now,
        },
        review: {
            candidate: reviewItems(scenario).length,
            quarantined: scenario === 'empty' ? 0 : 2,
            dream_low_confidence: scenario === 'empty' ? 0 : 2,
        },
        conflicts: scenario === 'empty' ? 0 : 1,
        active_sessions:
            scenario === 'empty'
                ? []
                : [
                      { harness: 'MacBook Pro', session_id: 'peer_mbp' },
                      { harness: 'Mac mini', session_id: 'peer_mini' },
                  ],
        dreaming: {
            status: scenario === 'empty' ? 'idle' : 'scheduled',
            next_run: '2026-05-09T03:00:00Z',
            last_run: { at: earlier, promoted: 3, queued: 1, dropped: 0 },
        },
        recall: { startup_total: 42, delta_total: 119, peer_update_total: 8 },
    };
}

function entityGraphPayload(scenario: ApiScenario) {
    if (scenario === 'empty') return { nodes: [], edges: [] };
    return {
        nodes: [
            {
                id: 'ent_agent_memory',
                label: 'agent-memory',
                kind: 'project',
                namespace: 'project:agent-memory',
                memory_count: 42,
            },
            {
                id: 'ent_pnpm',
                label: 'pnpm',
                kind: 'tool',
                namespace: 'coding/typescript',
                memory_count: 18,
            },
            {
                id: 'ent_rust',
                label: 'Rust',
                kind: 'language',
                namespace: 'coding/rust',
                memory_count: 31,
            },
            {
                id: 'ent_acme',
                label: 'Acme',
                kind: 'org',
                namespace: 'work/clients/acme',
                memory_count: 9,
            },
            {
                id: 'ent_operator',
                label: 'Operator',
                kind: 'person',
                namespace: 'me/preferences',
                memory_count: 12,
            },
            {
                id: 'ent_home_office',
                label: 'Home office',
                kind: 'place',
                namespace: 'personal/logistics',
                memory_count: 7,
            },
        ],
        edges: [
            {
                source: 'ent_agent_memory',
                target: 'ent_pnpm',
                kind: 'co_mentioned',
                weight: 0.84,
                temporal_from: '2026-04-01',
                temporal_to: null,
            },
            {
                source: 'ent_agent_memory',
                target: 'ent_rust',
                kind: 'co_mentioned',
                weight: 0.72,
                temporal_from: '2026-02-15',
                temporal_to: null,
            },
        ],
    };
}

function entityDetailPayload(pathname: string) {
    const entityId = pathname.split('/').pop() || 'ent_agent_memory';
    return {
        entity_id: entityId,
        label: entityId.replace(/^ent_/, '').replaceAll('_', ' '),
        mentions: [`${entityId}_mem_a`, `${entityId}_mem_b`],
        related_memories: [
            {
                id: `${entityId}_mem_a`,
                namespace: 'project:agent-memory',
                status: 'active',
                confidence: 0.92,
            },
            {
                id: `${entityId}_mem_b`,
                namespace: 'project:agent-memory',
                status: 'active',
                confidence: 0.78,
            },
        ],
        first_seen: '2026-03-01T00:00:00Z',
        last_seen: now,
        memories: [
            {
                id: `${entityId}_mem_a`,
                namespace: 'project:agent-memory',
                status: 'active',
                confidence: 0.92,
            },
        ],
        supersession_chain: [],
        recall_history: [{ at: now, count: 12 }],
    };
}

function realityCheckPayload(scenario: ApiScenario) {
    const count = scenario === 'empty' ? 0 : scenario === 'heavy' ? 50 : 5;
    return {
        kind: 'pending',
        session_id: 'rc_20260507_001',
        total_scored: count,
        last_completed_at: null,
        items: Array.from({ length: count }, (_, index) => ({
            memory_id: index === 0 ? 'mem_20260507_a1b2c3d4e5f60718_000010' : `mem_reality_${index}`,
            title: index === 0 ? 'Project uses pnpm, never npm' : `Reality check memory ${index}`,
            namespace: index === 1 ? 'personal/family' : 'coding/typescript',
            status: 'active',
            sensitivity: index === 1 ? 'sensitive' : null,
            score: 0.82 - index * 0.03,
            component_scores: {
                days_since_observed_norm: 0.72,
                recall_frequency_norm: 0.66,
                cross_source_corroboration: 0.55,
                confidence_decay: 0.41,
                sensitivity_weight: index === 1 ? 0.9 : 0.1,
            },
            encrypted: index === 1,
            last_observed_at: '2026-04-01T12:00:00Z',
            recall_count_30d: 4 + index,
            last_recalled_at: now,
        })),
    };
}

function recallHitsPayload(scenario: ApiScenario, url: globalThis.URL) {
    const requestedLimit = Number(url.searchParams.get('limit') ?? (scenario === 'heavy' ? 9000 : 80));
    const count =
        scenario === 'empty' ? 0 : Math.min(scenario === 'heavy' || requestedLimit > 500 ? requestedLimit : 80, 9000);
    const summaries = [
        'Project uses pnpm, never npm',
        'Editor preference disagreement',
        'Acme renewal date — Mar 14, 2026',
        "Daughter's school name (verify)",
        'Pattern: prefers Rust over Go for systems work',
    ];
    return {
        since: null,
        limit: requestedLimit,
        hits: Array.from({ length: count }, (_, index) => ({
            event_id: `recall_${index}`,
            device: index % 3 === 0 ? 'mbp' : index % 3 === 1 ? 'mini' : 'phone',
            seq: index + 1,
            memory_id: index % 5 === 0 ? 'mem_20260507_a1b2c3d4e5f60718_000010' : `mem_recall_${index}`,
            recalled_at: `2026-05-${String((index % 28) + 1).padStart(2, '0')}T${String(index % 24).padStart(2, '0')}:00:00Z`,
            summary: summaries[index % summaries.length],
        })),
    };
}

function auditPayload(pathname: string) {
    const id = pathname.split('/')[3] || 'mem_20260507_a1b2c3d4e5f60718_000010';
    return {
        memory_id: id,
        title: 'Project uses pnpm, never npm',
        body: 'Project uses pnpm for package management. Avoid package-lock.json drift.',
        status: 'active',
        namespace: 'coding/typescript',
        confidence: 0.92,
        confidence_reason: 'grounded by recent sessions',
        recall_count_total: 42,
        recall_count_30d: 12,
        last_recalled: now,
        provenance_chain: [],
        policy_decisions: [],
        privacy_scan: { labels_detected: [], storage_action: 'plaintext' },
        supersession_history: [],
        sync_state: { devices: ['mbp', 'mini'], merge_status: 'clean', claim_lock_status: [] },
    };
}

function syncPayload(scenario: ApiScenario) {
    if (scenario === 'empty') {
        return {
            sync: statusPayload(scenario).sync,
            last_commit: null,
            peer_presence: {
                coordination_level: 0,
                active_session_count: 0,
                active_sessions: [],
                recent_delivery_count: 0,
            },
            claim_locks: { active_count: 0, locks: [] },
        };
    }
    return {
        sync: statusPayload(scenario).sync,
        last_commit: 'fixture-sync-commit',
        peer_presence: {
            coordination_level: 3,
            active_session_count: 4,
            active_sessions: [
                {
                    session_id: 'peer_mbp',
                    harness: 'MacBook Pro',
                    namespace: 'project:agent-memory',
                    salient_entities: ['agent-memory'],
                    started_at: now,
                    last_heartbeat_age_seconds: 120,
                },
                {
                    session_id: 'peer_mini',
                    harness: 'Mac mini',
                    namespace: 'project:agent-memory',
                    salient_entities: ['agent-memory'],
                    started_at: now,
                    last_heartbeat_age_seconds: 1020,
                },
                {
                    session_id: 'peer_old',
                    harness: 'Old laptop',
                    namespace: 'archive/devices',
                    salient_entities: [],
                    started_at: '2026-03-24T09:00:00Z',
                    last_heartbeat_age_seconds: 3_888_000,
                },
                {
                    session_id: 'peer_phone',
                    harness: 'Travel phone',
                    namespace: 'personal/logistics',
                    salient_entities: ['travel'],
                    started_at: now,
                    last_heartbeat_age_seconds: 2460,
                },
            ],
            recent_delivery_count: scenario === 'heavy' ? 4096 : 189,
        },
        claim_locks: {
            active_count: 3,
            locks: [
                {
                    id: 'lock_mbp_a',
                    holder: 'peer_mbp',
                    namespace: 'project:agent-memory',
                    memory_id: 'mem_20260507_a1b2c3d4e5f60718_000010',
                    age_seconds: 120,
                },
                {
                    id: 'lock_phone_a',
                    holder: 'peer_phone',
                    namespace: 'personal/logistics',
                    memory_id: 'mem_due_school',
                    age_seconds: 2400,
                },
                {
                    id: 'lock_phone_b',
                    holder: 'peer_phone',
                    namespace: 'personal/logistics',
                    memory_id: 'mem_phone_trip',
                    age_seconds: 2500,
                },
            ],
        },
    };
}

function policyPayload() {
    return {
        source: 'fixture',
        raw_yaml: 'name: project-standard\nversion: 2\nscope: project\nconfidence_floor: 0.7\n',
        writable: true,
        files: ['project-standard.yaml'],
        policies: [
            {
                scope: 'project',
                selected_policy: 'project-standard',
                policy_source: 'disk',
                confidence_floor: 0.7,
                review_gates: ['low_confidence'],
                requires_grounding: true,
            },
        ],
    };
}

function notificationsStream(): ApiMockResponse {
    const body = `event: heartbeat\ndata: ${JSON.stringify({
        kind: 'heartbeat',
        notifications: [
            {
                id: 'notif_review_threshold',
                title: 'Review queue over threshold',
                body: 'Governance queue has crossed the warning threshold.',
                tone: 'warn',
                created_at: now,
            },
            {
                id: 'notif_dream_scheduled',
                title: 'Dream run scheduled for 03:00',
                body: 'Nightly synthesis pass is queued.',
                tone: 'ok',
                created_at: now,
            },
        ],
    })}\n\n`;
    return { status: 200, contentType: 'text/event-stream; charset=utf-8', body };
}

export function payloadForApiRequest(
    method: string,
    requestUrl: string,
    scenario: ApiScenario = 'happy',
    body?: unknown,
): ApiMockResponse {
    const url = new globalThis.URL(requestUrl, 'http://127.0.0.1:5173');
    const route = `${method.toUpperCase()} ${url.pathname}`;
    const error = scenarioError(scenario, route);
    if (error) return error;

    if (method === 'GET' && url.pathname === '/api/status') return jsonResponse(statusPayload(scenario));
    if (method === 'GET' && url.pathname === '/api/entity-graph') return jsonResponse(entityGraphPayload(scenario));
    if (method === 'GET' && url.pathname.startsWith('/api/entity-graph/'))
        return jsonResponse(entityDetailPayload(url.pathname));
    if (method === 'GET' && url.pathname === '/api/roi') {
        return jsonResponse({
            window_days: Number(url.searchParams.get('window') ?? 90),
            promotion_rate: scenario === 'empty' ? 0 : 0.68,
            promotion_precision: scenario === 'empty' ? 0 : 0.91,
            refusal_breakdown: scenario === 'empty' ? {} : { contradiction: 1, grounding: 7, policy: 2, tombstone: 3 },
            dreaming: {
                candidates_generated: 18,
                promoted_silent: 9,
                entered_review_queue: 5,
                dropped: 4,
                review_queue_approval_rate: 0.8,
            },
            reality_check_adherence: { weeks_completed: 4, weeks_skipped: 1 },
        });
    }
    if (method === 'GET' && url.pathname === '/api/reality-check') return jsonResponse(realityCheckPayload(scenario));
    if (method === 'GET' && url.pathname === '/api/reality-check/history') {
        return jsonResponse({
            sessions:
                scenario === 'empty'
                    ? []
                    : [
                          {
                              completed_at: '2026-05-01T14:00:00Z',
                              confirmed: 5,
                              corrected: 1,
                              forgotten: 0,
                              not_relevant: 1,
                              skipped: 0,
                          },
                      ],
        });
    }
    if (method === 'POST' && url.pathname === '/api/reality-check/respond') {
        const payload = (body ?? {}) as { session_id?: string; memory_id?: string; action?: string };
        return jsonResponse({
            accepted: true,
            session_id: payload.session_id ?? 'rc_20260507_001',
            memory_id: payload.memory_id ?? 'mem_unknown',
            action: payload.action ?? 'confirm',
            completion: { progress: { remaining: 0, deferred: 0 } },
        });
    }
    if (method === 'GET' && url.pathname === '/api/recall-hits') return jsonResponse(recallHitsPayload(scenario, url));
    if (method === 'GET' && /^\/api\/audit\/[^/]+$/.test(url.pathname)) return jsonResponse(auditPayload(url.pathname));
    if (method === 'GET' && url.pathname.endsWith('/walk')) {
        return jsonResponse({
            memory_id: url.pathname.split('/')[3],
            direction: url.searchParams.get('direction') ?? 'up',
            depth: Number(url.searchParams.get('depth') ?? 3),
            nodes: [{ id: 'root', kind: 'memory', label: 'Project uses pnpm' }],
            edges: [],
        });
    }
    if (method === 'GET' && url.pathname.endsWith('/temporal')) {
        return jsonResponse({
            memory_id: url.pathname.split('/')[3],
            at: url.searchParams.get('at'),
            viewing_historical_state: true,
            artifact: auditPayload(url.pathname),
        });
    }
    if (method === 'GET' && url.pathname === '/api/review') {
        const items = reviewItems(scenario);
        return jsonResponse({
            items,
            limit: Number(url.searchParams.get('limit') ?? 50),
            offset: Number(url.searchParams.get('offset') ?? 0),
        });
    }
    if (method === 'POST' && url.pathname === '/api/review/action') {
        const payload = (body ?? {}) as { id?: string; action?: string };
        return jsonResponse({
            ok: true,
            id: payload.id ?? 'mem_unknown',
            action: payload.action ?? 'approve',
        });
    }
    if (method === 'GET' && url.pathname === '/api/notifications/stream') return notificationsStream();
    if (method === 'GET' && url.pathname === '/api/policy-editor') return jsonResponse(policyPayload());
    if (method === 'POST' && url.pathname === '/api/policy-editor') {
        const payload = (body ?? {}) as { file_name?: string };
        return jsonResponse({
            accepted: true,
            file_name: payload.file_name ?? 'project-standard.yaml',
            policies: policyPayload().policies,
        });
    }
    if (method === 'GET' && url.pathname === '/api/sync-dashboard') return jsonResponse(syncPayload(scenario));

    return jsonResponse({ error: 'unhandled_mock_route', route }, 404);
}
