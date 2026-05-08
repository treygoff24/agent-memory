import js from '@eslint/js';
import tseslint from '@typescript-eslint/eslint-plugin';
import tsParser from '@typescript-eslint/parser';
import react from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';

export default [
    { ignores: ['dist/**', 'node_modules/**', 'playwright-report/**', 'test-results/**', 'scripts/**'] },
    js.configs.recommended,
    {
        files: ['**/*.{ts,tsx}'],
        languageOptions: {
            parser: tsParser,
            parserOptions: { ecmaVersion: 'latest', sourceType: 'module', ecmaFeatures: { jsx: true } },
            globals: {
                document: 'readonly',
                window: 'readonly',
                localStorage: 'readonly',
                matchMedia: 'readonly',
                HTMLElement: 'readonly',
                HTMLDivElement: 'readonly',
                HTMLMetaElement: 'readonly',
                Headers: 'readonly',
                RequestInit: 'readonly',
                MessageEvent: 'readonly',
                EventSource: 'readonly',
                KeyboardEvent: 'readonly',
                URLSearchParams: 'readonly',
                fetch: 'readonly',
                process: 'readonly',
                console: 'readonly',
            },
        },
        plugins: { react, 'react-hooks': reactHooks, '@typescript-eslint': tseslint },
        settings: { react: { version: 'detect' } },
        rules: {
            ...tseslint.configs.recommended.rules,
            ...react.configs.recommended.rules,
            ...reactHooks.configs.recommended.rules,
            'react/react-in-jsx-scope': 'off',
            '@typescript-eslint/no-explicit-any': 'error',
        },
    },
];
