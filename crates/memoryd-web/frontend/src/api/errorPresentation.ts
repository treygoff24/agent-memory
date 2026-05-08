import { ApiError } from './client';

export function apiErrorTitle(error: unknown, label: string): string {
    if (error instanceof ApiError) {
        if (error.status === 403) return `${label}: permission required`;
        if (error.status === 409) return `${label}: conflict`;
        if (error.status === 503) return `${label}: backend unavailable`;
        return `${label}: request failed`;
    }
    return `${label}: request failed`;
}

export function apiErrorBody(error: unknown): string {
    if (error instanceof ApiError) {
        return error.body.message || error.body.error || `HTTP ${error.status}`;
    }
    return error instanceof Error ? error.message : 'Unknown API error.';
}

export function apiErrorTone(error: unknown): 'bad' | 'warn' {
    return error instanceof ApiError && error.status === 409 ? 'warn' : 'bad';
}
