import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  CheckCircle2,
  XCircle,
  AlertTriangle,
  Info,
  RefreshCw,
  Loader2,
  ChevronDown,
  ChevronUp,
  Copy,
  Check,
  RotateCcw,
  Trash2,
  TerminalSquare,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { isMac } from "@/lib/platform";
import { InstallLogPanel } from "@/components/settings/InstallLogPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { useInstallLogStream } from "@/hooks/useInstallLogStream";
import {
  terminalSetupApi,
  type DetectReport,
  type Component,
  type TerminalSetupResult,
  type RemoveResult,
} from "@/lib/api/terminalSetup";

// ─── 组件元数据 ───────────────────────────────────────────────

interface ComponentMeta {
  id: Component;
  nameKey: string;
  descKey: string;
}

const COMPONENT_LIST: ComponentMeta[] = [
  {
    id: "ghostty",
    nameKey: "terminalSetup.components.ghostty",
    descKey: "terminalSetup.components.ghosttyDesc",
  },
  {
    id: "zoxide",
    nameKey: "terminalSetup.components.zoxide",
    descKey: "terminalSetup.components.zoxideDesc",
  },
  {
    id: "yazi",
    nameKey: "terminalSetup.components.yazi",
    descKey: "terminalSetup.components.yaziDesc",
  },
  {
    id: "oh-my-zsh",
    nameKey: "terminalSetup.components.ohMyZsh",
    descKey: "terminalSetup.components.ohMyZshDesc",
  },
  {
    id: "zsh-plugins",
    nameKey: "terminalSetup.components.zshPlugins",
    descKey: "terminalSetup.components.zshPluginsDesc",
  },
];

// ─── 子组件：一键复制按钮 ──────────────────────────────────────

function CopyButton({ text, className }: { text: string; className?: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // 静默处理
    }
  };

  return (
    <button
      type="button"
      onClick={handleCopy}
      className={cn(
        "inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs font-mono",
        "bg-muted hover:bg-muted/80 border border-border transition-colors cursor-pointer",
        className,
      )}
    >
      {copied ? (
        <Check className="h-3 w-3 text-emerald-500" />
      ) : (
        <Copy className="h-3 w-3 text-muted-foreground" />
      )}
      <code>{text}</code>
    </button>
  );
}

// ─── 主面板 ────────────────────────────────────────────────────

export function TerminalSetupPanel() {
  const { t } = useTranslation();

  // macOS 保护
  if (!isMac()) {
    return (
      <div className="flex flex-col items-center justify-center h-full py-20 text-muted-foreground">
        <Info className="h-10 w-10 mb-3 opacity-40" />
        <p className="text-sm">{t("terminalSetup.macOnly")}</p>
      </div>
    );
  }

  return <TerminalSetupPanelInner />;
}

