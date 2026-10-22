// SPDX-License-Identifier: Apache-2.0

use crate::agent::model;
use crate::app::agents::SubagentStatus;
use crate::app::footer::{FooterFieldId, FooterLayout, FooterSide};
use crate::app::{App, MessageBlock, MessageRole};
use crate::ui::theme;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const FOOTER_COLUMN_GAP: u16 = 1;
const FIELD_SEPARATOR: &str = " \u{b7} ";
/// Ctrl+T lists the agents (Enter there inspects one); Left Arrow on an empty
/// composer opens the `/agents` inspector directly.
const SHELF_HINT: &str = "ctrl+t list \u{b7} \u{2190} inspect";
fn footer_context_value() -> Color {
    theme::secondary()
}

type FooterItem = Option<(String, Color)>;

/// The plan line and the agent shelf (when there is one), the configured status row, then
/// the user's `statusLine` command output (when set).
pub(crate) struct SerializedFooterRows {
    pub rows: Vec<Line<'static>>,
}

pub(crate) fn serialize_footer_rows(app: &App, total_width: u16) -> SerializedFooterRows {
    let layout = app.config.footer.layout();
    let mut rows = Vec::new();
    if let Some(plan) = build_plan_line(app, total_width) {
        rows.push(plan);
    }
    rows.extend(build_agent_shelf(app, total_width));
    rows.push(serialize_footer(app, &layout, total_width));
    if let Some(status_line) = app.footer.status_line.as_ref() {
        rows.push(compose_footer_row(
            Line::from(Span::styled(
                status_line.clone(),
                Style::default().fg(footer_context_value()),
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
    let running = agents.iter().filter(|a| a.status.is_running()).count();
    if running == 0 {
        return Vec::new();
    }
    let listed = agents.len().min(shelf_row_capacity(app.chat_render.terminal_height));
    let failed = agents.iter().filter(|a| a.status == SubagentStatus::Failed).count();
    let done = agents
        .iter()
        .filter(|a| matches!(a.status, SubagentStatus::Completed | SubagentStatus::Stopped))
        .count();
    let mut header = vec![
        Span::styled("agents  ", Style::default().fg(theme::tertiary())),
        Span::styled(format!("{running} running"), Style::default().fg(app.session_runtime.accent)),
    ];
    if done > 0 {
        header.push(Span::styled(format!(" · {done} done"), Style::default().fg(theme::success())));
    }
    if failed > 0 {
        header.push(Span::styled(
            format!(" · {failed} failed"),
            Style::default().fg(theme::danger()),
        ));
    }
    if listed < agents.len() {
        header.push(Span::styled(
            format!("  +{} more", agents.len() - listed),
            Style::default().fg(theme::tertiary()),
        ));
    }
    let mut rows = vec![compose_footer_row(
        Line::from(header),
        Some((SHELF_HINT.to_owned(), theme::tertiary())),
        width,
        20,
    )];
    let columns = AgentColumns::for_rows(&agents[..listed]);
    rows.extend(agents[..listed].iter().map(|a| render_agent_row(a, width, &columns, false)));
    rows
}

/// Format the configured fields and notices within the chat footer's single row.
pub(crate) fn serialize_footer(app: &App, layout: &FooterLayout, width: u16) -> Line<'static> {
    let width = usize::from(width);
    let notice = footer_notice(app)
        .and_then(|(text, color)| fit_footer_right_text(&text, width).map(|text| (text, color)));
    let notice_width = notice.as_ref().map(|(text, _)| text.width());
    let mut fields: Vec<_> = layout
        .fields()
        .iter()
        .filter(|field| field.visible)
        .filter_map(|field| {
            format_field(app, field.id).map(|(text, color)| RenderedField {
                id: field.id,
                side: field.side,
                text,
                color: field.color.map_or(color, crate::app::footer::FooterColor::color),
            })
        })
        .collect();
    let mut overflow = fields_width(&fields, notice_width).saturating_sub(width);
    // Flexible workspace values shrink before any status badge is omitted.
    for field in fields.iter_mut().rev() {
        let minimum = match field.id {
            FooterFieldId::Directory => 10,
            FooterFieldId::GitBranch => 12,
            _ => continue,
        };
        let reduction = overflow.min(field.text.width().saturating_sub(minimum));
        let target = field.text.width().saturating_sub(reduction);
        if reduction > 0 {
            field.text = if field.id == FooterFieldId::Directory {
                format!(
                    "Loc: {}",
                    fit_location_value(&app.cwd, target.saturating_sub(5)).unwrap_or_default()
                )
            } else {
                fit_footer_right_text(&field.text, target).unwrap_or_default()
            };
            overflow = overflow.saturating_sub(reduction);
        }
    }
    // Earlier entries take precedence; groups never overlap or wrap.
    while fields_width(&fields, notice_width) > width {
        fields.pop();
    }
    let mut left = group_spans(&fields, FooterSide::Left);
    let mut right = group_spans(&fields, FooterSide::Right);
    if let Some((text, color)) = notice {
        if !right.is_empty() {
            right.push(Span::styled(FIELD_SEPARATOR, Style::default().fg(theme::tertiary())));
        }
        right.push(Span::styled(text, Style::default().fg(color)));
    }
    let occupied: usize = left.iter().chain(&right).map(Span::width).sum();
    if !right.is_empty() {
        left.push(Span::raw(" ".repeat(width.saturating_sub(occupied))));
        left.extend(right);
    }
    Line::from(left)
}

struct RenderedField {
    id: FooterFieldId,
    side: FooterSide,
    text: String,
    color: Color,
}

fn fields_width(fields: &[RenderedField], notice_width: Option<usize>) -> usize {
    let left_count = fields.iter().filter(|field| field.side == FooterSide::Left).count();
    let right_count = fields.len() - left_count + usize::from(notice_width.is_some());
    fields.iter().map(|field| field.text.width()).sum::<usize>()
        + notice_width.unwrap_or(0)
        + (left_count.saturating_sub(1) + right_count.saturating_sub(1)) * FIELD_SEPARATOR.width()
        + usize::from(left_count > 0 && right_count > 0)
}

fn group_spans(fields: &[RenderedField], side: FooterSide) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for field in fields.iter().filter(|field| field.side == side) {
        if !spans.is_empty() {
            spans.push(Span::styled(FIELD_SEPARATOR, Style::default().fg(theme::tertiary())));
        }
        spans.push(Span::styled(field.text.clone(), Style::default().fg(field.color)));
    }
    spans
}

fn format_field(app: &App, id: FooterFieldId) -> FooterItem {
    let gray = footer_context_value();
    match id {
        FooterFieldId::SessionTitle => app
            .session_runtime
            .session_title
            .as_ref()
            .filter(|title| !title.is_empty())
            .map(|title| (title.clone(), app.session_runtime.accent)),
        FooterFieldId::PermissionMode => app.session_runtime.mode.as_ref().map(|mode| {
            (format!("[{}]", mode.current_mode_name), mode_color(&mode.current_mode_id))
        }),
        FooterFieldId::Model => app
            .session_runtime
            .current_model
            .as_ref()
            .map(|model| (format!("[{}]", model.display_name_short), gray)),
        FooterFieldId::Effort => app
            .session_runtime
            .current_model
            .as_ref()
            .filter(|model| model.supports_effort)
            .and_then(|_| app.session_effort())
            .map(|effort| (format!("[{}]", footer_effort_label(effort)), gray)),
        FooterFieldId::Ultracode => app
            .session_runtime
            .ultracode
            .filter(|state| state.effective())
            .map(|_| ("[Ultracode]".to_owned(), theme::ultracode_accent())),
        FooterFieldId::FastMode => {
            let (text, color) = fast_mode_badge(app.session_runtime.fast_mode_state);
            Some((format!("[{text}]"), color))
        }
        FooterFieldId::RunningSubagents => {
            let running = crate::app::agents::running_subagent_count(app);
            (running > 0).then(|| (format!("[AGENTS:{running}]"), theme::warning()))
        }
        FooterFieldId::Directory => Some((format!("Loc: {}", app.cwd), gray)),
        FooterFieldId::GitBranch => app
            .git_branch()
            .filter(|branch| !branch.is_empty())
            .map(|branch| (format!("Branch: {branch}"), gray)),
        FooterFieldId::ContextRemaining => {
            app.session_runtime.session_usage.context_usage_percent.map(|used| {
                (format!("Context: {}%", 100_u8.saturating_sub(used)), gauge_color(f64::from(used)))
            })
        }
        FooterFieldId::RateLimits => rate_limits_field(app),
        FooterFieldId::Agents => finished_agents_field(app),
    }
}

/// Used share of the five-hour and seven-day windows, from the SDK usage snapshot, or from
/// the latest rate-limit event while no snapshot has arrived. Colored by the fuller window.
fn rate_limits_field(app: &App) -> FooterItem {
    let snapshot =
        app.usage.snapshot.as_ref().filter(|s| s.source == crate::app::UsageSourceKind::Sdk);
    let mut windows: Vec<(&str, f64)> = [
        ("5h", snapshot.and_then(|s| s.five_hour.as_ref())),
        ("7d", snapshot.and_then(|s| s.seven_day.as_ref())),
    ]
    .into_iter()
    .filter_map(|(label, window)| window.map(|window| (label, window.utilization)))
    .collect();
    if windows.is_empty()
        && let Some(update) = app.session_runtime.last_rate_limit_update.as_ref()
        && let (Some(kind), Some(value)) = (update.rate_limit_type.as_deref(), update.utilization)
    {
        match kind {
            "five_hour" => windows.push(("5h", value * 100.0)),
            "seven_day" => windows.push(("7d", value * 100.0)),
            _ => {}
        }
    }
    let fullest = windows
        .iter()
        .map(|(_, percent)| *percent)
        .fold(None, |max: Option<f64>, p| Some(max.map_or(p, |max| max.max(p))))?;
    let text = windows
        .iter()
        .map(|(label, percent)| format!("{label} {percent:.0}%"))
        .collect::<Vec<_>>()
        .join(" ");
    Some((format!("Usage: {text}"), gauge_color(fullest)))
}

/// The agents of the session once none is running; while one runs the shelf shows them.
fn finished_agents_field(app: &App) -> FooterItem {
    let agents = crate::ui::agent_rows::session_agent_rows(app);
    if agents.is_empty() || agents.iter().any(|agent| agent.status.is_running()) {
        return None;
    }
    let failed = agents.iter().filter(|agent| agent.status == SubagentStatus::Failed).count();
    let count = agents.len();
    let noun = if count == 1 { "agent" } else { "agents" };
    Some(if failed > 0 {
        (format!("{count} {noun}, {failed} failed"), theme::danger())
    } else {
        (format!("{count} {noun}"), theme::tertiary())
    })
}

fn gauge_color(percent: f64) -> Color {
    if percent >= 90.0 {
        theme::danger()
    } else if percent >= 70.0 {
        theme::warning()
    } else {
        footer_context_value()
    }
}

fn footer_notice(app: &App) -> FooterItem {
    let permissions = pending_permission_request_count(app);
    if permissions > 0 {
        return Some((format!("{permissions} PEND. PERM."), theme::warning()));
    }
    let auth = mcp_needs_auth_count(app);
    if auth > 0 && should_show_startup_mcp_hint(app) {
        return Some((format!("{auth} MCP NEEDS AUTH"), theme::warning()));
    }
    app.session_runtime
        .mode
        .is_none()
        .then(|| startup_status_badge(app))
        .flatten()
        .map(|(text, color)| (format!("[{text}]"), color))
}

const fn footer_effort_label(effort: model::EffortLevel) -> &'static str {
    match effort {
        model::EffortLevel::Low => "Low",
        model::EffortLevel::Medium => "Med",
        model::EffortLevel::High => "High",
        model::EffortLevel::XHigh => "XHigh",
        model::EffortLevel::Max => "Max",
    }
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

fn should_show_startup_mcp_hint(app: &App) -> bool {
    !app.transcript
        .messages
        .iter()
        .any(|message| matches!(message.role, MessageRole::User | MessageRole::Assistant))
}

fn mode_color(mode_id: &str) -> Color {
    match mode_id {
        "default" => theme::tertiary(),
        "auto" | "acceptEdits" => theme::warning(),
        "plan" => theme::info(),
        "bypassPermissions" | "dontAsk" => theme::danger(),
        _ => theme::purple(),
    }
}

fn fast_mode_badge(state: model::FastModeState) -> (&'static str, Color) {
    match state {
        model::FastModeState::Unknown => ("FAST:?", theme::tertiary()),
        model::FastModeState::Off => ("FAST:OFF", theme::tertiary()),
        model::FastModeState::Cooldown => ("FAST:CD", theme::warning()),
        model::FastModeState::On => ("FAST:ON", theme::accent()),
    }
}

fn startup_status_badge(app: &App) -> Option<(&'static str, Color)> {
    match app.displayed_status() {
        crate::app::AppStatus::Connecting => None,
        crate::app::AppStatus::Ready => Some(("READY", theme::tertiary())),
        crate::app::AppStatus::CommandPending
        | crate::app::AppStatus::Thinking
        | crate::app::AppStatus::Running => Some(("WORKING", theme::warning())),
        crate::app::AppStatus::Error => Some(("ERROR", theme::danger())),
    }
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
    use crate::app::{
        App, AppStatus, ChatMessage, InlinePermission, MessageBlock, MessageRole, ModeState,
        ToolCallInfo,
    };
    use tokio::sync::oneshot;

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|span| span.content.as_ref()).collect()
    }

    fn app_with_mode() -> App {
        let mut app = App::test_default();
        app.session_runtime.mode = Some(ModeState {
            current_mode_id: "default".to_owned(),
            current_mode_name: "default".to_owned(),
            available_modes: Vec::new(),
        });
        app
    }

    #[test]
    fn fit_footer_right_text_truncates_when_needed() {
        let text = "Update available: v9.9.9 (current v0.2.0)";
        let fitted = fit_footer_right_text(text, 12).expect("fitted text");
        assert!(fitted.ends_with("..."));
        assert!(UnicodeWidthStr::width(fitted.as_str()) <= 12);
    }

    #[test]
    fn fit_footer_suffix_text_keeps_path_tail() {
        let text = "~/work/company/claude_rust";
        let fitted = fit_footer_suffix_text(text, 14).expect("fitted text");
        assert!(fitted.starts_with("..."));
        assert!(fitted.ends_with("claude_rust"));
        assert!(UnicodeWidthStr::width(fitted.as_str()) <= 14);
    }

    #[test]
    fn serialize_footer_rows_produces_exactly_one_row() {
        let app = App::test_default();
        let serialized = serialize_footer_rows(&app, 80);

        assert_eq!(serialized.rows.len(), 1);
    }

    #[test]
    fn first_row_preserves_mode_and_fast_mode_badges() {
        let app = app_with_mode();
        let serialized = serialize_footer_rows(&app, 80);
        let text = line_text(&serialized.rows[0]);

        assert!(text.contains("[default]"));
        assert!(text.contains("[FAST:OFF]"));
    }

    #[test]
    fn footer_counts_running_subagents_and_hides_the_count_at_zero() {
        let subagent = |id: &str, status, terminal: Option<&str>| {
            let mut task = model::TaskItem::new(id, id, status);
            task.metadata = Some(
                serde_json::json!({ "task_type": "local_agent", "terminal_status": terminal }),
            );
            task
        };
        let mut shell = model::TaskItem::new("shell", "shell", model::TaskStatus::InProgress);
        shell.metadata = Some(serde_json::json!({ "task_type": "local_bash" }));
        let mut app = app_with_mode();
        app.sdk_inventory.tasks = vec![
            subagent("agent-1", model::TaskStatus::InProgress, None),
            subagent("agent-2", model::TaskStatus::InProgress, None),
            subagent("agent-3", model::TaskStatus::Completed, Some("completed")),
            shell,
        ];
        // The configured row; the fork's agent shelf sits above it while subagents run.
        let status_row =
            |app: &App| line_text(&serialize_footer(app, &app.config.footer.layout(), 80));
        let text = status_row(&app);
        assert!(text.contains("[FAST:OFF] · [AGENTS:2]"), "{text}");

        for task in &mut app.sdk_inventory.tasks {
            task.status = model::TaskStatus::Completed;
        }
        let text = status_row(&app);
        assert!(!text.contains("AGENTS"), "{text}");
    }

    #[test]
    fn reported_session_effort_updates_the_independent_effort_field() {
        let mut app = app_with_mode();
        app.session_runtime.session_id = Some("sess-1".into());
        app.session_runtime.current_model =
            Some(model::CurrentModel::new("opus", "Opus", "Opus").supports_effort(true));
        for (value, label) in [
            ("low", "Low"),
            ("medium", "Med"),
            ("high", "High"),
            ("xhigh", "XHigh"),
            ("max", "Max"),
        ] {
            crate::app::handle_client_event(
                &mut app,
                crate::agent::events::ClientEvent::SessionUpdate {
                    session_id: "sess-1".to_owned(),
                    update: model::SessionUpdate::ConfigOptionUpdate(model::ConfigOptionUpdate {
                        option_id: "effortLevel".to_owned(),
                        value: serde_json::json!(value),
                    }),
                },
            );
            let footer = serialize_footer_rows(&app, 120);
            assert!(line_text(&footer.rows[0]).contains(&format!("[{label}]")));
            assert!(line_text(&footer.rows[0]).contains("[Opus]"));
        }
        crate::app::handle_client_event(
            &mut app,
            crate::agent::events::ClientEvent::SessionUpdate {
                session_id: "sess-1".to_owned(),
                update: model::SessionUpdate::ConfigOptionUpdate(model::ConfigOptionUpdate {
                    option_id: "effortLevel".to_owned(),
                    value: serde_json::Value::Null,
                }),
            },
        );
        assert!(line_text(&serialize_footer_rows(&app, 120).rows[0]).contains("[Opus]"));
    }

    #[test]
    fn ultracode_badge_appears_only_for_verified_effective_state() {
        for state in [
            None,
            model::UltracodeState::new(true, false, false),
            model::UltracodeState::new(false, true, false),
            model::UltracodeState::new(false, false, false),
            model::UltracodeState::new(true, true, true),
        ] {
            let mut app = app_with_mode();
            app.session_runtime.ultracode = state;
            let footer = serialize_footer_rows(&app, 120);
            assert_eq!(
                line_text(&footer.rows[0]).contains("Ultracode"),
                state.is_some_and(model::UltracodeState::effective)
            );
            if state.is_some_and(model::UltracodeState::effective) {
                let label = footer.rows[0]
                    .spans
                    .iter()
                    .find(|span| span.content == "[Ultracode]")
                    .expect("Ultracode label");
                assert_eq!(label.style.fg, Some(theme::ultracode_accent()));
                assert!(
                    footer.rows[0].spans.iter().any(|span| span.content.contains("test-model")
                        && span.style.fg == Some(footer_context_value()))
                );
            }
        }
    }

    #[test]
    fn first_row_omits_connecting_badge_when_mode_is_unset() {
        let mut app = App::test_default();
        app.status = AppStatus::Connecting;

        let serialized = serialize_footer_rows(&app, 80);
        let text = line_text(&serialized.rows[0]);

        assert!(!text.contains("[CONNECTING]"));
        assert!(text.contains("[FAST:OFF]"));
    }

    #[test]
    fn footer_preserves_directory_when_space_allows() {
        let app = App::test_default();
        let serialized = serialize_footer_rows(&app, 160);
        let text = line_text(&serialized.rows[0]);

        assert!(text.contains("Loc:"));
    }

    #[test]
    fn pending_permission_hint_wins_on_first_row() {
        let mut app = App::test_default();
        let (response_tx, _response_rx) = oneshot::channel();
        app.transcript.messages.push(ChatMessage::new(
            MessageRole::Assistant,
            vec![MessageBlock::ToolCall(Box::new(ToolCallInfo {
                title: "Read".into(),
                content: vec![],
                pending_permission: Some(InlinePermission {
                    options: vec![],
                    display: None,
                    subagent_context: None,
                    response_tx,
                    selected_index: 0,
                    focused: true,
                }),
                ..crate::app::test_support::named_tool_call_info(
                    "perm-1",
                    "Read",
                    model::ToolCallStatus::Pending,
                )
            }))],
            None,
        ));
        app.index_tool_call("perm-1".into(), 0, 0);
        app.pending_interaction_ids.push("perm-1".into());

        let serialized = serialize_footer_rows(&app, 80);
        let text = line_text(&serialized.rows[0]);
        assert!(text.ends_with("1 PEND. PERM."));
        let directory = tempfile::tempdir().expect("preferences");
        let path = directory.path().join("settings.json");
        std::fs::write(&path, r#"{"presentation":{"footer":[]}}"#).expect("save layout");
        app.config.load_footer(Some(&path));
        let text = line_text(&serialize_footer_rows(&app, 80).rows[0]);
        assert!(text.ends_with("1 PEND. PERM."));
        assert!(!text.contains("FAST:"));
        assert!(!text.contains("Loc:"));
    }

    #[test]
    fn mcp_auth_notice_and_context_are_both_visible_when_space_allows() {
        let mut app = App::test_default();
        app.session_runtime.session_usage.context_usage_percent = Some(62);
        app.mcp.servers.push(McpServerStatus {
            name: "filesystem".to_owned(),
            status: McpServerConnectionStatus::NeedsAuth,
            server_info: None,
            error: None,
            config: None,
            scope: None,
            source: None,
            tools: Vec::new(),
        });

        let serialized = serialize_footer_rows(&app, 160);
        let text = line_text(&serialized.rows[0]);
        assert!(text.contains("1 MCP NEEDS AUTH"));
        assert!(text.contains("Context: 38%"));
    }

    #[test]
    fn narrow_width_preserves_notice_before_configured_fields() {
        let mut app = App::test_default();
        app.mcp.servers.push(McpServerStatus {
            name: "filesystem".to_owned(),
            status: McpServerConnectionStatus::NeedsAuth,
            server_info: None,
            error: None,
            config: None,
            scope: None,
            source: None,
            tools: Vec::new(),
        });

        let serialized = serialize_footer_rows(&app, 36);
        let text = line_text(&serialized.rows[0]);
        assert!(text.ends_with("1 MCP NEEDS AUTH"));
        assert!(text.width() <= 36);
        let layout = FooterLayout::from_saved(Some(&serde_json::json!([
            {"id":"fastMode","visible":true,"side":"left"}
        ])))
        .expect("layout");
        let exact_fit = line_text(&serialize_footer(&app, &layout, 27));
        assert!(exact_fit.starts_with("[FAST:OFF]"));
        assert!(exact_fit.ends_with("1 MCP NEEDS AUTH"));
        assert_eq!(exact_fit.width(), 27);
    }

    #[test]
    fn directory_and_branch_render_without_each_other() {
        let mut app = app_with_mode();
        app.cwd = "~/work/claude_rust".into();
        app.set_git_branch_for_test(Some("feature/footer"));
        for (id, expected, hidden) in [
            ("directory", "Loc: ~/work/claude_rust", "Branch:"),
            ("gitBranch", "Branch: feature/footer", "Loc:"),
        ] {
            let layout = FooterLayout::from_saved(Some(&serde_json::json!([
                {"id":id,"visible":true,"side":"left"}
            ])))
            .expect("layout");
            let text = line_text(&serialize_footer(&app, &layout, 80));
            assert!(text.contains(expected), "{text}");
            assert!(!text.contains(hidden), "{text}");
        }
    }

    #[test]
    fn effort_and_ultracode_do_not_require_a_visible_model() {
        let mut app = app_with_mode();
        app.session_runtime.current_model =
            Some(model::CurrentModel::new("opus", "Opus", "Opus").supports_effort(true));
        app.session_runtime.config_options.insert("effortLevel".into(), serde_json::json!("high"));
        app.session_runtime.ultracode = model::UltracodeState::new(true, true, true);
        let layout = FooterLayout::from_saved(Some(&serde_json::json!([
            {"id":"effort","visible":true,"side":"left"},
            {"id":"ultracode","visible":true,"side":"right"}
        ])))
        .expect("layout");
        let text = line_text(&serialize_footer(&app, &layout, 80));
        assert!(text.starts_with("[High]"));
        assert!(text.ends_with("[Ultracode]"));
        assert!(!text.contains("Opus"));
        app.session_runtime.current_model.as_mut().expect("model").supports_effort = false;
        assert!(!line_text(&serialize_footer(&app, &layout, 80)).contains("High"));
    }

    #[test]
    fn configured_groups_preserve_order_and_anchor_to_opposite_edges() {
        let mut app = app_with_mode();
        app.set_git_branch_for_test(Some("main"));
        app.session_runtime.session_usage.context_usage_percent = Some(28);
        let layout = FooterLayout::from_saved(Some(&serde_json::json!([
            {"id":"fastMode","visible":true,"side":"left"},
            {"id":"permissionMode","visible":true,"side":"left"},
            {"id":"contextRemaining","visible":true,"side":"right"},
            {"id":"gitBranch","visible":true,"side":"right"}
        ])))
        .expect("layout");
        let text = line_text(&serialize_footer(&app, &layout, 100));
        assert!(text.starts_with("[FAST:OFF] · [default]"));
        assert!(text.ends_with("Context: 72% · Branch: main"));
        assert_eq!(text.width(), 100);
    }

    #[test]
    fn unicode_values_never_overlap_or_exceed_the_single_row() {
        let mut app = app_with_mode();
        app.cwd = "~/工作/项目/claude_rust".into();
        app.set_git_branch_for_test(Some("feature/日本語の長いブランチ"));
        let layout = FooterLayout::default();
        for width in 0..=160 {
            let line = serialize_footer(&app, &layout, width);
            assert!(line.width() <= usize::from(width), "width {width}: {}", line_text(&line));
        }
    }

    #[test]
    fn shortened_branch_retains_part_of_its_name_or_is_omitted() {
        let mut app = app_with_mode();
        app.set_git_branch_for_test(Some("feature/configurable-footer"));
        let layout = FooterLayout::from_saved(Some(&serde_json::json!([
            {"id":"gitBranch","visible":true,"side":"left"}
        ])))
        .expect("layout");
        assert!(line_text(&serialize_footer(&app, &layout, 11)).is_empty());
        let shortened = line_text(&serialize_footer(&app, &layout, 12));
        assert!(shortened.contains("f..."));
        assert_eq!(shortened.width(), 12);
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
    fn agent(id: &str, status: model::TaskStatus, name: &str, model: &str) -> model::TaskItem {
        let mut task = model::TaskItem::new(id, "Map the footer code", status);
        task.metadata = Some(
            serde_json::json!({"task_type":"local_agent","subagent_type":name,"model_display":model,"summary":"Analyzing render path","usage":{"total_tokens":34000,"tool_uses":12,"duration_ms":62000}}),
        );
        task
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
        assert!(line_text(&rows[0]).contains("2 running · 1 done"));
        assert!(line_text(&rows[0]).contains("ctrl+t list · ← inspect"), "{}", line_text(&rows[0]));
        for (index, name, model) in [
            (1, "Explore", "Sonnet 5.5"),
            (2, "implementer", "Opus 5.5"),
            (3, "verifier", "Sonnet 5.5"),
        ] {
            let row = line_text(&rows[index]);
            assert!(row.contains(name) && row.contains(model), "{row}");
            assert!(row.contains("12 tools  34k  1m02s"));
        }
        app.chat_render.terminal_height = 23;
        let rows = serialize_footer_rows(&app, 200).rows;
        assert_eq!(rows.len(), 2);
        assert!(line_text(&rows[0]).contains("+3 more"));
        for task in &mut app.sdk_inventory.tasks {
            task.status = model::TaskStatus::Completed;
        }
        let rows = serialize_footer_rows(&app, 200).rows;
        assert_eq!(rows.len(), 1);
        assert!(line_text(&rows[0]).contains("3 agents"));
        assert!(!line_text(&rows[0]).contains("ctrl+t"));
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
        assert!(line_text(&rows[0]).contains("0/1 Current step"));
        assert!(line_text(&rows[1]).contains("agents"));
        assert_eq!(line_text(rows.last().expect("status")), "owner command output");
        app.footer.apply_todo_plan(vec![]);
        assert!(
            !serialize_footer_rows(&app, 100).rows.iter().any(|r| line_text(r).contains("0/1"))
        );
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
                    );
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
        app.sdk_inventory.task_observations.get_mut("old").expect("old").finished_at = Some(
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(5))
                .expect("test clock permits subtraction"),
        );
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
        assert!(line_text(&shelf[1]).contains("new-worker"));
        assert!(!shelf.iter().any(|row| line_text(row).contains("old-worker")));
        assert_eq!(crate::ui::agent_rows::session_agent_rows(&app).len(), 2);
    }

    #[test]
    fn fork_fields_render_title_usage_windows_and_finished_agents() {
        let mut app = fixture();
        app.session_runtime.session_id = Some("session".into());
        crate::app::handle_client_event(
            &mut app,
            crate::agent::events::ClientEvent::StructuredUsageReceived {
                session_id: "session".into(),
                snapshot: Some(
                    serde_json::from_value(serde_json::json!({
                        "five_hour": {"utilization": 72, "resets_at": null},
                        "seven_day": {"utilization": 12, "resets_at": null}
                    }))
                    .expect("SDK snapshot"),
                ),
                error: None,
            },
        );
        let mut task = agent("failed", model::TaskStatus::Completed, "verifier", "Sonnet 5.5");
        task.metadata.as_mut().expect("metadata")["terminal_status"] = serde_json::json!("failed");
        app.sdk_inventory.tasks.push(task);
        let layout = FooterLayout::from_saved(Some(&serde_json::json!([
            {"id":"sessionTitle","visible":true,"side":"left"},
            {"id":"rateLimits","visible":true,"side":"right"},
            {"id":"agents","visible":true,"side":"right"}
        ])))
        .expect("layout");
        let row = serialize_footer(&app, &layout, 120);
        let text = line_text(&row);
        assert!(text.starts_with("system-config@main-2"), "{text}");
        assert!(text.ends_with("Usage: 5h 72% 7d 12% \u{b7} 1 agent, 1 failed"), "{text}");
        let title = row.spans.iter().find(|span| span.content == "system-config@main-2");
        assert_eq!(title.and_then(|span| span.style.fg), Some(app.session_runtime.accent));
        let usage = row.spans.iter().find(|span| span.content.starts_with("Usage:"));
        assert_eq!(usage.and_then(|span| span.style.fg), Some(theme::warning()));
    }

    #[test]
    fn context_remaining_turns_warning_then_danger_as_the_window_fills() {
        assert_eq!(gauge_color(69.0), footer_context_value());
        assert_eq!(gauge_color(70.0), theme::warning());
        assert_eq!(gauge_color(89.0), theme::warning());
        assert_eq!(gauge_color(90.0), theme::danger());
        let mut app = fixture();
        app.session_runtime.session_usage.context_usage_percent = Some(92);
        let row = serialize_footer_rows(&app, 200).rows.remove(0);
        let context = row.spans.iter().find(|span| span.content == "Context: 8%");
        assert_eq!(context.and_then(|span| span.style.fg), Some(theme::danger()));
    }

    #[test]
    fn each_informational_field_renders_its_own_color_instead_of_a_global_override() {
        let mut app = app_with_mode();
        app.cwd = "~/work".into();
        app.set_git_branch_for_test(Some("feature/footer"));
        app.session_runtime.session_usage.context_usage_percent = Some(42);
        let layout = FooterLayout::from_saved(Some(&serde_json::json!([
            {"id":"model","visible":true,"side":"left","color":"cyan"},
            {"id":"directory","visible":true,"side":"left","color":"orange"},
            {"id":"gitBranch","visible":true,"side":"right","color":"pink"},
            {"id":"contextRemaining","visible":true,"side":"right","color":"green"}
        ])))
        .expect("layout");
        let line = serialize_footer(&app, &layout, 160);
        for (text, color) in [
            ("test-model", Color::Rgb(34, 211, 238)),
            ("Loc: ~/work", Color::Rgb(251, 146, 60)),
            ("Branch: feature/footer", Color::Rgb(244, 114, 182)),
            ("Context: 58%", Color::Rgb(74, 222, 128)),
        ] {
            let span = line
                .spans
                .iter()
                .find(|span| span.content.contains(text))
                .expect("associated field");
            assert_eq!(span.style.fg, Some(color), "{text}");
        }
    }
}
