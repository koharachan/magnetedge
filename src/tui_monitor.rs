use chrono::{DateTime, Local};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::collections::VecDeque;
use std::{
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};

// 监控数据结构
pub struct MonitorData {
    pub online_tasks: AtomicUsize,
    pub processing_tasks: AtomicUsize,
    pub completed_tasks: AtomicUsize,
    pub wallet_balance: Mutex<f64>,
    pub balance_history: Mutex<VecDeque<(DateTime<Local>, f64)>>,
    pub task_progresses: Mutex<Vec<TaskProgress>>,
}

pub struct TaskProgress {
    pub id: usize,
    pub progress: f64,
    pub status: TaskStatus,
    pub start_time: DateTime<Local>,
    pub end_time: Option<DateTime<Local>>,
}

#[derive(PartialEq)]
pub enum TaskStatus {
    Waiting,
    Processing,
    Completed,
    Failed,
}

impl Default for MonitorData {
    fn default() -> Self {
        MonitorData {
            online_tasks: AtomicUsize::new(0),
            processing_tasks: AtomicUsize::new(0),
            completed_tasks: AtomicUsize::new(0),
            wallet_balance: Mutex::new(0.0),
            balance_history: Mutex::new(VecDeque::with_capacity(10)),
            task_progresses: Mutex::new(Vec::new()),
        }
    }
}

impl MonitorData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_balance(&self, balance: f64) {
        let mut wallet_balance = self.wallet_balance.lock().unwrap();
        *wallet_balance = balance;

        let mut history = self.balance_history.lock().unwrap();
        history.push_back((Local::now(), balance));
        if history.len() > 10 {
            history.pop_front();
        }
    }

    pub fn add_task(&self, id: usize) {
        let mut tasks = self.task_progresses.lock().unwrap();
        tasks.push(TaskProgress {
            id,
            progress: 0.0,
            status: TaskStatus::Waiting,
            start_time: Local::now(),
            end_time: None,
        });
        self.online_tasks.fetch_add(1, Ordering::SeqCst);
    }

    pub fn update_task_progress(&self, id: usize, progress: f64) {
        let mut tasks = self.task_progresses.lock().unwrap();
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.progress = progress;
            if task.status == TaskStatus::Waiting {
                task.status = TaskStatus::Processing;
                self.processing_tasks.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    pub fn complete_task(&self, id: usize, success: bool) {
        let mut tasks = self.task_progresses.lock().unwrap();
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.status = if success {
                TaskStatus::Completed
            } else {
                TaskStatus::Failed
            };
            task.end_time = Some(Local::now());
            if success {
                self.completed_tasks.fetch_add(1, Ordering::SeqCst);
            }
            self.processing_tasks.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

// TUI应用程序
pub struct TuiApp {
    data: Arc<MonitorData>,
    should_quit: bool,
}

impl TuiApp {
    pub fn new(data: Arc<MonitorData>) -> Self {
        TuiApp {
            data,
            should_quit: false,
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let tick_rate = Duration::from_millis(500); // 降低刷新频率
        let mut last_tick = Instant::now();

        loop {
            terminal.draw(|f| self.ui(f))?;

            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));

            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                        _ => {}
                    }
                }
            }

            if last_tick.elapsed() >= tick_rate {
                last_tick = Instant::now();
            }

            if self.should_quit {
                break;
            }
        }

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    fn ui<B: Backend>(&self, f: &mut Frame<B>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Length(3), // 标题
                    Constraint::Min(12),   // 任务区域
                    Constraint::Length(7), // 钱包信息
                ]
                .as_ref(),
            )
            .split(f.size());

        // 标题
        let title = Paragraph::new("Magnet POW 挖矿监控 (按 'q' 退出)")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // 任务显示
        self.render_tasks(f, chunks[1]);

        // 钱包信息
        self.render_wallet_info(f, chunks[2]);
    }

    fn render_tasks<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        let tasks_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(5)].as_ref())
            .split(area);

        // 任务统计
        let online = self.data.online_tasks.load(Ordering::Relaxed);
        let processing = self.data.processing_tasks.load(Ordering::Relaxed);
        let completed = self.data.completed_tasks.load(Ordering::Relaxed);

        let stats = Paragraph::new(format!(
            "在线任务: {}  处理中: {}  已完成: {}",
            online, processing, completed
        ))
        .block(Block::default().title("任务统计").borders(Borders::ALL))
        .style(Style::default().fg(Color::Green));
        f.render_widget(stats, tasks_area[0]);

        // 任务详情
        let tasks = self.data.task_progresses.lock().unwrap();
        if tasks.is_empty() {
            let no_tasks = Paragraph::new("暂无任务")
                .block(Block::default().title("任务详情").borders(Borders::ALL));
            f.render_widget(no_tasks, tasks_area[1]);
            return;
        }

        // 使用更高效的列表渲染
        let items: Vec<ListItem> = tasks
            .iter()
            .map(|task| {
                let status = match task.status {
                    TaskStatus::Waiting => ("等待中", Color::Yellow),
                    TaskStatus::Processing => ("处理中", Color::Blue),
                    TaskStatus::Completed => ("已完成", Color::Green),
                    TaskStatus::Failed => ("失败", Color::Red),
                };
                
                ListItem::new(format!(
                    "任务#{}: {} {:.1}% 开始于: {}",
                    task.id,
                    status.0,
                    task.progress * 100.0,
                    task.start_time.format("%H:%M:%S")
                )).style(Style::default().fg(status.1))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().title("任务列表").borders(Borders::ALL))
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));
        f.render_widget(list, tasks_area[1]);
    }

    fn render_wallet_info<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        let wallet_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(area);

        // 当前余额
        let balance = *self.data.wallet_balance.lock().unwrap();
        let balance_info = Paragraph::new(format!("当前余额: {:.6} MAG", balance))
            .block(Block::default().title("钱包信息").borders(Borders::ALL))
            .style(Style::default().fg(Color::White));
        f.render_widget(balance_info, wallet_areas[0]);

        // 历史余额
        let history = self.data.balance_history.lock().unwrap();
        let history_items: Vec<ListItem> = history
            .iter()
            .map(|(time, balance)| {
                ListItem::new(format!("{}: {:.6} MAG", time.format("%H:%M:%S"), balance))
            })
            .collect();

        let history_list = List::new(history_items)
            .block(Block::default().title("余额历史").borders(Borders::ALL))
            .style(Style::default().fg(Color::White));
        f.render_widget(history_list, wallet_areas[1]);
    }
}

pub fn start_tui(data: Arc<MonitorData>) -> io::Result<()> {
    let mut app = TuiApp::new(data);
    app.run()
}

pub fn start_monitor() -> Arc<MonitorData> {
    let data = Arc::new(MonitorData::new());
    let data_clone = Arc::clone(&data);

    std::thread::spawn(move || {
        if let Err(err) = start_tui(data_clone) {
            eprintln!("TUI错误: {:?}", err);
        }
    });

    data
}