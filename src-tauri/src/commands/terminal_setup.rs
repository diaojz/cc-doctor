//! 终端美化套件的 Tauri 命令入口（macOS 专属）。
//!
//! 三个命令：
//! - `terminal_setup_detect`：探测 brew、zsh、各组件、配置文件状态。
//! - `terminal_setup_install`：按需安装组件 + 写入配置（支持中途取消）。
//! - `terminal_setup_remove`：移除 `.zshrc` 标记块（其余组件 / 配置文件不主动删，
//!   只把备份路径提示给用户）。
//!
//! 取消机制复用现有的 `cancel_install`（在 doctor.rs 里），不再重定义。

#![cfg(target_os = "macos")]

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::services::stream_command::{emit_done, emit_error_line, emit_progress, ProcessRegistry};
use crate::services::terminal_setup::{
    self, Component, DetectReport,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStepResult {
    pub component: Component,
    pub success: bool,
    pub skipped: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSetupResult {
    pub success: bool,
    pub cancelled: bool,
    pub steps: Vec<InstallStepResult>,
    pub config_backups: Vec<String>,
    pub zshrc_backup: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveResult {
    pub block_removed: bool,
    /// 提示给用户的"未自动删除"项：cask、~/.oh-my-zsh、配置文件备份等，
    /// 由前端展示成清单让用户自行决定恢复/删除。
    pub kept_config_files: Vec<String>,
}

#[tauri::command]
pub async fn terminal_setup_detect() -> Result<DetectReport, String> {
    // detect 自身是同步的且很快，丢线程也行；这里直接调用即可。
    Ok(terminal_setup::detect())
}

#[tauri::command]
pub async fn terminal_setup_install(
    app: AppHandle,
    registry: State<'_, ProcessRegistry>,
    components: Vec<Component>,
    write_configs: bool,
    channel_id: String,
) -> Result<TerminalSetupResult, String> {
    let cid = channel_id.as_str();
    let state = registry.begin_session(cid).await;
    emit_progress(&app, cid, "===== 开始安装终端美化套件 =====");

    let mut steps: Vec<InstallStepResult> = Vec::new();
    let mut config_backups: Vec<String> = Vec::new();
    let mut zshrc_backup: Option<String> = None;
    let mut error: Option<String> = None;
    let mut cancelled = false;

    // 任何待装组件都需要 brew —— 这里只做前置安装，不作为独立 Component 暴露给前端。
    let needs_brew = components.iter().any(|c| {
        matches!(
            c,
            Component::Ghostty | Component::Zoxide | Component::Yazi
        )
    });

    if needs_brew {
        // 先清掉任何 stale brew lock（无真实 brew 进程时才清），避免上次
        // 中断/取消遗留的孤儿 lock 阻塞本次安装。
        terminal_setup::cleanup_stale_brew_locks(&app, cid);

        if let Err(e) = terminal_setup::install_brew(&app, &state, cid).await {
            if e == "cancelled" {
                cancelled = true;
            } else {
                emit_error_line(&app, cid, format!("Homebrew 准备失败: {}", e));
                error = Some(format!("brew_required: {}", e));
            }
        }
    }

    // 依赖顺序：brew 系（Ghostty/字体/zoxide/yazi）→ oh-my-zsh → 插件。
    let order = [
        Component::Ghostty,
        Component::Zoxide,
        Component::Yazi,
        Component::OhMyZsh,
        Component::ZshPlugins,
    ];

    for comp in order {
        if error.is_some() || cancelled {
            break;
        }
        if !components.contains(&comp) {
            continue;
        }
        if registry.is_cancelled(cid).await {
            cancelled = true;
            break;
        }

        let step_res = run_one_component(&app, &state, cid, comp).await;
        match step_res {
            Ok(skipped) => steps.push(InstallStepResult {
                component: comp,
                success: true,
                skipped,
                error: None,
            }),
            Err(e) if e == "cancelled" => {
                cancelled = true;
                steps.push(InstallStepResult {
                    component: comp,
                    success: false,
                    skipped: false,
                    error: Some("cancelled".to_string()),
                });
                break;
            }
            Err(e) => {
                steps.push(InstallStepResult {
                    component: comp,
                    success: false,
                    skipped: false,
                    error: Some(e.clone()),
                });
                error = Some(e);
                break;
            }
        }
    }

    // 写配置：只有当 install 阶段没失败且没取消时才执行。
    if write_configs && error.is_none() && !cancelled {
        let install_set: std::collections::HashSet<Component> = components.iter().copied().collect();

        if install_set.contains(&Component::Ghostty) {
            emit_progress(&app, cid, "写入 Ghostty 配置 (~/.config/ghostty/config) ...");
            match terminal_setup::write_ghostty_config() {
                Ok(Some(b)) => {
                    let s = b.display().to_string();
                    emit_progress(&app, cid, format!("已备份原 Ghostty 配置 -> {}", s));
                    config_backups.push(s);
                }
                Ok(None) => {}
                Err(e) => {
                    emit_error_line(&app, cid, format!("写 Ghostty 配置失败: {}", e));
                    error = Some(format!("write_ghostty_config_failed: {}", e));
                }
            }
        }

        if error.is_none() && install_set.contains(&Component::Yazi) {
            emit_progress(&app, cid, "写入 Yazi 配置 (~/.config/yazi/*.toml) ...");
            match terminal_setup::write_yazi_configs() {
                Ok(backups) => {
                    for b in backups {
                        let s = b.display().to_string();
                        emit_progress(&app, cid, format!("已备份原 Yazi 配置 -> {}", s));
                        config_backups.push(s);
                    }
                }
                Err(e) => {
                    emit_error_line(&app, cid, format!("写 Yazi 配置失败: {}", e));
                    error = Some(format!("write_yazi_configs_failed: {}", e));
                }
            }
        }

        let needs_zshrc = install_set.contains(&Component::Zoxide)
            || install_set.contains(&Component::Yazi)
            || install_set.contains(&Component::OhMyZsh)
            || install_set.contains(&Component::ZshPlugins);

        if error.is_none() && needs_zshrc {
            emit_progress(&app, cid, "写入 ~/.zshrc 的 cc-doctor 标记块 ...");
            match terminal_setup::write_zshrc_block() {
                Ok(b) => {
                    if let Some(p) = b {
                        let s = p.display().to_string();
                        emit_progress(&app, cid, format!("已备份原 ~/.zshrc -> {}", s));
                        zshrc_backup = Some(s);
                    }
                }
                Err(e) => {
                    emit_error_line(&app, cid, format!("写 ~/.zshrc 失败: {}", e));
                    error = Some(format!("write_zshrc_failed: {}", e));
                }
            }
        }
    }

    registry.end_session(cid).await;

    let success = error.is_none() && !cancelled;
    emit_done(&app, cid, success, None, cancelled);

    Ok(TerminalSetupResult {
        success,
        cancelled,
        steps,
        config_backups,
        zshrc_backup,
        error,
    })
}

/// 跑单个组件的安装，返回 `Ok(skipped)`：当组件已装时函数内部已 emit_progress
/// 跳过提示，这里统一标 `skipped=false`（已装也算 success；UI 不强依赖此字段）。
async fn run_one_component(
    app: &AppHandle,
    state: &crate::services::stream_command::SessionState,
    cid: &str,
    comp: Component,
) -> Result<bool, String> {
    match comp {
        Component::Ghostty => terminal_setup::install_ghostty(app, state, cid).await.map(|_| false),
        Component::Zoxide => terminal_setup::install_zoxide(app, state, cid).await.map(|_| false),
        Component::Yazi => terminal_setup::install_yazi(app, state, cid).await.map(|_| false),
        Component::OhMyZsh => terminal_setup::install_oh_my_zsh(app, state, cid).await.map(|_| false),
        Component::ZshPlugins => terminal_setup::install_zsh_plugins(app, state, cid).await.map(|_| false),
    }
}

#[tauri::command]
pub async fn terminal_setup_remove() -> Result<RemoveResult, String> {
    let block_removed = terminal_setup::remove_zshrc_block()?;

    // 我们不主动卸载任何 brew 包，也不删 ~/.oh-my-zsh 和独立配置文件，
    // 只把这些路径列给前端展示，让用户决定是否手动清理 / 恢复备份。
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let mut kept = Vec::new();
    kept.push(format!("{} (Oh My Zsh 目录，未删除)", home.join(".oh-my-zsh").display()));
    kept.push(format!("{} (Ghostty 配置)", home.join(".config/ghostty/config").display()));
    kept.push(format!("{} (Yazi 配置目录)", home.join(".config/yazi").display()));
    kept.push("brew cask: ghostty, font-maple-mono-nf-cn (未卸载)".to_string());
    kept.push("brew formula: zoxide, yazi, ffmpegthumbnailer, poppler (未卸载)".to_string());

    Ok(RemoveResult {
        block_removed,
        kept_config_files: kept,
    })
}
