//! 终端美化套件：Ghostty + Maple Mono NF + Zoxide + Yazi + Oh-My-Zsh。
//!
//! 仅 macOS。本模块整体在 `services/mod.rs` 中通过 `#[cfg(target_os = "macos")]`
//! 门控，其他平台不会被编译进二进制。
//!
//! 设计要点：
//!
//! - 安装函数全部走 `services::stream_command` 流式日志，与 `claude_installer`、
//!   `brew_migration` 共用一套事件协议（EVENT_LOG / EVENT_DONE）。
//! - 已安装的组件直接跳过，但仍 `emit_progress` 给前端一行可见反馈。
//! - `.zshrc` 写入用一对标记块（`# >>> cc-doctor terminal setup >>>` /
//!   `# <<< cc-doctor terminal setup <<<`）包裹，二次写只替换块内内容；
//!   首次插入到无标记的旧 `.zshrc` 时先备份成 `.zshrc.bak.<unix_ts>`。
//! - Ghostty config / Yazi 三件套独立配置文件，已存在则先备份为
//!   `<原名>.bak.<unix_ts>`，新内容用「临时文件 + rename」原子写入。

#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::services::stream_command::{
    capture_command, emit_error_line, emit_progress, stream_command, SessionState,
};

// ─── 标记 ──────────────────────────────────────────────────────────────────

pub const ZSHRC_MARKER_BEGIN: &str = "# >>> cc-doctor terminal setup >>>";
pub const ZSHRC_MARKER_END: &str = "# <<< cc-doctor terminal setup <<<";

/// 独立配置文件的首行标记，用于 detect 判断是否由 cc-doctor 托管。
const MANAGED_FILE_HEADER: &str = "# cc-doctor managed file";

// Maple Mono NF CN 字体不再作为可安装组件由 cc-doctor 自动安装：
// 多次实测下载常被中断、留下 .incomplete lock，且字体不是配置生效的强依赖
// （Ghostty 找不到时会回退到默认字体）。用户可自行 `brew install --cask
// font-maple-mono-nf-cn` 安装。

// ─── 配置文件内容（gist 原文，首行加 managed 标记） ────────────────────────

const GHOSTTY_CONFIG: &str = r#"# cc-doctor managed file — edits may be overwritten
# ============================================
# Ghostty Terminal - Complete Configuration
# ============================================

# --- Typography ---
font-family = "Maple Mono NF CN"
font-size = 15
font-thicken = true
adjust-cell-height = 6

# --- Theme and Colors ---
theme = Kanagawa Wave

# --- Window and Appearance ---
background-opacity = 1
macos-titlebar-style = transparent
window-padding-x = 14
window-padding-y = 10
window-save-state = never
window-width = 80
window-height = 24
window-theme = auto

# --- Cursor ---
cursor-style = bar
cursor-style-blink = true

# --- Mouse ---
mouse-hide-while-typing = true
copy-on-select = clipboard

# --- Quick Terminal (Quake-style dropdown) ---
quick-terminal-position = top
quick-terminal-screen = mouse
quick-terminal-autohide = true
quick-terminal-animation-duration = 0.15

# --- Close behavior ---
confirm-close-surface = false

# --- Security ---
clipboard-paste-protection = true
clipboard-paste-bracketed-safe = true

# --- Shell Integration ---
shell-integration = detect
shell-integration-features = cursor,sudo,no-title,ssh-env,ssh-terminfo,path

# --- Keybindings ---
# Tabs
keybind = cmd+t=new_tab
keybind = cmd+shift+left=previous_tab
keybind = cmd+shift+right=next_tab
keybind = cmd+w=close_surface

# Splits
keybind = cmd+d=new_split:right
keybind = cmd+shift+d=new_split:down
keybind = cmd+alt+left=goto_split:left
keybind = cmd+alt+right=goto_split:right
keybind = cmd+alt+up=goto_split:top
keybind = cmd+alt+down=goto_split:bottom

# Font size
keybind = cmd+plus=increase_font_size:1
keybind = cmd+minus=decrease_font_size:1
keybind = cmd+zero=reset_font_size

# Quick terminal global hotkey
keybind = global:ctrl+grave_accent=toggle_quick_terminal

