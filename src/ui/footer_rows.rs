// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

use crate::agent::model;
use crate::app::{App, MessageBlock};
use crate::ui::theme;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const FOOTER_COLUMN_GAP: u16 = 1;
const MIN_CONTEXT_LOCATION_WIDTH: usize = 10;
const MIN_CONTEXT_BRANCH_WIDTH: usize = 4;
const FOOTER_CONTEXT_VALUE: Color = Color::Gray;

type FooterItem = Option<(String, Color)>;

pub(crate) struct SerializedFooterRows {
    pub rows: Vec<Line<'static>>,
}

pub(crate) fn serialize_footer_rows(app: &App, total_width: u16) -> SerializedFooterRows {
    let mut rows = Vec::new();
    if let Some(plan) = build_plan_line(app, total_width) {
        rows.push(plan);
    }
    rows.extend(build_agent_shelf(app, total_width));
    rows.extend(base_rows(app, total_width));
    if let Some(status_line) = app.footer.status_line.as_ref() {
        rows.push(compose_footer_row(
            Line::from(Span::styled(
                status_line.clone(),
                Style::default().fg(FOOTER_CONTEXT_VALUE),
            )),
            None,
            total_width,
            0,
        ));
    }
    SerializedFooterRows { rows }
}

pub(crate) fn shelf_row_capacity(height: u16) -> usize {
    if height < 24 { 0 } else { usize::from(height / 4).saturating_sub(1).min(4) }
}

fn build_agent_shelf(app: &App, width: u16) -> Vec<Line<'static>> {
    use crate::ui::agent_rows::{AgentColumns, render_agent_row, session_agent_rows};
    let mut agents = session_agent_rows(app);
    if let Some(wave) = app.sdk_inventory.agent_shelf_wave_started_at {
        agents.retain(|row| {
            row.task.status == model::TaskStatus::InProgress
                || app
                    .sdk_inventory
                    .task_observations
                    .get(&row.task.task_id)
                    .and_then(|o| o.finished_at)
                    .is_some_and(|finished| finished >= wave)
        });
    }
    let running = agents.iter().filter(|a| a.task.status == model::TaskStatus::InProgress).count();
    if running == 0 {
        return Vec::new();
    }
    let listed = agents.len().min(shelf_row_capacity(app.chat_render.terminal_height));
    let failed = agents.iter().filter(|a| terminal_status(a.task) == Some("failed")).count();
    let done = agents
        .iter()
        .filter(|a| {
            a.task.status == model::TaskStatus::Completed
                && terminal_status(a.task) != Some("failed")
        })
        .count();
    let mut header = vec![
        Span::styled("agents  ", Style::default().fg(theme::DIM)),
        Span::styled(format!("{running} running"), Style::default().fg(app.session_runtime.accent)),
    ];
    if done > 0 {
        header.push(Span::styled(format!(" · {done} done"), Style::default().fg(Color::Green)));
    }
    if failed > 0 {
        header.push(Span::styled(format!(" · {failed} failed"), Style::default().fg(Color::Red)));
    }
    if listed < agents.len() {
        header.push(Span::styled(
            format!("  +{} more", agents.len() - listed),
            Style::default().fg(theme::DIM),
        ));
    }
    let mut rows = vec![compose_footer_row(
        Line::from(header),
        Some(("ctrl+t inspect".to_owned(), theme::DIM)),
        width,
        20,
    )];
    let columns = AgentColumns::for_rows(&agents[..listed]);
    rows.extend(agents[..listed].iter().map(|a| render_agent_row(a, width, &columns, false)));
    rows
}

fn terminal_status(task: &model::TaskItem) -> Option<&str> {
    Some(crate::ui::agent_rows::task_status(task))
}

fn gauge_color(percent: f64) -> Color {
    if percent >= 90.0 {
        Color::Red
    } else if percent >= 70.0 {
        Color::Yellow
    } else {
        FOOTER_CONTEXT_VALUE
    }
}

