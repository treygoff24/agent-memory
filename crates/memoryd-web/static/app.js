const csrfToken = document.querySelector('meta[name="csrf-token"]')?.content ?? '';

document.documentElement.dataset.memorumDashboard = 'ready';
window.memorumDashboard = Object.freeze({
    csrfToken,
    routes: ['/api/status', '/api/entity-graph', '/api/roi', '/api/reality-check', '/api/recall-hits', '/api/review'],
});
