use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::cancel::CancellationToken;
use crate::config::{self, Config};
use crate::engine::{CheckOptions, Engine, ProgressEvent};
use crate::ignore::IgnoreStore;
use crate::model::Severity;
use crate::paths;
use crate::report::{self, OutputFormat, RenderOptions};
use crate::rules::RuleRegistry;
use crate::ruleset;
use crate::semantic::{DeviceRequest, SemanticMode};
use crate::version;

#[derive(Debug, Parser)]
#[command(
    name = "errator",
    version,
    about = "Errator — 本地文本勘误报告生成器：读取 txt / md / docx，执行规则、词典与可选本地语义检查，生成可机读也可人读的勘误报告。",
    long_about = None,
    after_help = "示例：\n  errator check doc.md\n  errator check ./docs --format md -o report.md\n  errator check doc.txt --semantic on --device auto\n  errator tui ./docs\n  errator rules list\n\n无参数启动 TUI（仅在交互式终端中）。"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 检查文件或目录，输出报告
    Check(CheckArgs),
    /// 启动终端交互界面
    Tui(TuiArgs),
    /// 规则管理
    Rules {
        #[command(subcommand)]
        command: RulesCommand,
    },
    /// 管理持久化误报忽略项
    Ignore {
        #[command(subcommand)]
        command: IgnoreCommand,
    },
    /// 管理规则集
    #[command(alias = "rulesets")]
    Ruleset {
        #[command(subcommand)]
        command: RulesetCommand,
    },
    /// 管理本地语义模型
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
    },
    /// 初始化配置文件、忽略库和模型目录
    Init(InitArgs),
    /// 显示版本信息
    Version(VersionArgs),
    /// 列出推理设备及其可用状态
    Devices,
}

