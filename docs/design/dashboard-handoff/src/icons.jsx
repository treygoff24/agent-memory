// Minimal Phosphor-style inline SVG icons (Regular, 16/20px). Self-hosted via inline.
const Icon = ({ name, size = 16, strokeWidth = 1.5 }) => {
    const s = size;
    const sw = strokeWidth;
    const common = {
        width: s,
        height: s,
        viewBox: '0 0 24 24',
        fill: 'none',
        stroke: 'currentColor',
        strokeWidth: sw,
        strokeLinecap: 'round',
        strokeLinejoin: 'round',
        'aria-hidden': true,
        focusable: 'false',
    };
    switch (name) {
        case 'inbox':
            return (
                <svg {...common}>
                    <path d="M3 13v5a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-5" />
                    <path d="M3 13l3-9h12l3 9" />
                    <path d="M3 13h4l1 3h8l1-3h4" />
                </svg>
            );
        case 'eye':
            return (
                <svg {...common}>
                    <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z" />
                    <circle
                        cx="12"
                        cy="12"
                        r="3"
                    />
                </svg>
            );
        case 'clock':
            return (
                <svg {...common}>
                    <circle
                        cx="12"
                        cy="12"
                        r="9"
                    />
                    <path d="M12 7v5l3 2" />
                </svg>
            );
        case 'moon':
            return (
                <svg {...common}>
                    <path d="M21 13a8 8 0 1 1-9-9 6.5 6.5 0 0 0 9 9z" />
                </svg>
            );
        case 'users':
            return (
                <svg {...common}>
                    <circle
                        cx="9"
                        cy="8"
                        r="3.5"
                    />
                    <path d="M2.5 20a6.5 6.5 0 0 1 13 0" />
                    <circle
                        cx="17"
                        cy="9"
                        r="2.8"
                    />
                    <path d="M15 20a4.5 4.5 0 0 1 6.5-4" />
                </svg>
            );
        case 'shield':
            return (
                <svg {...common}>
                    <path d="M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6l8-3z" />
                    <path d="M9 12l2 2 4-4" />
                </svg>
            );
        case 'graph':
            return (
                <svg {...common}>
                    <circle
                        cx="6"
                        cy="6"
                        r="2.5"
                    />
                    <circle
                        cx="18"
                        cy="6"
                        r="2.5"
                    />
                    <circle
                        cx="12"
                        cy="18"
                        r="2.5"
                    />
                    <path d="M7.5 7.5l3 8M16.5 7.5l-3 8M8.5 6h7" />
                </svg>
            );
        case 'gear':
            return (
                <svg {...common}>
                    <circle
                        cx="12"
                        cy="12"
                        r="3"
                    />
                    <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
                </svg>
            );
        case 'search':
            return (
                <svg {...common}>
                    <circle
                        cx="11"
                        cy="11"
                        r="6.5"
                    />
                    <path d="M20 20l-4-4" />
                </svg>
            );
        case 'command':
            return (
                <svg {...common}>
                    <path d="M9 9V6a3 3 0 1 0-3 3h3zm0 0v6m0-6h6m-6 6H6a3 3 0 1 0 3 3v-3zm6 0V9m0 6h3a3 3 0 1 0-3-3v3zm0-6V6a3 3 0 1 1 3 3h-3z" />
                </svg>
            );
        case 'bell':
            return (
                <svg {...common}>
                    <path d="M6 8a6 6 0 0 1 12 0c0 4 2 5 2 7H4c0-2 2-3 2-7z" />
                    <path d="M10 19a2 2 0 0 0 4 0" />
                </svg>
            );
        case 'x':
            return (
                <svg {...common}>
                    <path d="M6 6l12 12M6 18L18 6" />
                </svg>
            );
        case 'chevron-down':
            return (
                <svg {...common}>
                    <path d="M6 9l6 6 6-6" />
                </svg>
            );
        case 'chevron-right':
            return (
                <svg {...common}>
                    <path d="M9 6l6 6-6 6" />
                </svg>
            );
        case 'circle-half':
            return (
                <svg {...common}>
                    <circle
                        cx="12"
                        cy="12"
                        r="9"
                    />
                    <path
                        d="M12 3a9 9 0 0 1 0 18z"
                        fill="currentColor"
                        stroke="none"
                    />
                </svg>
            );
        case 'check':
            return (
                <svg {...common}>
                    <path d="M5 12l5 5L20 7" />
                </svg>
            );
        case 'warning':
            return (
                <svg {...common}>
                    <path d="M12 3l10 18H2L12 3z" />
                    <path d="M12 10v5M12 18v.5" />
                </svg>
            );
        case 'sliders':
            return (
                <svg {...common}>
                    <path d="M4 7h10M18 7h2M4 12h2M10 12h10M4 17h12M20 17h0" />
                    <circle
                        cx="16"
                        cy="7"
                        r="2"
                    />
                    <circle
                        cx="8"
                        cy="12"
                        r="2"
                    />
                    <circle
                        cx="18"
                        cy="17"
                        r="2"
                    />
                </svg>
            );
        case 'panel-right':
            return (
                <svg {...common}>
                    <rect
                        x="3"
                        y="4"
                        width="18"
                        height="16"
                        rx="2"
                    />
                    <path d="M14 4v16" />
                </svg>
            );
        case 'list':
            return (
                <svg {...common}>
                    <path d="M4 6h16M4 12h16M4 18h12" />
                </svg>
            );
        case 'play':
            return (
                <svg {...common}>
                    <path
                        d="M7 5l12 7-12 7V5z"
                        fill="currentColor"
                    />
                </svg>
            );
        case 'diamond':
            return (
                <svg {...common}>
                    <path d="M12 3l9 9-9 9-9-9 9-9z" />
                </svg>
            );
        default:
            return null;
    }
};
window.Icon = Icon;
