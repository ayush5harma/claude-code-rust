// This script is the authority for the render corpus. render-corpus.json is its committed
// output, kept in the tree because the Rust render benchmark
// (src/app/connect/render_corpus.rs) includes it at compile time and the PTY scenario
// `render-corpus` (fake-bridge.js) reads it at run time. Edit this file, never the JSON.
// Every value derives from an index or a fixed seed, so two runs print identical bytes, and
// the test `render_corpus_fixture_matches_its_generator` (tests/terminal_resize.rs) runs
// `--check` so a stale JSON fails `cargo test`.
//
//   node tests/fixtures/generate-render-corpus.js           rewrite the fixture
//   node tests/fixtures/generate-render-corpus.js --check   exit 1 when it drifted
//
// Shape: { history, stream }. Both are arrays of bridge `session_update` payloads
// (the `update` object of a wire event, so any consumer parses them with the
// bridge's own types). `history` is a resumed conversation (the real
// `connected.history_updates` path); `stream` is one further assistant reply
// delivered live, in many small chunks, the way a model streams.
const fs = require('node:fs');
const path = require('node:path');

const OUT = path.join(__dirname, 'render-corpus.json');

let seed = 0x2f6e2b1;
const rand = limit => {
  // Park-Miller: small, portable, and identical on every Node version.
  seed = (seed * 48271) % 2147483647;
  return seed % limit;
};

const text = value => ({ type: 'text', text: value });
const user = value => ({ type: 'user_message_chunk', content: text(value) });
const say = value => ({ type: 'agent_message_chunk', content: text(value) });
const notice = (severity, message) => ({ type: 'system_notice_update', severity, message });
const toolUse = (id, name, kind, title, input, content = []) => ({
  type: 'tool_call',
  tool_call: {
    tool_call_id: id, title, kind, status: 'in_progress', content, raw_input: input,
    locations: typeof input.file_path === 'string' ? [{ path: input.file_path }] : [],
    meta: { claudeCode: { toolName: name } },
  },
});
const toolDone = (id, fields) => ({
  type: 'tool_call_update',
  tool_call_update: { tool_call_id: id, fields },
});
const diff = (file, oldText, newText) => [{
  type: 'diff', old_path: file, new_path: file, old: oldText, new: newText,
}];

// Prose and Markdown ---------------------------------------------------------

const OVERVIEW = [
  '# Transcript renderer overview',
  '',
  'The renderer turns each chat message into styled terminal rows. A message is a list of blocks, '
    + 'and every block kind owns its own presentation: Markdown text is parsed once and cached by '
    + 'content hash, tool calls render a one-line title plus a capped body, and diffs are computed '
    + 'line by line with three lines of context around each hunk. Wrapping happens last, at the '
    + 'width the terminal has right now, so a resize re-wraps cached lines instead of re-parsing '
    + 'them. The only block that is re-rendered on every frame while a reply streams is the tail '
    + 'block, which is why a long unbroken paragraph, an open code fence, or a half-received '
    + 'table is the worst case for the streaming path and the best case for a benchmark.',
  '',
  '## Pipeline',
  '',
  '1. **Serialize** each block into logical lines.',
  '   - Markdown headings, emphasis, `inline code`, and [links](https://example.com/docs/render) '
    + 'keep their styles.',
  '   - Tool calls render a title row plus a capped body.',
  '     - Execute output keeps its ANSI colour and drops everything above the cap.',
  '     - Diffs show only the changed hunks.',
  '       - Added lines are green, removed lines are red, context is dim.',
  '2. **Wrap** the logical lines to the terminal width.',
  '3. **Commit** finished blocks to scrollback and keep the live tail mutable.',
  '',
  '## Cost model',
  '',
  '| Stage | Runs | Cached by | Typical cost |',
  '| --- | :---: | --- | ---: |',
  '| Parse Markdown | once per block | content hash | 40 us |',
  '| Highlight code | once per fence | language + text | 120 us |',
  '| Diff | once per edit | old + new text | 300 us |',
  '| Wrap | every width change | width | 15 us |',
  '| Paint | every frame | nothing | 5 us |',
  '',
  '> The tail block is re-rendered on every chunk. Keep it small.',
  '>',
  '> A second paragraph inside the quote, with `code` and a [link](https://example.com/quote).',
  '',
  'See <https://example.com/architecture> for the full picture, or the ~~old~~ current notes in '
    + '`docs/src/architecture.md`.',
  '',
].join('\n');

