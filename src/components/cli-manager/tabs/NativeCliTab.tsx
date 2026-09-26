import { useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import {
  AlertTriangle,
  ArrowRight,
  BookOpen,
  CheckCircle2,
  Copy,
  Download,
  ExternalLink,
  FileJson,
  FolderOpen,
  Network,
  RefreshCw,
  Settings2,
  Terminal,
} from "lucide-react";
import { toast } from "sonner";
import type { NativeCliKey } from "../../../constants/clis";
import { cliManagerNativeInfoGet } from "../../../services/cli/cliManager";
import { copyText } from "../../../services/clipboard";
import { openDesktopPath, openDesktopUrl } from "../../../services/desktop/opener";
import { useNativeCliTargetsQuery } from "../../../query/nativeCli";
import { CliBrandIcon } from "../../home/CliBrandIcon";
import { NativeTargetPicker } from "../../../pages/providers/native/NativeTargetPicker";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { Dialog } from "../../../ui/Dialog";
import { cn } from "../../../utils/cn";
import { NativeCliUpdateCard } from "../NativeCliUpdateCard";
import { OmpSettingsPanel } from "../omp/OmpSettingsPanel";
import { confirmDesktopDialog } from "../../../services/desktop/confirm";

const NATIVE_CLI_HELP = {
  pi: {
    name: "Pi",
    docs: "https://github.com/earendil-works/pi/tree/main/packages/coding-agent",
    releases: "https://github.com/earendil-works/pi/releases",
    commands: [
      {
        label: "npm · 安装或更新",
        command: "npm install -g @earendil-works/pi-coding-agent@latest",
      },
    ],
  },
  omp: {
    name: "Oh My Pi",
    docs: "https://github.com/can1357/oh-my-pi",
    releases: "https://github.com/can1357/oh-my-pi/releases",
    commands: [
      { label: "Windows · PowerShell", command: "irm https://omp.sh/install.ps1 | iex" },
      { label: "macOS / Linux · Shell", command: "curl -fsSL https://omp.sh/install | sh" },
      { label: "Bun · 安装或更新", command: "bun install -g @oh-my-pi/pi-coding-agent@latest" },
    ],
  },
} as const;

async function copy(value: string, label: string) {
  try {
    await copyText(value);
    toast.success(label + "已复制");
  } catch {
    toast.error("复制失败，请手动复制");
  }
}
async function openDirectory(path: string) {
  try {
    if (!(await openDesktopPath(path))) toast.error("无法打开目录，请确认目录已存在");
  } catch {
    toast.error("无法打开目录，请确认目录已存在");
  }
}
async function openLink(url: string) {
  try {
    if (!(await openDesktopUrl(url))) toast.error("无法打开链接，请稍后重试");
  } catch {
    toast.error("无法打开链接，请稍后重试");
  }
}
function InfoItem({
  label,
  value,
  icon,
  children,
}: {
  label: string;
  value: string | null | undefined;
  icon: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="min-w-0 rounded-lg border border-border bg-secondary/50 p-3">
      <div className="mb-2 flex items-center gap-1.5 text-xs text-muted-foreground">
        {icon}
        {label}
      </div>
      <div className="flex min-w-0 items-center gap-1.5">
        <span
          className="min-w-0 flex-1 truncate font-mono text-xs text-secondary-foreground"
          title={value ?? undefined}
        >
          {value || "—"}
        </span>
        {children}
      </div>
    </div>
  );
}

export function NativeCliTab({ client }: { client: NativeCliKey }) {
  const help = NATIVE_CLI_HELP[client];
  const info = useQuery({
    queryKey: ["cli-manager", "native-info", client],
    queryFn: () => cliManagerNativeInfoGet(client),
    retry: false,
  });
  const targets = useNativeCliTargetsQuery(client);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [targetDialogOpen, setTargetDialogOpen] = useState(false);
  const [settingsDirty, setSettingsDirty] = useState(false);
  const target = !targets.isError
    ? (targets.data?.find((row) => (selectedId ? row.targetId === selectedId : row.selected)) ??
      (selectedId ? undefined : targets.data?.[0]))
    : undefined;
  const infoError = info.isError || Boolean(info.data?.error);
  const installed = !infoError && info.data?.found;
  const loading = info.isFetching || targets.isFetching;
  const executable = !infoError ? info.data?.executable_path : null;
  const targetParam = target ? "&target=" + encodeURIComponent(target.targetId) : "";
  return (
    <div className="space-y-6" aria-label={help.name + " 管理"}>
      <Card padding="none">
        <div className="space-y-5 p-5 sm:p-6">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="flex items-center gap-4">
              <div className="flex h-14 w-14 shrink-0 items-center justify-center rounded-xl bg-secondary text-foreground">
                <CliBrandIcon cliKey={client} className="h-8 w-8 text-3xl" />
              </div>
              <div>
                <h2 className="text-base font-semibold text-foreground">{help.name}</h2>
                <div className="mt-1.5 flex flex-wrap items-center gap-2 text-xs">
                  {info.isPending ? (
                    <span
                      role="status"
                      className="inline-flex items-center gap-1.5 text-muted-foreground"
                    >
                      <RefreshCw className="h-3 w-3 animate-spin" />
                      检测中…
                    </span>
                  ) : infoError ? (
                    <span className="inline-flex items-center gap-1.5 text-destructive">
                      <AlertTriangle className="h-3 w-3" />
                      检测失败
                    </span>
                  ) : installed ? (
                    <span className="inline-flex items-center gap-1.5 rounded-full bg-emerald-50 px-2.5 py-0.5 font-medium text-emerald-700 ring-1 ring-inset ring-emerald-600/20 dark:bg-emerald-950/30 dark:text-emerald-400">
                      <CheckCircle2 className="h-3 w-3" />
                      已安装{info.data?.version ? " " + info.data.version : " · 版本未知"}
                    </span>
                  ) : (
                    <span className="rounded-full bg-secondary px-2.5 py-0.5 text-muted-foreground">
                      未检测到
                    </span>
                  )}
                  <span className="text-muted-foreground">本机 CLI</span>
                </div>
              </div>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Button size="sm" variant="ghost" onClick={() => void openLink(help.docs)}>
                <BookOpen className="h-3.5 w-3.5" />
                官方文档
              </Button>
              <Button
                size="sm"
                variant="secondary"
                disabled={loading}
                onClick={() => {
                  void info.refetch();
                  void targets.refetch();
                }}
              >
                <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
                刷新
              </Button>
            </div>
          </div>
          {infoError ? (
            <p
              role="alert"
              className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive"
            >
              无法确认安装状态，请重试。
            </p>
          ) : null}
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
            <InfoItem
              label="可执行文件"
              value={executable}
              icon={<Terminal className="h-3.5 w-3.5" />}
            >
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 shrink-0"
                disabled={!executable}
                aria-label="复制可执行路径"
                title="复制可执行路径"
                onClick={() => {
                  if (executable) void copy(executable, "可执行路径");
                }}
              >
                <Copy className="h-3 w-3" />
              </Button>
            </InfoItem>
            <InfoItem
              label="配置目录"
              value={target?.agentDir}
              icon={<FolderOpen className="h-3.5 w-3.5" />}
            >
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 shrink-0"
                disabled={!target}
                aria-label="打开配置目录"
                title="打开配置目录"
                onClick={() => {
                  if (target) void openDirectory(target.agentDir);
                }}
              >
                <ExternalLink className="h-3 w-3" />
              </Button>
            </InfoItem>
            <InfoItem
              label="模型配置文件"
              value={target?.modelsPath}
              icon={<FileJson className="h-3.5 w-3.5" />}
            >
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 shrink-0"
                disabled={!target}
                aria-label="复制模型配置路径"
                title="复制模型配置路径"
                onClick={() => {
                  if (target) void copy(target.modelsPath, "模型配置路径");
                }}
              >
                <Copy className="h-3 w-3" />
              </Button>
            </InfoItem>
            <InfoItem
              label="解析方式 / Shell"
              value={
                !infoError && info.data
                  ? [info.data.resolved_via, info.data.shell].filter(Boolean).join(" · ")
                  : null
              }
              icon={<Settings2 className="h-3.5 w-3.5" />}
            />
          </div>
          {installed && !info.data?.version ? (
            <p className="text-xs text-muted-foreground">
              未能读取当前安装的版本，可在终端运行 {client} --version 确认。
            </p>
          ) : null}
        </div>
      </Card>
      <NativeCliUpdateCard key={client} client={client} />
      <Card padding="none">
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-5 py-4 sm:px-6">
          <h3 className="flex items-center gap-2 text-sm font-semibold">
            <FolderOpen className="h-4 w-4 text-muted-foreground" />
            配置目标
          </h3>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => {
              void (async () => {
                if (
                  settingsDirty &&
                  !(await confirmDesktopDialog("切换配置目标会放弃未保存的 OMP 设置，是否继续？"))
                )
                  return;
                setTargetDialogOpen(true);
              })();
            }}
            disabled={targets.isPending || targets.isError}
          >
            <Settings2 className="h-3.5 w-3.5" />
            切换目录{client === "omp" ? " / Profile" : ""}
          </Button>
        </div>
        <div className="px-5 py-4 sm:px-6">
          {targets.isPending ? (
            <p role="status" className="text-sm text-muted-foreground">
              读取原生目录…
            </p>
          ) : targets.isError ? (
            <p role="alert" className="text-sm text-destructive">
              无法确认原生目录，请刷新重试。
            </p>
          ) : target ? (
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2 text-sm font-medium">
                  {target.profile ? "Profile " + target.profile : "当前配置目录"}
                  <span className="rounded-md bg-secondary px-2 py-0.5 text-xs font-normal text-muted-foreground">
                    {target.format.toUpperCase()}
                  </span>
                  {!target.writable ? (
                    <span className="text-xs text-amber-700 dark:text-amber-400">只读目标</span>
                  ) : null}
                </div>
                <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
                  {target.agentDir}
                </p>
              </div>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => void copy(target.agentDir, "配置目录")}
              >
                <Copy className="h-3.5 w-3.5" />
                复制目录
              </Button>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">暂无有效配置目标，请先选择目录。</p>
          )}
        </div>
      </Card>
      <div className="grid gap-4 lg:grid-cols-2">
        <Card className="flex flex-col gap-4">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Settings2 className="h-4 w-4 text-muted-foreground" />
            <h3>供应商与模型</h3>
          </div>
          <p className="flex-1 text-sm leading-relaxed text-muted-foreground">
            管理原生供应商、凭证和模型，选择加入或移出 {client === "pi" ? "Pi" : "OMP"} 配置。
          </p>
          <Button asChild variant="secondary" className="self-start">
            <Link to={"/providers?cli=" + client + "&view=native" + targetParam}>
              管理原生供应商
              <ArrowRight className="h-3.5 w-3.5" />
            </Link>
          </Button>
        </Card>
        <Card className="flex flex-col gap-4">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Network className="h-4 w-4 text-muted-foreground" />
            <h3>AIO 网关</h3>
          </div>
          <p className="flex-1 text-sm leading-relaxed text-muted-foreground">
            配置上游、发布 AIO 模型入口，使用路由、重试与统计。在 CLI 中选择 AIO 模型后生效。
          </p>
          <Button asChild variant="secondary" className="self-start">
            <Link to={"/providers?cli=" + client + "&view=gateway" + targetParam}>
              管理 AIO 网关
              <ArrowRight className="h-3.5 w-3.5" />
            </Link>
          </Button>
        </Card>
      </div>
      {client === "omp" && target && (
        <OmpSettingsPanel
          key={target.targetId}
          targetId={target.targetId}
          onDirtyChange={setSettingsDirty}
        />
      )}
      <Card padding="none">
        <details
          key={installed ? "installed" : "missing"}
          open={!info.isPending && !infoError && !installed}
          className="group"
        >
          <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-5 py-4 text-sm font-semibold sm:px-6 [&::-webkit-details-marker]:hidden">
            <span className="flex items-center gap-2">
              <Download className="h-4 w-4 text-muted-foreground" />
              安装与更新
            </span>
            <span className="text-xs font-normal text-muted-foreground group-open:hidden">
              查看指引
            </span>
          </summary>
          <div className="space-y-4 border-t border-border px-5 py-4 sm:px-6">
            <p className="text-xs leading-relaxed text-muted-foreground">
              也可手动安装与更新：复制适合当前安装方式的命令，在终端执行后刷新。自动检查只提示，安装和升级须由你主动确认。
            </p>
            {help.commands.map(({ label, command }) => (
              <div key={label}>
                <div className="mb-1.5 text-xs text-muted-foreground">{label}</div>
                <div className="flex items-start gap-2 rounded-lg border border-border bg-secondary/50 p-3">
                  <code className="min-w-0 flex-1 break-all text-xs leading-6">{command}</code>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="h-6 w-6 shrink-0"
                    aria-label={"复制" + label + "命令"}
                    title="复制命令"
                    onClick={() => void copy(command, "安装命令")}
                  >
                    <Copy className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
            ))}
            <Button size="sm" variant="ghost" onClick={() => void openLink(help.releases)}>
              <ExternalLink className="h-3.5 w-3.5" />
              查看官方发布版本
            </Button>
          </div>
        </details>
      </Card>
      <p className="px-1 text-xs leading-relaxed text-muted-foreground">
        {client === "omp"
          ? "默认模型和 Agent 配置可在上方保存；登录与运行中会话由原生 CLI 管理。"
          : "默认模型、登录和会话设置由原生 CLI 管理。"}
        AIO 入口与原生供应商并存，加入入口后不会自动切换默认模型。
      </p>
      <Dialog open={targetDialogOpen} onOpenChange={setTargetDialogOpen} title="选择原生配置目标">
        {targets.isError ? (
          <p role="alert">无法确认原生目录，请关闭后刷新重试。</p>
        ) : (
          <NativeTargetPicker
            client={client}
            targets={targets.data ?? []}
            targetId={target?.targetId ?? null}
            onSelect={(next) => {
              setSelectedId(next.targetId);
              setTargetDialogOpen(false);
            }}
          />
        )}
      </Dialog>
    </div>
  );
}
