//! Terminal user interface: file browser → checking → report → export/ignore.
//!
//! The UI deliberately owns no rule logic. Every check runs through `Engine`
//! on a worker thread so the event loop can render progress and honour a
//! cancellation request within the PRD's two-second target.

#[cfg(feature = "tui")]
mod ui {
    use std::io::{self, IsTerminal, Stdout};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{self, Receiver, TryRecvError};
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, bail};
    use crossterm::event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    };
    use crossterm::execute;
    use crossterm::terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    };
    use ratatui::backend::CrosstermBackend;
    use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span, Text};
    use ratatui::widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
    };
    use ratatui::{Frame, Terminal};
    use unicode_width::UnicodeWidthStr;

    use crate::cancel::CancellationToken;
    use crate::config::Config;
    use crate::engine::{CheckOptions, CheckOutcome, Engine, ProgressEvent};
    use crate::ignore::IgnoreStore;
    use crate::model::{Issue, Report, Severity};
    use crate::paths;
    use crate::report::{self, OutputFormat, RenderOptions, execution_device_label};
    use crate::semantic::{DeviceRequest, SemanticMode};

    const TICK: Duration = Duration::from_millis(120);

    #[derive(Debug, Clone)]
    pub struct TuiOptions {
        pub start: Option<PathBuf>,
        pub config: Config,
        pub min_severity: Severity,
        pub semantic_mode: SemanticMode,
        pub device: DeviceRequest,
        pub no_gpu: bool,
        pub include_context: bool,
    }

    pub fn run(options: TuiOptions) -> Result<()> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            bail!("TUI 需要交互式终端；非 TTY 环境请使用 errator check");
        }
        let mut app = App::new(options)?;
        if let Some(start) = app.options.start.clone() {
            if start.is_file() {
                app.start_check(start);
            } else if start.is_dir() {
                app.browser.load(&start)?;
            } else {
                bail!("路径不存在：{}", start.display());
            }
        }

        let mut terminal = setup_terminal()?;
        let result = event_loop(&mut terminal, &mut app);
        restore_terminal(&mut terminal);
        result
    }

    fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
        enable_raw_mode().context("无法启用终端 raw 模式")?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(error).context("无法进入备用屏幕");
        }
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        Ok(terminal)
    }

    fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) {
        let _ = disable_raw_mode();
        let _ = execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            crossterm::cursor::Show
        );
        let _ = terminal.show_cursor();
    }

    fn event_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
        loop {
            app.tick();
            app.poll_worker();
            terminal.draw(|frame| app.draw(frame))?;
            if app.should_quit {
                break;
            }
            if event::poll(TICK)? {
                match event::read()? {
                    Event::Key(key)
                        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                    {
                        app.handle_key(key)?;
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
        }
        app.cancel_active();
        Ok(())
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Screen {
        Browser,
        Checking,
        Report,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum IssueFilter {
        All,
        Error,
        Warning,
        Info,
    }

    impl IssueFilter {
        fn label(self) -> &'static str {
            match self {
                Self::All => "全部",
                Self::Error => "error",
                Self::Warning => "warning",
                Self::Info => "info",
            }
        }

        fn next(self) -> Self {
            match self {
                Self::All => Self::Error,
                Self::Error => Self::Warning,
                Self::Warning => Self::Info,
                Self::Info => Self::All,
            }
        }

        fn matches(self, severity: Severity) -> bool {
            match self {
                Self::All => true,
                Self::Error => severity == Severity::Error,
                Self::Warning => severity == Severity::Warning,
                Self::Info => severity == Severity::Info,
            }
        }
    }

    struct BrowserEntry {
        path: PathBuf,
        name: String,
        is_dir: bool,
        is_parent: bool,
    }

    struct Browser {
        current_dir: PathBuf,
        entries: Vec<BrowserEntry>,
        selected: usize,
        status: String,
    }

    impl Browser {
        fn new(directory: PathBuf) -> Result<Self> {
            let mut browser = Self {
                current_dir: directory,
                entries: Vec::new(),
                selected: 0,
                status: String::new(),
            };
            browser.load(&browser.current_dir.clone())?;
            Ok(browser)
        }

        fn load(&mut self, directory: &Path) -> Result<()> {
            let directory = if directory.is_absolute() {
                directory.to_owned()
            } else {
                std::env::current_dir()?.join(directory)
            };
            if !directory.is_dir() {
                bail!("不是目录：{}", directory.display());
            }
            let mut directories = Vec::new();
            let mut files = Vec::new();
            for entry in std::fs::read_dir(&directory)
                .with_context(|| format!("无法读取目录 {}", directory.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                let file_type = entry.file_type()?;
                if file_type.is_dir() {
                    directories.push(BrowserEntry {
                        path,
                        name,
                        is_dir: true,
                        is_parent: false,
                    });
                } else if file_type.is_file() && paths::is_supported_extension(&path) {
                    files.push(BrowserEntry {
                        path,
                        name,
                        is_dir: false,
                        is_parent: false,
                    });
                }
            }
            directories.sort_by_cached_key(|entry| entry.name.to_lowercase());
            files.sort_by_cached_key(|entry| entry.name.to_lowercase());
            let mut entries = Vec::new();
            if let Some(parent) = directory.parent() {
                entries.push(BrowserEntry {
                    path: parent.to_owned(),
                    name: "..".to_owned(),
                    is_dir: true,
                    is_parent: true,
                });
            }
            entries.append(&mut directories);
            entries.append(&mut files);
            self.current_dir = directory;
            self.entries = entries;
            self.selected = 0;
            self.status = format!("{} 个条目", self.entries.len());
            Ok(())
        }

        fn selected_entry(&self) -> Option<&BrowserEntry> {
            self.entries.get(self.selected)
        }

        fn move_selection(&mut self, delta: isize) {
            if self.entries.is_empty() {
                self.selected = 0;
                return;
            }
            let last = self.entries.len() - 1;
            let next = self.selected as isize + delta;
            self.selected = next.clamp(0, last as isize) as usize;
        }

        fn go_parent(&mut self) -> Result<()> {
            if let Some(parent) = self.current_dir.parent().map(Path::to_owned) {
                self.load(&parent)?;
            }
            Ok(())
        }
    }

    struct ReportView {
        report: Report,
        source_path: PathBuf,
        warnings: Vec<String>,
        filtered: Vec<usize>,
        selected: usize,
        filter: IssueFilter,
        list_state: ListState,
        detail_scroll: u16,
        status: String,
    }

    impl ReportView {
        fn new(report: Report, source_path: PathBuf, warnings: Vec<String>) -> Self {
            let mut view = Self {
                report,
                source_path,
                warnings,
                filtered: Vec::new(),
                selected: 0,
                filter: IssueFilter::All,
                list_state: ListState::default(),
                detail_scroll: 0,
                status: String::new(),
            };
            view.rebuild();
            view.status = if view.report.cancelled {
                "检查已取消，显示部分结果".to_owned()
            } else if view.report.issues.is_empty() {
                "未发现问题".to_owned()
            } else {
                format!("检查完成：{} 个问题", view.report.issues.len())
            };
            view
        }

        fn rebuild(&mut self) {
            self.filtered = self
                .report
                .issues
                .iter()
                .enumerate()
                .filter(|(_, issue)| self.filter.matches(issue.severity))
                .map(|(index, _)| index)
                .collect();
            if self.filtered.is_empty() {
                self.selected = 0;
                self.list_state.select(None);
            } else {
                self.selected = self.selected.min(self.filtered.len() - 1);
                self.list_state.select(Some(self.selected));
            }
            self.detail_scroll = 0;
        }

        fn set_filter(&mut self, filter: IssueFilter) {
            self.filter = filter;
            self.rebuild();
        }

        fn selected_issue(&self) -> Option<&Issue> {
            self.filtered
                .get(self.selected)
                .and_then(|index| self.report.issues.get(*index))
        }

        fn move_selection(&mut self, delta: isize) {
            if self.filtered.is_empty() {
                return;
            }
            let last = self.filtered.len() - 1;
            let next = self.selected as isize + delta;
            self.selected = next.clamp(0, last as isize) as usize;
            self.list_state.select(Some(self.selected));
            self.detail_scroll = 0;
        }

        fn ignore_selected(&mut self) {
            let Some(issue) = self.selected_issue().cloned() else {
                self.status = "没有可忽略的问题".to_owned();
                return;
            };
            let result = (|| -> Result<()> {
                let mut store = IgnoreStore::load_default()?;
                let entry = store.add(&issue.rule_id, &issue.original, None)?;
                self.report.issues.retain(|candidate| {
                    !(candidate.rule_id == issue.rule_id && candidate.original == issue.original)
                });
                self.report.recount();
                self.rebuild();
                self.status = format!(
                    "已忽略并持久化：{} · {}（{}）",
                    entry.rule_id, entry.original, entry.id
                );
                Ok(())
            })();
            if let Err(error) = result {
                self.status = format!("忽略失败：{error}");
            }
        }

        fn export(&mut self, format: OutputFormat) -> Result<PathBuf> {
            let directory = self
                .source_path
                .parent()
                .map(Path::to_owned)
                .unwrap_or_else(|| PathBuf::from("."));
            let stem = self
                .source_path
                .file_stem()
                .map(|stem| sanitize_filename(&stem.to_string_lossy()))
                .filter(|stem| !stem.is_empty())
                .unwrap_or_else(|| "errator-report".to_owned());
            let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let mut path =
                directory.join(format!("{stem}.errator-{timestamp}.{}", format.extension()));
            let mut counter = 2;
            while path.exists() {
                path = directory.join(format!(
                    "{stem}.errator-{timestamp}-{counter}.{}",
                    format.extension()
                ));
                counter += 1;
            }
            let rendered = report::render(&self.report, format, RenderOptions::default())?;
            report::write_report(&path, &rendered, false)?;
            self.status = format!("已导出：{}", path.display());
            Ok(path)
        }
    }

    fn sanitize_filename(value: &str) -> String {
        value
            .chars()
            .map(|character| {
                if matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                ) {
                    '_'
                } else {
                    character
                }
            })
            .collect()
    }

    enum WorkerMessage {
        Progress(ProgressEvent),
        Done(Box<Result<CheckOutcome, String>>),
    }

    struct CheckState {
        path: PathBuf,
        receiver: Receiver<WorkerMessage>,
        cancel: CancellationToken,
        file: String,
        index: usize,
        total: usize,
        issues: usize,
        semantic_device: Option<String>,
        started: Instant,
        spinner: usize,
    }

    struct App {
        options: TuiOptions,
        screen: Screen,
        browser: Browser,
        check: Option<CheckState>,
        report: Option<ReportView>,
        should_quit: bool,
        help: bool,
        export_menu: bool,
        status: String,
    }

    impl App {
        fn new(options: TuiOptions) -> Result<Self> {
            let start = options.start.clone().unwrap_or_else(|| PathBuf::from("."));
            let directory = if start.is_file() {
                start
                    .parent()
                    .map(Path::to_owned)
                    .unwrap_or_else(|| PathBuf::from("."))
            } else {
                start
            };
            let browser = Browser::new(directory)?;
            Ok(Self {
                options,
                screen: Screen::Browser,
                browser,
                check: None,
                report: None,
                should_quit: false,
                help: false,
                export_menu: false,
                status: String::new(),
            })
        }

        fn tick(&mut self) {
            if let Some(check) = self.check.as_mut() {
                check.spinner = check.spinner.wrapping_add(1);
            }
        }

        fn poll_worker(&mut self) {
            let mut finished = None;
            if let Some(check) = self.check.as_mut() {
                loop {
                    match check.receiver.try_recv() {
                        Ok(WorkerMessage::Progress(event)) => match event {
                            ProgressEvent::Started {
                                total_files,
                                semantic_device,
                            } => {
                                check.total = total_files;
                                check.semantic_device = semantic_device;
                            }
                            ProgressEvent::FileStarted { index, total, file } => {
                                check.index = index;
                                check.total = total;
                                check.file = file;
                            }
                            ProgressEvent::FileFinished { issues, .. } => {
                                check.issues = issues;
                            }
                            ProgressEvent::Cancelled => {
                                self.status = "正在取消检查…".to_owned();
                            }
                        },
                        Ok(WorkerMessage::Done(result)) => {
                            finished = Some(*result);
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            finished = Some(Err("检查线程意外结束".to_owned()));
                            break;
                        }
                    }
                }
            }
            if let Some(result) = finished {
                let path = self
                    .check
                    .as_ref()
                    .map(|check| check.path.clone())
                    .unwrap_or_default();
                self.check = None;
                match result {
                    Ok(outcome) => {
                        let warnings = outcome.warnings.clone();
                        let report = outcome.report;
                        self.report = Some(ReportView::new(report, path, warnings));
                        self.screen = Screen::Report;
                        self.status = "检查完成".to_owned();
                    }
                    Err(error) => {
                        self.screen = Screen::Browser;
                        self.status = format!("检查失败：{error}");
                    }
                }
            }
        }

        fn start_check(&mut self, path: PathBuf) {
            self.report = None;
            self.export_menu = false;
            self.help = false;
            let cancel = CancellationToken::new();
            let token = cancel.clone();
            let (sender, receiver) = mpsc::channel();
            let progress_sender = sender.clone();
            let worker_sender = sender;
            let config = self.options.config.clone();
            let options = CheckOptions {
                min_severity: self.options.min_severity,
                include_context: self.options.include_context,
                ignore_rule_ids: Vec::new(),
                semantic_mode: self.options.semantic_mode,
                device: self.options.device,
                no_gpu: self.options.no_gpu,
                cancellation_enabled: true,
            };
            let worker_path = path.clone();
            std::thread::Builder::new()
                .name("errator-check".to_owned())
                .spawn(move || {
                    let result = (|| -> Result<CheckOutcome> {
                        let engine = Engine::new(config)?;
                        engine.run(&worker_path, &options, &token, move |event| {
                            let _ = progress_sender.send(WorkerMessage::Progress(event));
                        })
                    })()
                    .map_err(|error| format!("{error:#}"));
                    let _ = worker_sender.send(WorkerMessage::Done(Box::new(result)));
                })
                .expect("failed to spawn check worker");

            self.check = Some(CheckState {
                path,
                receiver,
                cancel,
                file: String::new(),
                index: 0,
                total: 0,
                issues: 0,
                semantic_device: None,
                started: Instant::now(),
                spinner: 0,
            });
            self.screen = Screen::Checking;
            self.status.clear();
        }

        fn cancel_active(&self) {
            if let Some(check) = &self.check {
                check.cancel.cancel();
            }
        }

        fn browse_from_report(&mut self) {
            if let Some(report) = &self.report
                && report
                    .source_path
                    .parent()
                    .map(|parent| parent != self.browser.current_dir)
                    .unwrap_or(false)
            {
                let _ = self
                    .browser
                    .load(report.source_path.parent().unwrap_or(Path::new(".")));
            }
            self.screen = Screen::Browser;
            self.export_menu = false;
        }

        fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c'))
            {
                if self.check.is_some() {
                    self.cancel_active();
                    self.status = "已请求取消检查…".to_owned();
                } else {
                    self.should_quit = true;
                }
                return Ok(());
            }

            if self.help {
                self.help = false;
                return Ok(());
            }

            if self.export_menu {
                if let Some(report) = self.report.as_mut() {
                    let format = match key.code {
                        KeyCode::Char('m') => Some(OutputFormat::Markdown),
                        KeyCode::Char('h') => Some(OutputFormat::Html),
                        KeyCode::Char('j') => Some(OutputFormat::Json),
                        KeyCode::Char('c') => Some(OutputFormat::Csv),
                        _ => None,
                    };
                    if let Some(format) = format {
                        match report.export(format) {
                            Ok(path) => {
                                self.status = format!("已导出：{}", path.display());
                            }
                            Err(error) => {
                                self.status = format!("导出失败：{error}");
                            }
                        }
                    }
                }
                self.export_menu = false;
                return Ok(());
            }

            match self.screen {
                Screen::Browser => self.handle_browser_key(key),
                Screen::Checking => self.handle_checking_key(key),
                Screen::Report => self.handle_report_key(key),
            }
        }

        fn handle_browser_key(&mut self, key: KeyEvent) -> Result<()> {
            match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Esc => self.should_quit = true,
                KeyCode::Char('?') => self.help = true,
                KeyCode::Down | KeyCode::Char('j') => self.browser.move_selection(1),
                KeyCode::Up | KeyCode::Char('k') => self.browser.move_selection(-1),
                KeyCode::PageDown => self.browser.move_selection(10),
                KeyCode::PageUp => self.browser.move_selection(-10),
                KeyCode::Home | KeyCode::Char('g') => self.browser.selected = 0,
                KeyCode::End | KeyCode::Char('G') => {
                    self.browser.selected = self.browser.entries.len().saturating_sub(1);
                }
                KeyCode::Char('r') => {
                    let result = self.browser.load(&self.browser.current_dir.clone());
                    if let Err(error) = result {
                        self.status = format!("刷新失败：{error}");
                    } else {
                        self.status = "已刷新".to_owned();
                    }
                }
                KeyCode::Backspace | KeyCode::Left => {
                    if let Err(error) = self.browser.go_parent() {
                        self.status = format!("无法返回上级：{error}");
                    }
                }
                KeyCode::Enter | KeyCode::Right => {
                    let Some(entry) = self.browser.selected_entry() else {
                        return Ok(());
                    };
                    if entry.is_dir {
                        let path = entry.path.clone();
                        if let Err(error) = self.browser.load(&path) {
                            self.status = format!("无法打开目录：{error}");
                        }
                    } else {
                        let path = entry.path.clone();
                        self.start_check(path);
                    }
                }
                _ => {}
            }
            Ok(())
        }

        fn handle_checking_key(&mut self, key: KeyEvent) -> Result<()> {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.cancel_active();
                    self.status = "已请求取消，正在等待当前步骤结束…".to_owned();
                }
                KeyCode::Char('?') => self.help = true,
                _ => {}
            }
            Ok(())
        }

        fn handle_report_key(&mut self, key: KeyEvent) -> Result<()> {
            match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Esc => self.browse_from_report(),
                KeyCode::Char('?') => self.help = true,
                KeyCode::Char('e') => self.export_menu = true,
                KeyCode::Char('f') => {
                    if let Some(report) = self.report.as_mut() {
                        report.set_filter(report.filter.next());
                    }
                }
                KeyCode::Char('0') => {
                    if let Some(report) = self.report.as_mut() {
                        report.set_filter(IssueFilter::All);
                    }
                }
                KeyCode::Char('1') => {
                    if let Some(report) = self.report.as_mut() {
                        report.set_filter(IssueFilter::Error);
                    }
                }
                KeyCode::Char('2') => {
                    if let Some(report) = self.report.as_mut() {
                        report.set_filter(IssueFilter::Warning);
                    }
                }
                KeyCode::Char('3') => {
                    if let Some(report) = self.report.as_mut() {
                        report.set_filter(IssueFilter::Info);
                    }
                }
                KeyCode::Char('i') => {
                    if let Some(report) = self.report.as_mut() {
                        report.ignore_selected();
                        self.status = report.status.clone();
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(report) = self.report.as_mut() {
                        report.move_selection(1);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(report) = self.report.as_mut() {
                        report.move_selection(-1);
                    }
                }
                KeyCode::PageDown => {
                    if let Some(report) = self.report.as_mut() {
                        report.move_selection(10);
                    }
                }
                KeyCode::PageUp => {
                    if let Some(report) = self.report.as_mut() {
                        report.move_selection(-10);
                    }
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    if let Some(report) = self.report.as_mut() {
                        report.selected = 0;
                        report.list_state.select(Some(0));
                    }
                }
                KeyCode::End | KeyCode::Char('G') => {
                    if let Some(report) = self.report.as_mut()
                        && !report.filtered.is_empty()
                    {
                        report.selected = report.filtered.len() - 1;
                        report.list_state.select(Some(report.selected));
                    }
                }
                _ => {}
            }
            Ok(())
        }

        fn draw(&mut self, frame: &mut Frame<'_>) {
            match self.screen {
                Screen::Browser => self.draw_browser(frame),
                Screen::Checking => self.draw_checking(frame),
                Screen::Report => self.draw_report(frame),
            }
            if self.help {
                draw_help(frame);
            }
            if self.export_menu {
                draw_export_menu(frame);
            }
        }

        fn draw_browser(&mut self, frame: &mut Frame<'_>) {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(3),
                    Constraint::Length(4),
                ])
                .split(area);
            let title = format!(" Errator {} · 选择文件 ", env!("CARGO_PKG_VERSION"));
            let path_line = Line::from(vec![
                Span::styled("目录：", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    self.browser.current_dir.display().to_string(),
                    Style::default().fg(Color::Cyan),
                ),
            ]);
            let header = Paragraph::new(path_line).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(title),
            );
            frame.render_widget(header, chunks[0]);

            let items: Vec<ListItem> = self
                .browser
                .entries
                .iter()
                .map(|entry| {
                    let (prefix, style) = if entry.is_parent {
                        ("[..] ", Style::default().fg(Color::DarkGray))
                    } else if entry.is_dir {
                        ("[DIR] ", Style::default().fg(Color::Cyan))
                    } else {
                        let label = entry
                            .path
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .map(|extension| extension.to_ascii_uppercase())
                            .unwrap_or_default();
                        let style = if label == "DOCX" {
                            Style::default().fg(Color::Blue)
                        } else {
                            Style::default().fg(Color::Reset)
                        };
                        return ListItem::new(Line::from(vec![
                            Span::styled("[", style),
                            Span::styled(format!("{label:<4}"), style),
                            Span::styled("] ", style),
                            Span::raw(entry.name.clone()),
                        ]));
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, style),
                        Span::styled(entry.name.clone(), style),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(format!(" 文件（{} 个条目） ", self.browser.entries.len())),
                )
                .highlight_style(
                    Style::default()
                        .bg(Color::Cyan)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("› ");
            let mut state =
                ListState::default().with_selected(if self.browser.entries.is_empty() {
                    None
                } else {
                    Some(self.browser.selected)
                });
            frame.render_stateful_widget(list, chunks[1], &mut state);

            let footer = Paragraph::new(Text::from(vec![
                Line::from(Span::styled(
                    self.browser.status.clone(),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "↑/↓ 选择  Enter 打开  Backspace 上级  r 刷新  ? 帮助  q 退出",
                    Style::default().fg(Color::Gray),
                )),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded),
            );
            frame.render_widget(footer, chunks[2]);
        }

        fn draw_checking(&mut self, frame: &mut Frame<'_>) {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(35),
                    Constraint::Length(8),
                    Constraint::Min(1),
                ])
                .split(area);
            let Some(check) = &self.check else {
                return;
            };
            let spinner = ["|", "/", "-", "\\"][check.spinner % 4];
            let progress = if check.total > 0 {
                format!("{}/{}", check.index, check.total)
            } else {
                "准备中".to_owned()
            };
            let elapsed = check.started.elapsed().as_secs();
            let path = if check.file.is_empty() {
                check.path.display().to_string()
            } else {
                check.file.clone()
            };
            let text = vec![
                Line::from(Span::styled(
                    format!("{spinner} 正在检查  {progress}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(path, Style::default().fg(Color::Gray))),
                Line::from(""),
                Line::from(Span::styled(
                    format!("已用时 {elapsed}s · 可随时取消（Esc）"),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    format!(
                        "计算设备：{}",
                        checking_device_label(check.semantic_device.as_deref())
                    ),
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let paragraph = Paragraph::new(text)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(" Errator 检查 "),
                );
            frame.render_widget(paragraph, chunks[1]);
            if !self.status.is_empty() {
                let status = Paragraph::new(self.status.clone())
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::Yellow));
                frame.render_widget(status, chunks[2]);
            }
        }

        fn draw_report(&mut self, frame: &mut Frame<'_>) {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(5),
                    Constraint::Min(5),
                    Constraint::Length(4),
                ])
                .split(area);
            let Some(report) = self.report.as_mut() else {
                return;
            };
            let device = execution_device_label(&report.report.semantic);
            let source_line = truncate_width(
                &format!("源文件：{}", report.report.source),
                area.width.saturating_sub(2) as usize,
            );
            let header = Paragraph::new(Text::from(vec![
                Line::from(vec![
                    Span::styled("计算设备：", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        device.clone(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "   error {}  ·  warning {}  ·  info {}",
                            report.report.summary.by_severity.error,
                            report.report.summary.by_severity.warning,
                            report.report.summary.by_severity.info
                        ),
                        Style::default().fg(Color::Gray),
                    ),
                ]),
                Line::from(Span::styled(source_line, Style::default().fg(Color::Gray))),
                Line::from(Span::styled(
                    format!(
                        "筛选：{}（f 切换，0/1/2/3 直选）  显示 {}/{} 条{}",
                        report.filter.label(),
                        report.filtered.len(),
                        report.report.issues.len(),
                        if report.report.cancelled {
                            " · 已取消，部分结果"
                        } else {
                            ""
                        }
                    ),
                    Style::default().fg(Color::Yellow),
                )),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(format!(
                        " Errator {} · 勘误报告 ",
                        env!("CARGO_PKG_VERSION")
                    )),
            );
            frame.render_widget(header, chunks[0]);

            let use_horizontal = area.width >= 100;
            let panes = if use_horizontal {
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(54), Constraint::Percentage(46)])
                    .split(chunks[1])
            } else {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
                    .split(chunks[1])
            };

            let items: Vec<ListItem> = if report.filtered.is_empty() {
                vec![ListItem::new(Line::from(Span::styled(
                    if report.report.issues.is_empty() {
                        "未发现问题。"
                    } else {
                        "当前筛选条件下没有问题。"
                    },
                    Style::default().fg(Color::Green),
                )))]
            } else {
                report
                    .filtered
                    .iter()
                    .filter_map(|index| report.report.issues.get(*index))
                    .map(|issue| ListItem::new(issue_line(issue)))
                    .collect()
            };
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(" 问题 "),
                )
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("› ");
            frame.render_stateful_widget(list, panes[0], &mut report.list_state);

            let detail = report
                .selected_issue()
                .map(issue_detail_text)
                .unwrap_or_else(|| {
                    Text::from(vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            "没有可显示的问题。",
                            Style::default().fg(Color::DarkGray),
                        )),
                        Line::from(""),
                        Line::from(Span::styled(
                            "按 e 导出报告，按 Esc 返回文件浏览。",
                            Style::default().fg(Color::DarkGray),
                        )),
                    ])
                });
            let detail = Paragraph::new(detail)
                .wrap(Wrap { trim: false })
                .scroll((report.detail_scroll, 0))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(" 详情 "),
                );
            frame.render_widget(detail, panes[1]);

            let mut footer_lines = vec![Line::from(Span::styled(
                "↑/↓ 选择  i 忽略并持久化  e 导出  f 筛选  Esc 返回  ? 帮助  q 退出",
                Style::default().fg(Color::Gray),
            ))];
            let mut detail = format!("计算设备：{device}");
            if !self.status.is_empty() {
                detail.push_str(" · ");
                detail.push_str(&self.status);
            } else if let Some(warning) = &report.report.semantic_warning {
                detail.push_str(" · 语义提示：");
                detail.push_str(warning);
            } else if let Some(warning) = report.warnings.first() {
                detail.push_str(" · ");
                detail.push_str(warning);
            } else {
                detail.push_str(" · 忽略库：");
                detail.push_str(
                    &paths::ignores_path()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|_| "~/.errator/ignores.json".to_owned()),
                );
            }
            let detail = truncate_width(&detail, area.width.saturating_sub(2) as usize);
            footer_lines.push(Line::from(Span::styled(
                detail,
                Style::default().fg(Color::DarkGray),
            )));
            let footer = Paragraph::new(Text::from(footer_lines)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded),
            );
            frame.render_widget(footer, chunks[2]);
        }
    }

    fn issue_line(issue: &Issue) -> Line<'static> {
        let (label, color) = match issue.severity {
            Severity::Error => ("E", Color::Red),
            Severity::Warning => ("W", Color::Yellow),
            Severity::Info => ("I", Color::Cyan),
        };
        let location = match &issue.file {
            Some(file) => format!("{file} · {}", issue.location.short()),
            None => issue.location.short(),
        };
        Line::from(vec![
            Span::styled(
                format!("[{label}] "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<24}", truncate_width(&issue.rule_id, 24)),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("{:<12}", truncate_width(&location, 12)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(truncate_width(&one_line(&issue.original), 42)),
        ])
    }

    fn issue_detail_text(issue: &Issue) -> Text<'static> {
        let color = match issue.severity {
            Severity::Error => Color::Red,
            Severity::Warning => Color::Yellow,
            Severity::Info => Color::Cyan,
        };
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("[{}] ", issue.severity.label()),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    issue.rule_id.clone(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("位置：", Style::default().fg(Color::DarkGray)),
                Span::raw(format!(
                    "{}（{}）",
                    issue.location.short(),
                    issue.location.kind()
                )),
            ]),
        ];
        if let Some(file) = &issue.file {
            lines.push(Line::from(vec![
                Span::styled("文件：", Style::default().fg(Color::DarkGray)),
                Span::raw(file.clone()),
            ]));
        }
        lines.extend([
            Line::from(""),
            Line::from(Span::styled("原文", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(
                issue.original.clone(),
                Style::default().fg(Color::Red),
            )),
            Line::from(""),
            Line::from(Span::styled("建议", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(
                issue.suggestion.clone(),
                Style::default().fg(Color::Green),
            )),
            Line::from(""),
            Line::from(issue.message.clone()),
            Line::from(""),
            Line::from(vec![
                Span::styled("来源：", Style::default().fg(Color::DarkGray)),
                Span::raw(format!(
                    "{}（置信度 {:.2}）",
                    issue.source.label(),
                    issue.confidence
                )),
            ]),
        ]);
        if let Some(context) = issue.location.context()
            && context != issue.original
        {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "上下文",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                context.to_owned(),
                Style::default().fg(Color::Gray),
            )));
        }
        Text::from(lines)
    }

    fn draw_help(frame: &mut Frame<'_>) {
        let area = centered_rect(70, 70, frame.area());
        frame.render_widget(Clear, area);
        let text = vec![
            Line::from(Span::styled(
                "Errator 快捷键",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled("文件浏览", Style::default().fg(Color::Yellow))),
            Line::from("  ↑/↓ 或 j/k    移动选择"),
            Line::from("  Enter/→       打开目录或开始检查"),
            Line::from("  Backspace/←   返回上级目录"),
            Line::from("  r             刷新目录"),
            Line::from(""),
            Line::from(Span::styled("报告", Style::default().fg(Color::Yellow))),
            Line::from("  ↑/↓ 或 j/k    切换问题"),
            Line::from("  f / 0 / 1 / 2 / 3  筛选（全部/error/warning/info）"),
            Line::from("  i             忽略误报并持久化"),
            Line::from("  e             导出 Markdown / HTML / JSON / CSV"),
            Line::from("  Esc           返回文件浏览"),
            Line::from(""),
            Line::from(Span::styled("通用", Style::default().fg(Color::Yellow))),
            Line::from("  ?             显示/关闭帮助"),
            Line::from("  Ctrl+C        取消检查或退出"),
            Line::from("  q             退出"),
            Line::from(""),
            Line::from(Span::styled(
                "按任意键关闭帮助",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" 帮助 ")
                .style(Style::default().bg(Color::Black).fg(Color::White)),
        );
        frame.render_widget(paragraph, area);
    }

    fn draw_export_menu(frame: &mut Frame<'_>) {
        let area = centered_rect(54, 36, frame.area());
        frame.render_widget(Clear, area);
        let text = vec![
            Line::from(Span::styled(
                "选择导出格式",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  m   Markdown  (.md)"),
            Line::from("  h   HTML      (.html，自包含)"),
            Line::from("  j   JSON      (.json)"),
            Line::from("  c   CSV       (.csv)"),
            Line::from(""),
            Line::from(Span::styled(
                "Esc / 其他键取消",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" 导出 ")
                .style(Style::default().bg(Color::Black).fg(Color::White)),
        );
        frame.render_widget(paragraph, area);
    }

    fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(vertical[1])[1]
    }

    fn one_line(value: &str) -> String {
        value.replace(['\r', '\n', '\t'], " ")
    }

    fn checking_device_label(device: Option<&str>) -> &'static str {
        match device {
            Some("dml") => "GPU · DirectML（语义推理）",
            Some("coreml") => "GPU · CoreML（语义推理）",
            Some("cpu") => "CPU · ONNX Runtime（语义推理）",
            _ => "CPU（规则检查，未启用语义层）",
        }
    }

    fn truncate_width(value: &str, max_width: usize) -> String {
        if value.width() <= max_width {
            return value.to_owned();
        }
        if max_width <= 1 {
            return "…".to_owned();
        }
        let mut result = String::new();
        let mut width = 0;
        for character in value.chars() {
            let character_width = UnicodeWidthStr::width(character.to_string().as_str());
            if width + character_width + 1 > max_width {
                break;
            }
            result.push(character);
            width += character_width;
        }
        result.push('…');
        result
    }

    #[allow(dead_code)]
    fn _path_display(path: &Path) -> String {
        paths::display_path(path)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::model::{Issue, Location, Report, SemanticInfo, Severity, Source};
        use ratatui::backend::TestBackend;
        use std::fs;
        use tempfile::TempDir;

        fn sample_issue(original: &str) -> Issue {
            Issue {
                rule_id: "CJK_SPACE_MISSING".to_owned(),
                category: "spacing".to_owned(),
                severity: Severity::Warning,
                location: Location::Text {
                    line: 1,
                    column: 1,
                    context: Some(original.to_owned()),
                },
                file: None,
                original: original.to_owned(),
                suggestion: "使用 Rust 开发".to_owned(),
                message: "中英文之间建议加显式空格".to_owned(),
                source: Source::Rule,
                confidence: 1.0,
                offset: 0,
            }
        }

        fn text_of(terminal: &Terminal<TestBackend>) -> String {
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect()
        }

        fn app_for(directory: &Path) -> App {
            let options = TuiOptions {
                start: None,
                config: Config::default(),
                min_severity: Severity::Info,
                semantic_mode: SemanticMode::Auto,
                device: DeviceRequest::Cpu,
                no_gpu: true,
                include_context: true,
            };
            let mut app = App::new(options).unwrap();
            app.browser.load(directory).unwrap();
            app
        }

        #[test]
        fn browser_draws_files_and_navigates() {
            let temp = TempDir::new().unwrap();
            fs::write(temp.path().join("doc.md"), "hello\n").unwrap();
            fs::create_dir(temp.path().join("sub")).unwrap();
            let mut app = app_for(temp.path());
            assert!(
                app.browser
                    .entries
                    .iter()
                    .any(|entry| entry.name == "doc.md")
            );
            app.browser.move_selection(1);
            let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let text = text_of(&terminal);
            let compact: String = text
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            assert!(compact.contains("Errator"));
            assert!(compact.contains("doc.md"));
            assert!(
                compact.contains("选择文件") || compact.contains("文件（"),
                "rendered buffer:\n{text}"
            );
            assert!(compact.contains("q退出"), "rendered buffer:\n{text}");
        }

        #[test]
        fn report_view_filters_and_draws_details() {
            let temp = TempDir::new().unwrap();
            let source = temp.path().join("doc.md");
            fs::write(&source, "x").unwrap();
            let report = Report::new(
                "doc.md",
                SemanticInfo::disabled(),
                None,
                vec![
                    sample_issue("使用Rust开发"),
                    Issue {
                        severity: Severity::Error,
                        rule_id: "OTHER".to_owned(),
                        ..sample_issue("另一个错误")
                    },
                ],
                false,
            );
            let mut app = app_for(temp.path());
            let mut view = ReportView::new(report, source, Vec::new());
            view.set_filter(IssueFilter::Error);
            assert_eq!(view.filtered.len(), 1);
            view.set_filter(IssueFilter::All);
            view.move_selection(1);
            assert_eq!(view.selected, 1);
            app.report = Some(view);
            app.screen = Screen::Report;
            let mut terminal = Terminal::new(TestBackend::new(120, 38)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let text = text_of(&terminal);
            let compact: String = text
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            assert!(compact.contains("勘误报告"), "rendered buffer:\n{text}");
            assert!(compact.contains("另一个错误"));
            assert!(compact.contains("筛选"));
            assert!(compact.contains("CPU"), "rendered buffer:\n{text}");
            assert!(compact.contains("q退出"), "rendered buffer:\n{text}");
        }

        #[test]
        fn report_header_shows_device_even_with_a_very_long_source_path() {
            let temp = TempDir::new().unwrap();
            let long_source = format!("D:/{}/doc.md", "很长目录/".repeat(30));
            let report = Report::new(
                long_source.clone(),
                SemanticInfo::disabled(),
                None,
                Vec::new(),
                false,
            );
            let mut app = app_for(temp.path());
            app.report = Some(ReportView::new(
                report,
                PathBuf::from(&long_source),
                Vec::new(),
            ));
            app.screen = Screen::Report;
            let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let text = text_of(&terminal);
            let compact: String = text
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            assert!(
                compact.contains("计算设备：CPU（规则检查）"),
                "rendered buffer:\n{text}"
            );
            assert!(compact.contains("q退出"), "rendered buffer:\n{text}");
            assert!(compact.contains("筛选"), "rendered buffer:\n{text}");
        }

        #[test]
        fn checking_device_labels_name_the_hardware() {
            assert!(checking_device_label(None).contains("CPU"));
            assert!(checking_device_label(Some("cpu")).contains("CPU"));
            assert!(checking_device_label(Some("dml")).contains("GPU"));
            assert!(checking_device_label(Some("coreml")).contains("GPU"));
        }

        #[test]
        fn truncation_is_unicode_width_aware() {
            assert_eq!(truncate_width("中文", 10), "中文");
            assert!(truncate_width("这是一个很长的中文句子", 8).ends_with('…'));
        }

        #[test]
        fn report_export_writes_a_markdown_file() {
            let temp = TempDir::new().unwrap();
            let source = temp.path().join("doc.md");
            fs::write(&source, "使用Rust开发\n").unwrap();
            let report = Report::new(
                "doc.md",
                SemanticInfo::disabled(),
                None,
                vec![sample_issue("使用Rust开发")],
                false,
            );
            let mut view = ReportView::new(report, source.clone(), Vec::new());
            let exported = view.export(OutputFormat::Markdown).unwrap();
            assert!(exported.is_file());
            assert!(
                fs::read_to_string(&exported)
                    .unwrap()
                    .contains("# Errator 勘误报告")
            );
        }
    }
}

#[cfg(feature = "tui")]
pub use ui::{TuiOptions, run};

#[cfg(not(feature = "tui"))]
#[derive(Debug, Clone)]
pub struct TuiOptions {
    pub start: Option<std::path::PathBuf>,
    pub config: crate::config::Config,
    pub min_severity: crate::model::Severity,
    pub semantic_mode: crate::semantic::SemanticMode,
    pub device: crate::semantic::DeviceRequest,
    pub no_gpu: bool,
    pub include_context: bool,
}

#[cfg(not(feature = "tui"))]
pub fn run(_options: TuiOptions) -> anyhow::Result<()> {
    anyhow::bail!("此构建未包含 TUI；请使用 --features tui 重新构建，或使用 errator check")
}
