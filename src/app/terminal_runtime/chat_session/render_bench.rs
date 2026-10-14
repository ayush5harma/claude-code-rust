// SPDX-License-Identifier: Apache-2.0

//! Render benchmark: how long one chat frame takes over a realistic transcript.
//!
//! Each frame runs the functions `ChatTerminalSession::draw` runs:
//! `serialize_live_rows_with_boundaries_excluding`, `build_composer_surface`,
//! `prepare_static_history_flush`, `plan_chat_frame`, `render_chat_frame`, and
//! `record_viewport_anchor`. The inline stdout terminal is replaced by a ratatui
//! `Terminal` on a `TestBackend`, which still diffs and flushes every frame. So the
//! numbers cover serialization, Markdown, tool and diff rendering, layout, the composer
//! and footer, and the buffer diff. They exclude scrollback insertion (the history
//! batches `prepare_static_history_flush` builds are never written) and terminal writes
//! (escape sequences, cursor anchoring, synchronized updates).
//!
//! Known limit: `FrameBench::draw` copies the bookkeeping `draw`, `draw_incremental` and
//! `complete_history_flush` do around those calls, because they are bound to the stdout
//! terminal: choosing the excluded ids (none while reading), taking the repaint flag,
//! calling `prepare_static_history_flush` rather than `prepare_history_flush` (no replay
//! path), placing the viewport at the bottom of the screen, treating a frame's history
//! as confirmed afterwards, and not running `complete_draw`. A step added to those
//! functions has to be mirrored here by hand; the planning and painting themselves are
//! shared and need no mirroring.
//!
//! The transcript is the shared render corpus (`tests/fixtures/render-corpus.json`).
//! `scripts/dev/bench-render.mjs` runs this against two refs and compares medians.

use super::{ChatTerminalSession, plan_chat_frame, record_viewport_anchor, render_chat_frame};
use crate::app::connect::render_corpus::RenderCorpus;
use crate::app::{App, HistoryOutputId};
use crate::ui::inline_chat_rows::serialize_live_rows_with_boundaries_excluding;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::collections::BTreeSet;
use std::time::Instant;

const SIZES: [(u16, u16); 2] = [(120, 40), (200, 60)];
/// Redraws of an unchanged, cache-warm transcript per size.
const WARM_FRAMES: usize = 300;
/// Fresh apps whose first frame is measured per size; caches are empty, so this is where
/// Markdown, highlighting and diffing run.
const COLD_FRAMES: usize = 30;
/// Streams of the whole corpus reply per size; every chunk is one measured frame.
const STREAM_PASSES: usize = 10;
/// Rows one scroll step moves; one row per frame gives the most samples.
const SCROLL_STEP_ROWS: isize = 1;
/// Upper bound that keeps a scroll that never reaches the top from running forever.
const SCROLL_FRAME_LIMIT: usize = 20_000;

/// One chat frame at a time, with the bookkeeping `ChatTerminalSession` keeps between them.
struct FrameBench {
    terminal: Terminal<TestBackend>,
    width: u16,
    height: u16,
    /// Rows already handed to scrollback; the live frame leaves them out, as in `draw`.
    confirmed: BTreeSet<HistoryOutputId>,
}

impl FrameBench {
    fn new(app: &mut App, width: u16, height: u16) -> Self {
        app.chat_render.observe_terminal_size(width, height);
        let terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        Self { terminal, width, height, confirmed: BTreeSet::new() }
    }

    /// Draws one frame and returns the first live row it showed.
    fn draw(&mut self, app: &mut App) -> usize {
        let excluded = if app.chat_render.viewport.is_reading() {
            BTreeSet::new()
        } else {
            self.confirmed.clone()
        };
        let serialized = serialize_live_rows_with_boundaries_excluding(app, self.width, &excluded);
        app.surface_dirty.chat.take_repaint();
        let composer = ChatTerminalSession::build_composer_surface(app, self.width);
        let mut history_plan = ChatTerminalSession::prepare_static_history_flush(
            &serialized,
            self.width,
            self.confirmed.clone(),
        );
        let plan = plan_chat_frame(app, &serialized, &mut history_plan, &composer, self.height);
        let viewport_height = plan.requested_layout.viewport_height;
        let area = Rect::new(0, self.height - viewport_height, self.width, viewport_height);
        self.terminal
            .draw(|frame| render_chat_frame(frame, area, app, &plan, &composer))
            .expect("test draw");
        record_viewport_anchor(
            app,
            &serialized,
            plan.layout.live_window.start,
            plan.visible_live_rows.len(),
        );
        // What `complete_history_flush` does once the terminal confirms the insert.
        self.confirmed = history_plan.excluded_ids;
        plan.layout.live_window.start
    }