fn gauges(app: &App) -> Vec<Line<'static>> {
    let mut result = Vec::new();
    if let Some(percent) = app.session_runtime.session_usage.context_usage_percent {
        let filled = (usize::from(percent.min(100)) * 8).div_ceil(100).min(8);
        result.push(Line::from(vec![
            Span::styled("ctx ", Style::default().fg(theme::DIM)),
            Span::styled(
                format!("{}{} {percent}%", "▓".repeat(filled), "░".repeat(8 - filled)),
                Style::default().fg(gauge_color(f64::from(percent))),
            ),
        ]));
    }
    let snapshot =
        app.usage.snapshot.as_ref().filter(|s| s.source == crate::app::UsageSourceKind::Sdk);
    for (label, window) in [
        ("5h", snapshot.and_then(|s| s.five_hour.as_ref())),
        ("7d", snapshot.and_then(|s| s.seven_day.as_ref())),
    ] {
        if let Some(window) = window {
            result.push(rate_gauge(label, window.utilization));
        }
    }
    if result.len()
        == usize::from(app.session_runtime.session_usage.context_usage_percent.is_some())
        && let Some(update) = app.session_runtime.last_rate_limit_update.as_ref()
        && let (Some(kind), Some(value)) = (update.rate_limit_type.as_deref(), update.utilization)
    {
        match kind {
            "five_hour" => result.push(rate_gauge("5h", value * 100.0)),
            "seven_day" => result.push(rate_gauge("7d", value * 100.0)),
            _ => {}
        }
    }
    let agents: Vec<_> = app
        .sdk_inventory
        .tasks
        .iter()
        .filter(|t| crate::app::session_agents::is_session_agent_task(t))
        .collect();
    if !agents.is_empty() && !agents.iter().any(|t| t.status == model::TaskStatus::InProgress) {
        let failed = agents.iter().filter(|t| terminal_status(t) == Some("failed")).count();
        let mut spans =
            vec![Span::styled(format!("{} agents", agents.len()), Style::default().fg(theme::DIM))];
        if failed > 0 {
            spans
                .push(Span::styled(format!(" · {failed} failed"), Style::default().fg(Color::Red)));
        }
        result.push(Line::from(spans));
    }
    result
}

fn rate_gauge(label: &str, percent: f64) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label} "), Style::default().fg(theme::DIM)),
        Span::styled(format!("{percent:.0}%"), Style::default().fg(gauge_color(percent))),
    ])
}

fn alerts(app: &App, short: bool) -> Line<'static> {
    let permissions = pending_permission_request_count(app);
    let auth = mcp_needs_auth_count(app);
    let mut items = Vec::new();
    if permissions > 0 {
        items.push(Line::from(Span::styled(
            if short {
                format!("{permissions} perm")
            } else {
                format!(
                    "{permissions} permission{} waiting",
                    if permissions == 1 { "" } else { "s" }
                )
            },
            Style::default().fg(Color::Yellow),
        )));
    }
    if auth > 0 {
        items.push(Line::from(Span::styled(
            if short {
                format!("{auth} MCP auth")
            } else {
                format!("{auth} MCP server{} need auth", if auth == 1 { "" } else { "s" })
            },
            Style::default().fg(Color::Yellow),
        )));
    }
    join_items(items)
}

fn join_items(items: impl IntoIterator<Item = Line<'static>>) -> Line<'static> {
    let mut spans = Vec::new();
    for item in items {
        if item.width() == 0 {
            continue;
        }
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.extend(item.spans);
    }
    Line::from(spans)
}

fn pair(left: Line<'static>, right: Line<'static>, width: usize) -> Line<'static> {
    if right.width() == 0 {
        return left;
    }
    let mut spans = left.spans;
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used + right.width()))));
    spans.extend(right.spans);
    Line::from(spans)
}

fn base_rows(app: &App, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width);
    let identity = identity_line(app, usize::MAX, true, false);
    let location = build_context_line(app, usize::MAX);
    let gauge_items = gauges(app);
    let right = join_items([alerts(app, false), location.clone(), join_items(gauge_items.clone())]);
    if identity.width() + right.width() + 3 <= width {
        return vec![pair(identity, right, width)];
    }
    let mut alerts = alerts(app, false);
    let mut gauge_items = gauge_items;
    if join_items(gauge_items.clone()).width() + alerts.width() + 3 > width {
        alerts = self::alerts(app, true);
    }
    for label in ["7d ", "5h "] {
        if join_items(gauge_items.clone()).width() + alerts.width() + 3 <= width {
            break;
        }
        gauge_items.retain(|item| {
            !(item.spans.first().is_some_and(|s| s.content.starts_with(label))
                && !item.spans.iter().any(|s| s.style.fg == Some(Color::Red)))
        });
    }
    if join_items(gauge_items.clone()).width() + alerts.width() + 3 > width {
        gauge_items.retain(|item| {
            !item.spans.first().is_some_and(|span| span.content.contains(" agents"))
        });
    }
    let mut gauges = join_items(gauge_items.clone());
    if gauges.width() + alerts.width() + 3 > width {
        gauges = Line::from(
            gauges
                .spans
                .into_iter()
                .map(|mut span| {
                    if span.content.contains('▓') || span.content.contains('░') {
                        span.content =
                            span.content.split_whitespace().last().unwrap_or("").to_owned().into();
                    }
                    span
                })
                .collect::<Vec<_>>(),
        );
    }
    if gauges.width() + alerts.width() + 3 > width {
        for label in ["7d ", "5h "] {
            gauge_items.retain(|item| {
                !item.spans.first().is_some_and(|span| span.content.starts_with(label))
            });
            gauges = join_items(gauge_items.clone());
            if gauges.width() + alerts.width() + 3 <= width {
                break;
            }
        }
    }
    let mut location = build_context_line(app, width.saturating_sub(identity.width() + 3));
    if location.width() < MIN_CONTEXT_LOCATION_WIDTH {
        location = Line::default();
    }
    let mut identity = identity;
    if identity.width() + location.width() + 3 > width {
        location = Line::default();
    }
    if identity.width() > width {
        identity = identity_line(app, 12, true, false);
    }
    if identity.width() > width {
        identity = identity_line(app, 12, false, false);
    }
    if identity.width() > width {
        let fixed = identity_line(app, 0, false, true).width();
        identity = identity_line(app, width.saturating_sub(fixed + 2).min(12), false, true);
    }
    vec![pair(identity, location, width), pair(gauges, alerts, width)]
}

