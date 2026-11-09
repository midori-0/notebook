// Pure helpers: which notebook files a command writes, and a unified line diff.

const WRITES = new Set(['make', 'add', 'edit', 'rm', 'restore', 'link'])

/** The .nb/.nt files that write commands in a shell command name (first such argument of each). */
export function targets(command: string): string[] {
  const files: string[] = []
  const call = /(?:^|[\s;&|(/])(?:nb|notebook)\s+([a-z]+)((?:[ \t]+(?:"[^"]*"|'[^']*'|[^\s;&|]+))*)/g
  for (const m of command.matchAll(call)) {
    if (!WRITES.has(m[1]!)) continue
    const args = [...m[2]!.matchAll(/"([^"]*)"|'([^']*)'|(\S+)/g)].map(t => t[1] ?? t[2] ?? t[3]!)
    const file = args.find(a => /\.(nb|nt)$/.test(a))
    if (file && !files.includes(file)) files.push(file)
  }
  return files
}

/** `cd <dir> && ...` at the start of a command: the folder its relative paths are in. */
export function leadingCd(command: string): string | undefined {
  return command.match(/^\s*cd\s+("[^"]*"|'[^']*'|\S+)\s*&&/)?.[1]!.replace(/^["']|["']$/g, '')
}

export function join(dir: string | undefined, p: string): string {
  return !dir || p.startsWith('/') ? p : `${dir.replace(/\/$/, '')}/${p}`
}

export function dirname(p: string): string {
  const i = p.lastIndexOf('/')
  return i < 0 ? '' : p.slice(0, i) || '/'
}

/** The notebook a .nt link points at, from its `notebook:` line, relative to the link's folder. */
export function linkTarget(ntPath: string, ntText: string): string | undefined {
  const rel = ntText.match(/^notebook: (.+)$/m)?.[1]
  return rel === undefined ? undefined : join(dirname(ntPath) || undefined, rel)
}

type Op = { k: ' ' | '-' | '+'; t: string; a: number; b: number }

const lines = (s: string) => (s === '' ? [] : s.replace(/\n$/, '').split('\n'))

/** Line ops between two texts: common prefix/suffix trimmed, LCS in between. */
function ops(before: string, after: string): Op[] {
  const a = lines(before)
  const b = lines(after)
  let s = 0
  while (s < a.length && s < b.length && a[s]! === b[s]!) s++
  let ea = a.length
  let eb = b.length
  while (ea > s && eb > s && a[ea - 1]! === b[eb - 1]!) {
    ea--
    eb--
  }
  const out: Op[] = []
  for (let i = 0; i < s; i++) out.push({ k: ' ', t: a[i]!, a: i, b: i })
  const n = ea - s
  const m = eb - s
  let i = 0
  let j = 0
  // shortcut: LCS table is O(n*m); past ~1M cells the changed block is shown as remove-then-add.
  if (n * m <= 1_000_000) {
    const w = m + 1
    const L = new Uint32Array((n + 1) * w)
    for (let x = n - 1; x >= 0; x--)
      for (let y = m - 1; y >= 0; y--)
        L[x * w + y] = a[s + x]! === b[s + y]! ? L[(x + 1) * w + y + 1]! + 1 : Math.max(L[(x + 1) * w + y]!, L[x * w + y + 1]!)
    while (i < n && j < m) {
      if (a[s + i]! === b[s + j]!) {
        out.push({ k: ' ', t: a[s + i]!, a: s + i, b: s + j })
        i++
        j++
      } else if (L[(i + 1) * w + j]! >= L[i * w + j + 1]!) {
        out.push({ k: '-', t: a[s + i]!, a: s + i, b: s + j })
        i++
      } else {
        out.push({ k: '+', t: b[s + j]!, a: s + i, b: s + j })
        j++
      }
    }
  }
  for (; i < n; i++) out.push({ k: '-', t: a[s + i]!, a: s + i, b: s + j })
  for (; j < m; j++) out.push({ k: '+', t: b[s + j]!, a: s + n, b: s + j })
  for (let x = 0; x < a.length - ea; x++) out.push({ k: ' ', t: a[ea + x]!, a: ea + x, b: eb + x })
  return out
}

export type Hunk = { oldStart: number; oldLines: number; newStart: number; newLines: number; lines: string[] }

/** Hunks with `ctx` lines of context, in the Edit tool's structuredPatch form. */
export function hunks(before: string, after: string, ctx = 3): Hunk[] {
  const o = ops(before, after)
  const out: Hunk[] = []
  let i = 0
  while (i < o.length) {
    while (i < o.length && o[i]!.k === ' ') i++
    if (i >= o.length) break
    const start = Math.max(0, i - ctx)
    let last = i
    for (let j = i; j < o.length; j++) {
      if (o[j]!.k !== ' ') last = j
      else if (j - last > 2 * ctx) break
    }
    const end = Math.min(o.length, last + ctx + 1)
    const h = o.slice(start, end)
    const oldLines = h.filter(x => x.k !== '+').length
    const newLines = h.filter(x => x.k !== '-').length
    out.push({
      oldStart: oldLines ? h[0]!.a + 1 : h[0]!.a,
      oldLines,
      newStart: newLines ? h[0]!.b + 1 : h[0]!.b,
      newLines,
      lines: h.map(x => x.k + x.t),
    })
    i = end
  }
  return out
}

/** The changed region of each text (common leading and trailing lines dropped). */
export function region(before: string, after: string): { oldString: string; newString: string } {
  const a = lines(before)
  const b = lines(after)
  let s = 0
  while (s < a.length && s < b.length && a[s] === b[s]) s++
  let ea = a.length
  let eb = b.length
  while (ea > s && eb > s && a[ea - 1] === b[eb - 1]) {
    ea--
    eb--
  }
  return { oldString: a.slice(s, ea).join('\n'), newString: b.slice(s, eb).join('\n') }
}

/** Unified-diff text of the same hunks; '' when nothing changed. */
export function unified(before: string, after: string, ctx = 3): string {
  return hunks(before, after, ctx)
    .map(h => [`@@ -${h.oldStart},${h.oldLines} +${h.newStart},${h.newLines} @@`, ...h.lines].join('\n'))
    .join('\n')
}
