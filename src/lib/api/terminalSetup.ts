import { invoke } from "@tauri-apps/api/core";

// ─── Types ────────────────────────────────────────────────────

export type Component =
  | "ghostty"
  | "zoxide"
  | "yazi"
  | "oh-my-zsh"
  | "zsh-plugins";

export interface ComponentStatus {
  component: Component;
  installed: boolean;
  detail: string | null;
}

export interface ConfigStatus {
  path: string;
  exists: boolean;
  writtenByCcDoctor: boolean;
}

export interface DetectReport {
  brewInstalled: boolean;
  brewPath: string | null;
  currentShell: string | null;
  isZsh: boolean;
  components: ComponentStatus[];
  zshrcHasMarker: boolean;
  configs: ConfigStatus[];
}

export interface InstallStepResult {
  component: Component;
  success: boolean;
  skipped: boolean;
  error: string | null;
}

export interface TerminalSetupResult {
  success: boolean;
  cancelled: boolean;
  steps: InstallStepResult[];
  configBackups: string[];
  zshrcBackup: string | null;
  error: string | null;
}

export interface RemoveResult {
  blockRemoved: boolean;
  keptConfigFiles: string[];
}

export interface InstallOptions {
  components: Component[];
  writeConfigs: boolean;
  channelId: string;
}

// ─── API ──────────────────────────────────────────────────────

export const terminalSetupApi = {
  /**
   * 检测当前终端环境，返回各组件安装状态与配置状态。
   */
  async detect(): Promise<DetectReport> {
    return await invoke("terminal_setup_detect");
  },

  /**
   * 流式安装终端组件。channelId 由前端生成（来自 useInstallLogStream.start()），
   * 后端按该 id emit install-log / install-log-done 事件。
   */
  async install(options: InstallOptions): Promise<TerminalSetupResult> {
    return await invoke("terminal_setup_install", {
      components: options.components,
      writeConfigs: options.writeConfigs,
      channelId: options.channelId,
    });
  },

  /**
   * 移除 .zshrc 中由 cc-doctor 标记块包裹的内容。
   * 不卸载任何 brew 包，不删除 ~/.oh-my-zsh 或独立配置文件。
   */
  async remove(): Promise<RemoveResult> {
    return await invoke("terminal_setup_remove");
  },

  /**
   * 取消正在进行的安装会话。命令幂等：channel 不存在或已结束都返回 false。
   * 复用 cancel_install 命令。
   */
  async cancel(channelId: string): Promise<boolean> {
    return await invoke("cancel_install", { channelId });
  },

  /**
   * 将备份文件（.bak.<ts> 格式）还原到原路径，备份本身保留。
   * 返回还原后的目标路径。
   */
  async restoreBackup({ backupPath }: { backupPath: string }): Promise<string> {
    return await invoke("terminal_setup_restore_backup", { backupPath });
  },

  /**
   * 删除备份文件。后端只接受 .bak.<digits> 命名，其他路径会被拒绝。
   */
  async deleteBackup({ backupPath }: { backupPath: string }): Promise<void> {
    return await invoke("terminal_setup_delete_backup", { backupPath });
  },

  /**
   * 打开新终端窗口（优先 Ghostty → iTerm → Terminal）。
   * 返回实际打开的 app 名称。
   */
  async openTerminal(): Promise<string> {
    return await invoke("terminal_setup_open_terminal");
  },
};