#[derive(Debug, Args)]
struct CheckArgs {
    /// 文件或目录（目录会递归检查 .txt / .md / .docx）
    path: PathBuf,
    /// 报告格式
    #[arg(long, value_enum, default_value = "text")]
    format: OutputFormat,
    /// 写入文件（默认输出到 stdout）
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// 自定义配置文件
    #[arg(long)]
    config: Option<PathBuf>,
    /// 加载已安装的规则集（可重复）
    #[arg(long = "ruleset")]
    rulesets: Vec<String>,
    /// 语义检查：auto | on | off
    #[arg(long, value_enum)]
    semantic: Option<SemanticMode>,
    /// 推理设备：auto | cpu | dml | coreml
    #[arg(long, value_enum)]
    device: Option<DeviceRequest>,
    /// 等价于 --device cpu
    #[arg(long)]
    no_gpu: bool,
    /// 只显示不低于该级别的问题
    #[arg(long, value_enum)]
    min_severity: Option<Severity>,
    /// 本次忽略指定规则 ID（可重复或逗号分隔）
    #[arg(long, value_delimiter = ',')]
    ignore: Vec<String>,
    /// 不在问题中附带上下文
    #[arg(long)]
    no_context: bool,
    /// 禁用 ANSI 颜色
    #[arg(long)]
    no_color: bool,
    /// 只输出摘要（text 格式）
    #[arg(long)]
    quiet: bool,
    /// 兼容参数：check 永远不会自动进入 TUI
    #[arg(long)]
    no_tui: bool,
    /// 忽略 Ctrl+C 取消请求
    #[arg(long)]
    no_cancel: bool,
    /// 覆盖已存在的输出文件
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct TuiArgs {
    /// 起始文件或目录
    path: Option<PathBuf>,
    /// 自定义配置文件
    #[arg(long)]
    config: Option<PathBuf>,
    /// 加载已安装的规则集（可重复）
    #[arg(long = "ruleset")]
    rulesets: Vec<String>,
    /// 语义检查：auto | on | off
    #[arg(long, value_enum)]
    semantic: Option<SemanticMode>,
    /// 推理设备：auto | cpu | dml | coreml
    #[arg(long, value_enum)]
    device: Option<DeviceRequest>,
    /// 等价于 --device cpu
    #[arg(long)]
    no_gpu: bool,
    /// 只显示不低于该级别的问题
    #[arg(long, value_enum)]
    min_severity: Option<Severity>,
}

#[derive(Debug, Subcommand)]
enum RulesCommand {
    /// 列出内置与自定义规则
    List {
        /// 输出格式
        #[arg(long, value_enum, default_value = "text")]
        format: ListFormat,
        /// 自定义配置文件
        #[arg(long)]
        config: Option<PathBuf>,
        /// 加载已安装的规则集（可重复）
        #[arg(long = "ruleset")]
        rulesets: Vec<String>,
    },
    /// 运行自定义规则中的正例/反例测试
    Test {
        /// 自定义配置文件
        #[arg(long)]
        config: Option<PathBuf>,
        /// 加载已安装的规则集（可重复）
        #[arg(long = "ruleset")]
        rulesets: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum RulesetCommand {
    /// 安装一个 TOML 规则集
    Add {
        /// 规则集文件
        path: PathBuf,
        /// 覆盖同名规则集
        #[arg(long)]
        force: bool,
    },
    /// 列出已安装规则集
    List,
    /// 删除已安装规则集
    Remove {
        /// 规则集名称
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum IgnoreCommand {
    /// 添加一条持久化忽略
    Add {
        /// 规则 ID
        #[arg(long)]
        rule: String,
        /// 要忽略的原文（需精确匹配）
        #[arg(long)]
        text: String,
        /// 备注
        #[arg(long)]
        note: Option<String>,
        /// 自定义配置文件（用于校验规则 ID）
        #[arg(long)]
        config: Option<PathBuf>,
        /// 加载已安装的规则集（可重复）
        #[arg(long = "ruleset")]
        rulesets: Vec<String>,
    },
    /// 列出持久化忽略项
    List {
        /// 输出格式
        #[arg(long, value_enum, default_value = "text")]
        format: ListFormat,
    },
    /// 按 ID 删除持久化忽略项
    Remove {
        /// 忽略项 ID
        id: String,
    },
    /// 清空全部持久化忽略项
    Clear,
}

#[derive(Debug, Subcommand)]
enum ModelsCommand {
    /// 显式下载 ONNX 模型到 ~/.errator/models/
    Download {
        /// 模型下载 URL（也可通过 ERRATOR_MODEL_URL 提供）
        #[arg(long)]
        url: Option<String>,
        /// 保存的文件名（必须以 .onnx 结尾）
        #[arg(long)]
        name: Option<String>,
        /// 期望的 SHA-256（十六进制）
        #[arg(long)]
        sha256: Option<String>,
        /// 覆盖已存在的模型
        #[arg(long)]
        force: bool,
    },
    /// 列出模型目录中的 ONNX 模型
    List,
}

#[derive(Debug, Args)]
struct InitArgs {
    /// 使用自定义配置文件路径
    #[arg(long)]
    config: Option<PathBuf>,
    /// 覆盖已有配置和忽略库
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct VersionArgs {
    /// 机器可读输出
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ListFormat {
    Text,
    Json,
}

fn interrupt_token() -> &'static CancellationToken {
    static TOKEN: OnceLock<CancellationToken> = OnceLock::new();
    TOKEN.get_or_init(CancellationToken::new)
}

fn install_signal_handler() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let token = interrupt_token().clone();
        // A failure to install the handler must not prevent normal operation.
        let _ = ctrlc::set_handler(move || token.cancel());
    });
}

/// Load the config then layer configured and CLI-specified rulesets on top.
fn load_config(
    config_path: Option<&std::path::Path>,
    cli_rulesets: &[String],
) -> Result<(Config, Vec<String>)> {
    let mut config = Config::load(config_path)?;
    let mut names = config.general.rulesets.clone();
    for name in cli_rulesets {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    let loaded = ruleset::apply(&mut config, &names)?;
    Ok((config, loaded.warnings))
}

fn print_warnings(warnings: &[String]) {
    for warning in warnings {
        eprintln!("提示：{warning}");
    }
}

pub fn run() -> i32 {
    install_signal_handler();
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Check(args)) => run_check(args),
        Some(Command::Tui(args)) => run_tui_command(args),
        Some(Command::Rules { command }) => run_rules(command),
        Some(Command::Ignore { command }) => run_ignore(command),
        Some(Command::Ruleset { command }) => run_ruleset(command),
        Some(Command::Models { command }) => run_models(command),
        Some(Command::Init(args)) => run_init(args),
        Some(Command::Version(args)) => run_version(args),
        Some(Command::Devices) => run_devices(),
        None => run_bare(),
    };

    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("错误：{error:#}");
            2
        }
    }
}