    fn timed_draw(&mut self, app: &mut App, samples: &mut Vec<u128>) -> usize {
        let started = Instant::now();
        let start = self.draw(app);
        samples.push(started.elapsed().as_micros());
        start
    }
}

fn full_redraw_warm(corpus: &RenderCorpus, (width, height): (u16, u16)) -> Vec<u128> {
    let mut app = corpus.finished_app();
    let mut bench = FrameBench::new(&mut app, width, height);
    // Reading mode serializes every row each frame (nothing is left to scrollback), and the
    // viewport follows the bottom of the transcript until the user scrolls.
    app.chat_render.viewport.pause();
    bench.draw(&mut app);
    let mut samples = Vec::with_capacity(WARM_FRAMES);
    for _ in 0..WARM_FRAMES {
        bench.timed_draw(&mut app, &mut samples);
    }
    samples
}

fn agent_shelf_warm(corpus: &RenderCorpus, (width, height): (u16, u16)) -> Vec<u128> {
    let mut app = corpus.finished_app();
    let tasks = ["Explore", "implementer", "verifier"].into_iter().enumerate().map(|(index, name)| {
        let mut task = crate::agent::model::TaskItem::new(format!("bench-agent-{index}"), "Inspect rendering behavior", crate::agent::model::TaskStatus::InProgress);
        task.metadata = Some(serde_json::json!({ "task_type": "local_agent", "subagent_type": name, "model_display": "Sonnet 5.5", "summary": "Reading footer render functions", "usage": { "total_tokens": 34000, "tool_uses": 12, "duration_ms": 62000 } }));
        task
    }).collect();
    crate::app::tasks::apply_task_state_update(
        &mut app,
        crate::agent::model::TaskStateUpdate::new(crate::agent::model::TaskUpdateSource::Lifecycle)
            .tasks(tasks),
    );
    let mut bench = FrameBench::new(&mut app, width, height);
    bench.draw(&mut app);
    let mut samples = Vec::with_capacity(WARM_FRAMES);
    for _ in 0..WARM_FRAMES {
        bench.timed_draw(&mut app, &mut samples);
    }
    samples
}

fn full_redraw_cold(corpus: &RenderCorpus, (width, height): (u16, u16)) -> Vec<u128> {
    let mut samples = Vec::with_capacity(COLD_FRAMES);
    for _ in 0..COLD_FRAMES {
        let mut app = corpus.finished_app();
        let mut bench = FrameBench::new(&mut app, width, height);
        bench.timed_draw(&mut app, &mut samples);
    }
    samples
}

fn streaming(corpus: &RenderCorpus, (width, height): (u16, u16)) -> Vec<u128> {
    let mut samples = Vec::new();
    for _ in 0..STREAM_PASSES {
        let mut app = corpus.app_with_history();
        let mut bench = FrameBench::new(&mut app, width, height);
        // Commit the resumed history to scrollback first; the reply then streams below it.
        bench.draw(&mut app);
        for update in corpus.stream() {
            RenderCorpus::apply_stream_update(&mut app, update);
            bench.timed_draw(&mut app, &mut samples);
        }
    }
    samples
}

fn scroll_through_history(
    corpus: &RenderCorpus,
    (width, height): (u16, u16),
    step_rows: isize,
) -> Vec<u128> {
    let mut app = corpus.finished_app();
    let mut bench = FrameBench::new(&mut app, width, height);
    app.chat_render.viewport.pause();
    bench.draw(&mut app);
    let mut samples = Vec::new();
    for _ in 0..SCROLL_FRAME_LIMIT {
        app.chat_render.viewport.scroll(-step_rows);
        if bench.timed_draw(&mut app, &mut samples) == 0 {
            break;
        }
    }
    samples
}

