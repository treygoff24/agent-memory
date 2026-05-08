import { describe, expect, it } from 'vitest';

import { builtIndexHtml } from './dist';

function inlineScriptBodies(html: string): string[] {
    return [...html.matchAll(/<script\b(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/gi)]
        .map((match) => match[1]?.trim() ?? '')
        .filter(Boolean);
}

function inlineStyleBodies(html: string): string[] {
    return [...html.matchAll(/<style\b[^>]*>([\s\S]*?)<\/style>/gi)]
        .map((match) => match[1]?.trim() ?? '')
        .filter(Boolean);
}

describe('budgets csp', () => {
    it('ships a CSP-strict index without inline scripts or styles', () => {
        const html = builtIndexHtml();

        expect(inlineScriptBodies(html)).toEqual([]);
        expect(inlineStyleBodies(html)).toEqual([]);
    });
});