# Splits management
keybind = cmd+shift+e=equalize_splits
keybind = cmd+shift+f=toggle_split_zoom

# Reload config
keybind = cmd+shift+comma=reload_config

# --- Performance ---
scrollback-limit = 25000000
"#;

const YAZI_TOML: &str = r#"# cc-doctor managed file — edits may be overwritten
[mgr]
ratio = [1, 2, 5]
sort_by = "natural"
sort_sensitive = false
sort_reverse = false
sort_dir_first = true
linemode = "size"
show_hidden = false
show_symlink = true
scrolloff = 5
mouse_events = ["click", "scroll"]
title_format = "Yazi: {cwd}"

[preview]
max_width = 600
max_height = 900
image_filter = "lanczos3"
image_quality = 75

[opener]
edit = [
  { run = 'code %s', desc = "VSCode", for = "unix" },
]
open = [
  { run = 'open %s', desc = "Open", for = "macos" },
]
reveal = [
  { run = 'open -R %1', desc = "Reveal in Finder", for = "macos" },
]

[open]
prepend_rules = [
  { mime = "text/*", use = ["edit", "open", "reveal"] },
  { mime = "application/json", use = ["edit", "open", "reveal"] },
  { mime = "*/javascript", use = ["edit", "open", "reveal"] },
  { mime = "*/typescript", use = ["edit", "open", "reveal"] },
  { mime = "*/x-yaml", use = ["edit", "open", "reveal"] },
]

[tasks]
micro_workers = 10
macro_workers = 25
bizarre_retry = 5

[plugin]
prepend_fetchers = [
  { id = "git", name = "*", run = "git", prio = "normal" },
]
"#;

const YAZI_KEYMAP_TOML: &str = r#"# cc-doctor managed file — edits may be overwritten
[[manager.prepend_keymap]]
on = ["g", "h"]
run = "cd ~"
desc = "Go to home directory"

[[manager.prepend_keymap]]
on = ["g", "c"]
run = "cd ~/.config"
desc = "Go to config directory"

[[manager.prepend_keymap]]
on = ["g", "d"]
run = "cd ~/Downloads"
desc = "Go to downloads"

[[manager.prepend_keymap]]
on = ["g", "w"]
run = "cd ~/work"
desc = "Go to work directory"

[[manager.prepend_keymap]]
on = ["g", "D"]
run = "cd ~/Desktop"
desc = "Go to desktop"

[[manager.prepend_keymap]]
on = ["g", "t"]
run = "cd /tmp"
desc = "Go to tmp"
"#;

const YAZI_THEME_TOML: &str = r#"# cc-doctor managed file — edits may be overwritten
# Tokyo Night inspired theme - works well with most terminal color schemes

[mode]
normal_main = { fg = "black", bg = "blue", bold = true }
normal_alt = { fg = "blue", bg = "reset", bold = true }
select_main = { fg = "black", bg = "green", bold = true }
select_alt = { fg = "green", bg = "reset", bold = true }
unset_main = { fg = "black", bg = "red", bold = true }
unset_alt = { fg = "red", bg = "reset", bold = true }

[status]
sep_left = { open = "", close = "" }
sep_right = { open = "", close = "" }
overall = { fg = "reset", bg = "reset" }

[filetype]
rules = [
  # Media
  { mime = "image/*", fg = "magenta" },
  { mime = "video/*", fg = "yellow" },
  { mime = "audio/*", fg = "yellow" },

  # Archives
  { mime = "application/zip", fg = "red" },
  { mime = "application/gzip", fg = "red" },
  { mime = "application/x-tar", fg = "red" },
  { mime = "application/x-bzip2", fg = "red" },
  { mime = "application/x-7z-compressed", fg = "red" },
  { mime = "application/x-rar", fg = "red" },
  { mime = "application/x-xz", fg = "red" },

  # Documents
  { mime = "application/pdf", fg = "cyan" },
  { mime = "application/*doc*", fg = "green" },
  { mime = "application/*sheet*", fg = "green" },
  { mime = "application/*presentation*", fg = "green" },

  # Fallback
  { name = "*", fg = "reset" },
  { name = "*/", fg = "blue", bold = true },
]
"#;

