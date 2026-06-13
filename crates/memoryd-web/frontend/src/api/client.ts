import type { ApiErrorBody } from './types';

export class ApiError extends Error {
    readonly status: number;
    readonly body: ApiErrorBody;
    constructor(status: number, body: ApiErrorBody) {
        super(body.message || body.error || `request failed with ${status}`);
        this.status = status;
        this.body = body;
    }
}

function csrfToken(): string {
    return document.querySelector<HTMLMetaElement>('meta[name="csrf-token"]')?.content ?? '';
}

export async function apiJson<T>(path: string, init: RequestInit = {}): Promise<T> {
    const headers = new Headers(init.headers);
    headers.set('accept', 'application/json');
    // The bearer token gates *every* data-bearing endpoint, GET reads included —
    // loopback reachability alone (e.g. another local user on a shared machine
    // connecting to this dashboard's port) must not be enough to read memory
    // bodies, search results, or the audit graph. require_local_host only closes
    // the browser cross-origin path; this token closes the local cross-process path.
    headers.set('x-memorum-csrf', csrfToken());
    if (init.body && !headers.has('content-type')) headers.set('content-type', 'application/json');
    const response = await fetch(path, { ...init, headers });
    const body = (await response.json().catch(() => ({}))) as ApiErrorBody | T;
    if (!response.ok) throw new ApiError(response.status, body as ApiErrorBody);
    return body as T;
}
