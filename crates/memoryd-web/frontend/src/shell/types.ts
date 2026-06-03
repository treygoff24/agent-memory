// Shared shell-chrome types. Extracted to a leaf module so `Shell` and its
// `TopBar`/`Footer` children can all depend on `ShellStatus` without forming an
// import cycle.

export interface ShellStatus {
    daemon: 'ok' | 'warn' | 'bad' | 'idle';
    syncLabel: string;
    peerLabel: string;
}