/// 写入 `.zshrc` 的内容块（不含标记本身）。
const ZSHRC_BLOCK_BODY: &str = r#"# Oh-My-Zsh
export ZSH="$HOME/.oh-my-zsh"
ZSH_THEME="agnoster"
DISABLE_AUTO_UPDATE="true"
plugins=(
    git
    zsh-syntax-highlighting
    zsh-autosuggestions
)
source $ZSH/oh-my-zsh.sh

# Locale
export LANG=en_US.UTF-8

# zsh-autosuggestions highlight
ZSH_AUTOSUGGEST_HIGHLIGHT_STYLE=fg=30

# Hide user@host in agnoster
DEFAULT_USER="your_username"

# Ghostty title
if [[ -n "${GHOSTTY_RESOURCES_DIR:-}" ]]; then
    ghostty_set_title() {
        local dir="${PWD/#$HOME/~}"
        printf '\033]2;%s\033\\' "$dir"
    }
    autoload -Uz add-zsh-hook
    add-zsh-hook chpwd ghostty_set_title
    add-zsh-hook precmd ghostty_set_title
    add-zsh-hook preexec ghostty_set_title
    ghostty_set_title
fi

# Yazi
function y() {
    local tmp="$(mktemp -t "yazi-cwd.XXXXXX")" cwd
    yazi "$@" --cwd-file="$tmp"
    if cwd="$(command cat -- "$tmp")" && [ -n "$cwd" ] && [ "$cwd" != "$PWD" ]; then
        builtin cd -- "$cwd"
    fi
    rm -f -- "$tmp"
}

# Zoxide
eval "$(zoxide init zsh)"
"#;

