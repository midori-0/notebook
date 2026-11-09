import { atom, read, update } from 'claude-code'
import type { Register } from 'claude-code'

import type { FileDiff } from '../types'
import { hunks, join, leadingCd, linkTarget, region, targets } from './diff'

// Changes by tool_use_id, so each row finds its own. Display only: nothing here reaches the
// model, and the Bash result it reads is untouched.
const diffs = atom({ plugin: 'nb-diff', key: 'diffs' } as const, {}, { shape: 'diffs-2' })
const KEEP = 200 // how many recent calls keep their changes for redraws

type Watched = { file: string; path: string }
type ReadOr = (path: string) => Promise<string | undefined>

export const register: Register = on => {
  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const files = targets(e.command)
    if (files.length === 0) return next(e)

    const readOr: ReadOr = p => $.fs.read(p).then(t => t as string, () => undefined)
    const base = join(await $.session.cwd(), leadingCd(e.command) ?? '')
    const watched = await Promise.all(files.map(f => notebookOf(f, join(base, f), readOr)))
    const before = await Promise.all(watched.map(w => readOr(w.path)))

    const ran = await next(e)

    try {
      const after = await Promise.all(watched.map(w => readOr(w.path)))
      const found = watched.flatMap((w, i) => change(w, before[i], after[i]))
      if (found.length > 0) {
        await update($, diffs, all => ({ ...Object.fromEntries(Object.entries(all).slice(-(KEEP - 1))), [e.tool_use_id]: found }))
      }
    } catch {
      // A diff is decoration: never let it change what the command returned.
    }
    return ran
  }).catch(($, e, next) => next(e)) // failed before running it: run the command as if the mod weren't here

  // The desktop folds runs of Bash calls into one group line. Unfold a group holding an nb
  // change so that call gets its own row.
  on('ui.render', { component: 'ToolGroup' }, async ($, e, next) => {
    if (e.props.isExpanded) return next(e)
    const all = await read($, diffs)
    const has = e.props.calls.some(c => typeof c.tool_use_id === 'string' && all[c.tool_use_id])
    return next(has ? { ...e, props: { ...e.props, isExpanded: true } } : e)
  }).catch(($, e, next) => next(e))

  // Draw an nb call's row as the app's own Edit card for the notebook it changed.
  on('ui.render', { component: 'ToolUse', props: { tool: 'Bash' } }, async ($, e, next) => {
    const d = (await read($, diffs))[e.requestId]?.[0]
    if (!d || e.props.isRunning || e.props.isErrored || e.props.isInterrupted) return next(e)
    return next({
      ...e,
      props: {
        ...e.props,
        tool: 'Edit',
        input: { file_path: d.path, old_string: d.oldString, new_string: d.newString },
        output: {
          filePath: d.path,
          oldString: d.oldString,
          newString: d.newString,
          originalFile: null,
          structuredPatch: d.patch,
          userModified: false,
          replaceAll: false,
        },
      },
    })
  }).catch(($, e, next) => next(e)) // a refused rewrite draws the ordinary Bash row
}

/** The file to watch for a command argument. A .nt link writes the notebook it points at. */
async function notebookOf(file: string, path: string, readOr: ReadOr): Promise<Watched> {
  if (!path.endsWith('.nt')) return { file, path }
  const text = await readOr(path)
  return { file, path: (text && linkTarget(path, text)) || path }
}

/** What changed in one file, shaped for its Edit card; nothing if it didn't change. */
function change(w: Watched, before: string | undefined, after: string | undefined): FileDiff[] {
  if (before === undefined && after === undefined) return []
  const patch = hunks(before ?? '', after ?? '')
  return patch.length > 0 ? [{ ...w, patch, ...region(before ?? '', after ?? '') }] : []
}
