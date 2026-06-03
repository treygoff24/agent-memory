// Shared view-model types for the Entities view. Extracted to a leaf module so the
// `Entities` container and its `EntityTable` child can both depend on them without
// forming an import cycle.

export type EntityKind = 'person' | 'org' | 'project' | 'place' | 'tool' | 'language' | 'unknown';
export type EntitySortKey = 'name' | 'kind' | 'mentions' | 'namespaces' | 'lastSeen' | 'firstSeen' | 'confidence';

export interface EntityViewItem {
    id: string;
    name: string;
    kind: EntityKind;
    mentions: number;
    namespaces: string[];
    firstSeen: string;
    lastSeen: string;
    confidence: number | null;
    sensitive?: boolean;
    recent: Array<{ id: string; title: string; weight: number }>;
}
