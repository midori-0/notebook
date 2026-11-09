/** One notebook file a command changed, in the shape the Edit tool's row is drawn from. */
export type FileDiff = {
  file: string // as the command named it
  path: string // absolute
  patch: { oldStart: number; oldLines: number; newStart: number; newLines: number; lines: string[] }[]
  oldString: string
  newString: string
}

declare module 'claude-code' {
  interface PluginState {
    'nb-diff': { diffs: Shaped<Record<string, FileDiff[]>> }
  }
}
