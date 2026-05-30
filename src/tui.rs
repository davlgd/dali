//! Interactive terminal wizard that gathers an [`InstallConfig`].
//!
//! The wizard is deliberately a single scrollable form rather than a sequence
//! of modal screens: the user sees every decision at once, can move freely
//! between fields, and the opinionated defaults are pre-filled so a complete
//! install is often just "set a disk, set a password, go". That directness is
//! the whole UX premise of DALI.

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::config::{InstallConfig, Secret};
use crate::error::{Error, Result};
use crate::system::probe;

/// Run the interactive wizard, returning the completed config, or `None` if
/// the user quit before starting the installation.
pub fn run_wizard(initial: InstallConfig) -> Result<Option<InstallConfig>> {
    let mut terminal = ratatui::try_init().map_err(|e| Error::Tui(e.to_string()))?;
    let result = Wizard::new(initial).run(&mut terminal);
    ratatui::try_restore().map_err(|e| Error::Tui(e.to_string()))?;
    result
}

/// Direction of movement within a wrapping ring of items.
#[derive(Clone, Copy)]
enum Dir {
    Prev,
    Next,
}

impl Dir {
    /// Advance `current` by one in this direction within `len` items, wrapping.
    fn step(self, current: usize, len: usize) -> usize {
        match self {
            Dir::Next => (current + 1) % len,
            Dir::Prev => (current + len - 1) % len,
        }
    }
}

/// What a form row edits.
enum Kind {
    /// Free text.
    Text(String),
    /// Masked text (passwords).
    Secret(String),
    /// A small set of options cycled left/right (e.g. the disk, zram on/off).
    Pick { options: Vec<String>, index: usize },
    /// One value picked from a large list via a filterable popup (locale,
    /// keymap, timezone) — type to filter, arrow-select, Enter to choose.
    Choice { options: Vec<String>, index: usize },
}

/// A single editable row of the form.
struct Field {
    label: &'static str,
    hint: &'static str,
    kind: Kind,
}

impl Field {
    fn text(label: &'static str, hint: &'static str, value: String) -> Self {
        Self {
            label,
            hint,
            kind: Kind::Text(value),
        }
    }

    fn secret(label: &'static str, hint: &'static str) -> Self {
        Self {
            label,
            hint,
            kind: Kind::Secret(String::new()),
        }
    }

    fn pick(label: &'static str, hint: &'static str, options: Vec<String>) -> Self {
        Self {
            label,
            hint,
            kind: Kind::Pick { options, index: 0 },
        }
    }

    /// A list-backed field; `initial` pre-selects a matching option if present.
    fn choice(
        label: &'static str,
        hint: &'static str,
        options: Vec<String>,
        initial: &str,
    ) -> Self {
        let index = options.iter().position(|o| o == initial).unwrap_or(0);
        Self {
            label,
            hint,
            kind: Kind::Choice { options, index },
        }
    }

    /// The value shown to the user (secrets masked).
    fn display(&self) -> String {
        match &self.kind {
            Kind::Text(value) => value.clone(),
            Kind::Secret(value) => "•".repeat(value.chars().count()),
            Kind::Pick { options, index } => options
                .get(*index)
                .map_or_else(|| "<no devices found>".to_owned(), |o| format!("‹ {o} ›")),
            Kind::Choice { options, index } => options.get(*index).cloned().unwrap_or_default(),
        }
    }
}

/// Stable indices of the form's fields.
const DISK: usize = 0;
const HOSTNAME: usize = 1;
const USERNAME: usize = 2;
const USER_PW: usize = 3;
const USER_PW_CONFIRM: usize = 4;
const ROOT_PW: usize = 5;
const ROOT_PW_CONFIRM: usize = 6;
const LOCALE: usize = 7;
const KEYMAP: usize = 8;
const TIMEZONE: usize = 9;
const ZRAM: usize = 10;
const EXTRA: usize = 11;

/// Open filterable-list state for a [`Kind::Choice`] field.
struct Picker {
    /// Index of the field being chosen.
    field: usize,
    /// Current filter text.
    query: String,
    /// Indices into the field's options that match `query`.
    matches: Vec<usize>,
    /// Cursor position within `matches`.
    cursor: usize,
}

