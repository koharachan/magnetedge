use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
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
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame, Terminal,
};
use chrono::{DateTime, Local};
use sysinfo::{System, SystemExt, ProcessExt, ComponentExt, CpuExt};
use std::collections::VecDeque;

// 监控数据结构
pub struct MonitorData {
    pub online_tasks: AtomicUsize,
    pub processing_tasks: AtomicUsize,
    pub completed_tasks: AtomicUsize,
    pub wallet_balance: Mutex<f64>,
    pub balance_history: Mutex<VecDeque<(DateTime<Local>, f64)>>,
    pub task_progresses: Mutex<Vec<TaskProgress>>,
    pub system_info: Mutex<SystemInfo>,
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

pub struct SystemInfo {
    pub cpu_usage: f64,
    pub max_threads: usize,
    pub memory_used: u64,
    pub memory_total: u64,
    pub temperature: Option<f32>,
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
            system_info: Mutex::new(SystemInfo {
                cpu_usage: 0.0,
                max_threads: num_cpus::get(),
                memory_used: 0,
                memory_total: 0,
                temperature: None,
            }),
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
            task.status = if success { TaskStatus::Completed } else { TaskStatus::Failed };
            task.end_time = Some(Local::now());
            if success {
                self.completed_tasks.fetch_add(1, Ordering::SeqCst);
            }
            self.processing_tasks.fetch_sub(1, Ordering::SeqCst);
        }
    }

    pub fn update_system_info(&self) {
        let mut system = System::new_all();
        system.refresh_all();

        let cpu_usage = system.global_cpu_info().cpu_usage() as f64;
        let memory_used = system.used_memory();
        let memory_total = system.total_memory();
        
        // 获取温度
        let temperature = system.components().first().map(|comp| comp.temperature());

        let mut sys_info = self.system_info.lock().unwrap();
        sys_info.cpu_usage = cpu_usage;
        sys_info.memory_used = memory_used;
        sys_info.memory_total = memory_total;
        sys_info.temperature = temperature;
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
        // 设置终端
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // 运行事件循环
        let tick_rate = Duration::from_millis(200);
        let mut last_tick = Instant::now();

        loop {
            // 绘制UI
            terminal.draw(|f| self.ui(f))?;

            // 处理输入事件
            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));
                
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
                        KeyCode::Esc => self.should_quit = true,
                        KeyCode::Char('e') => {
                            if key.modifiers == event::KeyModifiers::CONTROL {
                                self.should_quit = true;
                            }
                        },
                        _ => {}
                    }
                }
            }
            
            if last_tick.elapsed() >= tick_rate {
                last_tick = Instant::now();
                // 更新系统信息
                self.data.update_system_info();
            }

            if self.should_quit {
                break;
            }
        }

        // 恢复终端
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
        // 创建主布局
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Length(3),
                    Constraint::Length(8),
                    Constraint::Min(10),
                    Constraint::Length(7),
                ]
                .as_ref(),
            )
            .split(f.size());

        // 标题
        let title = Paragraph::new("Magnet POW 挖矿监控 (按 'q' 退出)")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // 系统信息
        self.render_system_info(f, chunks[1]);

        // 任务列表和进度
        self.render_tasks(f, chunks[2]);

        // 钱包信息
        self.render_wallet_info(f, chunks[3]);
    }

    fn render_system_info<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        let sys_info = self.data.system_info.lock().unwrap();
        
        let system_info = vec![
            format!("CPU利用率: {:.1}%", sys_info.cpu_usage),
            format!("最大线程数: {}", sys_info.max_threads),
            format!(
                "内存使用: {:.1}GB / {:.1}GB", 
                sys_info.memory_used as f64 / 1024.0 / 1024.0 / 1024.0,
                sys_info.memory_total as f64 / 1024.0 / 1024.0 / 1024.0
            ),
            format!(
                "温度: {}", 
                sys_info.temperature.map_or("不可用".to_string(), |t| format!("{:.1}°C", t))
            ),
        ];

        let system_info = Paragraph::new(system_info.join("\n"))
            .block(Block::default().title("系统信息").borders(Borders::ALL))
            .style(Style::default().fg(Color::White));
        f.render_widget(system_info, area);
    }

    fn render_tasks<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        let tasks_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(5)].as_ref())
            .split(area);

        // 任务统计
        let online = self.data.online_tasks.load(Ordering::SeqCst);
        let processing = self.data.processing_tasks.load(Ordering::SeqCst);
        let completed = self.data.completed_tasks.load(Ordering::SeqCst);

        let stats = Paragraph::new(format!(
            "在线任务: {}  处理中: {}  已完成: {}", 
            online, processing, completed
        ))
        .block(Block::default().title("任务统计").borders(Borders::ALL))
        .style(Style::default().fg(Color::Green));
        f.render_widget(stats, tasks_area[0]);

        // 任务详情和进度
        let tasks = self.data.task_progresses.lock().unwrap();
        if tasks.is_empty() {
            let no_tasks = Paragraph::new("暂无任务")
                .block(Block::default().title("任务详情").borders(Borders::ALL));
            f.render_widget(no_tasks, tasks_area[1]);
            return;
        }

        // 划分每个任务的区域
        let constraints = vec![Constraint::Length(3); tasks.len()];
        let task_areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(tasks_area[1]);

        for (i, task) in tasks.iter().enumerate() {
            if i >= task_areas.len() {
                break;
            }

            let status_text = match task.status {
                TaskStatus::Waiting => "等待中",
                TaskStatus::Processing => "处理中",
                TaskStatus::Completed => "已完成",
                TaskStatus::Failed => "失败",
            };

            let progress_percent = (task.progress * 100.0) as u16;
            let status_color = match task.status {
                TaskStatus::Waiting => Color::Yellow,
                TaskStatus::Processing => Color::Blue,
                TaskStatus::Completed => Color::Green,
                TaskStatus::Failed => Color::Red,
            };

            let gauge = Gauge::default()
                .block(Block::default().title(format!("任务 #{} - {}", task.id, status_text)))
                .gauge_style(Style::default().fg(status_color))
                .percent(progress_percent);
            f.render_widget(gauge, task_areas[i]);
        }
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
                ListItem::new(format!(
                    "{}: {:.6} MAG",
                    time.format("%H:%M:%S"),
                    balance
                ))
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

// 启动监控页面的函数
pub fn start_monitor() -> Arc<MonitorData> {
    let data = Arc::new(MonitorData::new());
    let data_clone = Arc::clone(&data);
    
    // 在新线程中启动TUI
    std::thread::spawn(move || {
        if let Err(err) = start_tui(data_clone) {
            eprintln!("TUI错误: {:?}", err);
        }
    });
    
    data
} 