fn build_plan_line(app: &App, width: u16) -> Option<Line<'static>> {
    let task_steps = app
        .sdk_inventory
        .tasks
        .iter()
        .filter(|task| crate::app::tasks::is_user_plan_task(task))
        .map(|task| (task.status, task.active_form.as_deref().unwrap_or(&task.subject)))
        .collect::<Vec<_>>();
    let steps = match app.footer.plan_source {
        crate::app::PlanSource::Todo => app
            .footer
            .todo_plan
            .iter()
            .map(|item| (item.status, item.active_form.as_deref().unwrap_or(&item.content)))
            .collect(),
        crate::app::PlanSource::None if task_steps.is_empty() => app
            .footer
            .todo_plan
            .iter()
            .map(|item| (item.status, item.active_form.as_deref().unwrap_or(&item.content)))
            .collect(),
        crate::app::PlanSource::None | crate::app::PlanSource::Tasks => task_steps,
    };
    if steps.is_empty() {
        return None;
    }
    let completed =
        steps.iter().filter(|(status, _)| *status == model::TaskStatus::Completed).count();
    let active = steps
        .iter()
        .find(|(status, _)| *status == model::TaskStatus::InProgress)
        .or_else(|| steps.iter().find(|(status, _)| *status == model::TaskStatus::Pending));
    let cells = steps.len().min(8);
    let filled = (completed * cells + steps.len() / 2) / steps.len();
    let meter = format!("{}{}", "▓".repeat(filled), "░".repeat(cells - filled));
    let label = active.map_or("Done", |(_, label)| *label);
    Some(compose_footer_row(
        Line::from(Span::styled(
            format!("{meter} {completed}/{} {label}", steps.len()),
            Style::default().fg(app.session_runtime.accent),
        )),
        None,
        width,
        0,
    ))
}

fn footer_row_widths(
    total_width: u16,
    right_text: Option<&str>,
    left_min_width: u16,
) -> (u16, u16) {
    if total_width == 0 {
        return (0, 0);
    }

    let Some(right_text) = right_text else {
        return (total_width, 0);
    };

    let left_min_width = left_min_width.min(total_width);
    let available_right =
        total_width.saturating_sub(left_min_width).saturating_sub(FOOTER_COLUMN_GAP);
    if available_right == 0 {
        return (total_width, 0);
    }

    let natural_right_width = u16::try_from(UnicodeWidthStr::width(right_text)).unwrap_or(u16::MAX);
    let right_width = natural_right_width.min(available_right);
    if right_width == 0 {
        return (total_width, 0);
    }

    let left_width = total_width.saturating_sub(right_width).saturating_sub(FOOTER_COLUMN_GAP);
    (left_width, right_width)
}

fn compose_footer_row(
    left: Line<'static>,
    right: FooterItem,
    total_width: u16,
    left_min_width: u16,
) -> Line<'static> {
    if total_width == 0 {
        return Line::default();
    }

    let (left_width, right_width) = footer_row_widths(
        total_width,
        right.as_ref().map(|(text, _)| text.as_str()),
        left_min_width,
    );

    let area = Rect::new(0, 0, total_width, 1);
    let mut buf = Buffer::empty(area);
    let left_area = Rect::new(0, 0, left_width, 1);
    Paragraph::new(left).render(left_area, &mut buf);

    if let Some((right_text, right_color)) = right
        && right_width > 0
        && let Some(fitted) = fit_footer_right_text(&right_text, usize::from(right_width))
    {
        let right_x = left_width.saturating_add(FOOTER_COLUMN_GAP);
        let right_area = Rect::new(right_x, 0, right_width, 1);
        let line = Line::from(Span::styled(fitted, Style::default().fg(right_color)));
        Paragraph::new(line).alignment(Alignment::Right).render(right_area, &mut buf);
    }

    buffer_row_to_line(&buf, area, 0)
}