/// The wizard state machine.
struct Wizard {
    fields: Vec<Field>,
    selected: usize,
    error: Option<String>,
    /// When `Some`, the wizard is in the modal confirmation step: the built
    /// config is held in [`Self::pending`] and the user must type the target
    /// device name (or "yes") here to actually start. This makes the
    /// destructive confirmation explicit and always visible, and means no
    /// single key can launch a wipe.
    confirm: Option<String>,
    /// The validated config awaiting confirmation.
    pending: Option<InstallConfig>,
    /// When `Some`, a filterable selection popup is open over the form.
    picker: Option<Picker>,
}

impl Wizard {
    fn new(initial: InstallConfig) -> Self {
        let disk_field = match probe::list_disks() {
            disks if !disks.is_empty() => Field::pick(
                "Target disk",
                "← → to choose — WILL BE ERASED",
                disks.into_iter().map(|d| d.to_string()).collect(),
            ),
            _ => Field::text(
                "Target disk",
                "no disks detected — type a device path",
                initial.disk.clone(),
            ),
        };

        // options are ["yes", "no"]: index 0 when zram is on, 1 when off.
        let zram_default = usize::from(!initial.zram_swap);
        let mut zram = Field::pick(
            "Zram swap",
            "← → compressed RAM swap",
            vec!["yes".to_owned(), "no".to_owned()],
        );
        if let Kind::Pick { index, .. } = &mut zram.kind {
            *index = zram_default;
        }

        let fields = vec![
            disk_field,
            Field::text("Hostname", "machine name", initial.hostname),
            Field::text("Username", "your admin account", initial.user.username),
            Field::secret("User password", "for your account"),
            Field::secret("Confirm user pw", "re-type the user password"),
            Field::secret("Root password", "leave empty to lock root (sudo only)"),
            Field::secret("Confirm root pw", "re-type the root password"),
            // Locale / keymap / timezone are list-backed when the system exposes
            // them (i.e. on the Arch ISO); free text otherwise so the wizard
            // stays usable when rehearsing on another distro.
            list_field(
                "Locale",
                "Enter to pick a locale",
                probe::list_locales(),
                &initial.locale,
            ),
            list_field(
                "Keymap",
                "Enter to pick a keymap",
                probe::list_keymaps(),
                &initial.keymap,
            ),
            list_field(
                "Timezone",
                "Enter to pick a timezone",
                probe::list_timezones(),
                &initial.timezone,
            ),
            zram,
            Field::text(
                "Extra packages",
                "optional, comma-separated",
                initial.extra_packages.join(", "),
            ),
        ];

        Self {
            fields,
            selected: 0,
            error: None,
            confirm: None,
            pending: None,
            picker: None,
        }
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> Result<Option<InstallConfig>> {
        loop {
            terminal
                .draw(|frame| self.draw(frame))
                .map_err(|e| Error::Tui(e.to_string()))?;

            let Event::Key(key) = event::read().map_err(|e| Error::Tui(e.to_string()))? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Ctrl-C quits from anywhere.
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(None);
            }

            // The filterable picker, when open, captures all keys.
            if self.picker.is_some() {
                self.handle_picker_key(key.code);
                continue;
            }

            // In the confirmation modal, keys drive the confirmation input only.
            if self.confirm.is_some() {
                if let Some(config) = self.handle_confirm_key(key.code) {
                    return Ok(Some(config));
                }
                continue;
            }

            // Esc quits the form.
            if key.code == KeyCode::Esc {
                return Ok(None);
            }

            // Any keypress clears a stale validation message.
            self.error = None;

            // On a list-backed field, Enter or typing opens the filter popup.
            let on_choice = matches!(self.fields[self.selected].kind, Kind::Choice { .. });
            if on_choice {
                match key.code {
                    KeyCode::Up | KeyCode::BackTab => self.select(Dir::Prev),
                    KeyCode::Down | KeyCode::Tab => self.select(Dir::Next),
                    KeyCode::Enter => self.open_picker(None),
                    KeyCode::Char(c) if !c.is_control() => self.open_picker(Some(c)),
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Up | KeyCode::BackTab => self.select(Dir::Prev),
                KeyCode::Down | KeyCode::Tab => self.select(Dir::Next),
                KeyCode::Left => self.cycle(Dir::Prev),
                KeyCode::Right => self.cycle(Dir::Next),
                KeyCode::Backspace => self.backspace(),
                KeyCode::Char(c) => self.type_char(c),
                KeyCode::Enter => {
                    if self.selected + 1 < self.fields.len() {
                        self.select(Dir::Next);
                    } else if let Some(config) = self.try_build() {
                        // Valid form: enter the modal confirmation step.
                        self.pending = Some(config);
                        self.confirm = Some(String::new());
                    }
                    // (an invalid form leaves `self.error` set by try_build)
                }
                _ => {}
            }
        }
    }

    /// Open the filterable picker for the focused [`Kind::Choice`] field,
    /// optionally seeding the filter with a first character.
    fn open_picker(&mut self, seed: Option<char>) {
        let query: String = seed.into_iter().collect();
        let mut picker = Picker {
            field: self.selected,
            query,
            matches: Vec::new(),
            cursor: 0,
        };
        self.recompute_matches(&mut picker);
        self.picker = Some(picker);
    }

    /// Recompute the filtered match list (case-insensitive substring).
    fn recompute_matches(&self, picker: &mut Picker) {
        let needle = picker.query.to_lowercase();
        if let Kind::Choice { options, .. } = &self.fields[picker.field].kind {
            picker.matches = options
                .iter()
                .enumerate()
                .filter(|(_, opt)| opt.to_lowercase().contains(&needle))
                .map(|(i, _)| i)
                .collect();
        }
        picker.cursor = picker.cursor.min(picker.matches.len().saturating_sub(1));
    }

    /// Handle a key while the selection picker is open.
    fn handle_picker_key(&mut self, code: KeyCode) {
        let Some(mut picker) = self.picker.take() else {
            return;
        };
        match code {
            KeyCode::Esc => return, // dropped: closes without changing the field
            KeyCode::Up => picker.cursor = picker.cursor.saturating_sub(1),
            KeyCode::Down => {
                if picker.cursor + 1 < picker.matches.len() {
                    picker.cursor += 1;
                }
            }
            KeyCode::Backspace => {
                picker.query.pop();
                self.recompute_matches(&mut picker);
            }
            KeyCode::Char(c) if !c.is_control() => {
                picker.query.push(c);
                self.recompute_matches(&mut picker);
            }
            KeyCode::Enter => {
                if let Some(&opt_index) = picker.matches.get(picker.cursor)
                    && let Kind::Choice { index, .. } = &mut self.fields[picker.field].kind
                {
                    *index = opt_index;
                }
                self.select(Dir::Next); // picker closes (not put back) and we advance
                return;
            }
            _ => {}
        }
        self.picker = Some(picker);
    }

    /// Handle a key while the confirmation modal is open. Returns the config to
    /// install once the user has typed the device name (or "yes").
    fn handle_confirm_key(&mut self, code: KeyCode) -> Option<InstallConfig> {
        match code {
            KeyCode::Esc => {
                // Back out to the form without losing anything.
                self.confirm = None;
                self.pending = None;
                self.error = None;
            }
            KeyCode::Backspace => {
                if let Some(input) = self.confirm.as_mut() {
                    input.pop();
                }
            }
            KeyCode::Char(c) if !c.is_control() => {
                if let Some(input) = self.confirm.as_mut() {
                    input.push(c);
                }
            }
            KeyCode::Enter => {
                let typed = self.confirm.clone().unwrap_or_default();
                let typed = typed.trim();
                let disk = self.disk_value();
                let basename = disk.rsplit('/').next().unwrap_or(&disk);
                if typed == basename || typed == disk || typed.eq_ignore_ascii_case("yes") {
                    return self.pending.take();
                }
                self.error = Some(format!("type `{basename}` (or yes) to confirm the wipe"));
            }
            _ => {}
        }
        None
    }

    /// Move the focused field one step in `dir`, wrapping at the ends.
    fn select(&mut self, dir: Dir) {
        let len = self.fields.len();
        self.selected = dir.step(self.selected, len);
    }

    /// Cycle the focused Pick field's option one step in `dir`.
    fn cycle(&mut self, dir: Dir) {
        if let Kind::Pick { options, index } = &mut self.fields[self.selected].kind
            && !options.is_empty()
        {
            *index = dir.step(*index, options.len());
        }
    }

    fn backspace(&mut self) {
        match &mut self.fields[self.selected].kind {
            Kind::Text(value) | Kind::Secret(value) => {
                value.pop();
            }
            Kind::Pick { .. } | Kind::Choice { .. } => {}
        }
    }

    fn type_char(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        match &mut self.fields[self.selected].kind {
            Kind::Text(value) | Kind::Secret(value) => value.push(c),
            Kind::Pick { .. } | Kind::Choice { .. } => {}
        }
    }

    /// Assemble and validate a config from the fields. On failure, store the
    /// message for display and return `None` so the wizard keeps running.
    fn try_build(&mut self) -> Option<InstallConfig> {
        // Catch mistyped masked passwords before they silently lock the user out.
        if self.text(USER_PW) != self.text(USER_PW_CONFIRM) {
            self.error = Some("user passwords do not match".to_owned());
            return None;
        }
        if self.text(ROOT_PW) != self.text(ROOT_PW_CONFIRM) {
            self.error = Some("root passwords do not match".to_owned());
            return None;
        }

        let config = InstallConfig {
            disk: self.disk_value(),
            hostname: self.text(HOSTNAME),
            user: crate::config::UserAccount {
                username: self.text(USERNAME),
                password: Secret::new(self.text(USER_PW)),
            },
            root_password: Secret::new(self.text(ROOT_PW)),
            locale: self.text(LOCALE),
            keymap: self.text(KEYMAP),
            timezone: self.text(TIMEZONE),
            zram_swap: self.pick_value(ZRAM) == "yes",
            extra_packages: parse_packages(&self.text(EXTRA)),
            // default_apps / provision keep their defaults (on); they are not
            // (yet) exposed in the wizard.
            ..InstallConfig::default()
        };

        match config.validate() {
            Ok(()) => Some(config),
            Err(e) => {
                self.error = Some(e.to_string());
                None
            }
        }
    }

    /// The selected option of a Pick field (empty for non-Pick / no options).
    fn pick_value(&self, index: usize) -> String {
        match &self.fields[index].kind {
            Kind::Pick { options, index } => options.get(*index).cloned().unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// The current string value of a text/secret/choice field (empty for a Pick).
    fn text(&self, index: usize) -> String {
        match &self.fields[index].kind {
            Kind::Text(value) | Kind::Secret(value) => value.clone(),
            Kind::Choice { options, index } => options.get(*index).cloned().unwrap_or_default(),
            Kind::Pick { .. } => String::new(),
        }
    }

    /// The selected disk device path, parsed back out of the display string.
    fn disk_value(&self) -> String {
        match &self.fields[DISK].kind {
            Kind::Text(value) => value.clone(),
            Kind::Pick { options, index } => options
                .get(*index)
                .and_then(|o| o.split_whitespace().next())
                .unwrap_or_default()
                .to_owned(),
            Kind::Choice { options, index } => options.get(*index).cloned().unwrap_or_default(),
            Kind::Secret(_) => String::new(),
        }
    }

    fn draw(&self, frame: &mut Frame<'_>) {
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .areas(frame.area());

        let title = Paragraph::new("Davlgd Arch Linux Installer")
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL).title(" DALI "));
        frame.render_widget(title, header);

        let mut lines = self.field_lines();
        if self.confirm.is_none()
            && let Some(error) = &self.error
        {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                format!("  ⚠ {error}"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
        }

        let form = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Configuration (defaults pre-filled) "),
        );
        frame.render_widget(form, body);

        let help = Paragraph::new(
            "↑↓/Tab move   ←→ change option   Enter edit/pick from list   Esc back/quit",
        )
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(help, footer);

        // The confirmation modal floats over everything so it is always fully
        // visible, even on a short console where the form would scroll.
        if self.confirm.is_some() {
            self.draw_confirm_modal(frame);
        }
        if let Some(picker) = &self.picker {
            self.draw_picker_modal(frame, picker);
        }
    }

    /// Render the filterable selection popup for a Choice field.
    fn draw_picker_modal(&self, frame: &mut Frame<'_>, picker: &Picker) {
        const VISIBLE: usize = 10;
        let field = &self.fields[picker.field];
        let Kind::Choice { options, .. } = &field.kind else {
            return;
        };

        // height = VISIBLE rows + filter line + 2 borders + 1 slack.
        let area = centered_rect(frame.area(), 50, 14);
        frame.render_widget(Clear, area);

        let mut lines = vec![Line::from(vec![
            Span::styled("filter: ", Style::default().fg(Color::Gray)),
            Span::styled(
                picker.query.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "   ({} match{})",
                    picker.matches.len(),
                    if picker.matches.len() == 1 { "" } else { "es" }
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ])];

        // Show a window of results around the cursor.
        let start = picker
            .cursor
            .saturating_sub(VISIBLE - 1)
            .min(picker.matches.len().saturating_sub(VISIBLE));
        for (row, &opt_index) in picker.matches.iter().enumerate().skip(start).take(VISIBLE) {
            let focused = row == picker.cursor;
            let marker = if focused { "▶ " } else { "  " };
            let style = if focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{}", options[opt_index]),
                style,
            )));
        }
        if picker.matches.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no match)",
                Style::default().fg(Color::Red),
            )));
        }

        let modal = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Select {} — ↑↓ Enter, Esc cancel ", field.label))
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(modal, area);
    }

    /// Render the centered confirmation popup over the form.
    fn draw_confirm_modal(&self, frame: &mut Frame<'_>) {
        let area = centered_rect(frame.area(), 64, 13);
        frame.render_widget(Clear, area);
        let modal = Paragraph::new(self.confirm_lines())
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Confirm installation ")
                    .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            );
        frame.render_widget(modal, area);
    }

    /// One line per form field (plus a hint line under the focused one).
    fn field_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::with_capacity(self.fields.len() + 1);
        for (i, field) in self.fields.iter().enumerate() {
            let selected = i == self.selected;
            let marker = if selected { "▶ " } else { "  " };
            let label_style = if selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let value_style = Style::default().fg(Color::White).add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
            let mut spans = vec![
                Span::styled(format!("{marker}{:<16}", field.label), label_style),
                Span::styled(Self::value_or_placeholder(field), value_style),
            ];
            // The most destructive fact stays visible regardless of focus/colour.
            if i == DISK {
                spans.push(Span::styled(
                    "  ← WILL BE ERASED",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ));
            }
            lines.push(Line::from(spans));
            if selected {
                lines.push(Line::from(Span::styled(
                    format!("                  {}", field.hint),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        lines
    }

    /// The contents of the confirmation modal: a full summary of what will be
    /// erased and installed, plus the typed-confirmation prompt.
    fn confirm_lines(&self) -> Vec<Line<'static>> {
        let disk = self.disk_value();
        let basename = disk.rsplit('/').next().unwrap_or(&disk).to_owned();
        let root_state = if self.text(ROOT_PW).is_empty() {
            "locked (sudo only)"
        } else {
            "password set"
        };
        let extras = self.text(EXTRA);
        let extras = if extras.trim().is_empty() {
            "none".to_owned()
        } else {
            extras.trim().to_owned()
        };
        let typed = self.confirm.clone().unwrap_or_default();

        let mut lines = vec![
            Line::from(Span::styled(
                format!("This will ERASE {disk}"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::default(),
            Line::from(format!(
                "  hostname {} · user {}",
                self.text(HOSTNAME),
                self.text(USERNAME)
            )),
            Line::from(format!(
                "  locale {} · keymap {} · tz {}",
                self.text(LOCALE),
                self.text(KEYMAP),
                self.text(TIMEZONE)
            )),
            Line::from(format!(
                "  root {root_state} · zram {} · extras {extras}",
                self.pick_value(ZRAM)
            )),
            Line::default(),
            Line::from(Span::styled(
                format!("Type  {basename}  (or yes) then Enter to install — Esc to go back:"),
                Style::default().fg(Color::Yellow),
            )),
            Line::from(Span::styled(
                format!("> {typed}"),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
        ];
        if let Some(error) = &self.error {
            lines.push(Line::from(Span::styled(
                format!("⚠ {error}"),
                Style::default().fg(Color::Red),
            )));
        }
        lines
    }

    fn value_or_placeholder(field: &Field) -> String {
        let value = field.display();
        if value.is_empty() {
            "—".to_owned()
        } else {
            value
        }
    }
}

/// A `width`×`height` rectangle centered within `area` (clamped to it).
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let [horizontal] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(area);
    let [vertical] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(horizontal);
    vertical
}

/// Build a list-backed [`Kind::Choice`] field, falling back to free text when
/// the system list is empty (e.g. running off-Arch), so the wizard always works.
fn list_field(
    label: &'static str,
    hint: &'static str,
    options: Vec<String>,
    initial: &str,
) -> Field {
    if options.is_empty() {
        Field::text(
            label,
            "type a value (system list unavailable)",
            initial.to_owned(),
        )
    } else {
        Field::choice(label, hint, options, initial)
    }
}

/// Parse a comma-separated package list, trimming whitespace and dropping empties.
fn parse_packages(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_packages_trims_and_drops_empties() {
        assert_eq!(
            parse_packages("htop, git ,, neovim"),
            ["htop", "git", "neovim"]
        );
        assert!(parse_packages("   ").is_empty());
        assert!(parse_packages("").is_empty());
    }

    #[test]
    fn disk_value_extracts_device_from_pick_label() {
        let wizard = Wizard {
            fields: vec![Field::pick(
                "Target disk",
                "",
                vec!["/dev/vda (20.0 GiB, QEMU)".to_owned()],
            )],
            selected: 0,
            error: None,
            confirm: None,
            pending: None,
            picker: None,
        };
        assert_eq!(wizard.disk_value(), "/dev/vda");
    }

    #[test]
    fn secret_is_masked_in_display() {
        let field = Field {
            label: "pw",
            hint: "",
            kind: Kind::Secret("hunter2".into()),
        };
        assert_eq!(field.display(), "•".repeat(7));
    }

    fn choice_wizard() -> Wizard {
        Wizard {
            fields: vec![
                Field::choice(
                    "Locale",
                    "",
                    vec![
                        "en_US.UTF-8".to_owned(),
                        "fr_FR.UTF-8".to_owned(),
                        "de_DE.UTF-8".to_owned(),
                    ],
                    "en_US.UTF-8",
                ),
                Field::text("Hostname", "", String::new()),
            ],
            selected: 0,
            error: None,
            confirm: None,
            pending: None,
            picker: None,
        }
    }

    #[test]
    fn picker_filters_and_selects() {
        let mut w = choice_wizard();
        assert_eq!(w.text(0), "en_US.UTF-8"); // pre-selected default

        w.open_picker(None);
        w.handle_picker_key(KeyCode::Char('f'));
        w.handle_picker_key(KeyCode::Char('r'));
        // "fr" matches only the French locale.
        assert_eq!(w.picker.as_ref().unwrap().matches.len(), 1);

        w.handle_picker_key(KeyCode::Enter);
        assert!(w.picker.is_none(), "Enter closes the picker");
        assert_eq!(w.text(0), "fr_FR.UTF-8", "selection applied");
        assert_eq!(w.selected, 1, "focus advances after picking");
    }

    #[test]
    fn picker_escape_keeps_original_value() {
        let mut w = choice_wizard();
        w.open_picker(Some('d'));
        w.handle_picker_key(KeyCode::Esc);
        assert!(w.picker.is_none());
        assert_eq!(w.text(0), "en_US.UTF-8", "Esc cancels without changing");
    }

    #[test]
    fn move_selection_wraps_around() {
        let mut wizard = Wizard {
            fields: vec![
                Field::text("a", "", String::new()),
                Field::text("b", "", String::new()),
            ],
            selected: 0,
            error: None,
            confirm: None,
            pending: None,
            picker: None,
        };
        wizard.select(Dir::Prev);
        assert_eq!(wizard.selected, 1);
        wizard.select(Dir::Next);
        assert_eq!(wizard.selected, 0);
    }
}
