import { atom, read, update } from 'claude-code'
import type { Register } from 'claude-code'

import type { FileDiff } from '../types'
import { hunks, join, leadingCd, linkTarget, region, targets } from './diff'

// Changes by tool_use_id, so each row finds its own. Display only: nothing here reaches the
// model, and the Bash result it reads is untouched.
const diffs = atom({ plugin: 'nb-diff', key: 'diffs' } as const, {}, { shape: 'diffs-2' })
const KEEP = 200 // most recent calls whose changes are kept for redraws

export const register: Register = on => {
  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const files = targets(e.command)
    if (files.length === 0) return next(e)

    const cwd = await $.session.cwd()
    const base = join(cwd, leadingCd(e.command) ?? '')
    const readOr = (p: string) => $.fs.read(p).then(t => t as string, () => undefined)
    // A .nt argument writes the notebook it links to: snapshot that file instead.
    const paths = await Promise.all(
      files.map(async f => {
        const p = join(base, f)
        if (!p.endsWith('.nt')) return { file: f, path: p }
        const text = await readOr(p)
        return { file: f, path: (text && linkTarget(p, text)) || p }
      }),
    )
    const before = await Promise.all(paths.map(p => readOr(p.path)))

    const ran = await next(e)

    try {
      const found: FileDiff[] = []
      for (const [i, p] of paths.entries()) {
        const after = await readOr(p.path)
        if (after === undefined && before[i] === undefined) continue
        const patch = hunks(before[i] ?? '', after ?? '')
        if (patch.length > 0) found.push({ file: p.file, path: p.path, patch, ...region(before[i] ?? '', after ?? '') })
      }
      if (found.length > 0) {
        await update($, diffs, all => {
          const kept = Object.entries(all).slice(-(KEEP - 1))
          return { ...Object.fromEntries(kept), [e.tool_use_id]: found }
        })
      }
    } catch {
      // A diff is decoration: never let it change what the command returned.
    }
    return ran
  }).catch(($, e, next) => next(e)) // failed before running it: run the command as if the mod weren't here

  // The desktop folds runs of Bash calls into one group line: unfold a group holding an nb change
  // so its call draws as its own row.
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
