# Presentation Gaps Between Stock and claude-rs

Input for a later output-presentation feature. It lists where the same model
output reads worse in claude-rs than in the stock Claude Code TUI it runs on,
ranked by how much the difference costs a reader. It does not say that
claude-rs should look like stock: each row states a readability problem, and
the "Fixable" column says whether it can be addressed with claude-rs's own
styling inside the project's rules (the Agent SDK is the only integration
surface, no visual element imitates Claude Code's UI, presentation is owned by
Rust).

## How it was captured

`scripts/e2e-parity/run.sh --only present.capture` (the `present.capture`
check) sends two prompts to claude-rs and to stock, both on Haiku, in 130 by
110 tmux panes, and keeps every screen as plain text and with colour escapes
(`tmux capture-pane -p` and `-p -e`). Measured on 2026-10-13 with claude-rs
0.15.2-fork.2 (the fork's `main` at 99bfebc, release build) and Claude Code
2.1.296.

1. The model reads `fixtures/presentation-sample.md` with the Read tool and
   answers with its contents verbatim. The sample has three heading levels,
   bold, italic, strikethrough, inline code and a link, nested bullet and
   numbered lists, a block quote, a table, fenced `rust`, `bash` (first line
   `#!/usr/bin/env bash`), `json` and `diff` blocks, and one line longer than
   the pane. The reply reproduced the sample (checked against the claude-rs
   session transcript; stock's screen shows the same text), so the differences
   below are presentation. The excerpts are abridged captures.
2. The model runs `bash ansi.sh`, which prints red, bold green, underlined,
   256-colour, truecolor and reverse text.

Colours below are written `{fg=..|text}`; `aN` is the terminal's 16-colour
palette, `cN` the 256-colour palette, `r,g,b` truecolor.

## Ranked differences

| # | Difference | Readability impact | Fixable within upstream rules |
| --- | --- | --- | --- |
| 1 | A `#!` first line in a fenced block is rendered as `**!/usr/bin/env bash**` | High: the displayed code is not the code | Yes, it is a defect in code-block rendering |
| 2 | A Read result is printed as one wrapped paragraph with the line numbers glued to the text | High: 8 unreadable rows before every answer that follows a Read | Yes, tool-result rows are host-owned |
| 3 | Colour in tool output is dropped | Medium: colour carries pass/fail and diff meaning | Yes, if only SGR (`...m`) sequences are mapped to styles |
| 4 | Fence lines are shown as text and a code block has no indent or shading | Medium: code and prose are told apart by the marker rows alone | Yes, with claude-rs's own block styling |
| 5 | Vertical spacing between blocks is inconsistent | Medium-low: a paragraph, list and quote run together, a table and a fence are two rows apart | Yes |
| 6 | The three heading levels look the same | Medium-low: structure of a long answer is flat | Yes, with own weights or colours |
| 7 | A table has a header rule but no column separators or border | Low: fine while cells are short | Yes |
| 8 | Links print the URL after the label | Low: a long URL doubles the line | Partly: the URL can be shortened or dimmed; a clickable label needs OSC 8, which the control-sequence filter removes today |
| 9 | Nested lists lose 4 columns per level | Low: deep nesting wraps early | Yes |

## 1. Shebang line in a fenced block

Source line (the model's reply, from the transcript): `#!/usr/bin/env bash`.

~~~text
stock                                    claude-rs
  #!/usr/bin/env bash                    ```bash
  set -euo pipefail
  for f in *.txt; do echo ...; done      **!/usr/bin/env bash**
                                         set -euo pipefail
                                         for f in *.txt; do echo "PRES bash $f"; done
                                         ```
~~~

claude-rs draws the first line in the comment colour `{143,161,179|**!/usr/bin/env}` followed by `bash**`, drops the
`#`, adds literal asterisks and inserts an empty row after the fence. The same
happened with a block that holds only the shebang and one `echo` line, so it
does not depend on the rest of the sample. Recorded as a defect by the check.

## 2. Read results

```text
stock                                    claude-rs
  Thought for 5s, read 1 file              ✓ ⬚ Read presentation-sample.md
                                           │  1# PRES heading one 2 3## PRES heading two 4 5### PRES heading three 6 7A
                                           │  paragraph with bold text, italic text, struck text, inline code and a
                                           │  link label (https://example.com/pres). 8 9- PRES bullet one 10  - PRES
                                           │  nested a 11    - PRES nested deeper 12  - PRES nested b 13- PRES bullet
                                           │  two 14 151. PRES first 162. PRES second 17   - PRES numbered child 18 19>
                                           │  PRES quoted line one 20> PRES quoted line two 21 22| Name | Qty | Note | ...
                                           └─ ... 2 wrapped rows hidden ...
```

claude-rs joins the file's lines with spaces, so line numbers and content
merge (`151. PRES first` is line 15, `1. PRES first`) and the file's structure is
gone; only the first rows are kept. Stock shows a one-line summary and keeps the
content behind Ctrl+O. The same happens for every Read of a multi-line file.

## 3. Colour in tool output

`bash ansi.sh` printed seven colour and style sequences. The session
transcript holds them in the tool result (`\u001b[31mANSI_RED\u001b[0m ...`), so
the SDK delivers them.

```text
stock, after Ctrl+O                      claude-rs
  ⎿  {fg=a1|ANSI_RED} {b,fg=a2|ANSI_BOLD_GREEN} ANSI_UNDERLINE
     {fg=c208|ANSI_256_ORANGE} {fg=c45|ANSI_TRUECOLOR} {rev|ANSI_REVERSE}
     plain ANSI_PLAIN_LINE
                                         ✓ $ ⟩ Bash bash ansi.sh
                                             {fg=c8|ANSI_RED ANSI_BOLD_GREEN ANSI_UNDERLINE}
                                             {fg=c8|ANSI_256_ORANGE ANSI_TRUECOLOR ANSI_REVERSE}
                                             {fg=c8|plain ANSI_PLAIN_LINE}
```

claude-rs removes the escape bytes and shows the whole output in one dim grey,
which is the right default for untrusted bytes but loses what the colours said
(failing tests in red, `git diff --color`, linters). Stock's collapsed view
(no Ctrl+O) shows no tool output at all, so this gap is about the expanded
view.

## 4. Fenced code blocks

~~~text
stock                                    claude-rs
  fn main() {                            ```rust
      let items: Vec<u32> = ...          {180,142,173|fn} {143,161,179|main}() {
      println!("PRES rust {:?}", items);     let items: Vec<{180,142,173|u32}> = ...
  }                                      }
                                         ```
~~~

Both highlight; claude-rs uses a truecolor theme, stock the terminal's 16
colours. claude-rs prints the opening and closing fence lines as plain text
(no colour escape), which keeps the language name visible but adds two rows per
block and no other cue marks where code starts. Code lines also carry two
trailing spaces of padding.

## 5. Vertical spacing

The model's reply has a blank line between every block. As rendered by
claude-rs (blank rows shown as `.`):

~~~text
A paragraph with bold text, ...
  - PRES bullet one                      <- no blank row after the paragraph
  ...
  2. PRES second
      - PRES numbered child
> PRES quoted line one                   <- none between the list and the quote
> PRES quoted line two
.
Name     Qty   Note
...
kiwi      12   a longer cell ...
.
.                                        <- two rows before the next fence
```rust
~~~

Stock keeps one blank row between all blocks.

## 6. Headings

claude-rs renders `#`, `##` and `###` all as `{b|bold}`; stock renders level one
as `{b,i,u|bold italic underline}` and levels two and three as bold. Neither
separates level two from three, but in claude-rs a long answer with several
heading levels has no visible hierarchy.

## 7. Tables

```text
stock                                         claude-rs
  ┌───────┬─────┬────────────────────┐          {b|Name     Qty   Note}
  │ Name  │ Qty │        Note        │          {dim|─────   ────   ────────────────}
  ├───────┼─────┼────────────────────┤          apple      3   fresh
  │ apple │   3 │ fresh              │          kiwi      12   a longer cell that ...
  ├───────┼─────┼────────────────────┤
  │ kiwi  │  12 │ a longer cell ...  │
  └───────┴─────┴────────────────────┘
```

Both right-align the numeric column. claude-rs's compact form is easy to read
for short cells; with long or wrapping cells, rows have no separator to show
where one ends.

## 8 and 9. Links and nested lists

- Link: claude-rs prints `{u,fg=c4|link label} ({u,fg=c4|https://example.com/pres})`;
  stock prints the label only, as an OSC 8 hyperlink.
- Nested bullets: claude-rs indents `PRES nested deeper` by 10 columns, stock by 6.

## Where claude-rs reads better

- Strikethrough is applied (SGR 9); stock prints the literal `~~struck text~~`.
- The URL of a link is visible without hovering.
- Inline code has its own background (`{fg=c15,bg=c0|inline code}`), and the
  syntax theme is not tied to the terminal palette.

## What was not compared

Spinners, tool rows other than Read and Bash, diffs of Edit calls, the footer
and the welcome block, themes on a light background, and pane widths other than
130 columns. The long line wraps identically in both.