fn run_bare() -> Result<i32> {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        let (config, warnings) = load_config(None, &[])?;
        print_warnings(&warnings);
        run_tui(config, None, None, None, false, None)?;
        return Ok(0);
    }
    let mut command = <Cli as clap::CommandFactory>::command();
    command.print_help()?;
    println!();
    Ok(0)
}

fn run_check(args: CheckArgs) -> Result<i32> {
    if let Some(output) = &args.output
        && output.exists()
        && !args.force
    {
        bail!("输出文件已存在：{}（使用 --force 覆盖）", output.display());
    }
    let (config, config_warnings) = load_config(args.config.as_deref(), &args.rulesets)?;
    print_warnings(&config_warnings);
    let min_severity = args.min_severity.unwrap_or(config.general.min_severity);
    let include_context = !args.no_context && config.general.context;
    let semantic_mode = resolve_semantic_mode(args.semantic, &config)?;
    let device = resolve_device(args.device, &config)?;

    let engine = Engine::new(config)?;
    let options = CheckOptions {
        min_severity,
        include_context,
        ignore_rule_ids: args.ignore.clone(),
        semantic_mode,
        device,
        no_gpu: args.no_gpu,
        cancellation_enabled: !args.no_cancel,
    };

    let show_progress = args.format == OutputFormat::Text
        && !args.quiet
        && args.output.is_none()
        && std::io::stderr().is_terminal();
    let mut last_progress_len = 0usize;
    let token = interrupt_token().clone();
    let outcome = engine.run(&args.path, &options, &token, |event| {
        if !show_progress {
            return;
        }
        match event {
            ProgressEvent::FileStarted { index, total, file } => {
                let text = format!("检查中 {index}/{total}：{file}");
                let padding = " ".repeat(last_progress_len.saturating_sub(text.len()));
                eprint!("\r{text}{padding}");
                let _ = std::io::stderr().flush();
                last_progress_len = text.len();
            }
            ProgressEvent::FileFinished { .. } => {}
            ProgressEvent::Cancelled => {
                eprintln!();
                eprintln!("已请求取消，正在收尾…");
                last_progress_len = 0;
            }
            ProgressEvent::Started { .. } => {}
        }
    })?;
    if show_progress && last_progress_len > 0 {
        eprintln!();
    }

    for warning in &outcome.warnings {
        eprintln!("提示：{warning}");
    }
    if let Some(warning) = &outcome.report.semantic_warning
        && !outcome.warnings.contains(warning)
    {
        eprintln!("提示：{warning}");
    }

    let color = args.format == OutputFormat::Text
        && args.output.is_none()
        && !args.no_color
        && std::io::stdout().is_terminal();
    let rendered = report::render(
        &outcome.report,
        args.format,
        RenderOptions {
            color,
            quiet: args.quiet,
        },
    )?;

    if let Some(output) = &args.output {
        report::write_report(output, &rendered, args.force)?;
        if !args.quiet {
            eprintln!("报告已写入：{}", output.display());
        }
    } else {
        write_stdout(&rendered)?;
    }

    Ok(outcome.exit_code())
}

fn run_tui_command(args: TuiArgs) -> Result<i32> {
    let (config, warnings) = load_config(args.config.as_deref(), &args.rulesets)?;
    print_warnings(&warnings);
    let min_severity = args.min_severity.unwrap_or(config.general.min_severity);
    let mode = resolve_semantic_mode(args.semantic, &config)?;
    let device = resolve_device(args.device, &config)?;
    run_tui(
        config,
        args.path,
        Some(mode),
        Some(device),
        args.no_gpu,
        Some(min_severity),
    )?;
    Ok(0)
}

