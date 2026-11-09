// The mod's pure logic: which notebook files a shell command writes, and a line diff of each
// file's before and after. No I/O, so all of it is testable on plain strings.

const WRITES = new Set(['make', 'add', 'edit', 'rm', 'restore', 'link'])

/** The .nb/.nt file each nb write command names (its first such argument), in order, deduplicated. */
export function targets(command: string): string[] {
  const files: string[] = []
  const call = /(?:^|[\s;&|(/\\])(?:nb|notebook)(?:\.exe)?\s+([a-z]+)((?:[ \t]+(?:"[^"]*"|'[^']*'|[^\s;&|]+))*)/g
  for (const m of command.matchAll(call)) {
    if (!WRITES.has(m[1]!)) continue
    const args = [...m[2]!.matchAll(/"([^"]*)"|'([^']*)'|(\S+)/g)].map(t => t[1] ?? t[2] ?? t[3]!)
    const file = args.find(a => /\.(nb|nt)$/.test(a))
    if (file && !files.includes(file)) files.push(file)
  }
  return files
}

/** The folder of a leading `cd <dir> &&`, which the command's relative paths are under. */
export function leadingCd(command: string): string | undefined {
  return command.match(/^\s*cd\s+("[^"]*"|'[^']*'|\S+)\s*&&/)?.[1]!.replace(/^["']|["']$/g, '')
}

/** Absolute on any OS: `/x`, `\\x`, `C:\\x` or `C:/x`. */
export function isAbsolute(p: string): boolean {
  return /^([\\/]|[A-Za-z]:[\\/])/.test(p)
}

export function join(dir: string | undefined, p: string): string {
  return !dir || isAbsolute(p) ? p : `${dir.replace(/[\\/]$/, '')}/${p}`
}

export function dirname(p: string): string {
  const i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'))
  return i < 0 ? '' : p.slice(0, i) || p.slice(0, 1)
}

/** The notebook a .nt link points at: its `notebook:` line, taken relative to the link's folder. */
export function linkTarget(ntPath: string, ntText: string): string | undefined {
  const rel = ntText.match(/^notebook: (.+)$/m)?.[1]
  return rel === undefined ? undefined : join(dirname(ntPath) || undefined, rel)
}

const lines = (s: string) => (s === '' ? [] : s.replace(/\n$/, '').split('\n'))

/** Where the shared first and last lines end: only `a[s..ea)` and `b[s..eb)` differ. */
function trim(a: string[], b: string[]) {
  let s = 0
  while (s < a.length && s < b.length && a[s] === b[s]) s++
  let ea = a.length
  let eb = b.length
  while (ea > s && eb > s && a[ea - 1] === b[eb - 1]) {
    ea--
    eb--
  }
  return { s, ea, eb }
}

// Past this many changed lines the changed region is shown as removed, then added. Memory for
// the search grows with the square of this.
const MAX_EDITS = 1000

/**
 * Myers' O(ND) diff of the n lines from a[s] against the m lines from b[s]: the indexes (from s)
 * of the lines they keep in common, in order. Work grows with the number of changed lines, not
 * the file's size, so removing two notes far apart in a big notebook stays cheap.
 * Null past `MAX_EDITS` changes.
 */
function common(a: string[], b: string[], s: number, n: number, m: number): [number, number][] | null {
  // v[k + off]: how far right the best path so far reaches on diagonal k (k = x - y).
  const off = MAX_EDITS + 1
  const v = new Int32Array(2 * MAX_EDITS + 3)
  const trace: Int32Array[] = [] // v before each round, diagonals -d-1..d+1, for walking back
  for (let d = 0; d <= MAX_EDITS; d++) {
    trace.push(v.slice(off - d - 1, off + d + 2))
    for (let k = -d; k <= d; k += 2) {
      let x = k === -d || (k !== d && v[off + k - 1]! < v[off + k + 1]!) ? v[off + k + 1]! : v[off + k - 1]! + 1
      let y = x - k
      while (x < n && y < m && a[s + x] === b[s + y]) {
        x++
        y++
      }
      v[off + k] = x
      if (x >= n && y >= m) return walkBack(trace, n, m)
    }
  }
  return null
}

/** Retraces the path `common` found, from the end back to the start, collecting its diagonals. */
function walkBack(trace: Int32Array[], n: number, m: number): [number, number][] {
  const pairs: [number, number][] = []
  let x = n
  let y = m
  for (let d = trace.length - 1; d >= 0; d--) {
    const v = trace[d]!
    const at = (k: number) => v[k + d + 1]!
    const k = x - y
    const prev = k === -d || (k !== d && at(k - 1) < at(k + 1)) ? k + 1 : k - 1
    const px = at(prev)
    const py = px - prev
    while (x > px && y > py) pairs.push([--x, --y])
    x = px
    y = py
  }
  return pairs.reverse()
}

type Op = { k: ' ' | '-' | '+'; t: string; a: number; b: number }

/** Every line of both texts as kept, removed or added, with its index in each. */
function ops(before: string, after: string): Op[] {
  const a = lines(before)
  const b = lines(after)
  const { s, ea, eb } = trim(a, b)
  const out: Op[] = []
  for (let i = 0; i < s; i++) out.push({ k: ' ', t: a[i]!, a: i, b: i })
  const kept = common(a, b, s, ea - s, eb - s) ?? []
  let x = s
  let y = s
  // Between kept lines, removals come before additions, as in any unified diff.
  for (const [i, j] of [...kept.map(([i, j]) => [s + i, s + j]), [ea, eb]]) {
    for (; x < i!; x++) out.push({ k: '-', t: a[x]!, a: x, b: y })
    for (; y < j!; y++) out.push({ k: '+', t: b[y]!, a: x, b: y })
    if (x < ea) out.push({ k: ' ', t: a[x]!, a: x++, b: y++ })
  }
  for (let i = 0; i < a.length - ea; i++) out.push({ k: ' ', t: a[ea + i]!, a: ea + i, b: eb + i })
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

/** The part of each text that changed: shared first and last lines dropped. */
export function region(before: string, after: string): { oldString: string; newString: string } {
  const a = lines(before)
  const b = lines(after)
  const { s, ea, eb } = trim(a, b)
  return { oldString: a.slice(s, ea).join('\n'), newString: b.slice(s, eb).join('\n') }
}

/** The same hunks as unified-diff text; '' when nothing changed. */
export function unified(before: string, after: string, ctx = 3): string {
  return hunks(before, after, ctx)
    .map(h => [`@@ -${h.oldStart},${h.oldLines} +${h.newStart},${h.newLines} @@`, ...h.lines].join('\n'))
    .join('\n')
}