// ─── 公共类型 ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Component {
    Ghostty,
    Zoxide,
    Yazi,
    OhMyZsh,
    ZshPlugins,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentStatus {
    pub component: Component,
    pub installed: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigStatus {
    pub path: String,
    pub exists: bool,
    pub written_by_cc_doctor: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectReport {
    pub brew_installed: bool,
    pub brew_path: Option<String>,
    pub current_shell: Option<String>,
    pub is_zsh: bool,
    pub components: Vec<ComponentStatus>,
    pub zshrc_has_marker: bool,
    pub configs: Vec<ConfigStatus>,
}

// ─── 辅助：路径与文件 ──────────────────────────────────────────────────────

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn unix_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ghostty_config_path() -> PathBuf {
    home_dir().join(".config/ghostty/config")
}

fn yazi_dir() -> PathBuf {
    home_dir().join(".config/yazi")
}

fn yazi_config_paths() -> [PathBuf; 3] {
    let d = yazi_dir();
    [d.join("yazi.toml"), d.join("keymap.toml"), d.join("theme.toml")]
}

fn zshrc_path() -> PathBuf {
    home_dir().join(".zshrc")
}

/// 同步、短超时的 which 检查。
fn which_sync(bin: &str) -> Option<String> {
    let out = StdCommand::new("/usr/bin/which").arg(bin).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 同步检查 brew cask 是否已安装（`brew list --cask <name>` 退出码 0）。
fn brew_cask_installed_sync(cask: &str) -> bool {
    StdCommand::new("brew")
        .args(["list", "--cask", cask])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 返回当前系统中真实运行的 brew 安装/更新进程数。匹配模式刻意收紧到
/// `brew install` / `brew update` / `brew vendor-install` / `brew cleanup`，
/// 避免误伤把 brew 路径写进 env 的进程（如 Cursor / VSCode 这类）。
fn count_running_brew_processes() -> usize {
    let Ok(out) = StdCommand::new("pgrep")
        .args(["-f", r"brew (install|update|vendor-install|cleanup)"])
        .output()
    else {
        return 0;
    };
    if !out.status.success() {
        return 0;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

/// 通过 `which brew` 反推 brew 的 prefix 目录（Intel: /usr/local, ARM: /opt/homebrew）。
fn brew_prefix_dir() -> Option<PathBuf> {
    let bin_path = PathBuf::from(which_sync("brew")?);
    bin_path.parent()?.parent().map(PathBuf::from)
}

/// 清理 stale brew lock：当系统中**没有**真实的 brew install/update/vendor/cleanup
/// 进程时，清掉 locks 目录下的全局 lock 文件（update / cleanup / vendor-install-*）
/// 以及 downloads 目录下的 `.incomplete*` / `.lock` 残留。
///
/// 关键安全约束：
/// 1. 必须先确认无运行中的 brew 进程，否则什么都不做（避免误删活动 lock）。
/// 2. 只清「全局」lock，formula/cask 细粒度 lock 留给 brew 自愈。
pub fn cleanup_stale_brew_locks(app: &AppHandle, cid: &str) {
    if count_running_brew_processes() > 0 {
        emit_progress(app, cid, "检测到正在运行的 brew 进程，跳过 stale lock 预清理");
        return;
    }
    let Some(prefix) = brew_prefix_dir() else { return };

    let mut cleaned: Vec<String> = Vec::new();

    // 1) 清 var/homebrew/locks/ 下的全局 lock
    let locks_dir = prefix.join("var/homebrew/locks");
    if let Ok(entries) = fs::read_dir(&locks_dir) {
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(String::from) else { continue };
            let is_global = name == "update"
                || name == "cleanup"
                || name.starts_with("vendor-install-");
            if is_global && fs::remove_file(entry.path()).is_ok() {
                cleaned.push(name);
            }
        }
    }

    // 2) 清 ~/Library/Caches/Homebrew/downloads/ 下的 .incomplete* / .lock
    let downloads = home_dir().join("Library/Caches/Homebrew/downloads");
    if let Ok(entries) = fs::read_dir(&downloads) {
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(String::from) else { continue };
            let stale = name.ends_with(".incomplete")
                || name.ends_with(".incomplete.download.lock")
                || name.ends_with(".lock");
            if stale && fs::remove_file(entry.path()).is_ok() {
                cleaned.push(name);
            }
        }
    }

    if !cleaned.is_empty() {
        let preview = cleaned.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
        let suffix = if cleaned.len() > 5 {
            format!(" ... 共 {} 项", cleaned.len())
        } else {
            String::new()
        };
        emit_progress(
            app,
            cid,
            format!("已预清理 {} 个 stale brew lock：{}{}", cleaned.len(), preview, suffix),
        );
    }
}

/// 兜底探测 Ghostty.app：用户可能直接装 dmg 到 /Applications 而非走 brew cask。
fn detect_ghostty_app() -> Option<String> {
    let candidates = [
        PathBuf::from("/Applications/Ghostty.app"),
        home_dir().join("Applications/Ghostty.app"),
    ];
    candidates
        .into_iter()
        .find(|p| p.is_dir())
        .map(|p| p.display().to_string())
}

/// 文件首行是否以 `# cc-doctor` 开头。
fn file_managed_by_cc_doctor(path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    content
        .lines()
        .next()
        .map(|l| l.starts_with(MANAGED_FILE_HEADER))
        .unwrap_or(false)
}

/// 原子写文件：先写 `<dest>.tmp.<ts>` 再 rename。父目录不存在则创建。
fn atomic_write(dest: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败 {}: {}", parent.display(), e))?;
        }
    }
    let tmp = dest.with_extension(format!("tmp.{}", unix_ts()));
    fs::write(&tmp, content).map_err(|e| format!("写临时文件失败 {}: {}", tmp.display(), e))?;
    fs::rename(&tmp, dest)
        .map_err(|e| format!("rename {} -> {} 失败: {}", tmp.display(), dest.display(), e))?;
    Ok(())
}

/// 如果目标文件存在，备份为 `<path>.bak.<ts>`，返回备份路径。
fn backup_if_exists(path: &Path) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let bak = path.with_extension(format!(
        "{}.bak.{}",
        path.extension().and_then(|s| s.to_str()).unwrap_or(""),
        unix_ts()
    ));
    // 上面的 with_extension 在没有原扩展名时会产生 `.bak.<ts>`，但路径里
    // 可能出现 `..bak.<ts>`，统一改用 file_name 拼接更稳。
    let bak = match path.file_name().and_then(|s| s.to_str()) {
        Some(name) => path.with_file_name(format!("{}.bak.{}", name, unix_ts())),
        None => bak,
    };
    fs::copy(path, &bak)
        .map_err(|e| format!("备份 {} -> {} 失败: {}", path.display(), bak.display(), e))?;
    Ok(Some(bak))
}

// ─── 检测 ──────────────────────────────────────────────────────────────────

/// 同步检测当前系统状态。所有子命令都极短，因此不需要异步。
pub fn detect() -> DetectReport {
    let brew_path = which_sync("brew");
    let brew_installed = brew_path.is_some();

    let current_shell = std::env::var("SHELL").ok();
    let is_zsh = current_shell
        .as_deref()
        .map(|s| s.ends_with("/zsh"))
        .unwrap_or(false);

    // Ghostty：brew cask 优先，dmg 安装在 /Applications 也算数。
    let ghostty_app = detect_ghostty_app();
    let ghostty_via_brew = brew_installed && brew_cask_installed_sync("ghostty");
    let ghostty = ComponentStatus {
        component: Component::Ghostty,
        installed: ghostty_via_brew || ghostty_app.is_some(),
        detail: if ghostty_via_brew {
            Some("brew cask".to_string())
        } else {
            ghostty_app
        },
    };

    let zoxide_path = which_sync("zoxide");
    let zoxide = ComponentStatus {
        component: Component::Zoxide,
        installed: zoxide_path.is_some(),
        detail: zoxide_path,
    };
    let yazi_path = which_sync("yazi");
    let yazi = ComponentStatus {
        component: Component::Yazi,
        installed: yazi_path.is_some(),
        detail: yazi_path,
    };

    let omz_dir = home_dir().join(".oh-my-zsh");
    let oh_my_zsh = ComponentStatus {
        component: Component::OhMyZsh,
        installed: omz_dir.is_dir(),
        detail: Some(omz_dir.display().to_string()),
    };

    let custom_plugins = home_dir().join(".oh-my-zsh/custom/plugins");
    let plugins_ok = custom_plugins.join("zsh-syntax-highlighting").is_dir()
        && custom_plugins.join("zsh-autosuggestions").is_dir();
    let plugins = ComponentStatus {
        component: Component::ZshPlugins,
        installed: plugins_ok,
        detail: None,
    };

    let zshrc = zshrc_path();
    let zshrc_has_marker = fs::read_to_string(&zshrc)
        .map(|s| s.contains(ZSHRC_MARKER_BEGIN))
        .unwrap_or(false);

    let mut configs = Vec::with_capacity(4);
    let ghostty_cfg = ghostty_config_path();
    configs.push(ConfigStatus {
        exists: ghostty_cfg.exists(),
        written_by_cc_doctor: file_managed_by_cc_doctor(&ghostty_cfg),
        path: ghostty_cfg.display().to_string(),
    });
    for p in yazi_config_paths() {
        configs.push(ConfigStatus {
            exists: p.exists(),
            written_by_cc_doctor: file_managed_by_cc_doctor(&p),
            path: p.display().to_string(),
        });
    }

    DetectReport {
        brew_installed,
        brew_path,
        current_shell,
        is_zsh,
        components: vec![ghostty, zoxide, yazi, oh_my_zsh, plugins],
        zshrc_has_marker,
        configs,
    }
}

// ─── 安装函数 ──────────────────────────────────────────────────────────────

/// 跑官方 install.sh 安装 Homebrew。
pub async fn install_brew(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
) -> Result<(), String> {
    if which_sync("brew").is_some() {
        emit_progress(app, cid, "Homebrew 已安装，跳过");
        return Ok(());
    }
    emit_progress(app, cid, "开始安装 Homebrew（执行官方 install.sh）...");
    let outcome = stream_command(
        app,
        state,
        cid,
        "bash",
        &[
            "-c",
            "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"",
        ],
    )
    .await?;
    if outcome.cancelled {
        return Err("cancelled".to_string());
    }
    if !outcome.success {
        emit_error_line(app, cid, "Homebrew 安装失败");
        return Err("homebrew_install_failed".to_string());
    }
    // 验证
    let (ok, stdout, _) = capture_command(state, "/usr/bin/which", &["brew"]).await?;
    if !ok || stdout.trim().is_empty() {
        return Err("homebrew_verify_failed".to_string());
    }
    Ok(())
}

/// 用 `bash -c` 包一层执行 brew 命令，注入 HOMEBREW_NO_AUTO_UPDATE=1
/// 跳过 install 前的 auto-update。这是 brew 推荐的非交互式安装实践，能
/// 避开 `brew update` 与并发 install 之间的 lock 冲突。
async fn brew_run(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
    brew_args: &[&str],
) -> Result<bool, String> {
    let cmd = format!("HOMEBREW_NO_AUTO_UPDATE=1 brew {}", brew_args.join(" "));
    let outcome = stream_command(app, state, cid, "bash", &["-c", &cmd]).await?;
    if outcome.cancelled {
        return Err("cancelled".to_string());
    }
    Ok(outcome.success)
}

async fn brew_install_cask(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
    cask: &str,
    human_name: &str,
) -> Result<(), String> {
    if brew_cask_installed_sync(cask) {
        emit_progress(app, cid, format!("{} 已安装（cask: {}），跳过", human_name, cask));
        return Ok(());
    }
    emit_progress(app, cid, format!("开始安装 {}（cask: {}）...", human_name, cask));
    let ok = brew_run(app, state, cid, &["install", "--cask", cask]).await?;
    if !ok {
        emit_error_line(app, cid, format!("{} 安装失败", human_name));
        return Err(format!("install_{}_failed", cask));
    }
    Ok(())
}

async fn brew_install_formula(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
    formula: &str,
    bin_to_check: Option<&str>,
) -> Result<bool, String> {
    if let Some(bin) = bin_to_check {
        if which_sync(bin).is_some() {
            emit_progress(app, cid, format!("{} 已安装（which {} 命中），跳过", formula, bin));
            return Ok(true);
        }
    }
    emit_progress(app, cid, format!("开始安装 {}（brew install）...", formula));
    let ok = brew_run(app, state, cid, &["install", formula]).await?;
    if !ok {
        emit_error_line(app, cid, format!("{} 安装失败", formula));
        return Err(format!("install_{}_failed", formula));
    }
    Ok(true)
}

pub async fn install_ghostty(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
) -> Result<(), String> {
    // 跳过条件与 detect 保持一致：brew cask 已记录 **或** /Applications 里已有 .app
    if brew_cask_installed_sync("ghostty") {
        emit_progress(app, cid, "Ghostty 已安装（brew cask），跳过");
        return Ok(());
    }
    if let Some(p) = detect_ghostty_app() {
        emit_progress(app, cid, format!("Ghostty 已安装（{}），跳过", p));
        return Ok(());
    }
    brew_install_cask(app, state, cid, "ghostty", "Ghostty").await
}

pub async fn install_zoxide(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
) -> Result<(), String> {
    brew_install_formula(app, state, cid, "zoxide", Some("zoxide")).await?;
    Ok(())
}

pub async fn install_yazi(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
) -> Result<(), String> {
    // 逐个组件检测：yazi、ffmpegthumbnailer、poppler(pdfinfo 是 poppler 提供的)
    // 只装真正缺失的，避免对已装且 up-to-date 的包白跑 brew install。
    let to_install: Vec<&str> = [
        ("yazi", "yazi"),
        ("ffmpegthumbnailer", "ffmpegthumbnailer"),
        ("poppler", "pdfinfo"),
    ]
    .into_iter()
    .filter_map(|(formula, bin)| {
        if which_sync(bin).is_some() {
            emit_progress(app, cid, format!("{} 已安装（which {} 命中），跳过", formula, bin));
            None
        } else {
            Some(formula)
        }
    })
    .collect();

    if to_install.is_empty() {
        emit_progress(app, cid, "Yazi 及预览依赖均已安装，全部跳过");
        return Ok(());
    }

    emit_progress(
        app,
        cid,
        format!("开始安装缺失的包: {}", to_install.join(", ")),
    );
    let mut args = vec!["install"];
    args.extend(to_install.iter().copied());
    let ok = brew_run(app, state, cid, &args).await?;
    if !ok {
        emit_error_line(app, cid, "yazi 安装失败");
        return Err("install_yazi_failed".to_string());
    }
    Ok(())
}

pub async fn install_oh_my_zsh(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
) -> Result<(), String> {
    let omz_dir = home_dir().join(".oh-my-zsh");
    if omz_dir.is_dir() {
        emit_progress(app, cid, "Oh My Zsh 已安装（~/.oh-my-zsh 存在），跳过");
        return Ok(());
    }
    emit_progress(app, cid, "开始安装 Oh My Zsh（unattended）...");
    let outcome = stream_command(
        app,
        state,
        cid,
        "bash",
        &[
            "-c",
            "RUNZSH=no CHSH=no KEEP_ZSHRC=yes sh -c \"$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)\" \"\" --unattended",
        ],
    )
    .await?;
    if outcome.cancelled {
        return Err("cancelled".to_string());
    }
    if !outcome.success {
        emit_error_line(app, cid, "Oh My Zsh 安装失败");
        return Err("install_oh_my_zsh_failed".to_string());
    }
    Ok(())
}

pub async fn install_zsh_plugins(
    app: &AppHandle,
    state: &SessionState,
    cid: &str,
) -> Result<(), String> {
    let custom = home_dir().join(".oh-my-zsh/custom/plugins");
    if !custom.exists() {
        fs::create_dir_all(&custom)
            .map_err(|e| format!("创建 {} 失败: {}", custom.display(), e))?;
    }

    for (name, repo) in [
        (
            "zsh-syntax-highlighting",
            "https://github.com/zsh-users/zsh-syntax-highlighting.git",
        ),
        (
            "zsh-autosuggestions",
            "https://github.com/zsh-users/zsh-autosuggestions.git",
        ),
    ] {
        let dest = custom.join(name);
        if dest.is_dir() {
            emit_progress(app, cid, format!("{} 已存在，跳过", name));
            continue;
        }
        emit_progress(app, cid, format!("git clone {} ...", name));
        let dest_str = dest.display().to_string();
        let outcome =
            stream_command(app, state, cid, "git", &["clone", "--depth=1", repo, &dest_str])
                .await?;
        if outcome.cancelled {
            return Err("cancelled".to_string());
        }
        if !outcome.success {
            emit_error_line(app, cid, format!("克隆 {} 失败", name));
            return Err(format!("clone_{}_failed", name));
        }
    }
    Ok(())
}

// ─── 配置写入 ──────────────────────────────────────────────────────────────

/// 写 ghostty config，若已存在则备份。返回备份路径（如有）。
pub fn write_ghostty_config() -> Result<Option<PathBuf>, String> {
    let path = ghostty_config_path();
    let backup = backup_if_exists(&path)?;
    atomic_write(&path, GHOSTTY_CONFIG)?;
    Ok(backup)
}

/// 写 yazi 三件套，返回所有产生的备份路径。
pub fn write_yazi_configs() -> Result<Vec<PathBuf>, String> {
    let mut backups = Vec::new();
    let pairs: [(&Path, &str); 3] = [
        (Path::new(""), YAZI_TOML), // 占位（被下面 paths[i] 替换）
        (Path::new(""), YAZI_KEYMAP_TOML),
        (Path::new(""), YAZI_THEME_TOML),
    ];
    let paths = yazi_config_paths();
    for (i, (_, content)) in pairs.iter().enumerate() {
        let p = &paths[i];
        if let Some(b) = backup_if_exists(p)? {
            backups.push(b);
        }
        atomic_write(p, content)?;
    }
    Ok(backups)
}

/// 写 `.zshrc` 的 cc-doctor 块。
///
/// - 文件不存在 → 直接创建，仅含标记块。无备份。
/// - 文件存在且含标记块 → 原地替换块内内容。无备份。
/// - 文件存在但无标记块 → 备份 `.zshrc.bak.<ts>`，并在文件末尾追加块。
pub fn write_zshrc_block() -> Result<Option<PathBuf>, String> {
    let path = zshrc_path();
    let new_block = format!(
        "{}\n{}\n{}\n",
        ZSHRC_MARKER_BEGIN,
        ZSHRC_BLOCK_BODY.trim_end_matches('\n'),
        ZSHRC_MARKER_END
    );

    if !path.exists() {
        atomic_write(&path, &new_block)?;
        return Ok(None);
    }

    let original =
        fs::read_to_string(&path).map_err(|e| format!("读 {} 失败: {}", path.display(), e))?;

    if original.contains(ZSHRC_MARKER_BEGIN) && original.contains(ZSHRC_MARKER_END) {
        // 原地替换两标记之间（含标记）的整段。
        let new_content = replace_marker_block(&original, &new_block);
        atomic_write(&path, &new_content)?;
        return Ok(None);
    }

    // 无标记块：先备份，再在末尾追加。
    let backup = backup_if_exists(&path)?;
    let sep = if original.ends_with('\n') { "" } else { "\n" };
    let appended = format!("{}{}\n{}", original, sep, new_block);
    atomic_write(&path, &appended)?;
    Ok(backup)
}

/// 删除 `.zshrc` 中两个标记之间（含标记）的内容。返回是否实际改动。
pub fn remove_zshrc_block() -> Result<bool, String> {
    let path = zshrc_path();
    if !path.exists() {
        return Ok(false);
    }
    let original =
        fs::read_to_string(&path).map_err(|e| format!("读 {} 失败: {}", path.display(), e))?;
    if !original.contains(ZSHRC_MARKER_BEGIN) {
        return Ok(false);
    }
    let new_content = remove_marker_block(&original);
    if new_content == original {
        return Ok(false);
    }
    atomic_write(&path, &new_content)?;
    Ok(true)
}

/// 把两个标记之间（含标记）的整段替换为 `replacement`（replacement 自带换行）。
fn replace_marker_block(original: &str, replacement: &str) -> String {
    let Some(start) = original.find(ZSHRC_MARKER_BEGIN) else {
        return original.to_string();
    };
    let Some(end_rel) = original[start..].find(ZSHRC_MARKER_END) else {
        return original.to_string();
    };
    let end = start + end_rel + ZSHRC_MARKER_END.len();
    // 把 end 之后紧跟的单个换行也一起吃掉，避免越改越多空行。
    let mut tail_start = end;
    if original[tail_start..].starts_with('\n') {
        tail_start += 1;
    }
    let mut out = String::with_capacity(original.len());
    out.push_str(&original[..start]);
    out.push_str(replacement);
    out.push_str(&original[tail_start..]);
    out
}

/// 删除标记块，前后保留一个换行避免黏连。
fn remove_marker_block(original: &str) -> String {
    let Some(start) = original.find(ZSHRC_MARKER_BEGIN) else {
        return original.to_string();
    };
    let Some(end_rel) = original[start..].find(ZSHRC_MARKER_END) else {
        return original.to_string();
    };
    let end = start + end_rel + ZSHRC_MARKER_END.len();

    // 向前合并紧邻的换行
    let mut head_end = start;
    while head_end > 0 && original.as_bytes()[head_end - 1] == b'\n' {
        head_end -= 1;
    }
    // 向后吃掉紧邻的换行
    let mut tail_start = end;
    while tail_start < original.len() && original.as_bytes()[tail_start] == b'\n' {
        tail_start += 1;
    }

    let mut out = String::with_capacity(original.len());
    out.push_str(&original[..head_end]);
    // 在中间保留一个换行（如果两侧都有内容）
    if head_end > 0 && tail_start < original.len() {
        out.push('\n');
    }
    out.push_str(&original[tail_start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_marker_block_basic() {
        let original = "before\n# >>> cc-doctor terminal setup >>>\nOLD\n# <<< cc-doctor terminal setup <<<\nafter\n";
        let replacement = "# >>> cc-doctor terminal setup >>>\nNEW\n# <<< cc-doctor terminal setup <<<\n";
        let out = replace_marker_block(original, replacement);
        assert!(out.contains("NEW"));
        assert!(!out.contains("OLD"));
        assert!(out.starts_with("before\n"));
        assert!(out.ends_with("after\n"));
    }

    #[test]
    fn remove_marker_block_keeps_surroundings() {
        let original = "A\n# >>> cc-doctor terminal setup >>>\nX\n# <<< cc-doctor terminal setup <<<\nB\n";
        let out = remove_marker_block(original);
        assert!(!out.contains("cc-doctor"));
        assert!(out.contains("A"));
        assert!(out.contains("B"));
    }

    #[test]
    fn remove_no_marker_is_noop() {
        let original = "plain\n.zshrc\n";
        assert_eq!(remove_marker_block(original), original);
    }
}
