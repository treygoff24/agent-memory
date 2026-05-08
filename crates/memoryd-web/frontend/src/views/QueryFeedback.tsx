import { apiErrorBody, apiErrorTitle, apiErrorTone } from '../api/errorPresentation';
import { Banner } from '../ui';

export function QueryErrorBanner({ error, label }: { error: unknown; label: string }) {
    if (!error) return null;
    return (
        <Banner
            title={apiErrorTitle(error, label)}
            body={apiErrorBody(error)}
            tone={apiErrorTone(error)}
        />
    );
}

export function QueryLoadingBanner({ label }: { label: string }) {
    return (
        <Banner
            title={`${label}: loading`}
            body="Fetching the latest dashboard data from memoryd-web."
            tone="ok"
        />
    );
}