fn identity_line(app: &App, title_width: usize, effort: bool, compact: bool) -> Line<'static> {
    use crate::ui::agent_rows::clip;
    let mut items = Vec::new();
    if let Some(title) = app.session_runtime.session_title.as_ref().filter(|s| !s.is_empty()) {
        items.push(Line::from(Span::styled(
            clip(title, title_width),
            Style::default().fg(app.session_runtime.accent).bold(),
        )));
    }
    if let Some(model) = app.session_runtime.current_model.as_ref() {
        let mut spans = vec![Span::styled(
            if compact {
                clip(&model.display_name_short, 12)
            } else {
                model.display_name_short.clone()
            },
            Style::default().fg(FOOTER_CONTEXT_VALUE),
        )];
        if effort
            && model.supports_effort
            && let Some(level) = app.session_effort()
        {
            spans.push(Span::styled(
                format!(" {}", footer_effort_label(level)),
                Style::default().fg(theme::DIM),
            ));
        }
        items.push(Line::from(spans));
    }
    if app.session_runtime.ultracode.is_some_and(model::UltracodeState::effective) {
        items.push(Line::from(Span::styled(
            if compact { "Ultra" } else { "Ultracode" },
            Style::default().fg(theme::ULTRACODE_ACCENT),
        )));
    }
    if let Some(mode) = app.session_runtime.mode.as_ref().filter(|m| m.current_mode_id != "default")
    {
        items.push(Line::from(Span::styled(
            if compact {
                let label = match mode.current_mode_id.as_str() {
                    "bypassPermissions" => "Bypass",
                    "acceptEdits" => "Accept",
                    "dontAsk" => "Don't ask",
                    "auto" => "Auto",
                    "plan" => "Plan",
                    _ => &mode.current_mode_name,
                };
                clip(label, 8)
            } else {
                mode.current_mode_name.clone()
            },
            Style::default().fg(mode_color(&mode.current_mode_id)),
        )));
    }
    match app.session_runtime.fast_mode_state {
        model::FastModeState::On => {
            items.push(Line::from(Span::styled("fast", Style::default().fg(theme::RUST_ORANGE))))
        }
        model::FastModeState::Cooldown => items.push(Line::from(Span::styled(
            if compact { "cooldown" } else { "fast cooldown" },
            Style::default().fg(Color::Yellow),
        ))),
        _ => {}
    }
    join_items(items)
}

const fn footer_effort_label(effort: model::EffortLevel) -> &'static str {
    match effort {
        model::EffortLevel::Low => "low",
        model::EffortLevel::Medium => "med",
        model::EffortLevel::High => "high",
        model::EffortLevel::XHigh => "xhigh",
        model::EffortLevel::Max => "max",
    }
}

fn build_context_line(app: &App, max_width: usize) -> Line<'static> {
    let Some((location, branch)) = context_values(app, max_width) else {
        return Line::default();
    };
    let mut spans = vec![Span::styled(location, Style::default().fg(FOOTER_CONTEXT_VALUE))];
    if let Some(branch) = branch {
        spans.push(Span::styled(" · ", Style::default().fg(theme::DIM)));
        spans.push(Span::styled(branch, Style::default().fg(FOOTER_CONTEXT_VALUE)));
    }
    Line::from(spans)
}

fn context_values(app: &App, max_width: usize) -> Option<(String, Option<String>)> {
    const LOCATION_LABEL_WIDTH: usize = 0;
    const BRANCH_WRAP_WIDTH: usize = 3;

    let location_only_width = max_width.saturating_sub(LOCATION_LABEL_WIDTH);
    let branch = app.git_branch().filter(|branch| !branch.is_empty());

    if let Some(branch) = branch {
        let fixed_width = LOCATION_LABEL_WIDTH + BRANCH_WRAP_WIDTH;
        let available_values = max_width.saturating_sub(fixed_width);
        if available_values >= MIN_CONTEXT_LOCATION_WIDTH + MIN_CONTEXT_BRANCH_WIDTH {
            let branch_width = UnicodeWidthStr::width(branch)
                .min(available_values.saturating_sub(MIN_CONTEXT_LOCATION_WIDTH));
            let branch_value = fit_footer_right_text(branch, branch_width);
            let branch_display_width =
                branch_value.as_ref().map_or(0, |value| UnicodeWidthStr::width(value.as_str()));
            let location_width = available_values.saturating_sub(branch_display_width);
            if let Some(location_value) = fit_location_value(&app.cwd, location_width) {
                return Some((location_value, branch_value));
            }
        }
    }

    fit_location_value(&app.cwd, location_only_width).map(|location_value| (location_value, None))
}

fn fit_location_value(cwd: &str, max_width: usize) -> Option<String> {
    if max_width == 0 {
        return None;
    }

    for candidate in location_candidates(cwd) {
        if UnicodeWidthStr::width(candidate.as_str()) <= max_width {
            return Some(candidate);
        }
    }

    fit_footer_suffix_text(cwd, max_width)
}

fn location_candidates(cwd: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique(&mut candidates, Some(cwd.to_owned()));
    push_unique(&mut candidates, trailing_path_components(cwd, 2));
    push_unique(&mut candidates, trailing_path_components(cwd, 1));
    candidates
}

fn trailing_path_components(path: &str, count: usize) -> Option<String> {
    let separator = if path.contains('\\') { "\\" } else { "/" };
    let components: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|component| !component.is_empty() && *component != "~")
        .collect();
    if components.is_empty() {
        return None;
    }
    let start = components.len().saturating_sub(count);
    Some(components[start..].join(separator))
}