const FENCES = [
  ['rust', [
    'use std::collections::HashMap;',
    '',
    '/// Count how often each word occurs, ignoring case.',
    'pub fn word_counts(input: &str) -> HashMap<String, usize> {',
    '    let mut counts = HashMap::new();',
    '    for word in input.split_whitespace() {',
    '        *counts.entry(word.to_lowercase()).or_insert(0) += 1;',
    '    }',
    '    counts',
    '}',
  ]],
  ['ts', [
    'export interface Row { id: number; label: string; tags?: string[] }',
    '',
    'export function groupByTag(rows: Row[]): Map<string, Row[]> {',
    '  const groups = new Map<string, Row[]>();',
    '  for (const row of rows) {',
    '    for (const tag of row.tags ?? ["untagged"]) {',
    '      groups.set(tag, [...(groups.get(tag) ?? []), row]);',
    '    }',
    '  }',
    '  return groups;',
    '}',
  ]],
  ['python', [
    'from dataclasses import dataclass',
    '',
    '@dataclass(frozen=True)',
    'class Span:',
    '    start: int',
    '    end: int',
    '',
    '    def overlaps(self, other: "Span") -> bool:',
    '        return self.start < other.end and other.start < self.end',
  ]],
  ['bash', [
    '#!/usr/bin/env bash',
    'set -euo pipefail',
    'for crate in core render widgets; do',
    '  cargo test -p "$crate" --quiet || { echo "failed: $crate" >&2; exit 1; }',
    'done',
  ]],
  ['json', [
    '{',
    '  "name": "render-corpus",',
    '  "widths": [80, 120, 200],',
    '  "options": { "wrap": true, "highlight": "auto", "tabs": null }',
    '}',
  ]],
  ['diff', [
    '--- a/src/ui/wrap.rs',
    '+++ b/src/ui/wrap.rs',
    '@@ -41,7 +41,7 @@ fn wrap_line(line: &Line, width: usize) -> Vec<Line> {',
    '     let mut rows = Vec::new();',
    '-    let limit = width;',
    '+    let limit = width.saturating_sub(1).max(1);',
    '     for span in &line.spans {',
    '         push_span(&mut rows, span, limit);',
    '     }',
  ]],
];
const fenced = ([lang, lines]) => `\`\`\`${lang}\n${lines.join('\n')}\n\`\`\`\n`;

// Tool output ----------------------------------------------------------------