fn percentile(sorted: &[u128], percent: usize) -> u128 {
    let index = (sorted.len() * percent).div_ceil(100).saturating_sub(1);
    sorted[index.min(sorted.len() - 1)]
}

fn report(size: (u16, u16), scenario: &str, mut samples: Vec<u128>) {
    samples.sort_unstable();
    // `parseBench` in scripts/dev/bench-render.mjs parses these lines.
    println!(
        "BENCH {}x{} {scenario} median_us={} p95_us={} frames={}",
        size.0,
        size.1,
        percentile(&samples, 50),
        percentile(&samples, 95),
        samples.len(),
    );
}

#[test]
#[ignore = "benchmark: cargo test --release --lib render_bench -- --ignored --nocapture"]
fn render_bench() {
    let corpus = RenderCorpus::load();
    // One unmeasured pass loads the syntax and theme sets and warms the allocator.
    full_redraw_cold(&corpus, SIZES[0]);
    for size in SIZES {
        report(size, "full_redraw_warm", full_redraw_warm(&corpus, size));
        report(size, "full_redraw_cold", full_redraw_cold(&corpus, size));
        if std::env::var_os("CLAUDE_RS_BENCH_AGENTS").is_some() {
            report(size, "agent_shelf_warm", agent_shelf_warm(&corpus, size));
        }
        report(size, "streaming", streaming(&corpus, size));
        report(size, "scroll", scroll_through_history(&corpus, size, SCROLL_STEP_ROWS));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::input::prompt_prefix_text;

    fn screen_rows(bench: &FrameBench) -> Vec<String> {
        let buffer = bench.terminal.backend().buffer();
        (0..bench.height)
            .map(|y| {
                (0..bench.width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn reading_frame_puts_the_transcript_tail_above_the_composer_and_footer() {
        let mut app = RenderCorpus::load().finished_app();
        let mut bench = FrameBench::new(&mut app, 120, 40);
        app.chat_render.viewport.pause();
        bench.draw(&mut app);

        let rows = screen_rows(&bench);
        let tail = rows
            .iter()
            .position(|row| row == "That is the full picture.")
            .expect("the last line of the reply is on screen");
        assert!(rows[tail + 1].starts_with("Reading output"), "{:?}", rows[tail + 1]);
        assert!(rows[tail + 2].contains(&prompt_prefix_text()), "{:?}", rows[tail + 2]);
        assert!(
            rows.last().is_some_and(|row| row.contains("Fake")
                || row.contains("test-model")
                || row.contains("Github")),
            "{:?}",
            rows.last()
        );
    }

    #[test]
    fn following_frame_shows_only_the_live_reply_once_history_is_committed() {
        let corpus = RenderCorpus::load();
        let mut app = corpus.app_with_history();
        let mut bench = FrameBench::new(&mut app, 120, 40);
        bench.draw(&mut app);
        assert!(
            !screen_rows(&bench).iter().any(|row| row.contains("Transcript renderer overview")),
            "committed history belongs to scrollback, not the live viewport"
        );

        for update in corpus.stream() {
            RenderCorpus::apply_stream_update(&mut app, update);
        }
        bench.draw(&mut app);
        let rows = screen_rows(&bench);
        // Everything the reply committed so far is in scrollback; the open tail block and
        // the activity row above the composer are what the live viewport still owns.
        let tail = rows
            .iter()
            .position(|row| row == "That is the full picture.")
            .expect("the open tail of the reply is live");
        assert!(rows[tail + 1].contains('…'), "activity row follows: {:?}", rows[tail + 1]);
        assert!(!rows.iter().any(|row| row.contains("Finding 11")), "{rows:#?}");
    }

    #[test]
    fn scrolling_reaches_the_first_row_and_percentiles_are_nearest_rank() {
        // The corpus renders to about 430 rows at this width, so a 100-row step needs a
        // handful of frames; hitting the limit would mean the top was never reached.
        let samples = scroll_through_history(&RenderCorpus::load(), (120, 40), 100);
        assert!((4..=8).contains(&samples.len()), "scrolled in {} frames", samples.len());

        let sorted: Vec<u128> = (1..=100).collect();
        assert_eq!((percentile(&sorted, 50), percentile(&sorted, 95)), (50, 95));
    }
}