fn run_rules(command: RulesCommand) -> Result<i32> {
    match command {
        RulesCommand::List {
            format,
            config,
            rulesets,
        } => {
            let (config, warnings) = load_config(config.as_deref(), &rulesets)?;
            print_warnings(&warnings);
            let registry = RuleRegistry::build(&config)?;
            let rules = registry.info();
            match format {
                ListFormat::Json => {
                    write_stdout(&format!("{}\n", serde_json::to_string_pretty(&rules)?))?;
                }
                ListFormat::Text => {
                    println!(
                        "{:<28} {:<12} {:<9} {:<6} DESCRIPTION",
                        "ID", "CATEGORY", "SEVERITY", "ENABLED"
                    );
                    println!("{}", "-".repeat(96));
                    for rule in &rules {
                        println!(
                            "{:<28} {:<12} {:<9} {:<6} {}",
                            rule.id,
                            rule.category,
                            rule.severity.label(),
                            if rule.enabled { "yes" } else { "no" },
                            rule.description
                        );
                    }
                }
            }
            Ok(0)
        }
        RulesCommand::Test { config, rulesets } => {
            let (config, warnings) = load_config(config.as_deref(), &rulesets)?;
            print_warnings(&warnings);
            let tests = crate::rules::test_support::run_custom_tests(&config)?;
            if tests.is_empty() {
                println!("未配置自定义规则测试。");
                return Ok(0);
            }
            let mut failed = 0;
            for test in &tests {
                if test.passed {
                    println!("PASS  {}  {}", test.rule_id, truncate(&test.input, 48));
                } else {
                    failed += 1;
                    println!("FAIL  {}  {}", test.rule_id, truncate(&test.input, 48));
                    println!("      期望：{:?}", test.expected);
                    println!("      实际：{:?}", test.actual);
                }
            }
            println!(
                "\n通过 {} / {}，失败 {}",
                tests.len() - failed,
                tests.len(),
                failed
            );
            Ok(if failed > 0 { 1 } else { 0 })
        }
    }
}

fn run_ruleset(command: RulesetCommand) -> Result<i32> {
    match command {
        RulesetCommand::Add { path, force } => {
            let installed = ruleset::install(&path, force)?;
            println!("已安装规则集：{} {}", installed.name, installed.version);
            if !installed.description.is_empty() {
                println!("说明：{}", installed.description);
            }
            println!("文件：{}", installed.path);
            println!("使用：errator check <path> --ruleset {}", installed.name);
        }
        RulesetCommand::List => {
            let rulesets = ruleset::list_installed()?;
            if rulesets.is_empty() {
                println!("尚未安装规则集。");
                println!("使用 errator ruleset add <file.toml> 安装。");
            } else {
                println!(
                    "{:<28} {:<10} {:<10} DESCRIPTION",
                    "NAME", "VERSION", "EXTENDS"
                );
                println!("{}", "-".repeat(90));
                for ruleset in rulesets {
                    println!(
                        "{:<28} {:<10} {:<10} {}",
                        ruleset.name,
                        ruleset.version,
                        ruleset.extends.join(","),
                        ruleset.description
                    );
                }
            }
        }
        RulesetCommand::Remove { name } => {
            let path = ruleset::remove(&name)?;
            println!("已删除规则集：{name}（{}）", path.display());
        }
    }
    Ok(0)
}