const ESC = '\u001b';
const sgr = (codes, value) => `${ESC}[${codes}m${value}${ESC}[0m`;
const CRATES = ['serde', 'tokio', 'ratatui', 'syntect', 'similar', 'crossterm', 'anyhow', 'regex'];
// A cargo-like build log: coloured status words, 256-colour and truecolor
// paths, bold warnings, and plain lines in between.
function buildLog(lineCount) {
  const lines = [];
  for (let i = 0; i < lineCount; i++) {
    const crate = CRATES[i % CRATES.length];
    const version = `${i % 3}.${i % 17}.${i % 9}`;
    switch (i % 9) {
      case 0: case 1: case 2:
        lines.push(`${sgr('1;32', '   Compiling')} ${crate}-${i} v${version}`);
        break;
      case 3:
        lines.push(`${sgr('1;33', 'warning')}${sgr('1', `: unused variable: \`value_${i}\``)}`);
        lines.push(`  ${sgr('1;34', '-->')} src/${crate}/mod.rs:${10 + (i % 90)}:${5 + (i % 20)}`);
        i += 1;
        break;
      case 4:
        lines.push(`${sgr('38;5;244', `    Checking ${crate} v${version}`)} ${sgr('38;2;120;200;255', `(/work/${crate})`)}`);
        break;
      case 5:
        lines.push(`${sgr('4;36', `build.rs:${i}`)} ran in ${rand(900) + 100}ms`);
        break;
      case 6:
        lines.push(`${sgr('1;31', 'error[E0308]')}${sgr('1', ': mismatched types')}`);
        break;
      default:
        lines.push(`test ${crate}::tests::case_${i} ... ${sgr('32', 'ok')}`);
    }
  }
  return lines.slice(0, lineCount).join('\n');
}

const FAILED_OUTPUT = [
  `${sgr('1;31', 'error[E0432]')}${sgr('1', ': unresolved import `crate::ui::render_cache`')}`,
  `  ${sgr('1;34', '-->')} src/ui/diff.rs:7:5`,
  '   |',
  `7  | use crate::ui::render_cache::DiffCache;`,
  `   |     ${sgr('1;31', '^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^')} no \`render_cache\` in \`ui\``,
  '',
  `${sgr('1;31', 'error')}${sgr('1', ': could not compile `claude-code-rust` (lib test) due to 1 previous error')}`,
].join('\n');

// Edits ----------------------------------------------------------------------

const rustFunction = index => [
  `/// Handler ${index}: validates the input and forwards it downstream.`,
  `pub fn handler_${index}(input: &Input) -> Result<Output, Error> {`,
  '    if input.is_empty() {',
  `        return Err(Error::Empty { handler: ${index} });`,
  '    }',
  `    let scaled = input.value().saturating_mul(${index + 2});`,
  '    Ok(Output::new(scaled, input.label()))',
  '}',
  '',
];
const rustFile = lineCount => {
  const lines = [];
  for (let i = 0; lines.length < lineCount; i++) lines.push(...rustFunction(i));
  return lines.slice(0, lineCount).join('\n') + '\n';
};
const tsLine = index => `export const slot${index}: Slot = { id: ${index}, weight: ${(index * 7) % 13}, label: "slot-${index}" };`;
const tsFile = (lineCount, changed) => {
  const lines = [];
  for (let i = 0; i < lineCount; i++) lines.push(changed(i) ? tsLine(i).replace('weight', 'priority') : tsLine(i));
  return lines.join('\n') + '\n';
};

// The conversation -----------------------------------------------------------

const history = [
  user('Walk me through how the transcript renderer works, then fix the failing snapshot test in `src/ui/diff.rs` and show me the build output.'),
  { type: 'agent_activity_update', phase: 'thinking' },
  { type: 'agent_response_started' },
  say(OVERVIEW),
  say('\nA few snippets show the shapes the renderer has to handle.\n\n'),
  ...FENCES.map(fence => say(`${fenced(fence)}\n`)),
  notice('info', 'Context is 62% full. Run /compact to free space.'),
  toolUse('toolu_build', 'Bash', 'execute', 'cargo build --release 2>&1 | tee build.log', {
    command: 'cargo build --release 2>&1 | tee build.log',
    description: 'Build the release binary and keep the log',
  }),
  toolDone('toolu_build', { status: 'completed', raw_output: buildLog(2000) }),
  say('The build is green apart from warnings. The snapshot failure is in the diff header, so I will start there.\n\n'),
  toolUse('toolu_edit_small', 'Edit', 'edit', 'Edit src/ui/diff.rs', {
    file_path: 'src/ui/diff.rs',
    old_string: 'let limit = width;\n',
    new_string: 'let limit = width.saturating_sub(1).max(1);\n',
  }, diff('src/ui/diff.rs', 'let limit = width;\n', 'let limit = width.saturating_sub(1).max(1);\n')),
  toolDone('toolu_edit_small', { status: 'completed' }),
  toolUse('toolu_write_small', 'Write', 'edit', 'Write scripts/check.py', {
    file_path: 'scripts/check.py',
    content: 'import sys\n\nprint("ok" if len(sys.argv) > 1 else "usage: check.py FILE")\n',
  }, diff('scripts/check.py', '', 'import sys\n\nprint("ok" if len(sys.argv) > 1 else "usage: check.py FILE")\n')),
  toolDone('toolu_write_small', { status: 'completed' }),
  toolUse('toolu_write_large', 'Write', 'edit', 'Write src/generated/handlers.rs', {
    file_path: 'src/generated/handlers.rs', content: rustFile(500),
  }, diff('src/generated/handlers.rs', '', rustFile(500))),
  toolDone('toolu_write_large', { status: 'completed' }),
  toolUse('toolu_edit_large', 'Edit', 'edit', 'Edit web/src/slots.ts', {
    file_path: 'web/src/slots.ts',
    old_string: tsFile(250, () => false),
    new_string: tsFile(250, () => true),
  }, diff('web/src/slots.ts', tsFile(250, () => false), tsFile(250, () => true))),
  toolDone('toolu_edit_large', { status: 'completed' }),
  toolUse('toolu_test', 'Bash', 'execute', 'cargo test --no-run', {
    command: 'cargo test --no-run', description: 'Compile the tests',
  }),
  toolDone('toolu_test', { status: 'failed', raw_output: FAILED_OUTPUT }),
  notice('warning', 'The test build failed. The generated handlers may need an import.'),
  say('The tests do not compile yet: `render_cache` does not exist in this tree, so the import I added is wrong. I will drop it and re-run.\n'),
  user('Good catch. Keep the generated file, remove the import, and summarise what changed in two sentences.'),
  say('Removed the unused import and left the generated handlers in place. The snapshot now matches because the header width is clamped to at least one column.\n'),
];

// One long live reply: every Markdown shape again, so the tail block is a
// paragraph, a list, a table, and an open fence at different moments.
const STREAM_SECTION = index => [
  `## Finding ${index}: ${['wrapping', 'highlighting', 'diffing', 'scrollback', 'caching', 'layout'][index % 6]}`,
  '',
  `Observation ${index} is that the renderer spends its time where the content changes, not where it is large. `
    + `A ${100 + index * 40}-line block that never changes costs one parse; a three-line block that grows on every `
    + 'chunk costs a parse per frame. That asymmetry is the whole reason the tail block is special.',
  '',
  '- keep the tail block small',
  `  - split at paragraph ${index} boundaries once a blank line arrives`,
  '  - never split inside a fence',
  '- measure with `cargo test --release -- --ignored render_bench`',
  '',
  '| Case | Before | After |',
  '| --- | ---: | ---: |',
  `| paragraph ${index} | ${200 + index * 3} us | ${90 + index} us |`,
  `| table ${index} | ${340 + index * 5} us | ${150 + index} us |`,
  '',
  ...(index % 2 === 0
    ? [fenced(FENCES[index % FENCES.length]).trimEnd(), '']
    : ['> Note: a quote with `code` inside it still wraps like a paragraph.', '']),
].join('\n');
const STREAM_TEXT = ['# Review of the renderer', '', ...Array.from({ length: 12 }, (_, i) => STREAM_SECTION(i)), 'That is the full picture.', ''].join('\n');

// About 200 chunks of uneven size, cut anywhere (including mid-word), as a
// model's token stream would be.
function chunkText(value, count) {
  const average = Math.ceil(value.length / count);
  const chunks = [];
  let position = 0;
  while (position < value.length) {
    const size = Math.max(1, average - 8 + rand(17));
    chunks.push(value.slice(position, position + size));
    position += size;
  }
  return chunks;
}

const stream = [
  { type: 'agent_activity_update', phase: 'thinking' },
  { type: 'agent_response_started' },
  ...chunkText(STREAM_TEXT, 200).map(say),
];

const serialized = `{\n  "history": [\n${history.map(entry => `    ${JSON.stringify(entry)}`).join(',\n')}\n  ],\n`
  + `  "stream": [\n${stream.map(entry => `    ${JSON.stringify(entry)}`).join(',\n')}\n  ]\n}\n`;

if (process.argv.includes('--check')) {
  if (!fs.existsSync(OUT) || fs.readFileSync(OUT, 'utf8') !== serialized) {
    console.error('render-corpus.json is stale: run node tests/fixtures/generate-render-corpus.js');
    process.exit(1);
  }
} else {
  fs.writeFileSync(OUT, serialized);
}