function TerminalSetupPanelInner() {
  const { t } = useTranslation();

  // ── 状态 ──
  const [report, setReport] = useState<DetectReport | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [selectedComponents, setSelectedComponents] = useState<
    Set<Component>
  >(new Set(COMPONENT_LIST.map((c) => c.id)));
  const [writeConfigs, setWriteConfigs] = useState(true);
  const [installResult, setInstallResult] =
    useState<TerminalSetupResult | null>(null);
  const [removeResult, setRemoveResult] = useState<RemoveResult | null>(null);
  const [dangerExpanded, setDangerExpanded] = useState(false);
  const [confirmRemoveOpen, setConfirmRemoveOpen] = useState(false);
  const [isCancelling, setIsCancelling] = useState(false);

  const { status, lines, start, finish, reset, channelId } =
    useInstallLogStream();

  const isRunning = status === "running";

  // ── 检测 ──
  const runDetect = useCallback(async () => {
    setDetecting(true);
    try {
      const result = await terminalSetupApi.detect();
      setReport(result);
    } catch (err) {
      console.error("terminal_setup_detect 失败:", err);
    } finally {
      setDetecting(false);
    }
  }, []);

  useEffect(() => {
    runDetect();
  }, [runDetect]);

  // ── 安装 ──
  const handleInstall = async () => {
    reset();
    setInstallResult(null);
    const cid = start();
    try {
      const result = await terminalSetupApi.install({
        components: Array.from(selectedComponents),
        writeConfigs,
        channelId: cid,
      });
      setInstallResult(result);
      if (result.cancelled) {
        finish("cancelled");
      } else if (result.success) {
        finish("success");
      } else {
        finish("failed");
      }
      // 安装完成后重新检测
      await runDetect();
    } catch (err) {
      finish("failed");
      console.error("terminal_setup_install 失败:", err);
    }
  };

  // ── 取消 ──
  const handleCancel = async () => {
    if (!channelId) return;
    setIsCancelling(true);
    try {
      await terminalSetupApi.cancel(channelId);
    } catch (err) {
      console.error("cancel_install 失败:", err);
    } finally {
      setIsCancelling(false);
    }
  };

  // ── 移除 ──
  const handleRemove = async () => {
    setConfirmRemoveOpen(false);
    try {
      const result = await terminalSetupApi.remove();
      setRemoveResult(result);
      await runDetect();
    } catch (err) {
      console.error("terminal_setup_remove 失败:", err);
    }
  };

  // ── 组件勾选 ──
  const toggleComponent = (id: Component) => {
    setSelectedComponents((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  // ── 渲染 ──

  return (
    <div className="flex flex-col h-full overflow-y-auto px-6 pb-12">
      {/* 顶部头部 */}
      <div className="flex items-start justify-between py-5 border-b border-border mb-5">
        <div>
          <h2 className="text-lg font-semibold text-foreground">
            {t("terminalSetup.title")}
          </h2>
          <p className="text-sm text-muted-foreground mt-0.5">
            {t("terminalSetup.subtitle")}
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={runDetect}
          disabled={detecting || isRunning}
          className="shrink-0 ml-4"
        >
          {detecting ? (
            <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5 mr-1.5" />
          )}
          {t("terminalSetup.detect")}
        </Button>
      </div>

      <div className="space-y-4">
        {/* Homebrew 状态卡片 */}
        {report && (
          <div
            className={cn(
              "rounded-lg border px-4 py-3 flex items-start gap-3",
              report.brewInstalled
                ? "border-emerald-200 bg-emerald-50 dark:border-emerald-800/50 dark:bg-emerald-900/20"
                : "border-orange-200 bg-orange-50 dark:border-orange-800/50 dark:bg-orange-900/20",
            )}
          >
            {report.brewInstalled ? (
              <CheckCircle2 className="h-4 w-4 text-emerald-600 dark:text-emerald-400 shrink-0 mt-0.5" />
            ) : (
              <AlertTriangle className="h-4 w-4 text-orange-600 dark:text-orange-400 shrink-0 mt-0.5" />
            )}
            <div className="text-sm">
              {report.brewInstalled ? (
                <span className="text-emerald-800 dark:text-emerald-300">
                  {t("terminalSetup.brew.ready")}
                  {report.brewPath && (
                    <span className="ml-1 font-mono text-xs opacity-70">
                      ({report.brewPath})
                    </span>
                  )}
                </span>
              ) : (
                <span className="text-orange-800 dark:text-orange-300">
                  {t("terminalSetup.brew.missing")}
                </span>
              )}
            </div>
          </div>
        )}

        {/* Shell 提示卡片（仅非 zsh 时显示） */}
        {report && !report.isZsh && (
          <div className="rounded-lg border border-yellow-200 bg-yellow-50 dark:border-yellow-800/50 dark:bg-yellow-900/20 px-4 py-3">
            <div className="flex items-start gap-3">
              <AlertTriangle className="h-4 w-4 text-yellow-600 dark:text-yellow-400 shrink-0 mt-0.5" />
              <div className="text-sm text-yellow-800 dark:text-yellow-300 space-y-2">
                <p>
                  {t("terminalSetup.shell.notZsh", {
                    shell: report.currentShell ?? "未知",
                  })}
                </p>
                <CopyButton text="chsh -s /bin/zsh" />
              </div>
            </div>
          </div>
        )}

        {/* 字体说明：cc-doctor 不再自动装字体，Ghostty 找不到字体时会自动回退 */}
        <div className="rounded-lg border border-blue-200 bg-blue-50 dark:border-blue-800/50 dark:bg-blue-900/20 px-4 py-3">
          <div className="flex items-start gap-3">
            <Info className="h-4 w-4 text-blue-600 dark:text-blue-400 shrink-0 mt-0.5" />
            <p className="text-sm text-blue-800 dark:text-blue-300">
              {t("terminalSetup.font.notice")}
            </p>
          </div>
        </div>

        {/* 组件列表 */}
        <div className="rounded-lg border border-border bg-card overflow-hidden">
          <div className="px-4 py-3 border-b border-border bg-muted/30">
            <p className="text-sm font-medium text-foreground">
              选择要安装的组件
            </p>
          </div>
          <div className="divide-y divide-border">
            {COMPONENT_LIST.map((meta) => {
              const componentStatus = report?.components.find(
                (c) => c.component === meta.id,
              );
              const isInstalled = componentStatus?.installed ?? false;
              const isChecked = selectedComponents.has(meta.id);

              return (
                <div
                  key={meta.id}
                  className="flex items-center gap-3 px-4 py-3 hover:bg-muted/20 transition-colors"
                >
                  <Checkbox
                    id={`component-${meta.id}`}
                    checked={isChecked}
                    onCheckedChange={() => toggleComponent(meta.id)}
                    disabled={isRunning}
                    className={cn(isInstalled && "opacity-60")}
                  />
                  <Label
                    htmlFor={`component-${meta.id}`}
                    className="flex-1 cursor-pointer"
                  >
                    <span className="text-sm font-medium text-foreground">
                      {t(meta.nameKey)}
                    </span>
                    <span className="ml-2 text-xs text-muted-foreground">
                      {t(meta.descKey)}
                    </span>
                  </Label>
                  {/* 安装状态徽标 */}
                  {report && (
                    <span
                      className={cn(
                        "inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded-full border shrink-0",
                        isInstalled
                          ? "border-emerald-200 bg-emerald-50 text-emerald-700 dark:border-emerald-800/50 dark:bg-emerald-900/20 dark:text-emerald-400"
                          : "border-border bg-muted text-muted-foreground",
                      )}
                    >
                      {isInstalled ? (
                        <CheckCircle2 className="h-3 w-3" />
                      ) : (
                        <XCircle className="h-3 w-3" />
                      )}
                      {isInstalled
                        ? t("terminalSetup.installed")
                        : t("terminalSetup.notInstalled")}
                    </span>
                  )}
                </div>
              );
            })}
          </div>
        </div>

        {/* 配置写入开关 */}
        <div className="rounded-lg border border-border bg-card px-4 py-3">
          <div className="flex items-center gap-3">
            <Switch
              id="write-configs"
              checked={writeConfigs}
              onCheckedChange={setWriteConfigs}
              disabled={isRunning}
            />
            <Label htmlFor="write-configs" className="cursor-pointer">
              <span className="text-sm font-medium text-foreground">
                {t("terminalSetup.writeConfigs")}
              </span>
            </Label>
          </div>
          {writeConfigs && (
            <p className="mt-2 text-xs text-muted-foreground leading-relaxed pl-10">
              {t("terminalSetup.writeConfigs.hint")}
            </p>
          )}
        </div>

        {/* 操作按钮区 */}
        <div className="flex items-center gap-3">
          <Button
            onClick={handleInstall}
            disabled={isRunning || selectedComponents.size === 0}
            className={cn(
              "bg-orange-500 hover:bg-orange-600 text-white border-0",
              "disabled:opacity-50 disabled:cursor-not-allowed",
            )}
          >
            {isRunning ? (
              <>
                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                {t("terminalSetup.installing")}
              </>
            ) : (
              t("terminalSetup.install")
            )}
          </Button>
          {isRunning && (
            <Button
              variant="outline"
              onClick={handleCancel}
              disabled={isCancelling}
              className="border-red-300 text-red-600 hover:bg-red-50 hover:text-red-700 dark:border-red-800 dark:text-red-400"
            >
              {isCancelling ? (
                <Loader2 className="h-4 w-4 mr-1.5 animate-spin" />
              ) : null}
              {t("terminalSetup.cancel")}
            </Button>
          )}
        </div>

        {/* 安装完成总结卡片 */}
        {installResult && status !== "running" && (
          <InstallSummaryCard result={installResult} />
        )}

        {/* 日志面板 */}
        {status !== "idle" && (
          <InstallLogPanel
            status={status}
            lines={lines}
            onCancel={handleCancel}
            isCancelling={isCancelling}
          />
        )}

        {/* 移除结果展示 */}
        {removeResult && (
          <RemoveResultCard result={removeResult} />
        )}

        {/* 危险区折叠组 */}
        <div className="rounded-lg border border-red-200 dark:border-red-900/50 overflow-hidden mt-6">
          <button
            type="button"
            onClick={() => setDangerExpanded((v) => !v)}
            className="w-full flex items-center justify-between px-4 py-3 text-sm font-medium text-red-700 dark:text-red-400 bg-red-50 dark:bg-red-900/20 hover:bg-red-100 dark:hover:bg-red-900/30 transition-colors"
          >
            <span className="flex items-center gap-2">
              <AlertTriangle className="h-4 w-4" />
              {t("terminalSetup.danger.title")}
            </span>
            {dangerExpanded ? (
              <ChevronUp className="h-4 w-4" />
            ) : (
              <ChevronDown className="h-4 w-4" />
            )}
          </button>
          {dangerExpanded && (
            <div className="px-4 py-4 space-y-3 bg-card">
              <p className="text-sm text-muted-foreground leading-relaxed">
                {t("terminalSetup.danger.desc")}
              </p>
              <Button
                variant="destructive"
                size="sm"
                onClick={() => setConfirmRemoveOpen(true)}
                disabled={isRunning}
              >
                {t("terminalSetup.danger.confirm")}
              </Button>
            </div>
          )}
        </div>
      </div>

      {/* 移除二次确认弹窗 */}
      <ConfirmDialog
        isOpen={confirmRemoveOpen}
        title={t("terminalSetup.danger.confirmTitle")}
        message={t("terminalSetup.danger.confirmDesc")}
        confirmText={t("terminalSetup.danger.confirm")}
        variant="destructive"
        onConfirm={handleRemove}
        onCancel={() => setConfirmRemoveOpen(false)}
      />
    </div>
  );
}

// ─── 安装总结卡片 ──────────────────────────────────────────────

/**
 * 单条备份路径行：复制 + 恢复 + 删除 三个操作
 */
function BackupItem({
  backupPath,
  onDelete,
}: {
  backupPath: string;
  onDelete: (path: string) => void;
}) {
  const { t } = useTranslation();
  const [confirmRestoreOpen, setConfirmRestoreOpen] = useState(false);
  const [confirmDeleteOpen, setConfirmDeleteOpen] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [deleting, setDeleting] = useState(false);

  /** 还原备份 */
  const handleRestore = async () => {
    setConfirmRestoreOpen(false);
    setRestoring(true);
    try {
      const restoredPath = await terminalSetupApi.restoreBackup({ backupPath });
      toast.success(t("terminalSetup.backup.restored", { path: restoredPath }));
    } catch (err) {
      toast.error(String(err));
    } finally {
      setRestoring(false);
    }
  };

  /** 删除备份 */
  const handleDelete = async () => {
    setConfirmDeleteOpen(false);
    setDeleting(true);
    try {
      await terminalSetupApi.deleteBackup({ backupPath });
      toast.success(t("terminalSetup.backup.deleted"));
      onDelete(backupPath);
    } catch (err) {
      toast.error(String(err));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <>
      <div className="flex items-center gap-1.5 flex-wrap">
        {/* 复制路径 */}
        <CopyButton text={backupPath} />

        {/* 恢复按钮 */}
        <button
          type="button"
          disabled={restoring || deleting}
          onClick={() => setConfirmRestoreOpen(true)}
          className={cn(
            "inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs",
            "border border-blue-200 bg-blue-50 text-blue-700 hover:bg-blue-100",
            "dark:border-blue-800/50 dark:bg-blue-900/20 dark:text-blue-400 dark:hover:bg-blue-900/40",
            "transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed",
          )}
        >
          {restoring ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : (
            <RotateCcw className="h-3 w-3" />
          )}
          {t("terminalSetup.backup.restore")}
        </button>

        {/* 删除按钮 */}
        <button
          type="button"
          disabled={restoring || deleting}
          onClick={() => setConfirmDeleteOpen(true)}
          className={cn(
            "inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs",
            "border border-red-200 bg-red-50 text-red-700 hover:bg-red-100",
            "dark:border-red-800/50 dark:bg-red-900/20 dark:text-red-400 dark:hover:bg-red-900/40",
            "transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed",
          )}
        >
          {deleting ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : (
            <Trash2 className="h-3 w-3" />
          )}
          {t("terminalSetup.backup.delete")}
        </button>
      </div>

      {/* 还原二次确认 */}
      <ConfirmDialog
        isOpen={confirmRestoreOpen}
        title={t("terminalSetup.backup.restoreConfirmTitle")}
        message={t("terminalSetup.backup.restoreConfirmDesc", {
          backup: backupPath,
        })}
        confirmText={t("terminalSetup.backup.restore")}
        variant="info"
        onConfirm={handleRestore}
        onCancel={() => setConfirmRestoreOpen(false)}
      />

      {/* 删除二次确认 */}
      <ConfirmDialog
        isOpen={confirmDeleteOpen}
        title={t("terminalSetup.backup.deleteConfirmTitle")}
        message={t("terminalSetup.backup.deleteConfirmDesc", {
          backup: backupPath,
        })}
        confirmText={t("terminalSetup.backup.delete")}
        variant="destructive"
        onConfirm={handleDelete}
        onCancel={() => setConfirmDeleteOpen(false)}
      />
    </>
  );
}

function InstallSummaryCard({ result }: { result: TerminalSetupResult }) {
  const { t } = useTranslation();
  const hasZshrc =
    result.zshrcBackup !== null || result.configBackups.length > 0;

  // 本地维护备份列表，以便删除后从 UI 移除
  const [configBackups, setConfigBackups] = useState<string[]>(
    result.configBackups,
  );
  const [zshrcBackup, setZshrcBackup] = useState<string | null>(
    result.zshrcBackup,
  );
  const [openingTerminal, setOpeningTerminal] = useState(false);

  /** 从列表中移除已删除的备份 */
  const handleDeletedBackup = (path: string) => {
    if (path === zshrcBackup) {
      setZshrcBackup(null);
    } else {
      setConfigBackups((prev) => prev.filter((p) => p !== path));
    }
  };

  /** 打开新终端窗口 */
  const handleOpenTerminal = async () => {
    setOpeningTerminal(true);
    try {
      const app = await terminalSetupApi.openTerminal();
      toast.success(t("terminalSetup.openTerminalOpened", { app }));
    } catch (err) {
      toast.error(String(err));
    } finally {
      setOpeningTerminal(false);
    }
  };

  return (
    <div className="rounded-lg border border-border bg-card overflow-hidden">
      <div className="px-4 py-3 border-b border-border bg-muted/30">
        <p className="text-sm font-medium text-foreground">
          {t("terminalSetup.completed")}
        </p>
      </div>
      <div className="px-4 py-3 space-y-2">
        {/* 每个步骤结果 */}
        {result.steps.map((step) => (
          <div key={step.component} className="flex items-center gap-2 text-sm">
            {step.success ? (
              <CheckCircle2 className="h-4 w-4 text-emerald-500 shrink-0" />
            ) : step.skipped ? (
              <XCircle className="h-4 w-4 text-muted-foreground shrink-0" />
            ) : (
              <XCircle className="h-4 w-4 text-red-500 shrink-0" />
            )}
            <span
              className={cn(
                !step.success && !step.skipped && "text-red-600 dark:text-red-400",
              )}
            >
              {step.component}
              {step.skipped && (
                <span className="ml-1 text-xs text-muted-foreground">
                  (跳过)
                </span>
              )}
              {step.error && (
                <span className="ml-1 text-xs text-red-500">
                  — {step.error}
                </span>
              )}
            </span>
          </div>
        ))}

        {/* 备份路径列表 */}
        {(configBackups.length > 0 || zshrcBackup) && (
          <div className="mt-3 pt-3 border-t border-border space-y-2">
            <p className="text-xs font-medium text-muted-foreground">
              已备份的配置文件：
            </p>
            {zshrcBackup && (
              <BackupItem
                backupPath={zshrcBackup}
                onDelete={handleDeletedBackup}
              />
            )}
            {configBackups.map((path) => (
              <BackupItem
                key={path}
                backupPath={path}
                onDelete={handleDeletedBackup}
              />
            ))}
          </div>
        )}

        {/* source 提示（含打开新终端按钮） */}
        {hasZshrc && (
          <div className="mt-3 pt-3 border-t border-border rounded-lg bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800/50 px-3 py-2 space-y-2">
            <p className="text-xs text-amber-800 dark:text-amber-300">
              {t("terminalSetup.sourceHintRevised")}
            </p>
            <div className="flex items-center gap-2 flex-wrap">
              {/* 打开新终端主按钮 */}
              <button
                type="button"
                disabled={openingTerminal}
                onClick={handleOpenTerminal}
                className={cn(
                  "inline-flex items-center gap-1.5 px-3 py-1 rounded text-xs font-medium",
                  "bg-amber-600 hover:bg-amber-700 text-white",
                  "dark:bg-amber-700 dark:hover:bg-amber-600",
                  "transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed",
                )}
              >
                {openingTerminal ? (
                  <Loader2 className="h-3 w-3 animate-spin" />
                ) : (
                  <TerminalSquare className="h-3 w-3" />
                )}
                {t("terminalSetup.openTerminal")}
              </button>

              {/* 备选：复制 source 命令 */}
              <CopyButton text="source ~/.zshrc" />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ─── 移除结果卡片 ──────────────────────────────────────────────

function RemoveResultCard({ result }: { result: RemoveResult }) {
  return (
    <div className="rounded-lg border border-border bg-card overflow-hidden">
      <div className="px-4 py-3 border-b border-border bg-muted/30">
        <div className="flex items-center gap-2 text-sm font-medium">
          {result.blockRemoved ? (
            <CheckCircle2 className="h-4 w-4 text-emerald-500" />
          ) : (
            <Info className="h-4 w-4 text-muted-foreground" />
          )}
          <span>
            {result.blockRemoved
              ? "已删除 cc-doctor 配置块"
              : "未找到 cc-doctor 配置块（可能已删除或从未写入）"}
          </span>
        </div>
      </div>
      {result.keptConfigFiles.length > 0 && (
        <div className="px-4 py-3 space-y-1.5">
          <p className="text-xs text-muted-foreground">
            以下独立配置文件已保留（未删除）：
          </p>
          {result.keptConfigFiles.map((path) => (
            <div key={path} className="font-mono text-xs text-foreground/80">
              {path}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