fn push_unique(candidates: &mut Vec<String>, candidate: Option<String>) {
    let Some(candidate) = candidate else {
        return;
    };
    if !candidate.is_empty() && !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}

fn fit_footer_right_text(text: &str, max_width: usize) -> Option<String> {
    if max_width == 0 || text.trim().is_empty() {
        return None;
    }

    if UnicodeWidthStr::width(text) <= max_width {
        return Some(text.to_owned());
    }

    if max_width <= 3 {
        return Some(".".repeat(max_width));
    }

    let mut fitted = String::new();
    let mut width: usize = 0;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width.saturating_add(ch_width).saturating_add(3) > max_width {
            break;
        }
        fitted.push(ch);
        width = width.saturating_add(ch_width);
    }

    if fitted.is_empty() {
        return Some("...".to_owned());
    }
    fitted.push_str("...");
    Some(fitted)
}

fn fit_footer_suffix_text(text: &str, max_width: usize) -> Option<String> {
    if max_width == 0 || text.trim().is_empty() {
        return None;
    }

    if UnicodeWidthStr::width(text) <= max_width {
        return Some(text.to_owned());
    }

    if max_width <= 3 {
        return Some(".".repeat(max_width));
    }

    let mut fitted = String::new();
    let mut width = 0usize;
    for ch in text.chars().rev() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width.saturating_add(ch_width).saturating_add(3) > max_width {
            break;
        }
        fitted.insert(0, ch);
        width = width.saturating_add(ch_width);
    }

    if fitted.is_empty() {
        return Some("...".to_owned());
    }

    Some(format!("...{fitted}"))
}

fn pending_permission_request_count(app: &App) -> usize {
    app.pending_interaction_ids
        .iter()
        .filter(|tool_id| {
            let Some((mi, bi)) = app.lookup_tool_call(tool_id) else {
                return false;
            };
            matches!(
                app.transcript.messages.get(mi).and_then(|msg| msg.blocks.get(bi)),
                Some(MessageBlock::ToolCall(tc)) if tc.pending_permission.is_some()
            )
        })
        .count()
}

fn mcp_needs_auth_count(app: &App) -> usize {
    app.mcp
        .servers
        .iter()
        .filter(|server| {
            matches!(server.status, crate::agent::model::McpServerConnectionStatus::NeedsAuth)
        })
        .count()
}

fn mode_color(mode_id: &str) -> Color {
    match mode_id {
        "default" => theme::DIM,
        "auto" | "acceptEdits" => Color::Yellow,
        "plan" => Color::Blue,
        "bypassPermissions" | "dontAsk" => Color::Red,
        _ => Color::Magenta,
    }
}

