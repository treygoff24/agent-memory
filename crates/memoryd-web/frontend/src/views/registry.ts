export const viewIds = ['inbox', 'reality', 'recall', 'dreams', 'peers', 'governance', 'entities', 'settings'] as const;

export type ViewId = (typeof viewIds)[number];