fn run_ignore(command: IgnoreCommand) -> Result<i32> {
    match command {
        IgnoreCommand::Add {
            rule,
            text,
            note,
            config,
            rulesets,
        } => {
            let (config, warnings) = load_config(config.as_deref(), &rulesets)?;
            print_warnings(&warnings);
            let registry = RuleRegistry::build(&config)?;
            if !registry.contains(&rule) {
                eprintln!(
                    "提示：{rule} 不是当前启用的内置/自定义规则（外部检查器或语义规则 ID 仍可添加）"
                );
            }
            let mut store = IgnoreStore::load_default()?;
            let entry = store.add(&rule, &text, note)?;
            println!("已添加忽略项：{}", entry.id);
            println!("规则：{}", entry.rule_id);
            println!("原文：{}", entry.original);
            println!("忽略库：{}", store.path().display());
        }
        IgnoreCommand::List { format } => {
            let store = IgnoreStore::load_default()?;
            if store.is_empty() {
                println!("忽略库为空。");
                return Ok(0);
            }
            match format {
                ListFormat::Json => write_stdout(&format!(
                    "{}\n",
                    serde_json::to_string_pretty(&store.entries)?
                ))?,
                ListFormat::Text => {
                    println!("{:<16} {:<24} {:<28} NOTE", "ID", "RULE", "ORIGINAL");
                    println!("{}", "-".repeat(100));
                    for entry in &store.entries {
                        println!(
                            "{:<16} {:<24} {:<28} {}",
                            entry.id,
                            entry.rule_id,
                            truncate(&entry.original, 26),
                            entry.note.as_deref().unwrap_or("")
                        );
                    }
                    println!("\n忽略库：{}", store.path().display());
                }
            }
        }
        IgnoreCommand::Remove { id } => {
            let mut store = IgnoreStore::load_default()?;
            match store.remove(&id)? {
                Some(removed) => {
                    println!(
                        "已删除忽略项：{}（{} · {}）",
                        removed.id, removed.rule_id, removed.original
                    );
                }
                None => bail!("未找到忽略项：{id}"),
            }
        }
        IgnoreCommand::Clear => {
            let mut store = IgnoreStore::load_default()?;
            let removed = store.clear()?;
            if removed == 0 {
                println!("忽略库已为空。");
            } else {
                println!("已清空 {removed} 条忽略项。");
            }
            println!("忽略库：{}", store.path().display());
        }
    }
    Ok(0)
}

fn run_models(command: ModelsCommand) -> Result<i32> {
    match command {
        ModelsCommand::Download {
            url,
            name,
            sha256,
            force,
        } => {
            let url = url
                .or_else(crate::models::default_model_url)
                .context(
                    "缺少 --url（或设置 ERRATOR_MODEL_URL）。Errator 核心工具不会隐式联网，模型下载必须显式触发。",
                )?;
            let result = crate::models::download_model(
                &crate::models::DownloadOptions {
                    url,
                    name,
                    sha256,
                    force,
                },
                |received, total| {
                    if !std::io::stderr().is_terminal() {
                        return;
                    }
                    match total {
                        Some(total) if total > 0 => {
                            eprint!(
                                "\r下载中：{:.1}% ({}/{})",
                                received as f64 / total as f64 * 100.0,
                                format_bytes(received),
                                format_bytes(total)
                            );
                        }
                        _ => eprint!("\r下载中：{}", format_bytes(received)),
                    }
                    let _ = std::io::stderr().flush();
                },
            )?;
            if std::io::stderr().is_terminal() {
                eprintln!();
            }
            println!("模型已保存：{}", result.path.display());
            println!("大小：{}", format_bytes(result.bytes));
            println!("SHA-256：{}", result.sha256);
            println!("提示：下次检查会按 semantic.mode 自动发现该模型。");
        }
        ModelsCommand::List => {
            let directory = crate::models::ensure_models_dir()?;
            let models = crate::models::list_models()?;
            if models.is_empty() {
                println!("模型目录为空：{}", directory.display());
                println!("可使用 errator models download --url <URL> 显式下载。");
            } else {
                println!("模型目录：{}", directory.display());
                for (name, size) in models {
                    println!("  {name}  ({})", format_bytes(size));
                }
            }
        }
    }
    Ok(0)
}

fn run_init(args: InitArgs) -> Result<i32> {
    let home = paths::home_dir()?;
    std::fs::create_dir_all(&home).with_context(|| format!("无法创建目录 {}", home.display()))?;
    let config_path = args.config.clone().unwrap_or(paths::config_path()?);
    let created = config::write_default_config(&config_path, args.force)?;
    if created {
        println!("已写入配置：{}", config_path.display());
    } else {
        println!("配置已存在，未覆盖：{}", config_path.display());
    }

    let ignore_path = paths::ignores_path()?;
    if !ignore_path.exists() || args.force {
        let mut store = IgnoreStore::load(&ignore_path)?;
        if args.force {
            store.entries.clear();
        }
        store.save()?;
        println!("已初始化忽略库：{}", ignore_path.display());
    } else {
        println!("忽略库已存在，未覆盖：{}", ignore_path.display());
    }

    let models = crate::models::ensure_models_dir()?;
    println!("模型目录：{}", models.display());
    let rulesets = paths::rulesets_dir()?;
    std::fs::create_dir_all(&rulesets)
        .with_context(|| format!("无法创建规则集目录 {}", rulesets.display()))?;
    println!("规则集目录：{}", rulesets.display());
    println!("初始化完成。");
    Ok(0)
}

