import { expect, test } from 'claude-code/testing'

import { join, linkTarget, targets, unified } from './diff'

const BEFORE = 'notebook v1\nuid t-1\n\n@@ 1 2026-11-09T00:00:00Z [] one\nfirst\n\n@@ 2 2026-11-09T00:00:00Z [] two\nsecond\n\n'
const AFTER = 'notebook v1\nuid t-1\n\n@@ 1 2026-11-09T00:00:00Z [] one\nfirst\n\n@@ 2 2026-11-09T00:01:00Z [] two\nsecond, edited\n\n'

test('targets finds the notebook of each write command, and skips reads', () => {
  expect(targets('nb add bio.nb -i cells -t "A b" --text x')).toEqual(['bio.nb'])
  expect(targets('cd x && nb edit docs/a.nt 3 -t y; notebook rm "my notes.nb" 4')).toEqual(['docs/a.nt', 'my notes.nb'])
  expect(targets('/Users/me/.cargo/bin/nb rm bio.nb 2')).toEqual(['bio.nb'])
  expect(targets('C:\\Users\\me\\.cargo\\bin\\nb.exe edit C:\\notes\\bio.nb 2 -t x')).toEqual(['C:\\notes\\bio.nb'])
  expect(targets('nb show bio.nb -n 2 && nb list bio.nb && nb ids bio.nb')).toEqual([])
  expect(targets('grep nbx file.nb')).toEqual([])
})

test('linkTarget resolves a .nt link against its own folder', () => {
  expect(linkTarget('docs/a.nt', 'notebook link v1\nnotebook: ../bio.nb\nids: a\n')).toBe('docs/../bio.nb')
  expect(linkTarget('a.nt', 'notebook link v1\nnotebook: bio.nb\n')).toBe('bio.nb')
  expect(linkTarget('C:\\proj\\docs\\a.nt', 'notebook link v1\nnotebook: ../bio.nb\n')).toBe('C:\\proj\\docs/../bio.nb')
  expect(join('C:\\proj', 'C:\\other\\x.nb')).toBe('C:\\other\\x.nb')
  expect(join('C:\\proj\\', 'x.nb')).toBe('C:\\proj/x.nb')
})

test('unified shows an edit as one hunk with context', () => {
  expect(unified(BEFORE, AFTER)).toBe(
    [
      '@@ -4,6 +4,6 @@',
      ' @@ 1 2026-11-09T00:00:00Z [] one',
      ' first',
      ' ',
      '-@@ 2 2026-11-09T00:00:00Z [] two',
      '-second',
      '+@@ 2 2026-11-09T00:01:00Z [] two',
      '+second, edited',
      ' ',
    ].join('\n'),
  )
  expect(unified(BEFORE, BEFORE)).toBe('')
  expect(unified('', 'notebook v1\nuid t-1\n')).toBe('@@ -0,0 +1,2 @@\n+notebook v1\n+uid t-1')
})

test('unified keeps unchanged notes between two removed ones as context', () => {
  const notes = (ns: number[]) => 'notebook v1\n\n' + ns.map(n => `@@ ${n} t [] n${n}\nbody ${n}\n\n`).join('')
  const d = unified(notes([1, 2, 3, 4, 5, 6, 7, 8, 9]), notes([2, 3, 4, 5, 6, 7, 8]))
  expect(d.split('\n').filter(l => l.startsWith('-')).length).toBe(6) // notes 1 and 9, three lines each
  expect(d.split('\n').filter(l => l.startsWith('+')).length).toBe(0)
})

test('unified shows only the removed notes when they sit far apart in a big notebook', () => {
  const notes = (ns: number[]) => 'notebook v1\n\n' + ns.map(n => `@@ ${n} t [] n${n}\nbody ${n}\n\n`).join('')
  const all = Array.from({ length: 5000 }, (_, i) => i + 1)
  const d = unified(notes(all), notes(all.filter(n => n !== 3 && n !== 4990)))
  expect(d.split('\n').filter(l => /^[-+]/.test(l)).length).toBe(6)
  expect(d.split('\n').filter(l => l.startsWith('@@ -')).length).toBe(2)
})

test("an nb call's row is drawn as the app's Edit card; the model-facing result is unchanged", async ($, on) => {
  let reads = 0
  on('session.cwd', () => ({ value: '/proj' }))
  on('fs.read', (_, e) => ({ value: e.path === '/proj/bio.nb' ? (reads++ === 0 ? BEFORE : AFTER) : '' }))
  on('tool.call', () => ({ result: { stdout: '', stderr: '' } }))
  // Stands for the engine's own drawing: records what each row was asked to draw.
  const drawn: Record<string, unknown> = {}
  on('ui.render', ($, e) => {
    drawn[e.requestId] = e.props
    const { Text } = $.ui.resolve(e)
    const p = e.props as { tool?: string; isExpanded?: boolean }
    return <Text>{`engine ${e.component} ${p.tool ?? (p.isExpanded ? 'expanded' : 'folded')}`}</Text>
  })

  const ran = await $.tool.call({ tool: 'Bash', command: 'nb edit bio.nb 2 --text "second, edited"', tool_use_id: 'call-1' })
  expect(ran.result).toEqual({ stdout: '', stderr: '' })

  const row = (id: string) => ({
    tool_use_id: id, tool: 'Bash', input: { command: 'x' }, isRunning: false, isErrored: false, isInterrupted: false,
    output: { stdout: '', stderr: '' },
  })
  for (const surface of ['terminal', 'desktop', 'vscode', 'mobile'] as const) {
    const ui = await $.ui.mount({ plugin: 'nb-diff', surface, component: 'ToolUse', requestId: 'call-1', props: row('call-1') })
    expect((await ui.find({ type: 'Text' }))?.text).toBe('engine ToolUse Edit')
    await ui.unmount()
  }
  expect(drawn['call-1']).toMatchObject({
    tool: 'Edit',
    input: { file_path: '/proj/bio.nb', old_string: '@@ 2 2026-11-09T00:00:00Z [] two\nsecond' },
    output: {
      filePath: '/proj/bio.nb',
      newString: '@@ 2 2026-11-09T00:01:00Z [] two\nsecond, edited',
      structuredPatch: [{ oldStart: 4, oldLines: 6, newStart: 4, newLines: 6 }],
    },
  })

  // Other Bash rows draw as before; a folded group unfolds only when it holds an nb change.
  const other = await $.ui.mount({ plugin: 'nb-diff', surface: 'desktop', component: 'ToolUse', requestId: 'other', props: row('other') })
  expect((await other.find({ type: 'Text' }))?.text).toBe('engine ToolUse Bash')
  await other.unmount()
  const call = (id: string) => ({ tool_use_id: id, tool: 'Bash', input: {}, isRunning: false, isErrored: false, isInterrupted: false })
  for (const [id, want] of [['call-1', 'engine ToolGroup expanded'], ['other', 'engine ToolGroup folded']] as const) {
    const ui = await $.ui.mount({
      plugin: 'nb-diff', surface: 'desktop', component: 'ToolGroup', requestId: `group:${id}`,
      props: { calls: [call(id)], isActive: false, isExpanded: false },
    })
    expect((await ui.find({ type: 'Text' }))?.text).toBe(want)
    await ui.unmount()
  }
})