fn buffer_row_to_line(buf: &Buffer, area: Rect, row: u16) -> Line<'static> {
    let y = area.y.saturating_add(row);
    let mut cells = Vec::with_capacity(usize::from(area.width));
    for x in 0..area.width {
        if let Some(cell) = buf.cell((area.x.saturating_add(x), y)) {
            cells.push((cell.symbol().to_owned(), cell.style()));
        }
    }

    let Some(last_non_blank) = cells
        .iter()
        .rposition(|(symbol, _)| !symbol.is_empty() && !symbol.chars().all(char::is_whitespace))
    else {
        return Line::default();
    };

    let mut spans = Vec::new();
    let mut current_style: Option<Style> = None;
    let mut current_text = String::new();

    for (symbol, style) in cells.into_iter().take(last_non_blank + 1) {
        if symbol.is_empty() {
            continue;
        }
        match current_style {
            Some(existing) if existing == style => current_text.push_str(&symbol),
            Some(existing) => {
                spans.push(Span::styled(std::mem::take(&mut current_text), existing));
                current_text.push_str(&symbol);
                current_style = Some(style);
            }
            None => {
                current_text.push_str(&symbol);
                current_style = Some(style);
            }
        }
    }

    if let Some(style) = current_style
        && !current_text.is_empty()
    {
        spans.push(Span::styled(current_text, style));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model::{McpServerConnectionStatus, McpServerStatus};
    use crate::app::{BlockCache, ChatMessage, InlinePermission, ModeState, ToolCallInfo};
    use tokio::sync::oneshot;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }
    fn fixture() -> App {
        let mut app = App::test_default();
        app.cwd = "~/Github/system-config".to_owned();
        app.session_runtime.session_title = Some("system-config@main-2".to_owned());
        app.session_runtime.current_model =
            Some(model::CurrentModel::new("opus", "Opus 5.5", "Opus 5.5").supports_effort(true));
        app.session_runtime.mode = Some(ModeState {
            current_mode_id: "auto".to_owned(),
            current_mode_name: "Auto".to_owned(),
            available_modes: vec![],
        });
        app.session_runtime.session_usage.context_usage_percent = Some(6);
        app.set_git_branch_for_test(Some("main"));
        app
    }
    fn permission(app: &mut App) {
        let (response_tx, _) = oneshot::channel();
        app.transcript.messages.push(ChatMessage::new(
            crate::app::MessageRole::Assistant,
            vec![MessageBlock::ToolCall(Box::new(ToolCallInfo {
                id: "permission".into(),
                source_message_uuids: vec![],
                title: "Read".into(),
                sdk_tool_name: "Read".into(),
                raw_input: None,
                raw_input_bytes: 0,
                locations: vec![],
                output_metadata: None,
                task_metadata: None,
                status: model::ToolCallStatus::Pending,
                content: vec![],
                hidden: false,
                terminal_id: None,
                terminal_command: None,
                terminal_output: None,
                terminal_output_len: 0,
                cache: BlockCache::default(),
                pending_permission: Some(InlinePermission {
                    options: vec![],
                    display: None,
                    subagent_context: None,
                    response_tx,
                    selected_index: 0,
                    focused: true,
                }),
                pending_question: None,
                history: crate::app::ToolCallHistory::Live,
            }))],
            None,
        ));
        app.index_tool_call("permission".into(), 0, 0);
        app.pending_interaction_ids.push("permission".into());
    }
    fn agent(id: &str, status: model::TaskStatus, name: &str, model: &str) -> model::TaskItem {
        let mut task = model::TaskItem::new(id, "Map the footer code", status);
        task.metadata = Some(
            serde_json::json!({"task_type":"local_agent","subagent_type":name,"model_display":model,"summary":"Analyzing render path","usage":{"total_tokens":34000,"tool_uses":12,"duration_ms":62000}}),
        );
        task
    }
    #[test]
    fn responsive_base_rows_keep_unique_facts_and_alerts() {
        let mut app = fixture();
        permission(&mut app);
        let wide = serialize_footer_rows(&app, 200).rows;
        assert_eq!(wide.len(), 1);
        let row = text(&wide[0]);
        for fact in [
            "system-config@main-2",
            "Opus 5.5",
            "Auto",
            "~/Github/system-config · main",
            "ctx ▓░░░░░░░ 6%",
            "1 permission waiting",
        ] {
            assert_eq!(row.matches(fact).count(), 1, "{row}");
        }
        let medium = serialize_footer_rows(&app, 100).rows;
        assert_eq!(medium.len(), 2);
        assert!(text(&medium[0]).contains("Opus 5.5"));
        assert!(text(&medium[0]).contains("system-config"));
        assert!(text(&medium[1]).contains("1 permission waiting"));
        let narrow = serialize_footer_rows(&app, 50).rows;
        assert_eq!(narrow.len(), 2);
        assert!(text(&narrow[0]).contains("Opus 5.5"));
        assert!(text(&narrow[0]).contains("Auto"));
        assert!(text(&narrow[1]).contains("ctx"));
        assert!(text(&narrow[1]).contains("1 permission waiting"));
        for line in narrow {
            assert!(line.width() <= 50, "{}", text(&line));
        }
    }
    #[test]
    fn alerts_coexist_with_context_and_rate_thresholds() {
        let mut app = fixture();
        permission(&mut app);
        app.session_runtime.session_usage.context_usage_percent = Some(90);
        app.mcp.servers.push(McpServerStatus {
            name: "filesystem".into(),
            status: McpServerConnectionStatus::NeedsAuth,
            server_info: None,
            error: None,
            config: None,
            scope: None,
            source: None,
            tools: vec![],
        });
        let rows = serialize_footer_rows(&app, 50).rows;
        assert!(text(&rows[1]).contains("ctx"));
        assert!(text(&rows[1]).contains("1 perm"));
        assert!(text(&rows[1]).contains("1 MCP auth"));
        assert!(
            rows[1]
                .spans
                .iter()
                .any(|s| s.content.contains("90%") && s.style.fg == Some(Color::Red))
        );
        assert_eq!(gauge_color(69.0), FOOTER_CONTEXT_VALUE);
        assert_eq!(gauge_color(70.0), Color::Yellow);
        assert_eq!(gauge_color(89.0), Color::Yellow);
        assert_eq!(gauge_color(90.0), Color::Red);
    }
    #[test]
    fn defaults_hidden_effective_states_and_effort_shown() {
        let mut app = fixture();
        app.session_runtime.mode.as_mut().expect("mode").current_mode_id = "default".into();
        app.session_runtime.session_id = Some("session".into());
        for (value, label) in [
            ("low", "low"),
            ("medium", "med"),
            ("high", "high"),
            ("xhigh", "xhigh"),
            ("max", "max"),
        ] {
            crate::app::handle_client_event(
                &mut app,
                crate::agent::events::ClientEvent::SessionUpdate {
                    session_id: "session".into(),
                    update: model::SessionUpdate::ConfigOptionUpdate(model::ConfigOptionUpdate {
                        option_id: "effortLevel".into(),
                        value: serde_json::json!(value),
                    }),
                },
            );
            let rows = serialize_footer_rows(&app, 200).rows;
            let row = text(&rows[0]);
            assert!(row.contains(&format!("Opus 5.5 {label}")));
            assert!(!row.contains("Auto"));
            assert!(!row.contains("fast"));
        }
        for state in [
            None,
            model::UltracodeState::new(true, false, false),
            model::UltracodeState::new(true, true, true),
        ] {
            app.session_runtime.ultracode = state;
            let row = serialize_footer_rows(&app, 200).rows.remove(0);
            assert_eq!(
                text(&row).contains("Ultracode"),
                state.is_some_and(model::UltracodeState::effective)
            );
        }
        app.session_runtime.fast_mode_state = model::FastModeState::Cooldown;
        assert!(text(&serialize_footer_rows(&app, 200).rows[0]).contains("fast cooldown"));
    }
    #[test]
    fn shelf_caps_rows_and_leaves_finished_count_in_base() {
        let mut app = fixture();
        app.chat_render.terminal_height = 40;
        app.sdk_inventory.tasks = vec![
            agent("a", model::TaskStatus::InProgress, "Explore", "Sonnet 5.5"),
            agent("b", model::TaskStatus::InProgress, "implementer", "Opus 5.5"),
            agent("c", model::TaskStatus::Completed, "verifier", "Sonnet 5.5"),
        ];
        let rows = serialize_footer_rows(&app, 200).rows;
        assert_eq!(rows.len(), 5);
        assert!(text(&rows[0]).contains("2 running · 1 done"));
        for (index, name, model) in [
            (1, "Explore", "Sonnet 5.5"),
            (2, "implementer", "Opus 5.5"),
            (3, "verifier", "Sonnet 5.5"),
        ] {
            let row = text(&rows[index]);
            assert!(row.contains(name) && row.contains(model), "{row}");
            assert!(row.contains("12 tools  34k  1m02s"));
        }
        app.chat_render.terminal_height = 23;
        let rows = serialize_footer_rows(&app, 200).rows;
        assert_eq!(rows.len(), 2);
        assert!(text(&rows[0]).contains("+3 more"));
        for task in &mut app.sdk_inventory.tasks {
            task.status = model::TaskStatus::Completed;
        }
        let rows = serialize_footer_rows(&app, 200).rows;
        assert_eq!(rows.len(), 1);
        assert!(text(&rows[0]).contains("3 agents"));
        assert!(!text(&rows[0]).contains("ctrl+t"));
    }
    #[test]
    fn plan_precedes_shelf_and_status_command_stays_last() {
        let mut app = fixture();
        app.chat_render.terminal_height = 24;
        app.footer.status_line = Some("owner command output".into());
        app.footer.apply_todo_plan(vec![model::TodoPlanItem {
            content: "Current step".into(),
            active_form: None,
            status: model::TaskStatus::InProgress,
        }]);
        app.sdk_inventory.tasks.push(agent(
            "a",
            model::TaskStatus::InProgress,
            "Explore",
            "Sonnet 5.5",
        ));
        let rows = serialize_footer_rows(&app, 100).rows;
        assert!(text(&rows[0]).contains("0/1 Current step"));
        assert!(text(&rows[1]).contains("agents"));
        assert_eq!(text(rows.last().expect("status")), "owner command output");
        app.footer.apply_todo_plan(vec![]);
        assert!(!serialize_footer_rows(&app, 100).rows.iter().any(|r| text(r).contains("0/1")));
    }
    #[test]
    fn test_backend_preserves_agent_column_associations_under_resize() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = fixture();
        app.chat_render.terminal_height = 40;
        app.sdk_inventory.tasks.push(agent(
            "a",
            model::TaskStatus::InProgress,
            "Explore",
            "Sonnet 5.5",
        ));
        for width in [200, 100, 50] {
            let mut terminal = Terminal::new(TestBackend::new(width, 8)).expect("terminal");
            terminal
                .draw(|frame| {
                    frame.render_widget(
                        Paragraph::new(serialize_footer_rows(&app, width).rows),
                        frame.area(),
                    )
                })
                .expect("draw");
            let buffer = terminal.backend().buffer();
            let agent: String = (0..width).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(agent.contains("Explore") && agent.contains("Sonnet 5.5"), "{agent}");
            assert!(agent.contains("1m02s"));
            if width < 60 {
                assert!(!agent.contains("Analyzing"));
                assert!(!agent.contains("tools"));
            }
        }
    }
    #[test]
    fn critical_windows_finished_agents_and_alerts_fit_narrow_terminal() {
        let mut app = fixture();
        permission(&mut app);
        app.session_runtime.session_usage.context_usage_percent = Some(90);
        app.mcp.servers.push(McpServerStatus {
            name: "filesystem".into(),
            status: McpServerConnectionStatus::NeedsAuth,
            server_info: None,
            error: None,
            config: None,
            scope: None,
            source: None,
            tools: vec![],
        });
        let mut task = agent("failed", model::TaskStatus::Completed, "verifier", "Sonnet 5.5");
        task.metadata.as_mut().expect("metadata")["terminal_status"] = serde_json::json!("failed");
        app.sdk_inventory.tasks.push(task);
        app.session_runtime.last_rate_limit_update = Some(model::RateLimitUpdate {
            status: model::RateLimitStatus::Allowed,
            error_code: None,
            resets_at: None,
            utilization: Some(0.95),
            rate_limit_type: Some("five_hour".into()),
            limit_scope: None,
            overage_status: None,
            overage_resets_at: None,
            overage_disabled_reason: None,
            is_using_overage: None,
            surpassed_threshold: None,
            can_user_purchase_credits: None,
            has_chargeable_saved_payment_method: None,
        });
        app.session_runtime.session_id = Some("session".into());
        crate::app::handle_client_event(&mut app,crate::agent::events::ClientEvent::StructuredUsageReceived {session_id:"session".into(),snapshot:Some(serde_json::from_value(serde_json::json!({"five_hour":{"utilization":95,"resets_at":null},"seven_day":{"utilization":95,"resets_at":null}})).expect("SDK snapshot")),error:None});
        assert!(
            app.usage.snapshot.as_ref().is_some_and(
                |snapshot| snapshot.five_hour.is_some() && snapshot.seven_day.is_some()
            )
        );
        let rows = serialize_footer_rows(&app, 50).rows;
        assert!(rows.iter().all(|row| row.width() <= 50));
        let gauge = text(&rows[1]);
        assert!(gauge.contains("ctx"));
        assert!(gauge.contains("1 perm"));
        assert!(gauge.contains("1 MCP auth"), "{gauge}");
    }
    #[test]
    fn required_model_mode_and_effective_states_fit_fifty_columns() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = fixture();
        let mode = app.session_runtime.mode.as_mut().expect("mode");
        mode.current_mode_id = "bypassPermissions".into();
        mode.current_mode_name = "Bypass Permissions".into();
        app.session_runtime.ultracode = model::UltracodeState::new(true, true, true);
        app.session_runtime.fast_mode_state = model::FastModeState::Cooldown;
        let rows = serialize_footer_rows(&app, 50).rows;
        assert!(rows.iter().all(|row| row.width() <= 50), "{:?}", rows);
        let mut terminal = Terminal::new(TestBackend::new(50, 2)).expect("terminal");
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new(rows), frame.area()))
            .expect("draw");
        let row: String = (0..50).map(|x| terminal.backend().buffer()[(x, 0)].symbol()).collect();
        for value in ["Opus 5.5", "Bypass", "Ultra", "cooldown"] {
            assert!(row.contains(value), "{row}");
        }
    }

    #[test]
    fn shelf_excludes_agents_from_finished_previous_waves() {
        let mut app = fixture();
        app.chat_render.terminal_height = 40;
        app.session_runtime.session_id = Some("session".into());
        let old = agent("old", model::TaskStatus::Completed, "old-worker", "Sonnet 5.5");
        crate::app::handle_client_event(
            &mut app,
            crate::agent::events::ClientEvent::SessionUpdate {
                session_id: "session".into(),
                update: model::SessionUpdate::TaskStateUpdate(
                    model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle)
                        .tasks(vec![old]),
                ),
            },
        );
        app.sdk_inventory.task_observations.get_mut("old").expect("old").finished_at =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(5));
        let running = agent("new", model::TaskStatus::InProgress, "new-worker", "Opus 5.5");
        crate::app::handle_client_event(
            &mut app,
            crate::agent::events::ClientEvent::SessionUpdate {
                session_id: "session".into(),
                update: model::SessionUpdate::TaskStateUpdate(
                    model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle)
                        .tasks(vec![running]),
                ),
            },
        );
        let shelf = build_agent_shelf(&app, 200);
        assert_eq!(shelf.len(), 2);
        assert!(text(&shelf[1]).contains("new-worker"));
        assert!(!shelf.iter().any(|row| text(row).contains("old-worker")));
        assert_eq!(crate::ui::agent_rows::session_agent_rows(&app).len(), 2);
    }
    #[test]
    fn all_permission_modes_preserve_model_and_state_at_fifty_columns() {
        for (id, name, label) in [
            ("auto", "Auto", "Auto"),
            ("acceptEdits", "Accept Edits", "Accept"),
            ("plan", "Plan", "Plan"),
            ("bypassPermissions", "Bypass Permissions", "Bypass"),
            ("dontAsk", "Don't ask", "Don't"),
        ] {
            let mut app = fixture();
            let mode = app.session_runtime.mode.as_mut().expect("mode");
            mode.current_mode_id = id.into();
            mode.current_mode_name = name.into();
            app.session_runtime.ultracode = model::UltracodeState::new(true, true, true);
            app.session_runtime.fast_mode_state = model::FastModeState::Cooldown;
            let rows = serialize_footer_rows(&app, 50).rows;
            assert!(rows.iter().all(|row| row.width() <= 50), "{id}: {:?}", rows);
            let identity = text(&rows[0]);
            assert!(identity.contains("Opus 5.5"));
            assert!(identity.contains(label), "{identity}");
            assert!(identity.contains("cooldown"));
        }
    }
}