fn run_version(args: VersionArgs) -> Result<i32> {
    if args.json {
        write_stdout(&format!(
            "{}\n",
            serde_json::to_string_pretty(&version::info())?
        ))?;
    } else {
        println!("{}", version::long());
    }
    Ok(0)
}

fn run_devices() -> Result<i32> {
    let (config, _) = load_config(None, &[])?;
    let model = crate::semantic::discover_model(&config);
    println!("推理设备：");
    println!("  cpu      可用    规则检查始终可用；CPU 语义推理需模型 + ONNX 后端");
    if cfg!(target_os = "windows") {
        println!(
            "  dml      {}    DirectML（Windows / DirectX 12 GPU）",
            if cfg!(feature = "semantic") {
                "已编译"
            } else {
                "未编译"
            }
        );
    }
    if cfg!(target_os = "macos") {
        println!(
            "  coreml   {}    CoreML（macOS）",
            if cfg!(feature = "semantic") {
                "已编译"
            } else {
                "未编译"
            }
        );
    }
    println!(
        "\n语义后端：{}",
        if cfg!(feature = "semantic") {
            "已启用（可选，需要本地模型）"
        } else {
            "未启用（可选；使用 --features semantic 重新构建）"
        }
    );
    println!(
        "本地模型：{}",
        model
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "未发现".to_owned())
    );
    println!("GPU 仅作可选加速，失败会自动降级到 CPU。");
    Ok(0)
}

fn resolve_semantic_mode(cli: Option<SemanticMode>, config: &Config) -> Result<SemanticMode> {
    if let Some(mode) = cli {
        return Ok(mode);
    }
    if let Some(mode) = SemanticMode::parse_config(&config.semantic.mode) {
        return Ok(mode);
    }
    bail!(
        "配置项 semantic.mode 无效：{}（可用 auto/on/off）",
        config.semantic.mode
    )
}

fn resolve_device(cli: Option<DeviceRequest>, config: &Config) -> Result<DeviceRequest> {
    if let Some(device) = cli {
        return Ok(device);
    }
    if let Some(device) = DeviceRequest::parse_config(&config.semantic.device) {
        return Ok(device);
    }
    bail!(
        "配置项 semantic.device 无效：{}（可用 auto/cpu/dml/coreml）",
        config.semantic.device
    )
}

fn run_tui(
    config: Config,
    path: Option<PathBuf>,
    semantic_mode: Option<SemanticMode>,
    device: Option<DeviceRequest>,
    no_gpu: bool,
    min_severity: Option<Severity>,
) -> Result<()> {
    #[cfg(feature = "tui")]
    {
        let mode = match semantic_mode {
            Some(mode) => mode,
            None => resolve_semantic_mode(None, &config)?,
        };
        let device = match device {
            Some(device) => device,
            None => resolve_device(None, &config)?,
        };
        let min_severity = min_severity.unwrap_or(config.general.min_severity);
        let include_context = config.general.context;
        let options = crate::tui::TuiOptions {
            start: path,
            config,
            min_severity,
            semantic_mode: mode,
            device,
            no_gpu,
            include_context,
        };
        crate::tui::run(options)
    }
    #[cfg(not(feature = "tui"))]
    {
        let _ = (config, path, semantic_mode, device, no_gpu, min_severity);
        bail!("此构建未包含 TUI；请使用 --features tui 重新构建，或使用 errator check");
    }
}

fn write_stdout(value: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(value.as_bytes()) {
        Ok(()) => {
            stdout.flush()?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut result: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars {
        result.push('…');
    }
    result
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
