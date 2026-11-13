import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  Boxes,
  Brush,
  Check,
  ChevronRight,
  Clock,
  Database,
  Download,
  ExternalLink,
  Eye,
  EyeOff,
  Feather,
  FileText,
  Flame,
  FolderOpen,
  Gamepad2,
  Globe2,
  Heart,
  History,
  Image as ImageIcon,
  Layers3,
  LoaderCircle,
  Map as MapIcon,
  Monitor,
  Music2,
  PackageOpen,
  Play,
  Plus,
  RefreshCw,
  ScrollText,
  Search,
  Server as ServerIcon,
  Settings2,
  ShieldCheck,
  Sparkles,
  Trash2,
  Wrench,
  X,
} from "lucide-react";
import {
  api,
  AccountView,
  AppInfo,
  DeviceCode,
  AppSettings,
  Changelog,
  MusicTrack,
  ChangelogEntry,
  ConfigEntry,
  ContentItem,
  ContentKind,
  ContentSummary,
  CurseForgeFile,
  DiscoverItem,
  Instance,
  LoaderCatalog,
  ModMeta,
  ResourceFile,
  ResourceVersion,
  ServerEntry,
  ServerInput,
  VersionManifest,
  YggdrasilProfile,
  LittleSkinOauthDevice,
  ResourceProgressPayload,
  MpStatus,
  LoaderProgressPayload,
  AiSettingsView,
} from "./api";

type View = "home" | "browse" | "instances" | "loaders" | "settings" | "changelog" | "music";
type ResourceKind = "mod" | "modpack" | "datapack" | "resourcepack" | "shader" | "world";
type LoaderId =
  | "vanilla"
  | "optifine"
  | "forge"
  | "neoforge"
  | "fabric"
  | "legacyfabric"
  | "cleanroom"
  | "labymod"
  | "liteloader";
type Icon = typeof Boxes;

const resourceKinds: { id: ResourceKind; label: string; icon: Icon }[] = [
  { id: "mod", label: "模组", icon: Boxes },
  { id: "modpack", label: "整合包", icon: PackageOpen },
  { id: "datapack", label: "数据包", icon: Database },
  { id: "resourcepack", label: "资源包", icon: Layers3 },
  { id: "shader", label: "光影包", icon: Sparkles },
  { id: "world", label: "世界", icon: Globe2 },
];

const loaderEntries: { id: LoaderId; label: string; icon: Icon; short: string }[] = [
  { id: "vanilla", label: "Minecraft", icon: Gamepad2, short: "原版" },
  { id: "optifine", label: "OptiFine", icon: Sparkles, short: "优化" },
  { id: "forge", label: "Forge", icon: Wrench, short: "模组" },
  { id: "neoforge", label: "NeoForge", icon: Flame, short: "模组" },
  { id: "fabric", label: "Fabric", icon: Layers3, short: "模组" },
  { id: "legacyfabric", label: "Legacy Fabric", icon: History, short: "旧版" },
  { id: "cleanroom", label: "Cleanroom", icon: Brush, short: "1.12.2" },
  { id: "labymod", label: "LabyMod", icon: Monitor, short: "客户端" },
  { id: "liteloader", label: "LiteLoader", icon: Feather, short: "旧版" },
];

const loaderOptions = [
  { id: "", label: "任意加载器" },
  { id: "fabric", label: "Fabric" },
  { id: "forge", label: "Forge" },
  { id: "neoforge", label: "NeoForge" },
  { id: "quilt", label: "Quilt" },
  { id: "legacyfabric", label: "Legacy Fabric" },
  { id: "liteloader", label: "LiteLoader" },
  { id: "iris", label: "Iris" },
  { id: "optifine", label: "OptiFine" },
];

const sortOptions = [
  { id: "downloads", label: "热门下载" },
  { id: "relevance", label: "相关度" },
  { id: "updated", label: "最近更新" },
  { id: "newest", label: "最新发布" },
];

const kindLabels: Record<ResourceKind, string> = {
  mod: "模组",
  modpack: "整合包",
  datapack: "数据包",
  resourcepack: "资源包",
  shader: "光影包",
  world: "世界",
};

function loaderLabel(id: string) {
  return loaderEntries.find((item) => item.id === id)?.label ?? id;
}

function modrinthLoaderFor(loader: string): string | undefined {
  switch (loader) {
    case "fabric":
    case "legacyfabric":
      return "fabric";
    case "forge":
      return "forge";
    case "neoforge":
      return "neoforge";
    case "quilt":
      return "quilt";
    case "liteloader":
      return "liteloader";
    default:
      return undefined;
  }
}

function cfLoaderFor(loader: string): number | undefined {
  switch (loader) {
    case "forge":
      return 1;
    case "liteloader":
      return 3;
    case "fabric":
    case "legacyfabric":
      return 4;
    case "quilt":
      return 5;
    case "neoforge":
      return 6;
    default:
      return undefined;
  }
}

function formatNumber(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(value);
}

function formatDate(value: string): string {
  if (!value) return "未知";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value.slice(0, 10);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GiB`;
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KiB`;
}

function App() {
  const [view, setView] = useState<View>("home");
  const [kind, setKind] = useState<ResourceKind>("mod");
  const [loaderFocus, setLoaderFocus] = useState<LoaderId>("fabric");
  const [instances, setInstances] = useState<Instance[]>([]);
  const [activeId, setActiveId] = useState("");
  const [manifest, setManifest] = useState<VersionManifest | null>(null);
  const [settings, setSettings] = useState<AppSettings>({
    proxy: "",
    sakura_enabled: false,
    shared_game_dir: "",
    download_source: "official",
    ms_client_id: "",
    active_account: "",
  });
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [sakura, setSakura] = useState("");
  const [javaCount, setJavaCount] = useState(0);
  const [notice, setNotice] = useState("");
  const [showCreate, setShowCreate] = useState(false);

  const activeInstance = instances.find((item) => item.id === activeId);

  const refreshInstances = async () => {
    const list = await api.instances();
    setInstances(list);
    setActiveId((current) =>
      list.some((item) => item.id === current) ? current : (list[0]?.id ?? ""),
    );
  };

  const refreshInfo = async () => {
    try {
      setInfo(await api.info());
    } catch (error) {
      setNotice(String(error));
    }
  };

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [appInfo, list, manifestData, appSettings, javaList] = await Promise.all([
          api.info(),
          api.instances(),
          api.versions().catch(() => null),
          api.settings(),
          api.java().catch(() => []),
        ]);
        if (cancelled) return;
        setInfo(appInfo);
        setInstances(list);
        setActiveId(list[0]?.id ?? "");
        setManifest(manifestData);
        setSettings(appSettings);
        setJavaCount(javaList.length);
        if (appSettings.sakura_enabled) {
          api
            .sakura()
            .then((background) => {
              if (!cancelled) setSakura(background.data_url);
            })
            .catch(() => undefined);
        }
      } catch (error) {
        if (!cancelled) setNotice(String(error));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const saveSettings = async (next: AppSettings) => {
    try {
      await api.saveSettings(next);
      setSettings(next);
      if (next.sakura_enabled) {
        const background = await api.sakura();
        setSakura(background.data_url);
      } else {
        setSakura("");
      }
    } catch (error) {
      setNotice(String(error));
    }
  };

  const refreshSakura = async () => {
    try {
      const background = await api.sakura(true);
      setSakura(background.data_url);
    } catch (error) {
      setNotice(String(error));
    }
  };

  const openExternal = (url: string) => {
    void api.openExternal(url).catch((error) => setNotice(String(error)));
  };

  const titles: Record<View, string> = {
    home: "总览",
    browse: `${kindLabels[kind]}下载`,
    instances: "游戏实例",
    loaders: `${loaderLabel(loaderFocus)} 安装`,
    settings: "设置",
    changelog: "更新内容",
    music: "音乐舱",
  };

  return (
    <div className={`app-shell ${sakura ? "with-sakura" : ""}`}>
      {sakura && (
        <>
          <div
            className="sakura-layer"
            style={{ backgroundImage: `url(${sakura})` }}
            aria-hidden="true"
          />
          <div className="sakura-shade" aria-hidden="true" />
        </>
      )}
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">✦</div>
          <div className="brand-copy">
            <strong>AURORA</strong>
            <small>MINECRAFT LAUNCHER</small>
          </div>
        </div>

        <SideSection title="资源">
          {resourceKinds.map((item) => (
            <SideButton
              key={item.id}
              active={view === "browse" && kind === item.id}
              icon={item.icon}
              label={item.label}
              onClick={() => {
                setKind(item.id);
                setView("browse");
              }}
            />
          ))}
        </SideSection>

        <SideSection title="安装包">
          {loaderEntries.map((item) => (
            <SideButton
              key={item.id}
              active={view === "loaders" && loaderFocus === item.id}
              icon={item.icon}
              label={item.label}
              hint={item.short}
              onClick={() => {
                setLoaderFocus(item.id);
                setView("loaders");
              }}
            />
          ))}
        </SideSection>

        <SideSection title="管理">
          <SideButton
            active={view === "instances"}
            icon={Boxes}
            label="游戏实例"
            count={instances.length}
            onClick={() => setView("instances")}
          />
          <SideButton
            active={view === "music"}
            icon={Music2}
            label="音乐舱"
            onClick={() => setView("music")}
          />
          <SideButton
            active={view === "changelog"}
            icon={ScrollText}
            label="更新内容"
            onClick={() => setView("changelog")}
          />
          <SideButton
            active={view === "settings"}
            icon={Settings2}
            label="设置"
            onClick={() => setView("settings")}
          />
        </SideSection>

        <div className="sidebar-bottom">
          <button className="account-card" onClick={() => setView("instances")}>
            <div className="avatar">A</div>
            <div className="account-copy">
              <strong>{activeInstance?.name ?? "未选择实例"}</strong>
              <small>
                {activeInstance
                  ? `${activeInstance.minecraft_version} · ${loaderLabel(activeInstance.loader)}`
                  : "创建实例后即可安装资源"}
              </small>
            </div>
            <ChevronRight size={16} />
          </button>
        </div>
      </aside>

      <main className="main-content">
        <header className="topbar">
          <div>
            <span className="eyebrow">AURORA / {view.toUpperCase()}</span>
            <h1>{titles[view]}</h1>
          </div>
          <div className="top-actions">
            {sakura && (
              <button
                className="icon-button"
                onClick={() => void refreshSakura()}
                title="换一张背景（随机二次元图）"
              >
                <ImageIcon size={18} />
              </button>
            )}
            {instances.length > 0 && (
              <select
                className="instance-select"
                value={activeId}
                onChange={(event) => setActiveId(event.target.value)}
              >
                {instances.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name} · {item.minecraft_version}
                  </option>
                ))}
              </select>
            )}
            <button className="icon-button" onClick={() => setView("settings")} title="设置">
              <Settings2 size={18} />
            </button>
            <button className="primary-button" onClick={() => setShowCreate(true)}>
              <Plus size={17} /> 新建实例
            </button>
          </div>
        </header>

        {info?.region_locked && (
          <div className="region-banner">
            <ShieldCheck size={16} />
            此发行版在你所在地区不提供资源下载，浏览与实例管理不受影响。
          </div>
        )}

        {notice && (
          <div className="notice">
            <span>{notice}</span>
            <button onClick={() => setNotice("")}>
              <X size={15} />
            </button>
          </div>
        )}

        {view === "home" && (
          <Home
            info={info}
            instances={instances}
            active={activeInstance}
            manifest={manifest}
            javaCount={javaCount}
            onBrowse={(nextKind) => {
              setKind(nextKind);
              setView("browse");
            }}
            onInstances={() => setView("instances")}
            onCreate={() => setShowCreate(true)}
            onLoaders={(id) => {
              setLoaderFocus(id);
              setView("loaders");
            }}
          />
        )}

        {view === "browse" && (
          <Browse
            kind={kind}
            instances={instances}
            activeId={activeId}
            setActiveId={setActiveId}
            manifest={manifest}
            info={info}
            onNotice={setNotice}
            onOpenExternal={openExternal}
            onOpenSettings={() => setView("settings")}
          />
        )}

        {view === "instances" && (
          <Instances
            instances={instances}
            activeId={activeId}
            setActiveId={setActiveId}
            manifest={manifest}
            onRefresh={refreshInstances}
            onNotice={setNotice}
            onCreate={() => setShowCreate(true)}
          />
        )}

        {view === "loaders" && (
          <Loaders
            focus={loaderFocus}
            setFocus={setLoaderFocus}
            instances={instances}
            activeId={activeId}
            manifest={manifest}
            onNotice={setNotice}
            onOpenExternal={openExternal}
            onCreate={() => setShowCreate(true)}
            onRefresh={refreshInstances}
          />
        )}

        {view === "music" && <MusicDeck onNotice={setNotice} />}

        {view === "changelog" && (
          <ChangelogPage currentVersion={info?.version ?? ""} onNotice={setNotice} />
        )}

        {view === "settings" && (
          <SettingsPage
            info={info}
            settings={settings}
            sakura={sakura}
            onSave={saveSettings}
            onRefreshSakura={refreshSakura}
            onRefreshInfo={refreshInfo}
            onNotice={setNotice}
          />
        )}
      </main>

      {showCreate && (
        <CreateInstance
          manifest={manifest}
          onClose={() => setShowCreate(false)}
          onNotice={setNotice}
          onCreated={async (instance) => {
            await refreshInstances();
            setActiveId(instance.id);
            setShowCreate(false);
            setView("instances");
            setNotice("实例已创建，目录已隔离");
          }}
        />
      )}
    </div>
  );
}

function SideSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="side-section">
      <span className="side-title">{title}</span>
      {children}
    </div>
  );
}

function SideButton({
  active,
  icon: Icon,
  label,
  hint,
  count,
  onClick,
}: {
  active: boolean;
  icon: Icon;
  label: string;
  hint?: string;
  count?: number;
  onClick: () => void;
}) {
  return (
    <button className={`side-button ${active ? "active" : ""}`} onClick={onClick}>
      <Icon size={16} />
      <span>{label}</span>
      {hint && <em>{hint}</em>}
      {count !== undefined && <em>{count}</em>}
    </button>
  );
}

function Home({
  info,
  instances,
  active,
  manifest,
  javaCount,
  onBrowse,
  onInstances,
  onCreate,
  onLoaders,
}: {
  info: AppInfo | null;
  instances: Instance[];
  active?: Instance;
  manifest: VersionManifest | null;
  javaCount: number;
  onBrowse: (kind: ResourceKind) => void;
  onInstances: () => void;
  onCreate: () => void;
  onLoaders: (id: LoaderId) => void;
}) {
  return (
    <div className="page-stack">
      <section className="hero-card">
        <div className="hero-glow" />
        <div className="hero-copy">
          <span className="pill accent">
            <Sparkles size={13} /> AURORA RESOURCE CENTER
          </span>
          <h2>
            找到想玩的，
            <br />
            <span>然后一键开始。</span>
          </h2>
          <p>自动预加载热门模组、光影、材质与整合包，按版本和加载器筛选，安装到独立实例。</p>
          <div className="hero-actions">
            <button className="primary-button" onClick={active ? onInstances : onCreate}>
              {active ? "查看我的实例" : "创建第一个实例"}
              <ChevronRight size={16} />
            </button>
            <button className="ghost-button" onClick={() => onBrowse("mod")}>
              浏览资源
            </button>
          </div>
        </div>
        <div className="hero-orbit">
          <div className="orbit-ring ring-one" />
          <div className="orbit-ring ring-two" />
          <div className="cube-orbit">✦</div>
        </div>
      </section>

      <div className="stats-grid">
        <Stat label="最新正式版" value={manifest?.latest_release ?? "同步中"} hint="Mojang 官方元数据" icon={Gamepad2} />
        <Stat label="游戏实例" value={String(instances.length)} hint={instances.length ? "每个实例独立目录" : "还没有实例"} icon={Boxes} />
        <Stat label="Java" value={javaCount ? "已检测" : "未找到"} hint={javaCount ? "可用于加载器安装器" : "安装 Java 17+"} icon={Wrench} />
        <Stat
          label="CurseForge"
          value={info?.curseforge_configured ? "已配置" : "未配置"}
          hint={info?.curseforge_configured ? info.curseforge_mode : "Modrinth 仍可用"}
          icon={ShieldCheck}
        />
      </div>

      <section className="section-heading">
        <div>
          <span className="eyebrow">QUICK ACCESS</span>
          <h3>热门资源分类</h3>
        </div>
        <button className="text-button" onClick={() => onBrowse("mod")}>
          查看全部 <ChevronRight size={15} />
        </button>
      </section>

      <div className="quick-grid">
        {resourceKinds.slice(0, 4).map((item) => (
          <button className="quick-card" key={item.id} onClick={() => onBrowse(item.id)}>
            <div className={`quick-icon ${item.id}`}>
              <item.icon size={21} />
            </div>
            <div>
              <strong>{item.label}</strong>
              <small>
                {item.id === "mod"
                  ? "社区热门模组"
                  : item.id === "modpack"
                    ? "完整体验整合"
                    : item.id === "datapack"
                      ? "扩展世界玩法"
                      : "替换视觉风格"}
              </small>
            </div>
            <ChevronRight size={16} />
          </button>
        ))}
      </div>

      <section className="section-heading">
        <div>
          <span className="eyebrow">INSTALLERS</span>
          <h3>加载器入口</h3>
        </div>
        <button className="text-button" onClick={() => onLoaders("fabric")}>
          查看全部 <ChevronRight size={15} />
        </button>
      </section>
      <div className="quick-grid">
        {loaderEntries.slice(4, 8).map((item) => (
          <button className="quick-card" key={item.id} onClick={() => onLoaders(item.id)}>
            <div className="quick-icon loader">
              <item.icon size={20} />
            </div>
            <div>
              <strong>{item.label}</strong>
              <small>{item.short}</small>
            </div>
            <ChevronRight size={16} />
          </button>
        ))}
      </div>

      {instances.length > 0 && (
        <>
          <section className="section-heading">
            <div>
              <span className="eyebrow">YOUR INSTANCES</span>
              <h3>最近实例</h3>
            </div>
            <button className="text-button" onClick={onInstances}>
              管理 <ChevronRight size={15} />
            </button>
          </section>
          <div className="mini-instance-grid">
            {instances.slice(0, 3).map((instance) => (
              <button className="mini-instance" key={instance.id} onClick={onInstances}>
                <div className="mini-cover">✦</div>
                <div>
                  <strong>{instance.name}</strong>
                  <small>
                    {instance.minecraft_version} · {loaderLabel(instance.loader)}
                  </small>
                </div>
                <ChevronRight size={15} />
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function Stat({ label, value, hint, icon: Icon }: { label: string; value: string; hint: string; icon: Icon }) {
  return (
    <div className="stat-card">
      <div className="stat-icon">
        <Icon size={18} />
      </div>
      <div>
        <span>{label}</span>
        <strong>{value}</strong>
        <small>{hint}</small>
      </div>
    </div>
  );
}

function Browse({
  kind,
  instances,
  activeId,
  setActiveId,
  manifest,
  info,
  onNotice,
  onOpenExternal,
  onOpenSettings,
}: {
  kind: ResourceKind;
  instances: Instance[];
  activeId: string;
  setActiveId: (id: string) => void;
  manifest: VersionManifest | null;
  info: AppInfo | null;
  onNotice: (message: string) => void;
  onOpenExternal: (url: string) => void;
  onOpenSettings: () => void;
}) {
  const [query, setQuery] = useState("");
  const [submitted, setSubmitted] = useState("");
  const [source, setSource] = useState("all");
  const [sort, setSort] = useState("downloads");
  const [gameVersion, setGameVersion] = useState("");
  const [loader, setLoader] = useState("");
  const [items, setItems] = useState<DiscoverItem[]>([]);
  const [total, setTotal] = useState(0);
  const [note, setNote] = useState("");
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<DiscoverItem | null>(null);
  const [favorites, setFavorites] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("aurora.favorites") ?? "[]");
    } catch {
      return [];
    }
  });

  const versions = useMemo(
    () => (manifest?.versions ?? []).filter((item) => item.kind === "release").slice(0, 120),
    [manifest],
  );

  useEffect(() => {
    try {
      localStorage.setItem("aurora.favorites", JSON.stringify(favorites));
    } catch {
      // Ignore storage failures.
    }
  }, [favorites]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setSelected(null);
    api
      .discover({
        kind,
        source,
        query: submitted,
        gameVersion: gameVersion || undefined,
        loader: loader || undefined,
        sort,
        offset: 0,
        limit: 20,
      })
      .then((page) => {
        if (cancelled) return;
        setItems(page.items);
        setTotal(page.total);
        setNote(page.note ?? "");
      })
      .catch((error) => {
        if (!cancelled) onNotice(String(error));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [kind, source, submitted, gameVersion, loader, sort, onNotice]);

  const loadMore = async () => {
    if (loading) return;
    setLoading(true);
    try {
      const page = await api.discover({
        kind,
        source,
        query: submitted,
        gameVersion: gameVersion || undefined,
        loader: loader || undefined,
        sort,
        offset: items.length,
        limit: 20,
      });
      setItems((current) => [...current, ...page.items]);
      setTotal(page.total);
    } catch (error) {
      onNotice(String(error));
    } finally {
      setLoading(false);
    }
  };

  const toggleFavorite = (item: DiscoverItem) => {
    const key = `${item.source}:${item.id}`;
    setFavorites((current) =>
      current.includes(key) ? current.filter((value) => value !== key) : [...current, key],
    );
  };

  const activeInstance = instances.find((item) => item.id === activeId);

  return (
    <div className="page-stack browse-page">
      <section className="filter-panel">
        <div className="search-row">
          <div className="search-box">
            <Search size={18} />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") setSubmitted(query.trim());
              }}
              placeholder={`搜索${kindLabels[kind]}，留空浏览热门内容`}
            />
          </div>
          <button className="primary-button" onClick={() => setSubmitted(query.trim())}>
            <Search size={17} /> 搜索
          </button>
        </div>

        <div className="filter-grid">
          <label className="filter-field">
            来源
            <select value={source} onChange={(event) => setSource(event.target.value)}>
              <option value="all">全部</option>
              <option value="modrinth">Modrinth</option>
              <option value="curseforge">CurseForge</option>
            </select>
          </label>
          <label className="filter-field">
            排序
            <select value={sort} onChange={(event) => setSort(event.target.value)}>
              {sortOptions.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.label}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-field">
            版本
            <select value={gameVersion} onChange={(event) => setGameVersion(event.target.value)}>
              <option value="">任意版本</option>
              {versions.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.id}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-field">
            加载器
            <select value={loader} onChange={(event) => setLoader(event.target.value)}>
              {loaderOptions.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.label}
                </option>
              ))}
            </select>
          </label>
          <button className="icon-button" onClick={() => setSubmitted(query.trim())} title="刷新">
            <RefreshCw size={17} />
          </button>
        </div>

        <div className="filter-footer">
          <span>
            当前实例：{activeInstance ? `${activeInstance.name} · ${activeInstance.minecraft_version}` : "未选择"}
          </span>
          <span>
            {info?.curseforge_configured ? "CurseForge 已配置" : "CurseForge 未配置，仅 Modrinth"}
          </span>
          {!info?.curseforge_configured && (
            <button className="text-button" onClick={onOpenSettings}>
              配置 Key
            </button>
          )}
        </div>
      </section>

      {note && <div className="inline-note">{note}</div>}

      <div className="list-heading">
        <span>
          共 {total.toLocaleString()} 个结果 · 已显示 {items.length}
        </span>
        {loading && <LoaderCircle className="spin" size={16} />}
      </div>

      <section className="resource-list">
        {items.map((item) => (
          <ResourceRow
            key={`${item.source}:${item.id}`}
            item={item}
            favorite={favorites.includes(`${item.source}:${item.id}`)}
            onFavorite={() => toggleFavorite(item)}
            onSelect={() => setSelected(item)}
            onOpen={() => onOpenExternal(item.url)}
          />
        ))}
        {!loading && items.length === 0 && (
          <div className="empty-state">
            <div className="empty-icon">
              <Search size={26} />
            </div>
            <h3>没有找到内容</h3>
            <p>尝试清空搜索词、切换来源，或换一个 Minecraft 版本与加载器。</p>
          </div>
        )}
      </section>

      {items.length < total && (
        <button className="load-more" onClick={() => void loadMore()} disabled={loading}>
          {loading ? <LoaderCircle className="spin" size={16} /> : <RefreshCw size={16} />} 加载更多
        </button>
      )}

      {selected && (
        <FilePanel
          item={selected}
          instances={instances}
          activeId={activeId}
          setActiveId={setActiveId}
          onClose={() => setSelected(null)}
          onNotice={onNotice}
          onOpenExternal={onOpenExternal}
        />
      )}
    </div>
  );
}

function ResourceRow({
  item,
  favorite,
  onFavorite,
  onSelect,
  onOpen,
}: {
  item: DiscoverItem;
  favorite: boolean;
  onFavorite: () => void;
  onSelect: () => void;
  onOpen: () => void;
}) {
  const versions = item.game_versions.slice(0, 3);
  return (
    <article className="resource-row" onClick={onSelect}>
      <div className="resource-icon">
        {item.icon_url ? <img src={item.icon_url} alt="" loading="lazy" /> : <PackageOpen size={24} />}
      </div>
      <div className="resource-main">
        <div className="resource-title">
          <h3>{item.name}</h3>
          <span className={`source-badge ${item.source.toLowerCase()}`}>{item.source}</span>
        </div>
        <p>{item.description || "暂无简介"}</p>
        <div className="resource-meta">
          <span>
            <Boxes size={13} /> {item.loaders.join(" / ") || "通用"}
          </span>
          <span>
            <Gamepad2 size={13} /> {versions.join(", ") || "全版本"}
            {item.game_versions.length > 3 ? " +" : ""}
          </span>
          <span>
            <Download size={13} /> {formatNumber(item.downloads)}
          </span>
          <span>
            <Clock size={13} /> {formatDate(item.updated)}
          </span>
        </div>
        <div className="chip-row">
          {item.categories.slice(0, 4).map((category) => (
            <span className="chip" key={category}>
              {category}
            </span>
          ))}
        </div>
      </div>
      <div className="resource-actions">
        <button
          className="ghost-button"
          onClick={(event) => {
            event.stopPropagation();
            onSelect();
          }}
        >
          <Download size={15} /> 安装
        </button>
        <button
          className={`icon-button ${favorite ? "favorited" : ""}`}
          title="收藏"
          onClick={(event) => {
            event.stopPropagation();
            onFavorite();
          }}
        >
          <Heart size={16} fill={favorite ? "currentColor" : "none"} />
        </button>
        <button
          className="icon-button"
          title="打开项目页"
          onClick={(event) => {
            event.stopPropagation();
            onOpen();
          }}
        >
          <ExternalLink size={16} />
        </button>
      </div>
    </article>
  );
}

type FileEntry = {
  key: string;
  title: string;
  detail: string;
  modrinth?: ResourceFile;
  curseforge?: CurseForgeFile;
};

function FilePanel({
  item,
  instances,
  activeId,
  setActiveId,
  onClose,
  onNotice,
  onOpenExternal,
}: {
  item: DiscoverItem;
  instances: Instance[];
  activeId: string;
  setActiveId: (id: string) => void;
  onClose: () => void;
  onNotice: (message: string) => void;
  onOpenExternal: (url: string) => void;
}) {
  const [instanceId, setInstanceId] = useState(activeId || instances[0]?.id || "");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [progress, setProgress] = useState<Record<string, ResourceProgressPayload>>({});

  useEffect(() => {
    const unlisten = listen<ResourceProgressPayload>("resource-progress", (event) => {
      setProgress((previous) => ({
        ...previous,
        [event.payload.filename]: event.payload,
      }));
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);
  const instance = instances.find((value) => value.id === instanceId);
  const manualOnly = item.kind === "modpack" || item.kind === "world";

  useEffect(() => {
    if (!instance) {
      setEntries([]);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setShowAll(false);
    const loaderFilter = item.kind === "mod" ? modrinthLoaderFor(instance.loader) : undefined;
    if (item.source === "Modrinth") {
      api
        .modrinthVersions(item.id, instance.minecraft_version, loaderFilter)
        .then((versions: ResourceVersion[]) => {
          if (cancelled) return;
          const list: FileEntry[] = [];
          for (const version of versions) {
            const file = version.files.find((value) => value.primary) ?? version.files[0];
            if (!file) continue;
            list.push({
              key: `m-${version.id}`,
              title: version.name || version.version_number,
              detail: `${file.filename} · ${formatBytes(file.size)} · ${version.loaders.join(", ") || "universal"}`,
              modrinth: file,
            });
          }
          setEntries(list);
        })
        .catch((error) => {
          if (!cancelled) onNotice(String(error));
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    } else {
      api
        .curseforgeFiles(
          Number(item.id),
          instance.minecraft_version,
          item.kind === "mod" ? cfLoaderFor(instance.loader) : undefined,
        )
        .then((files) => {
          if (cancelled) return;
          const list: FileEntry[] = files
            .filter((file) => Boolean(file.download_url))
            .map((file) => ({
              key: `c-${file.id}`,
              title: file.display_name || file.file_name,
              detail: `${file.file_name} · ${formatBytes(file.file_length)} · ${file.release_type}${file.mod_loader ? ` · ${file.mod_loader}` : ""}`,
              curseforge: file,
            }));
          setEntries(list);
        })
        .catch((error) => {
          if (!cancelled) onNotice(String(error));
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    }
    return () => {
      cancelled = true;
    };
  }, [item, instance, onNotice]);

  const install = async (entry: FileEntry) => {
    if (!instance) {
      onNotice("请先选择实例");
      return;
    }
    setBusyKey(entry.key);
    try {
      if (entry.modrinth) {
        if (manualOnly) {
          await api.downloadModrinthFile({ instanceId: instance.id, file: entry.modrinth });
        } else {
          await api.installModrinthFile({
            instanceId: instance.id,
            category: item.kind,
            file: entry.modrinth,
          });
        }
      } else if (entry.curseforge) {
        if (manualOnly) {
          await api.downloadCurseForgeFile({ instanceId: instance.id, file: entry.curseforge });
        } else {
          await api.installCurseForgeFile({
            instanceId: instance.id,
            category: item.kind,
            file: entry.curseforge,
          });
        }
      }
      onNotice(
        manualOnly
          ? "已下载到实例 downloads 目录；整合包/世界自动导入尚未接入"
          : "文件已校验并安装到实例",
      );
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusyKey(null);
    }
  };

  const visible = showAll ? entries : entries.slice(0, 15);

  return (
    <div className="drawer-backdrop" onMouseDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <section className="detail-drawer">
        <header className="drawer-head">
          <div className="drawer-project">
            <div className="resource-icon small">
              {item.icon_url ? <img src={item.icon_url} alt="" /> : <PackageOpen size={20} />}
            </div>
            <div>
              <span className="eyebrow">{item.source.toUpperCase()} · {kindLabels[item.kind as ResourceKind] ?? item.kind}</span>
              <h2>{item.name}</h2>
            </div>
          </div>
          <button className="icon-button" onClick={onClose}>
            <X size={17} />
          </button>
        </header>

        <div className="drawer-body">
          <p className="drawer-desc">{item.description || "暂无简介"}</p>

          <div className="drawer-filters">
            <label className="filter-field">
              安装到
              <select
                value={instance?.id ?? ""}
                onChange={(event) => {
                  setInstanceId(event.target.value);
                  setActiveId(event.target.value);
                }}
              >
                <option value="">请选择实例</option>
                {instances.map((value) => (
                  <option key={value.id} value={value.id}>
                    {value.name} · {value.minecraft_version} · {loaderLabel(value.loader)}
                  </option>
                ))}
              </select>
            </label>
            <button className="ghost-button" onClick={() => onOpenExternal(item.url)}>
              <ExternalLink size={15} /> 项目页
            </button>
          </div>

          {!instance && (
            <div className="inline-note">选择一个实例后，才会按它的版本和加载器请求文件。</div>
          )}
          {manualOnly && (
            <div className="inline-note">
              整合包和世界暂不自动导入：文件会下载到实例 downloads 目录，校验来源与哈希后由你手动导入。
            </div>
          )}

          <div className="file-list">
            {loading && (
              <div className="file-loading">
                <LoaderCircle className="spin" size={18} /> 正在获取文件列表
              </div>
            )}
            {!loading && visible.length === 0 && instance && (
              <div className="inline-note">没有与该实例匹配的文件。检查实例版本或加载器。</div>
            )}
            {visible.map((entry) => {
              const fileName = entry.modrinth?.filename ?? entry.curseforge?.file_name ?? "";
              const active = progress[fileName];
              const percent =
                active && active.total > 0
                  ? Math.min(100, Math.round((active.downloaded / active.total) * 100))
                  : null;
              return (
              <div className="file-row" key={entry.key}>
                <div>
                  <strong>{entry.title}</strong>
                  <small>{entry.detail}</small>
                  {busyKey === entry.key && percent !== null && (
                    <div className="download-progress">
                      <div className="progress-track">
                        <div className="progress-fill" style={{ width: `${percent}%` }} />
                      </div>
                      <small>
                        {percent}% · {formatBytes(active.downloaded)}
                        {active.total > 0 ? ` / ${formatBytes(active.total)}` : ""}
                      </small>
                    </div>
                  )}
                </div>
                <button
                  className="ghost-button"
                  disabled={busyKey === entry.key}
                  onClick={() => void install(entry)}
                >
                  {busyKey === entry.key ? (
                    <LoaderCircle className="spin" size={15} />
                  ) : manualOnly ? (
                    "下载"
                  ) : (
                    "安装"
                  )}
                </button>
              </div>
              );
            })}
            {entries.length > 15 && (
              <button className="text-button" onClick={() => setShowAll((value) => !value)}>
                {showAll ? "收起文件列表" : `显示全部 ${entries.length} 个文件`}
              </button>
            )}
          </div>
        </div>
      </section>
    </div>
  );
}

function useLoaderProgress(active: boolean) {
  const [progress, setProgress] = useState<LoaderProgressPayload | null>(null);
  useEffect(() => {
    if (!active) return;
    const unlisten = listen<LoaderProgressPayload>("loader-progress", (event) => {
      setProgress(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
      setProgress(null);
    };
  }, [active]);
  return progress;
}

function LoaderProgressLine({ progress }: { progress: LoaderProgressPayload }) {
  const percent =
    progress.total > 0 ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : null;
  return (
    <div className="download-progress">
      <div className="progress-track">
        <div className="progress-fill" style={{ width: `${percent ?? 0}%` }} />
      </div>
      <small>
        {progress.stage}
        {percent !== null
          ? ` · ${percent}%${progress.total > 0 ? ` · ${formatBytes(progress.downloaded)} / ${formatBytes(progress.total)}` : ""}`
          : ""}
      </small>
    </div>
  );
}

function MultiplayerCard({ onNotice }: { onNotice: (message: string) => void }) {
  const [status, setStatus] = useState<MpStatus | null>(null);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [log, setLog] = useState<string[]>([]);

  useEffect(() => {
    api.mpStatus().then(setStatus).catch(() => undefined);
    const unlisten = listen<{ line: string }>("mp-log", (event) => {
      setLog((previous) => [...previous.slice(-80), event.payload.line]);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  const act = async (action: () => Promise<MpStatus>, key: string, done?: string) => {
    setBusy(key);
    try {
      const next = await action();
      setStatus(next);
      if (done) onNotice(done);
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className="settings-card multiplayer-card">
      <div className="settings-heading">
        <div className="setting-icon network">
          <Globe2 size={22} />
        </div>
        <div>
          <h3>联机（EasyTier 虚拟局域网）</h3>
          <p>
            创建房间后对局域网开放世界，朋友用房间码加入并连接你的虚拟 IP；需要管理员权限创建虚拟网卡。
          </p>
        </div>
      </div>
      <div className="settings-actions" style={{ justifyContent: "flex-start" }}>
        <button
          className="primary-button"
          onClick={() => void act(() => api.mpHost(), "host", "房间已创建；把房间码发给朋友")}
          disabled={busy !== null}
        >
          {busy === "host" ? <LoaderCircle className="spin" size={15} /> : <Plus size={15} />} 创建房间
        </button>
        <input
          value={code}
          onChange={(event) => setCode(event.target.value)}
          placeholder="房间码：网络名#密钥"
          spellCheck={false}
        />
        <button
          className="ghost-button"
          onClick={() => void act(() => api.mpJoin(code), "join", "已加入房间，等待分配虚拟 IP")}
          disabled={busy !== null || !code.trim()}
        >
          {busy === "join" ? <LoaderCircle className="spin" size={15} /> : <Play size={15} />} 加入房间
        </button>
        <button
          className="ghost-button"
          onClick={() => void act(() => api.mpStop(), "stop", "联机已停止")}
          disabled={busy !== null}
        >
          停止
        </button>
      </div>
      {status?.running && (
        <div className="mp-status">
          <span className="pill mini">进行中</span>
          <span>
            角色：{status.role === "host" ? "房主" : "客人"} · 房间码{" "}
            <strong>{status.room_code}</strong> · 虚拟 IP：<strong>{status.virtual_ip || "分配中…"}</strong> ·
            已连接节点 {status.peers_connected}
          </span>
          <button
            className="text-button"
            onClick={() => {
              void navigator.clipboard
                ?.writeText(status.room_code)
                .then(() => onNotice("房间码已复制"))
                .catch(() => onNotice("复制失败，请手动选择房间码"));
            }}
          >
            复制房间码
          </button>
        </div>
      )}
      {log.length > 0 && (
        <div className="mp-log">
          {log.slice(-12).map((line, index) => (
            <div key={index}>{line}</div>
          ))}
        </div>
      )}
    </section>
  );
}

function Instances({
  instances,
  activeId,
  setActiveId,
  manifest,
  onRefresh,
  onNotice,
  onCreate,
}: {
  instances: Instance[];
  activeId: string;
  setActiveId: (id: string) => void;
  manifest: VersionManifest | null;
  onRefresh: () => Promise<void>;
  onNotice: (message: string) => void;
  onCreate: () => void;
}) {
  const [managerFor, setManagerFor] = useState<Instance | null>(null);
  const [logFor, setLogFor] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const remove = async (instance: Instance) => {
    if (!window.confirm(`删除实例「${instance.name}」及其目录？此操作不可撤销。`)) return;
    try {
      await api.deleteInstance(instance.id);
      await onRefresh();
      onNotice("实例已删除");
    } catch (error) {
      onNotice(String(error));
    }
  };

  const open = async (instance: Instance) => {
    try {
      await api.openInstance(instance.id);
    } catch (error) {
      onNotice(String(error));
    }
  };

  const launch = async (instance: Instance) => {
    setBusy(instance.id);
    try {
      const status = await api.launchStatus(instance.id);
      if (status === "running" || status === "preparing") {
        await api.killInstance(instance.id);
        onNotice("已发送停止指令");
        return;
      }
      setLogFor(instance.id);
      await api.launchInstance(instance.id);
      onNotice("正在启动游戏，日志面板已打开");
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="page-stack">
      <MultiplayerCard onNotice={onNotice} />
      <section className="section-heading">
        <div>
          <span className="eyebrow">ISOLATED INSTANCES</span>
          <h3>{instances.length ? `${instances.length} 个游戏实例` : "还没有游戏实例"}</h3>
          <p className="section-subtitle">
            每个实例拥有独立的 mods、资源包、光影、世界、日志与 game 目录；共享目录只复用只读的 assets/libraries。
          </p>
        </div>
        <button className="primary-button" onClick={onCreate}>
          <Plus size={17} /> 新建实例
        </button>
      </section>

      {instances.length === 0 ? (
        <div className="empty-state">
          <div className="empty-icon">
            <Boxes size={27} />
          </div>
          <h3>创建你的第一个实例</h3>
          <p>{manifest ? `当前最新正式版 ${manifest.latest_release}` : "版本列表正在同步"}，为不同版本与加载器建立独立环境。</p>
          <button className="primary-button" onClick={onCreate}>
            <Plus size={17} /> 新建实例
          </button>
        </div>
      ) : (
        <div className="instance-grid">
          {instances.map((instance) => (
            <article className={`instance-card ${activeId === instance.id ? "selected" : ""}`} key={instance.id}>
              <div className="instance-cover">
                <span>{instance.minecraft_version}</span>
                <div className="cover-glyph">✦</div>
                <small>{loaderLabel(instance.loader)}</small>
              </div>
              <div className="instance-body">
                <div className="row-between">
                  <div>
                    <h4>{instance.name}</h4>
                    <p>
                      {loaderLabel(instance.loader)}
                      {instance.loader_version ? ` · ${instance.loader_version}` : ""}
                    </p>
                  </div>
                  {activeId === instance.id && <span className="pill mini">当前</span>}
                </div>
                <div className="instance-meta">
                  <span>
                    <Gamepad2 size={14} /> {instance.minecraft_version}
                  </span>
                  <span>
                    <ShieldCheck size={14} /> {instance.memory_mb} MB
                  </span>
                </div>
                <p className="instance-dir" title={instance.directory}>
                  {instance.directory}
                </p>
                <div className="instance-actions">
                  <button className="launch-button" onClick={() => void launch(instance)} disabled={busy === instance.id}>
                    {busy === instance.id ? <LoaderCircle className="spin" size={15} /> : <Play size={15} />}
                    启动游戏
                  </button>
                  <button className="icon-button" title="实例管理" onClick={() => setManagerFor(instance)}>
                    <Settings2 size={16} />
                  </button>
                  <button className="icon-button" title="打开目录" onClick={() => void open(instance)}>
                    <FolderOpen size={16} />
                  </button>
                  <button className="danger-button" title="删除实例" onClick={() => void remove(instance)}>
                    <Trash2 size={16} />
                  </button>
                </div>
              </div>
            </article>
          ))}
        </div>
      )}

      {managerFor && (
        <InstanceManager
          instance={managerFor}
          onClose={() => setManagerFor(null)}
          onRefresh={onRefresh}
          onNotice={onNotice}
          onLaunch={() => {
            setManagerFor(null);
            void launch(managerFor);
          }}
        />
      )}
      {logFor && <LogPanel instanceId={logFor} onClose={() => setLogFor(null)} />}
    </div>
  );
}

type ManagerTab =
  | "settings"
  | "mod"
  | "shader"
  | "resourcepack"
  | "save"
  | "screenshot"
  | "schematic"
  | "server"
  | "config";

const managerTabs: { id: ManagerTab; label: string; icon: Icon }[] = [
  { id: "settings", label: "设置", icon: Settings2 },
  { id: "mod", label: "模组", icon: Boxes },
  { id: "shader", label: "光影包", icon: Sparkles },
  { id: "resourcepack", label: "资源包", icon: Layers3 },
  { id: "save", label: "存档", icon: Globe2 },
  { id: "screenshot", label: "截图", icon: ImageIcon },
  { id: "schematic", label: "原理图", icon: MapIcon },
  { id: "server", label: "服务器", icon: ServerIcon },
  { id: "config", label: "配置", icon: FileText },
];

function summaryCount(summary: ContentSummary | null, tab: ManagerTab): number | null {
  if (!summary) return null;
  switch (tab) {
    case "mod":
      return summary.mod_count;
    case "shader":
      return summary.shader_count;
    case "resourcepack":
      return summary.resourcepack_count;
    case "save":
      return summary.save_count;
    case "screenshot":
      return summary.screenshot_count;
    case "schematic":
      return summary.schematic_count;
    case "server":
      return summary.server_count;
    default:
      return null;
  }
}

function formatUnix(seconds: number | null | undefined): string {
  if (!seconds) return "-";
  return new Date(seconds * 1000).toLocaleString();
}

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

function InstanceManager({
  instance,
  onClose,
  onRefresh,
  onNotice,
  onLaunch,
}: {
  instance: Instance;
  onClose: () => void;
  onRefresh: () => Promise<void>;
  onNotice: (message: string) => void;
  onLaunch: () => void;
}) {
  const [tab, setTab] = useState<ManagerTab>("settings");
  const [summary, setSummary] = useState<ContentSummary | null>(null);
  const [java, setJava] = useState(instance.java_path ?? "");
  const [memory, setMemory] = useState(instance.memory_mb);
  const [loaderVersion, setLoaderVersion] = useState(instance.loader_version ?? "");
  const [installLog, setInstallLog] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const managerProgress = useLoaderProgress(busy === "loader");

  const reloadSummary = async () => {
    try {
      setSummary(await api.contentSummary(instance.id));
    } catch {
      setSummary(null);
    }
  };

  useEffect(() => {
    void reloadSummary();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instance.id]);

  const guard = async (key: string, action: () => Promise<void>) => {
    setBusy(key);
    try {
      await action();
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(null);
    }
  };

  const save = () =>
    guard("save", async () => {
      await api.updateInstance({ id: instance.id, javaPath: java.trim(), memoryMb: memory });
      await onRefresh();
      onNotice("实例设置已保存");
    });

  const pickJava = async () => {
    const path = await api.pickJavaFile();
    if (path) setJava(path);
  };

  const installLoader = () =>
    guard("loader", async () => {
      if (!loaderVersion.trim()) {
        onNotice("请填写加载器版本");
        return;
      }
      setInstallLog("");
      const result = await api.installLoader(instance.id, instance.loader, loaderVersion.trim());
      setInstallLog(result.log);
      await onRefresh();
      onNotice(`加载器安装完成：${result.version_id}`);
    });

  const installCompanion = () =>
    guard("companion", async () => {
      const message = await api.aiInstallCompanion(instance.id);
      onNotice(message);
    });

  const importPack = () =>
    guard("pack", async () => {
      const path = await api.pickPackFile();
      if (!path) return;
      const result = await api.importPack(instance.id, path);
      await onRefresh();
      onNotice(
        `已导入 ${result.name}：${result.files} 个文件、${result.overrides} 个覆盖文件` +
          (result.loader ? `；加载器 ${result.loader} ${result.loader_version ?? ""}` : ""),
      );
    });

  const exportPack = () =>
    guard("export", async () => {
      const path = await api.pickExportFile(instance.name);
      if (!path) return;
      const result = await api.exportPack(instance.id, path);
      onNotice(`整合包已导出：${result}`);
    });

  const tabs = managerTabs.map((item) => ({
    ...item,
    count: summaryCount(summary, item.id),
  }));

  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="modal wide manager-modal">
        <div className="modal-top">
          <span>{instance.name} · 实例管理</span>
          <button className="icon-button" onClick={onClose}>
            <X size={17} />
          </button>
        </div>
        <div className="manager-tabs">
          {tabs.map((item) => (
            <button
              key={item.id}
              className={`manager-tab ${tab === item.id ? "active" : ""}`}
              onClick={() => setTab(item.id)}
            >
              <item.icon size={14} />
              {item.label}
              {item.count !== null && <span className="tab-count">{item.count}</span>}
            </button>
          ))}
        </div>

        {tab === "settings" && (
          <div className="form-stack manager-scroll">
            <div className="two-columns">
              <label className="field-label">
                Java 路径
                <div className="inline-field">
                  <input
                    value={java}
                    onChange={(event) => setJava(event.target.value)}
                    placeholder="留空自动检测 Java 17/21"
                  />
                  <button className="ghost-button" onClick={() => void pickJava()}>
                    浏览
                  </button>
                </div>
              </label>
              <label className="field-label">
                内存 (MB)
                <input
                  type="number"
                  min={512}
                  max={32768}
                  value={memory}
                  onChange={(event) => setMemory(Number(event.target.value))}
                />
              </label>
            </div>
            <p className="security-note">
              启动版本：{instance.launch_version_id ?? instance.minecraft_version} · 目录：{instance.directory}
            </p>

            <div className="settings-actions">
              <button className="ghost-button" onClick={onLaunch}>
                <Play size={15} /> 启动
              </button>
              <button className="primary-button" onClick={() => void save()} disabled={busy === "save"}>
                {busy === "save" ? <LoaderCircle className="spin" size={15} /> : <Check size={15} />} 保存设置
              </button>
            </div>

            {instance.loader !== "vanilla" && (
              <section className="manager-section">
                <h4>加载器安装</h4>
                <div className="inline-field">
                  <input
                    value={loaderVersion}
                    onChange={(event) => setLoaderVersion(event.target.value)}
                    placeholder={`${loaderLabel(instance.loader)} 版本，例如 0.16.0 / 47.4.0`}
                  />
                  <button className="ghost-button" onClick={() => void installLoader()} disabled={busy === "loader"}>
                    {busy === "loader" ? <LoaderCircle className="spin" size={15} /> : <Download size={15} />} 自动安装
                  </button>
                </div>
                {busy === "loader" && managerProgress && <LoaderProgressLine progress={managerProgress} />}
                <div className="inline-field">
                  <button
                    className="ghost-button"
                    onClick={() => void installCompanion()}
                    disabled={busy === "companion"}
                  >
                    {busy === "companion" ? <LoaderCircle className="spin" size={15} /> : <Sparkles size={15} />} 安装 AI
                    伙伴（Fabric 1.21.x）
                  </button>
                </div>
                <p className="security-note">
                  Fabric / Legacy Fabric / Forge / NeoForge / Cleanroom 使用官方无头安装器；OptiFine、LabyMod、LiteLoader
                  请手动导入。
                </p>
                {installLog && <pre className="installer-log">{installLog}</pre>}
              </section>
            )}

            <section className="manager-section">
              <h4>整合包与世界</h4>
              <div className="manager-actions">
                <button className="ghost-button" onClick={() => void importPack()} disabled={busy === "pack"}>
                  {busy === "pack" ? <LoaderCircle className="spin" size={15} /> : <PackageOpen size={15} />} 导入整合包
                </button>
                <button className="ghost-button" onClick={() => void exportPack()} disabled={busy === "export"}>
                  {busy === "export" ? <LoaderCircle className="spin" size={15} /> : <Download size={15} />} 导出整合包
                </button>
              </div>
              <p className="security-note">
                支持 .mrpack 与 CurseForge zip 导入；导出为 .mrpack，本地 mod 放入 overrides/mods。世界的导入/备份在「存档」页。
              </p>
            </section>
          </div>
        )}

        {(tab === "mod" || tab === "shader" || tab === "resourcepack" || tab === "schematic") && (
          <ContentPanel instanceId={instance.id} kind={tab} onNotice={onNotice} onChanged={() => void reloadSummary()} />
        )}
        {tab === "save" && <SavesPanel instanceId={instance.id} onNotice={onNotice} onChanged={() => void reloadSummary()} />}
        {tab === "screenshot" && (
          <ScreenshotsPanel instanceId={instance.id} onNotice={onNotice} onChanged={() => void reloadSummary()} />
        )}
        {tab === "server" && <ServersPanel instanceId={instance.id} onNotice={onNotice} onChanged={() => void reloadSummary()} />}
        {tab === "config" && <ConfigPanel instanceId={instance.id} onNotice={onNotice} />}
      </div>
    </div>
  );
}

function ContentPanel({
  instanceId,
  kind,
  onNotice,
  onChanged,
}: {
  instanceId: string;
  kind: "mod" | "shader" | "resourcepack" | "schematic";
  onNotice: (message: string) => void;
  onChanged: () => void;
}) {
  const [items, setItems] = useState<ContentItem[] | null>(null);
  const [metas, setMetas] = useState<Record<string, ModMeta>>({});
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [sort, setSort] = useState<"name" | "enabled">("name");
  const [busy, setBusy] = useState<string | null>(null);

  const reload = async () => {
    setSelected(new Set());
    try {
      setItems(await api.contentList(instanceId, kind));
    } catch (error) {
      setItems([]);
      onNotice(String(error));
    }
    if (kind === "mod") {
      // 元数据解析失败不影响列表展示，退化为文件名。
      api
        .modMetadata(instanceId)
        .then(setMetas)
        .catch(() => setMetas({}));
    }
  };

  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId, kind]);

  const remove = (item: ContentItem) =>
    withBusy(setBusy, item.name, async () => {
      if (!window.confirm(`删除「${item.name}」？此操作不可撤销。`)) return;
      await api.contentDelete(instanceId, kind, item.name);
      await reload();
      onChanged();
      onNotice("已删除");
    });

  const toggle = (item: ContentItem) =>
    withBusy(setBusy, item.name, async () => {
      await api.modToggle(instanceId, item.name, !(item.enabled ?? true));
      await reload();
      onChanged();
    });

  const upload = () =>
    withBusy(setBusy, "upload", async () => {
      const source = await api.pickContentFile(kind);
      if (!source) return;
      const name = await api.contentUpload(instanceId, kind, source);
      await reload();
      onChanged();
      onNotice(`已导入 ${name}`);
    });

  // 拖拽导入：按扩展名过滤，逐个复制进对应目录。
  const [dragging, setDragging] = useState(false);
  const dropKinds: Record<string, string[]> = {
    mod: [".jar"],
    shader: [".zip"],
    resourcepack: [".zip"],
    schematic: [".litematic", ".schem", ".schematic", ".nbt"],
  };
  const dropHint = Object.entries(dropKinds)
    .map(([id, exts]) => (id === kind ? exts.join(" / ") : ""))
    .filter(Boolean)[0];

  const handleDrop = async (paths: string[]) => {
    if (!paths.length) return;
    const exts = dropKinds[kind] ?? [];
    const matched = paths.filter((path) => exts.some((ext) => path.toLowerCase().endsWith(ext)));
    const rejected = paths.length - matched.length;
    if (!matched.length) {
      onNotice(`拖入的文件类型不支持，需要 ${exts.join(" / ")}`);
      return;
    }
    setBusy("drop");
    let imported = 0;
    const errors: string[] = [];
    for (const path of matched) {
      try {
        await api.contentUpload(instanceId, kind, path);
        imported += 1;
      } catch (error) {
        errors.push(String(error));
      }
    }
    await reload();
    onChanged();
    setBusy(null);
    const parts: string[] = [];
    if (imported) parts.push(`已导入 ${imported} 个文件`);
    if (rejected) parts.push(`${rejected} 个类型不符已忽略`);
    if (errors.length) parts.push(`失败 ${errors.length}：${errors[0]}`);
    onNotice(parts.join("；"));
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          setDragging(true);
        } else if (payload.type === "leave") {
          setDragging(false);
        } else if (payload.type === "drop") {
          setDragging(false);
          void handleDrop(payload.paths);
        }
      })
      .then((fn) => {
        if (!active) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId, kind]);

  const labels: Record<string, string> = {
    mod: "模组",
    shader: "光影包",
    resourcepack: "资源包",
    schematic: "原理图",
  };
  const folder = { mod: "mods", shader: "shaderpacks", resourcepack: "resourcepacks", schematic: "schematics" }[kind];

  const keyword = filter.trim().toLowerCase();
  const shown = (items ?? [])
    .filter((item) => {
      if (!keyword) return true;
      const meta = metas[item.name];
      return [item.name, meta?.title, meta?.mod_id, meta?.version, item.level_name]
        .filter(Boolean)
        .some((value) => String(value).toLowerCase().includes(keyword));
    })
    .slice()
    .sort((a, b) => {
      if (kind === "mod" && sort === "enabled") {
        // 启用在前；同状态内按显示名排序。
        const rank = (item: ContentItem) => (item.enabled === false ? 1 : 0);
        const diff = rank(a) - rank(b);
        if (diff !== 0) return diff;
      }
      const nameA = (metas[a.name]?.title || a.name).toLowerCase();
      const nameB = (metas[b.name]?.title || b.name).toLowerCase();
      return nameA.localeCompare(nameB);
    });

  const selectable = shown.filter((item) => item.enabled !== null && item.enabled !== undefined);
  const selectedItems = selectable.filter((item) => selected.has(item.name));
  const allSelected = selectable.length > 0 && selectedItems.length === selectable.length;

  const toggleSelect = (name: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const toggleSelectAll = () => {
    setSelected(allSelected ? new Set() : new Set(selectable.map((item) => item.name)));
  };

  const bulkToggle = (enable: boolean) =>
    withBusy(setBusy, "bulk", async () => {
      if (!selectedItems.length) return;
      const changed = await api.modToggleMany(instanceId, selectedItems.map((item) => item.name), enable);
      await reload();
      onChanged();
      onNotice(`${enable ? "启用" : "禁用"}了 ${changed} 个模组`);
    });

  return (
    <div className={`content-panel manager-scroll ${dragging ? "drag-target" : ""}`}>
      {dragging && (
        <div className="drop-overlay">
          <Download size={22} />
          <span>松开导入到 {labels[kind]}（{dropHint}）</span>
        </div>
      )}
      <div className="content-toolbar">
        <span className="content-hint">
          {labels[kind]} · {keyword ? `${shown.length}/${items?.length ?? 0}` : (items?.length ?? "…")} 项 · 位于 {folder}/
        </span>
        <div className="manager-actions">
          <input
            className="content-search"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            placeholder={`搜索${labels[kind]}…`}
          />
          {kind === "mod" && (
            <select
              className="content-sort"
              value={sort}
              onChange={(event) => setSort(event.target.value as "name" | "enabled")}
              title="排序方式"
            >
              <option value="name">按名称</option>
              <option value="enabled">启用优先</option>
            </select>
          )}
          {(kind === "shader" || kind === "resourcepack" || kind === "schematic") && (
            <button className="ghost-button" onClick={() => void upload()} disabled={busy === "upload"}>
              {busy === "upload" ? <LoaderCircle className="spin" size={15} /> : <Plus size={15} />} 导入文件
            </button>
          )}
          <button className="ghost-button" onClick={() => void reload()}>
            <RefreshCw size={15} /> 刷新
          </button>
          <button className="ghost-button" onClick={() => void api.contentOpenFolder(instanceId, kind)}>
            <FolderOpen size={15} /> 打开目录
          </button>
        </div>
      </div>

      {kind === "mod" && selectable.length > 0 && (
        <div className="bulk-bar">
          <label className="bulk-select-all">
            <input type="checkbox" checked={allSelected} onChange={toggleSelectAll} />
            已选 {selectedItems.length}/{selectable.length}
          </label>
          <div className="manager-actions">
            <button
              className="ghost-button small"
              disabled={!selectedItems.length || busy === "bulk"}
              onClick={() => void bulkToggle(true)}
            >
              <Eye size={14} /> 批量启用
            </button>
            <button
              className="ghost-button small"
              disabled={!selectedItems.length || busy === "bulk"}
              onClick={() => void bulkToggle(false)}
            >
              <EyeOff size={14} /> 批量禁用
            </button>
            {busy === "bulk" && <LoaderCircle className="spin" size={15} />}
          </div>
        </div>
      )}

      {items === null ? (
        <div className="content-empty">正在读取…</div>
      ) : shown.length === 0 ? (
        <div className="content-empty">{keyword ? "没有匹配的条目" : "暂无内容"}</div>
      ) : (
        <div className="content-list">
          {shown.map((item) => {
            const disabledMod = kind === "mod" && item.enabled === false;
            const meta = metas[item.name];
            const title = meta?.title || item.name;
            const details = [meta?.version, meta?.loader, item.is_dir ? "文件夹" : formatSize(item.size)]
              .filter(Boolean)
              .join(" · ");
            return (
              <div className={`content-row ${disabledMod ? "disabled" : ""}`} key={item.name}>
                {kind === "mod" && item.enabled !== null && item.enabled !== undefined && (
                  <input
                    type="checkbox"
                    className="row-check"
                    checked={selected.has(item.name)}
                    onChange={() => toggleSelect(item.name)}
                  />
                )}
                <div className="content-main">
                  <span className="content-name" title={meta?.description || item.name}>
                    {title}
                    {meta?.loader && <em className={`mod-loader ${meta.loader}`}>{meta.loader}</em>}
                  </span>
                  <span className="content-meta">
                    {title !== item.name && (
                      <>
                        <span className="content-file" title={item.name}>
                          {item.name}
                        </span>
                        {" · "}
                      </>
                    )}
                    {details} · {formatUnix(item.modified)}
                  </span>
                </div>
                <div className="content-actions">
                  {kind === "mod" && item.enabled !== null && item.enabled !== undefined && (
                    <button
                      className="ghost-button small"
                      onClick={() => void toggle(item)}
                      disabled={busy === item.name}
                    >
                      {item.enabled ? <EyeOff size={14} /> : <Eye size={14} />}
                      {item.enabled ? "禁用" : "启用"}
                    </button>
                  )}
                  <button
                    className="danger-button small"
                    onClick={() => void remove(item)}
                    disabled={busy === item.name}
                  >
                    <Trash2 size={14} /> 删除
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function SavesPanel({
  instanceId,
  onNotice,
  onChanged,
}: {
  instanceId: string;
  onNotice: (message: string) => void;
  onChanged: () => void;
}) {
  const [items, setItems] = useState<ContentItem[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const reload = async () => {
    try {
      setItems(await api.contentList(instanceId, "save"));
    } catch (error) {
      setItems([]);
      onNotice(String(error));
    }
  };

  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId]);

  const remove = (item: ContentItem) =>
    withBusy(setBusy, item.name, async () => {
      if (!window.confirm(`删除世界「${item.name}」及其所有数据？此操作不可撤销。`)) return;
      await api.contentDelete(instanceId, "save", item.name);
      await reload();
      onChanged();
      onNotice("世界已删除");
    });

  // 导入与“还原备份”是同一操作：把世界 zip 解到 saves/ 下。
  const importWorld = () =>
    withBusy(setBusy, "import", async () => {
      const path = await api.pickWorldFile();
      if (!path) return;
      const target = await api.importWorld(instanceId, path);
      await reload();
      onChanged();
      onNotice(`世界已导入：${target}`);
    });

  const backup = (item: ContentItem) =>
    withBusy(setBusy, `backup:${item.name}`, async () => {
      const path = await api.pickSaveExport(item.level_name || item.name);
      if (!path) return;
      const detail = await api.exportWorld(instanceId, item.name, path);
      onNotice(`已备份到 ${path}（${detail}）`);
    });

  const [dragging, setDragging] = useState(false);

  // 拖入世界 zip 视为导入/还原备份。
  const handleDrop = async (paths: string[]) => {
    if (!paths.length) return;
    const zips = paths.filter((path) => path.toLowerCase().endsWith(".zip"));
    if (!zips.length) {
      onNotice("请拖入 .zip 世界压缩包");
      return;
    }
    setBusy("import");
    let imported = 0;
    const errors: string[] = [];
    for (const path of zips) {
      try {
        await api.importWorld(instanceId, path);
        imported += 1;
      } catch (error) {
        errors.push(String(error));
      }
    }
    await reload();
    onChanged();
    setBusy(null);
    const parts: string[] = [];
    if (imported) parts.push(`已导入 ${imported} 个世界`);
    if (errors.length) parts.push(`失败 ${errors.length}：${errors[0]}`);
    onNotice(parts.join("；"));
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          setDragging(true);
        } else if (payload.type === "leave") {
          setDragging(false);
        } else if (payload.type === "drop") {
          setDragging(false);
          void handleDrop(payload.paths);
        }
      })
      .then((fn) => {
        if (!active) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId]);

  return (
    <div className={`content-panel manager-scroll ${dragging ? "drag-target" : ""}`}>
      {dragging && (
        <div className="drop-overlay">
          <Download size={22} />
          <span>松开导入世界（.zip）</span>
        </div>
      )}
      <div className="content-toolbar">
        <span className="content-hint">存档 · {items?.length ?? "…"} 个世界 · 位于 saves/</span>
        <div className="manager-actions">
          <button className="ghost-button" onClick={() => void importWorld()} disabled={busy === "import"}>
            {busy === "import" ? <LoaderCircle className="spin" size={15} /> : <Download size={15} />} 导入 / 还原备份
          </button>
          <button className="ghost-button" onClick={() => void reload()}>
            <RefreshCw size={15} /> 刷新
          </button>
          <button className="ghost-button" onClick={() => void api.contentOpenFolder(instanceId, "save")}>
            <FolderOpen size={15} /> 打开目录
          </button>
        </div>
      </div>

      {items === null ? (
        <div className="content-empty">正在读取…</div>
      ) : items.length === 0 ? (
        <div className="content-empty">暂无存档，可点「导入 / 还原备份」放入世界 zip</div>
      ) : (
        <div className="content-list">
          {items.map((item) => (
            <div className="content-row" key={item.name}>
              <div className="content-main">
                <span className="content-name" title={item.name}>
                  {item.level_name || item.name}
                </span>
                <span className="content-meta">
                  {item.level_name && item.level_name !== item.name ? `目录 ${item.name} · ` : ""}
                  {formatSize(item.size)} · 最后游玩 {formatUnix(item.last_played ?? item.modified)}
                </span>
              </div>
              <div className="content-actions">
                <button
                  className="ghost-button small"
                  onClick={() => void backup(item)}
                  disabled={busy === `backup:${item.name}`}
                >
                  {busy === `backup:${item.name}` ? (
                    <LoaderCircle className="spin" size={14} />
                  ) : (
                    <Download size={14} />
                  )}{" "}
                  备份
                </button>
                <button className="ghost-button small" onClick={() => void api.saveOpen(instanceId, item.name)}>
                  <FolderOpen size={14} /> 打开
                </button>
                <button className="danger-button small" onClick={() => void remove(item)} disabled={busy === item.name}>
                  <Trash2 size={14} /> 删除
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function ScreenshotsPanel({
  instanceId,
  onNotice,
  onChanged,
}: {
  instanceId: string;
  onNotice: (message: string) => void;
  onChanged: () => void;
}) {
  const [items, setItems] = useState<ContentItem[] | null>(null);
  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const [preview, setPreview] = useState<{ name: string; url: string } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const cancelRef = useRef(0);

  const reload = async () => {
    cancelRef.current += 1;
    const generation = cancelRef.current;
    try {
      const list = await api.contentList(instanceId, "screenshot");
      setItems(list);
      setThumbs({});
      // 分批加载缩略图，避免一次性 IPC 过重。
      const targets = list.slice(0, 80);
      const batch = 6;
      for (let index = 0; index < targets.length; index += batch) {
        if (cancelRef.current !== generation) return;
        const slice = targets.slice(index, index + batch);
        const loaded = await Promise.all(
          slice.map(async (item) => {
            try {
              return [item.name, await api.imagePreview(instanceId, "screenshot", item.name)] as const;
            } catch {
              return [item.name, ""] as const;
            }
          }),
        );
        if (cancelRef.current !== generation) return;
        setThumbs((prev) => {
          const next = { ...prev };
          for (const [name, url] of loaded) next[name] = url;
          return next;
        });
      }
    } catch (error) {
      setItems([]);
      onNotice(String(error));
    }
  };

  useEffect(() => {
    void reload();
    return () => {
      cancelRef.current += 1;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId]);

  const openPreview = async (item: ContentItem) => {
    try {
      const url = thumbs[item.name] || (await api.imagePreview(instanceId, "screenshot", item.name));
      if (url) setPreview({ name: item.name, url });
    } catch (error) {
      onNotice(String(error));
    }
  };

  const stepPreview = async (delta: number) => {
    if (!preview || !items) return;
    const index = items.findIndex((item) => item.name === preview.name);
    const next = items[index + delta];
    if (index >= 0 && next) await openPreview(next);
  };

  useEffect(() => {
    if (!preview) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setPreview(null);
      if (event.key === "ArrowRight") void stepPreview(1);
      if (event.key === "ArrowLeft") void stepPreview(-1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [preview, items, thumbs]);

  const remove = (item: ContentItem) =>
    withBusy(setBusy, item.name, async () => {
      if (!window.confirm(`删除截图「${item.name}」？此操作不可撤销。`)) return;
      await api.contentDelete(instanceId, "screenshot", item.name);
      await reload();
      onChanged();
    });

  return (
    <div className="content-panel manager-scroll">
      <div className="content-toolbar">
        <span className="content-hint">截图 · {items?.length ?? "…"} 张 · 位于 screenshots/</span>
        <div className="manager-actions">
          <button className="ghost-button" onClick={() => void reload()}>
            <RefreshCw size={15} /> 刷新
          </button>
          <button className="ghost-button" onClick={() => void api.contentOpenFolder(instanceId, "screenshot")}>
            <FolderOpen size={15} /> 打开目录
          </button>
        </div>
      </div>

      {items === null ? (
        <div className="content-empty">正在读取…</div>
      ) : items.length === 0 ? (
        <div className="content-empty">暂无截图（游戏中按 F2 截图）</div>
      ) : (
        <div className="screenshot-grid">
          {items.map((item) => (
            <figure className="screenshot-card" key={item.name} onClick={() => void openPreview(item)}>
              <div className="screenshot-thumb">
                {thumbs[item.name] ? (
                  <img src={thumbs[item.name]} alt={item.name} loading="lazy" />
                ) : (
                  <ImageIcon size={26} />
                )}
              </div>
              <figcaption>
                <span title={item.name}>{item.name}</span>
                <span>{formatUnix(item.modified)}</span>
              </figcaption>
              <button
                className="danger-button small screenshot-delete"
                onClick={(event) => {
                  event.stopPropagation();
                  void remove(item);
                }}
                disabled={busy === item.name}
              >
                <Trash2 size={13} />
              </button>
            </figure>
          ))}
        </div>
      )}

      {preview && (
        <div className="modal-backdrop inner" onMouseDown={() => setPreview(null)}>
          <div className="preview-box">
            <div className="preview-top">
              <span>{preview.name}</span>
              <div className="preview-actions">
                <small>← → 切换 · Esc 关闭</small>
                <button className="icon-button" onClick={() => void stepPreview(-1)} title="上一张">
                  <ChevronRight size={16} className="flip" />
                </button>
                <button className="icon-button" onClick={() => void stepPreview(1)} title="下一张">
                  <ChevronRight size={16} />
                </button>
                <button className="icon-button" onClick={() => setPreview(null)}>
                  <X size={16} />
                </button>
              </div>
            </div>
            <img src={preview.url} alt={preview.name} />
          </div>
        </div>
      )}
    </div>
  );
}

function ServersPanel({
  instanceId,
  onNotice,
  onChanged,
}: {
  instanceId: string;
  onNotice: (message: string) => void;
  onChanged: () => void;
}) {
  const [entries, setEntries] = useState<ServerEntry[] | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = async () => {
    try {
      setEntries(await api.serverList(instanceId));
    } catch (error) {
      setEntries([]);
      onNotice(String(error));
    }
  };

  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId]);

  const update = (index: number, patch: Partial<ServerEntry>) => {
    setEntries((prev) => {
      if (!prev) return prev;
      const next = [...prev];
      next[index] = { ...next[index], ...patch };
      return next;
    });
  };

  const add = () => {
    setEntries((prev) => [...(prev ?? []), { name: "新的服务器", ip: "", hidden: false }]);
  };

  const removeAt = (index: number) => {
    setEntries((prev) => (prev ? prev.filter((_, i) => i !== index) : prev));
  };

  const save = async () => {
    if (!entries) return;
    setBusy(true);
    try {
      const payload: ServerInput[] = entries.map((entry) => ({
        name: entry.name.trim(),
        ip: entry.ip.trim(),
        hidden: entry.hidden,
        icon_base64: entry.icon_data_url ?? "",
      }));
      await api.serverSet(instanceId, payload);
      await reload();
      onChanged();
      onNotice("服务器列表已保存（原文件已备份）");
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="content-panel manager-scroll">
      <div className="content-toolbar">
        <span className="content-hint">多人游戏服务器 · 来自 servers.dat（保存时自动备份）</span>
        <div className="manager-actions">
          <button className="ghost-button" onClick={() => void reload()}>
            <RefreshCw size={15} /> 重新读取
          </button>
          <button className="ghost-button" onClick={add}>
            <Plus size={15} /> 添加服务器
          </button>
          <button className="primary-button" onClick={() => void save()} disabled={busy || entries === null}>
            {busy ? <LoaderCircle className="spin" size={15} /> : <Check size={15} />} 保存
          </button>
        </div>
      </div>

      {entries === null ? (
        <div className="content-empty">正在读取…</div>
      ) : entries.length === 0 ? (
        <div className="content-empty">还没有保存的服务器</div>
      ) : (
        <div className="server-list">
          {entries.map((entry, index) => (
            <div className="server-row" key={index}>
              {entry.icon_data_url ? (
                <img className="server-icon" src={entry.icon_data_url} alt="" />
              ) : (
                <div className="server-icon placeholder">
                  <ServerIcon size={18} />
                </div>
              )}
              <input
                className="server-name"
                value={entry.name}
                onChange={(event) => update(index, { name: event.target.value })}
                placeholder="名称"
              />
              <input
                value={entry.ip}
                onChange={(event) => update(index, { ip: event.target.value })}
                placeholder="服务器地址，例如 mc.example.com:25565"
              />
              <label className="server-hidden" title="在服务器列表中隐藏">
                <input
                  type="checkbox"
                  checked={entry.hidden}
                  onChange={(event) => update(index, { hidden: event.target.checked })}
                />
                隐藏
              </label>
              <button className="danger-button small" onClick={() => removeAt(index)}>
                <Trash2 size={14} />
              </button>
            </div>
          ))}
        </div>
      )}
      <p className="security-note">修改仅影响本实例的 servers.dat；游戏内添加/删除的服务器需点击「重新读取」后可见。</p>
    </div>
  );
}

function ConfigPanel({ instanceId, onNotice }: { instanceId: string; onNotice: (message: string) => void }) {
  const [files, setFiles] = useState<ConfigEntry[] | null>(null);
  const [activePath, setActivePath] = useState<string>("");
  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [filter, setFilter] = useState("");

  const reload = async () => {
    try {
      const list = await api.configList(instanceId);
      setFiles(list);
      return list;
    } catch (error) {
      setFiles([]);
      onNotice(String(error));
      return [];
    }
  };

  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId]);

  const open = async (path: string) => {
    // 切换文件前拦下未保存的改动，避免静默丢失。
    if (dirty && path !== activePath) {
      const leave = window.confirm(`「${activePath}」有未保存的改动，放弃并打开「${path}」？`);
      if (!leave) return;
    }
    try {
      const file = await api.configRead(instanceId, path);
      setActivePath(file.path);
      setDraft(file.content);
      setDirty(false);
    } catch (error) {
      onNotice(String(error));
    }
  };

  const save = async () => {
    if (!activePath) return;
    setBusy(true);
    try {
      await api.configWrite(instanceId, activePath, draft);
      await reload();
      setDirty(false);
      onNotice(`已保存 ${activePath}（原文件已备份）`);
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const create = async () => {
    const name = window.prompt("新配置文件名（将创建在 config/ 下，例如 modid-example.toml）");
    if (!name) return;
    const cleaned = name.trim().replace(/\\/g, "/").replace(/^\/+/, "");
    if (!cleaned || cleaned.includes("..")) {
      onNotice("文件名无效");
      return;
    }
    const target = `config/${cleaned}`;
    try {
      await api.configWrite(instanceId, target, "");
      await reload();
      await open(target);
    } catch (error) {
      onNotice(String(error));
    }
  };

  const shown = (files ?? []).filter((file) => !filter || file.path.toLowerCase().includes(filter.toLowerCase()));

  return (
    <div className="config-panel">
      <div className="config-side manager-scroll">
        <div className="content-toolbar">
          <span className="content-hint">配置文件</span>
        </div>
        <div className="inline-field">
          <input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="搜索文件…" />
        </div>
        <div className="config-actions">
          <button className="ghost-button small" onClick={() => void reload()}>
            <RefreshCw size={14} /> 刷新
          </button>
          <button className="ghost-button small" onClick={() => void create()}>
            <Plus size={14} /> 新建
          </button>
        </div>
        {files === null ? (
          <div className="content-empty">正在读取…</div>
        ) : shown.length === 0 ? (
          <div className="content-empty">没有可编辑的配置文件</div>
        ) : (
          <div className="config-file-list">
            {shown.map((file) => (
              <button
                key={file.path}
                className={`config-file ${activePath === file.path ? "active" : ""}`}
                title={`${file.path} · ${formatSize(file.size)}`}
                onClick={() => void open(file.path)}
              >
                <FileText size={13} />
                <span>{file.path}</span>
              </button>
            ))}
          </div>
        )}
      </div>
      <div className="config-editor">
        {activePath ? (
          <>
            <div className="content-toolbar">
              <span className="content-hint" title={activePath}>
                {activePath}
                {dirty ? " · 未保存" : ""}
              </span>
              <div className="manager-actions">
                <button className="primary-button" onClick={() => void save()} disabled={busy || !dirty}>
                  {busy ? <LoaderCircle className="spin" size={15} /> : <Check size={15} />} 保存
                </button>
              </div>
            </div>
            <textarea
              className="config-textarea"
              value={draft}
              spellCheck={false}
              onChange={(event) => {
                setDraft(event.target.value);
                setDirty(true);
              }}
              onKeyDown={(event) => {
                if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
                  event.preventDefault();
                  if (dirty && !busy) void save();
                }
              }}
            />
            <p className="security-note">保存时自动在同目录生成 .aurora-bak 备份；仅支持文本文件（≤1 MB）。</p>
          </>
        ) : (
          <div className="content-empty">从左侧选择一个配置文件进行编辑（options.txt 与 config/ 目录）</div>
        )}
      </div>
    </div>
  );
}

async function withBusy(
  setBusy: (value: string | null) => void,
  key: string,
  action: () => Promise<void>,
): Promise<void> {
  setBusy(key);
  try {
    await action();
  } catch (error) {
    console.error(error);
    throw error;
  } finally {
    setBusy(null);
  }
}

function LogPanel({ instanceId, onClose }: { instanceId: string; onClose: () => void }) {
  const [lines, setLines] = useState<string[]>([]);
  const [status, setStatus] = useState("idle");
  const endRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    void api.instanceLog(instanceId).then(setLines).catch(() => undefined);
    void api.launchStatus(instanceId).then(setStatus).catch(() => undefined);
    const unlistenLog = listen<{ instance_id: string; line: string }>("game-log", (event) => {
      if (event.payload.instance_id !== instanceId) return;
      setLines((current) => [...current, event.payload.line].slice(-1000));
    });
    const unlistenExit = listen<{ instance_id: string; code?: number | null }>("game-exit", (event) => {
      if (event.payload.instance_id !== instanceId) return;
      setStatus("exited");
      setLines((current) => [...current, `[Aurora] 进程退出，代码 ${event.payload.code ?? "-"}`]);
    });
    return () => {
      void unlistenLog.then((fn) => fn());
      void unlistenExit.then((fn) => fn());
    };
  }, [instanceId]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [lines]);

  return (
    <section className="log-panel">
      <header className="log-head">
        <div>
          <span className="eyebrow">GAME LOG · {status}</span>
          <strong>游戏控制台</strong>
        </div>
        <div className="log-actions">
          <button
            className="ghost-button"
            onClick={() => {
              void api.instanceLog(instanceId).then(setLines).catch(() => undefined);
            }}
          >
            <RefreshCw size={15} /> 刷新
          </button>
          <button
            className="ghost-button"
            onClick={() => {
              void api.killInstance(instanceId).catch(() => undefined);
            }}
          >
            停止
          </button>
          <button className="icon-button" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
      </header>
      <div className="log-lines">
        {lines.length === 0 && <p className="security-note">暂无日志，启动游戏后会在这里显示。</p>}
        {lines.map((line, index) => (
          <div className="log-line" key={`${index}-${line.slice(0, 12)}`}>{line}</div>
        ))}
        <div ref={endRef} />
      </div>
    </section>
  );
}

function Loaders({
  focus,
  setFocus,
  instances,
  activeId,
  manifest,
  onNotice,
  onOpenExternal,
  onCreate,
  onRefresh,
}: {
  focus: LoaderId;
  setFocus: (id: LoaderId) => void;
  instances: Instance[];
  activeId: string;
  manifest: VersionManifest | null;
  onNotice: (message: string) => void;
  onOpenExternal: (url: string) => void;
  onCreate: () => void;
  onRefresh: () => Promise<void>;
}) {
  const active = instances.find((item) => item.id === activeId);
  const [gameVersion, setGameVersion] = useState("");
  const [catalog, setCatalog] = useState<LoaderCatalog | null>(null);
  const [loading, setLoading] = useState(false);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  const [targetId, setTargetId] = useState(activeId || instances[0]?.id || "");
  const effectiveVersion = gameVersion || active?.minecraft_version || manifest?.latest_release || "1.21.1";
  const target = instances.find((item) => item.id === targetId) ?? instances[0];
  const versions = useMemo(
    () => (manifest?.versions ?? []).filter((item) => item.kind === "release").slice(0, 120),
    [manifest],
  );

  useEffect(() => {
    let cancelled = false;
    setCatalog(null);
    if (focus === "vanilla") return;
    setLoading(true);
    api
      .loaderVersions(focus, effectiveVersion)
      .then((value) => {
        if (!cancelled) setCatalog(value);
      })
      .catch((error) => {
        if (!cancelled) onNotice(String(error));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [focus, effectiveVersion, onNotice]);

  useEffect(() => {
    if (!targetId && instances[0]) setTargetId(instances[0].id);
  }, [instances, targetId]);

  const installerUrl = (version: string): string | null => {
    if (!catalog) return null;
    if (catalog.installer_url) return catalog.installer_url;
    if (catalog.installer_template) {
      return catalog.installer_template
        .replaceAll("{mc}", catalog.game_version)
        .replaceAll("{version}", version);
    }
    return null;
  };

  const downloadInstaller = async (version: string) => {
    if (!target) {
      onNotice("请先创建一个实例作为下载目标");
      return;
    }
    const url = installerUrl(version);
    if (!url) {
      onNotice("该加载器没有可自动下载的安装器，请打开官方页面");
      return;
    }
    setDownloading(version);
    try {
      const path = await api.downloadLoaderInstaller(target.id, url);
      onNotice(`安装器已下载到 ${path}`);
    } catch (error) {
      onNotice(String(error));
    } finally {
      setDownloading(null);
    }
  };

  const autoLoader = ["fabric", "legacyfabric", "forge", "neoforge", "cleanroom"].includes(focus);

  const loadersProgress = useLoaderProgress(installing !== null);
  const installVersion = async (version: string) => {
    if (!target) {
      onNotice("请先创建一个实例作为安装目标");
      return;
    }
    setInstalling(version);
    try {
      const result = await api.installLoader(target.id, focus, version);
      await onRefresh();
      onNotice(`加载器安装完成：${result.version_id}`);
    } catch (error) {
      onNotice(String(error));
    } finally {
      setInstalling(null);
    }
  };

  const entry = loaderEntries.find((item) => item.id === focus);

  return (
    <div className="page-stack">
      <section className="section-heading">
        <div>
          <span className="eyebrow">LOADER INSTALLERS</span>
          <h3>{entry?.label ?? focus}</h3>
          <p className="section-subtitle">官方版本列表与安装器下载；首版不自动执行安装器。</p>
        </div>
        <button className="ghost-button" onClick={onCreate}>
          <Plus size={16} /> 新建实例
        </button>
      </section>

      <div className="loader-tabs">
        {loaderEntries.map((item) => (
          <button
            key={item.id}
            className={focus === item.id ? "selected" : ""}
            onClick={() => setFocus(item.id)}
          >
            <item.icon size={16} /> {item.label}
          </button>
        ))}
      </div>

      {focus === "vanilla" ? (
        <div className="empty-state">
          <div className="empty-icon">
            <Gamepad2 size={27} />
          </div>
          <h3>原版 Minecraft</h3>
          <p>创建实例时选择原版即可获得独立目录；游戏本体下载与启动链路仍在开发中。</p>
          <button className="primary-button" onClick={onCreate}>
            <Plus size={17} /> 创建原版实例
          </button>
        </div>
      ) : (
        <section className="loader-panel">
          <div className="loader-panel-head">
            <label className="filter-field">
              Minecraft 版本
              <select value={effectiveVersion} onChange={(event) => setGameVersion(event.target.value)}>
                {versions.length === 0 && <option value={effectiveVersion}>{effectiveVersion}</option>}
                {versions.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.id}
                  </option>
                ))}
              </select>
            </label>
            <label className="filter-field">
              下载到实例
              <select value={target?.id ?? ""} onChange={(event) => setTargetId(event.target.value)}>
                <option value="">请选择实例</option>
                {instances.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name} · {item.minecraft_version}
                  </option>
                ))}
              </select>
            </label>
            <button className="ghost-button" onClick={() => onOpenExternal(catalog?.page ?? "https://minecraft.net/")}>
              <ExternalLink size={15} /> 官方页面
            </button>
          </div>

          {catalog?.note && <div className="inline-note">{catalog.note}</div>}
          {loading && (
            <div className="file-loading">
              <LoaderCircle className="spin" size={18} /> 正在获取版本列表
            </div>
          )}
          {!loading && catalog && catalog.versions.length === 0 && (
            <div className="inline-note">该加载器没有可自动获取的版本，请从官方页面下载后手动导入。</div>
          )}
          <div className="loader-version-list">
            {(catalog?.versions ?? []).map((version) => (
              <div className="loader-version-row" key={version}>
                <div>
                  <strong>{version}</strong>
                  <small>{catalog?.name} · {effectiveVersion}</small>
                </div>
                <div className="loader-version-actions">
                  <button
                    className="ghost-button"
                    disabled={downloading === version || !installerUrl(version)}
                    onClick={() => void downloadInstaller(version)}
                  >
                    {downloading === version ? <LoaderCircle className="spin" size={15} /> : <Download size={15} />}
                    下载安装器
                  </button>
                  {autoLoader && (
                    <button
                      className="primary-button"
                      disabled={installing === version || !target}
                      onClick={() => void installVersion(version)}
                    >
                      {installing === version ? <LoaderCircle className="spin" size={15} /> : <Play size={15} />}
                      安装到实例
                    </button>
                  )}
                  {installing === version && loadersProgress && (
                    <LoaderProgressLine progress={loadersProgress} />
                  )}
                </div>
              </div>
            ))}
          </div>
          {catalog?.installer_url && (
            <p className="security-note">安装器地址：{catalog.installer_url}</p>
          )}
        </section>
      )}
    </div>
  );
}

function AiCard({
  settings,
  onNotice,
}: {
  settings: AppSettings;
  onNotice: (message: string) => void;
}) {
  const [view, setView] = useState<AiSettingsView | null>(null);
  const [enabled, setEnabled] = useState(true);
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [name, setName] = useState("小七");
  const [key, setKey] = useState("");
  const [testMessage, setTestMessage] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [reply, setReply] = useState("");

  useEffect(() => {
    api
      .aiSettings()
      .then((loaded) => {
        setView(loaded);
        setEnabled(loaded.enabled);
        setBaseUrl(loaded.base_url);
        setModel(loaded.model);
        setName(loaded.name);
      })
      .catch((error) => onNotice(String(error)));
  }, [settings]);

  const save = async () => {
    setBusy("save");
    try {
      const saved = await api.aiSaveSettings({
        enabled,
        base_url: baseUrl.trim(),
        model: model.trim(),
        key: key.trim() || undefined,
        name: name.trim(),
      });
      setView(saved);
      setKey("");
      onNotice("AI 伙伴设置已保存（密钥 DPAPI 加密，不写日志）");
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(null);
    }
  };

  const test = async () => {
    setBusy("test");
    setReply("");
    try {
      const result = await api.aiTestChat(testMessage.trim() || "你好，做个自我介绍吧");
      setReply(result.reply);
    } catch (error) {
      setReply(`测试失败：${String(error)}`);
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className="settings-card">
      <div className="settings-heading">
        <div className="setting-icon ai">
          <Sparkles size={22} />
        </div>
        <div>
          <h3>AI 伙伴（游戏内）</h3>
          <p>
            安装 Companion 模组后，伙伴会随玩家进入世界、观察操作、陪你聊天；默认开启。
            「自我意识」是角色扮演设定，伙伴不会替你操作游戏。
          </p>
        </div>
      </div>
      <div className="toggle-row">
        <label className="toggle">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(event) => setEnabled(event.target.checked)}
          />
          <span>启用 AI 伙伴（启动游戏时自动注入）</span>
        </label>
      </div>
      <label className="field-label">
        伙伴名字
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="小七"
          spellCheck={false}
        />
      </label>
      <label className="field-label">
        接口地址（OpenAI 兼容；默认 DeepSeek）
        <input
          value={baseUrl}
          onChange={(event) => setBaseUrl(event.target.value)}
          placeholder="https://api.deepseek.com"
          spellCheck={false}
        />
      </label>
      <label className="field-label">
        模型
        <input
          value={model}
          onChange={(event) => setModel(event.target.value)}
          placeholder="deepseek-chat"
          spellCheck={false}
        />
      </label>
      <label className="field-label">
        API Key {view?.key_set && <span className="pill mini">已加密保存</span>}
        <input
          type="password"
          value={key}
          onChange={(event) => setKey(event.target.value)}
          placeholder={view?.key_set ? "已保存，留空保持不变" : "sk-..."}
          autoComplete="new-password"
        />
      </label>
      <div className="settings-actions">
        <button className="primary-button" onClick={() => void save()} disabled={busy !== null}>
          {busy === "save" ? <LoaderCircle className="spin" size={15} /> : <Check size={15} />} 保存
        </button>
      </div>
      <label className="field-label">
        对话测试
        <div className="inline-field">
          <input
            value={testMessage}
            onChange={(event) => setTestMessage(event.target.value)}
            placeholder="给我一点挖矿建议"
          />
          <button className="ghost-button" onClick={() => void test()} disabled={busy !== null}>
            {busy === "test" ? <LoaderCircle className="spin" size={15} /> : <Sparkles size={15} />} 发送
          </button>
        </div>
      </label>
      {reply && <p className="security-note">小七：{reply}</p>}
      <p className="security-note">
        Key 经 DPAPI 加密存放，只在启动器本地使用（模组 → 本地回环 → 启动器 → AI 接口），不写入日志，也不进模组配置。
        游戏内用 <b>#ai 你的问题</b> 与伙伴对话；单机模式还会出现跟随的伙伴实体。
      </p>
    </section>
  );
}

function SettingsPage({
  info,
  settings,
  sakura,
  onSave,
  onRefreshSakura,
  onRefreshInfo,
  onNotice,
}: {
  info: AppInfo | null;
  settings: AppSettings;
  sakura: string;
  onSave: (next: AppSettings) => Promise<void>;
  onRefreshSakura: () => Promise<void>;
  onRefreshInfo: () => Promise<void>;
  onNotice: (message: string) => void;
}) {
  const [key, setKey] = useState("");
  const [proxy, setProxy] = useState(settings.proxy);
  const [sakuraEnabled, setSakuraEnabled] = useState(settings.sakura_enabled);
  const [sharedGameDir, setSharedGameDir] = useState(settings.shared_game_dir);
  const [downloadSource, setDownloadSource] = useState(settings.download_source || "official");
  const [mpPeers, setMpPeers] = useState(settings.mp_peers || "");
  const [msClientId, setMsClientId] = useState(settings.ms_client_id);
  const [instancesDir, setInstancesDir] = useState(settings.instances_dir || "");
  const [dataDir, setDataDir] = useState(settings.data_dir || "");
  const [sakuraSource, setSakuraSource] = useState(settings.sakura_source || "");
  const [accounts, setAccounts] = useState<AccountView[]>([]);
  const [offlineName, setOfflineName] = useState("");
  const [deviceCode, setDeviceCode] = useState<DeviceCode | null>(null);
  const [pollMessage, setPollMessage] = useState("");
  const [pollHint, setPollHint] = useState<string | null>(null);
  const [checkingClient, setCheckingClient] = useState(false);
  const [clientCheck, setClientCheck] = useState("");
  const [lsUsername, setLsUsername] = useState("");
  const [lsPassword, setLsPassword] = useState("");
  const [lsProfiles, setLsProfiles] = useState<YggdrasilProfile[]>([]);
  const [lsPendingId, setLsPendingId] = useState<string | null>(null);
  const [lsSelected, setLsSelected] = useState("");
  const [lsBusy, setLsBusy] = useState(false);
  const [lsOauth, setLsOauth] = useState<LittleSkinOauthDevice | null>(null);
  const [lsOauthMessage, setLsOauthMessage] = useState("");
  const [lsOauthBusy, setLsOauthBusy] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState("");

  const loadAccounts = async () => {
    try {
      setAccounts(await api.accounts());
    } catch (error) {
      onNotice(String(error));
    }
  };

  useEffect(() => {
    setProxy(settings.proxy);
    setSakuraEnabled(settings.sakura_enabled);
    setSharedGameDir(settings.shared_game_dir);
    setDownloadSource(settings.download_source || "official");
    setMpPeers(settings.mp_peers || "");
    setInstancesDir(settings.instances_dir || "");
    setDataDir(settings.data_dir || "");
    setSakuraSource(settings.sakura_source || "");
    setMsClientId(settings.ms_client_id);
  }, [settings]);

  useEffect(() => {
    void loadAccounts();
  }, []);

  const saveKey = async () => {
    setBusy("key");
    try {
      await api.setCurseForgeKey(key);
      setKey("");
      await onRefreshInfo();
      onNotice("CurseForge API Key 已加密保存");
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(null);
    }
  };

  const clearKey = async () => {
    try {
      await api.clearCurseForgeKey();
      await onRefreshInfo();
      onNotice("CurseForge API Key 已移除");
    } catch (error) {
      onNotice(String(error));
    }
  };

  const test = async () => {
    setTesting(true);
    setTestResult("");
    try {
      const result = await api.testProxy(proxy.trim());
      setTestResult(`${result.ok ? "可用" : "失败"} · HTTP ${result.status || "-"} · ${result.latency_ms} ms · ${result.message}`);
    } catch (error) {
      setTestResult(String(error));
    } finally {
      setTesting(false);
    }
  };

  const saveProxy = async () => {
    await onSave({ ...settings, proxy: proxy.trim() });
    onNotice("网络设置已保存");
  };

  const saveDownload = async () => {
    await onSave({
      ...settings,
      download_source: downloadSource,
      shared_game_dir: sharedGameDir.trim(),
      mp_peers: mpPeers.trim(),
      instances_dir: instancesDir.trim(),
      data_dir: dataDir.trim(),
      sakura_source: sakuraSource.trim(),
    });
    onNotice(
      dataDir.trim() && dataDir.trim() !== (settings.data_dir || "")
        ? "数据目录已更新，现有数据（账户/实例索引/缓存）已复制到新位置；引导配置仍保留在默认目录"
        : "下载与目录设置已保存",
    );
  };

  const chooseSharedDir = async () => {
    const path = await api.pickFolder();
    if (path) setSharedGameDir(path);
  };

  const saveMsClient = async () => {
    await onSave({ ...settings, ms_client_id: msClientId.trim() });
    onNotice("微软 Client ID 已保存");
  };

  const checkMsClient = async () => {
    setCheckingClient(true);
    setClientCheck("");
    try {
      const result = await api.msCheckClientId(msClientId);
      setClientCheck(result.message);
    } catch (error) {
      setClientCheck(String(error));
    } finally {
      setCheckingClient(false);
    }
  };

  const openReview = async () => {
    await api
      .openExternal("https://aka.ms/AppRegInfo")
      .catch((error) => onNotice(String(error)));
  };

  const openAzureApps = async () => {
    await api
      .openExternal("https://portal.azure.com/#blade/Microsoft_AAD_RegisteredApps/ApplicationsListBlade")
      .catch((error) => onNotice(String(error)));
  };

  const loginLittleskin = async () => {
    if (!lsUsername.trim() || !lsPassword) {
      onNotice("请填写 LittleSkin 邮箱/用户名和密码");
      return;
    }
    setLsBusy(true);
    try {
      const result = await api.yggdrasilLogin("", lsUsername.trim(), lsPassword);
      if (result.account) {
        setLsPassword("");
        setLsProfiles([]);
        setLsPendingId(null);
        await loadAccounts();
        onNotice(`LittleSkin 登录成功：${result.account.name}`);
      } else if (result.pending_id) {
        setLsProfiles(result.profiles);
        setLsPendingId(result.pending_id);
        setLsSelected(result.profiles[0]?.id ?? "");
      } else {
        onNotice("LittleSkin 登录失败：服务器没有返回角色");
      }
    } catch (error) {
      onNotice(String(error));
    } finally {
      setLsBusy(false);
    }
  };

  const finishLittleskin = async () => {
    if (!lsPendingId || !lsSelected) return;
    setLsBusy(true);
    try {
      const account = await api.yggdrasilSelect(lsPendingId, lsSelected);
      setLsPassword("");
      setLsProfiles([]);
      setLsPendingId(null);
      await loadAccounts();
      onNotice(`LittleSkin 登录成功：${account.name}`);
    } catch (error) {
      onNotice(String(error));
    } finally {
      setLsBusy(false);
    }
  };

  const startLsOauth = async () => {
    setLsOauthBusy(true);
    setLsOauthMessage("");
    try {
      const device = await api.littleskinOauthStart();
      setLsOauth(device);
      void api
        .openExternal(device.verification_uri_complete || device.verification_uri)
        .catch(() => undefined);
      void navigator.clipboard?.writeText(device.user_code).catch(() => undefined);
    } catch (error) {
      onNotice(String(error));
    } finally {
      setLsOauthBusy(false);
    }
  };

  const copyLsOauthCode = () => {
    if (!lsOauth) return;
    void navigator.clipboard
      ?.writeText(lsOauth.user_code)
      .then(() => onNotice("授权码已复制"))
      .catch(() => onNotice("复制失败，请手动选择授权码"));
  };

  const openLsOauth = () => {
    if (!lsOauth) return;
    void api
      .openExternal(lsOauth.verification_uri_complete || lsOauth.verification_uri)
      .catch((error) => onNotice(String(error)));
  };

  const openLsOauthManage = () => {
    void api
      .openExternal("https://littleskin.cn/user/oauth/manage")
      .catch((error) => onNotice(String(error)));
  };

  const copyWhitelistTemplate = () => {
    const template = [
      "应用名称：aurora-launcher",
      "客户端 ID：800",
      "申请权限列表：",
      "Yggdrasil.MinecraftToken.Create",
      "Yggdrasil.PlayerProfiles.Select",
      "申请理由：Minecraft 启动器通过 OAuth 设备代码流实现 LittleSkin 外置登录。",
    ].join("\n");
    void navigator.clipboard
      ?.writeText(template)
      .then(() => onNotice("白名单申请内容已复制，可粘贴到邮件正文"))
      .catch(() => onNotice("复制失败，请手动填写：应用 aurora-launcher / 客户端 ID 800"));
  };

  useEffect(() => {
    if (!lsOauth) return;
    let cancelled = false;
    let delay = Math.max(3, lsOauth.interval) * 1000;
    let elapsed = 0;
    let timer = 0;
    const tick = async () => {
      if (cancelled) return;
      try {
        const result = await api.littleskinOauthPoll(lsOauth.device_code);
        if (cancelled) return;
        setLsOauthMessage(result.message);
        if (result.status === "success") {
          setLsOauth(null);
          setLsPassword("");
          await loadAccounts();
          onNotice("LittleSkin 授权登录成功");
          return;
        }
        if (result.status === "slow_down") {
          delay += 5000;
        }
        if (result.terminal) {
          return;
        }
      } catch (error) {
        if (!cancelled) setLsOauthMessage(String(error));
      }
      elapsed += delay;
      if (elapsed > lsOauth.expires_in * 1000) {
        setLsOauthMessage("授权码已过期，请重新发起授权登录");
        return;
      }
      timer = window.setTimeout(() => void tick(), delay);
    };
    timer = window.setTimeout(() => void tick(), delay);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [lsOauth?.device_code]);

  const toggleSakura = async (enabled: boolean) => {
    setSakuraEnabled(enabled);
    await onSave({ ...settings, sakura_enabled: enabled });
    if (enabled) {
      await onRefreshSakura();
    }
  };

  const addOffline = async () => {
    setBusy("offline");
    try {
      await api.addOfflineAccount(offlineName.trim());
      setOfflineName("");
      await loadAccounts();
      onNotice("离线账户已添加并设为当前账户");
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(null);
    }
  };

  const startMsLogin = async () => {
    setBusy("ms");
    setPollMessage("");
    setPollHint(null);
    try {
      const code = await api.msStartLogin();
      setDeviceCode(code);
      void api
        .openExternal(code.verification_uri_complete || code.verification_uri)
        .catch(() => undefined);
      void navigator.clipboard?.writeText(code.user_code).catch(() => undefined);
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(null);
    }
  };

  const pollMsLogin = async () => {
    if (!deviceCode) return;
    setBusy("poll");
    try {
      const result = await api.msPollLogin(deviceCode.device_code);
      setPollMessage(result.message);
      setPollHint(result.hint ?? null);
      if (result.status === "success") {
        setDeviceCode(null);
        setPollHint(null);
        await loadAccounts();
        onNotice("微软账户登录成功");
      }
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(null);
    }
  };

  useEffect(() => {
    if (!deviceCode) return;
    let cancelled = false;
    let delay = Math.max(5, deviceCode.interval) * 1000;
    let elapsed = 0;
    let timer = 0;
    const tick = async () => {
      if (cancelled) return;
      try {
        const result = await api.msPollLogin(deviceCode.device_code);
        if (cancelled) return;
        setPollMessage(result.message);
        setPollHint(result.hint ?? null);
        if (result.status === "success") {
          setDeviceCode(null);
          setPollHint(null);
          await loadAccounts();
          onNotice("微软账户登录成功");
          return;
        }
        if (result.status === "slow_down") {
          delay += 5000;
        }
        if (result.terminal) {
          return;
        }
      } catch (error) {
        if (!cancelled) setPollMessage(String(error));
      }
      elapsed += delay;
      if (elapsed > deviceCode.expires_in * 1000) {
        setPollMessage("设备码已过期，请重新点击微软登录");
        return;
      }
      timer = window.setTimeout(() => void tick(), delay);
    };
    timer = window.setTimeout(() => void tick(), delay);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [deviceCode?.device_code]);

  const sysOpen = async () => {
    if (!deviceCode) return;
    await api
      .openExternal(deviceCode.verification_uri_complete || deviceCode.verification_uri)
      .catch((error) => onNotice(String(error)));
  };

  return (
    <div className="page-stack settings-page">
      <section className="settings-card">
        <div className="settings-heading">
          <div className="setting-icon">
            <ShieldCheck size={22} />
          </div>
          <div>
            <h3>CurseForge API</h3>
            <p>密钥只在 Rust 侧使用，Windows 上用 DPAPI 加密保存，不写入日志或前端。</p>
          </div>
        </div>
        <div className={`connection-state ${info?.curseforge_configured ? "ok" : "muted"}`}>
          <span className="status-dot">●</span>
          {info?.curseforge_configured
            ? info.curseforge_mode === "proxy"
              ? "通过自建 HTTPS 代理访问，客户端不持有密钥"
              : "已配置本地加密凭据"
            : "未配置；仍可使用 Modrinth"}
        </div>
        <label className="field-label">
          API Key
          <input
            type="password"
            value={key}
            onChange={(event) => setKey(event.target.value)}
            placeholder={info?.curseforge_configured ? "输入新 Key 以替换" : "粘贴 console.curseforge.com 生成的 API Key"}
            autoComplete="off"
          />
        </label>
        <div className="settings-actions">
          <button className="ghost-button" onClick={() => void clearKey()} disabled={!info?.curseforge_configured}>
            移除本地 Key
          </button>
          <button className="primary-button" onClick={() => void saveKey()} disabled={busy === "key" || key.trim().length < 20}>
            {busy === "key" ? <LoaderCircle className="spin" size={17} /> : <Check size={17} />} 保存 Key
          </button>
        </div>
      </section>

      <section className="settings-card">
        <div className="settings-heading">
          <div className="setting-icon">
            <Gamepad2 size={22} />
          </div>
          <div>
            <h3>账户</h3>
            <p>微软正版登录使用设备码流程；需要你自己的 Azure 公共客户端 Client ID。</p>
          </div>
        </div>
        <label className="field-label">
          Azure Client ID
          <div className="inline-field">
            <input
              value={msClientId}
              onChange={(event) => setMsClientId(event.target.value)}
              placeholder="留空使用内置的 Aurora 官方 Client ID（已过审）"
              spellCheck={false}
            />
            <button className="ghost-button" onClick={() => void saveMsClient()}>保存</button>
            <button className="ghost-button" onClick={() => void checkMsClient()} disabled={checkingClient}>
              {checkingClient ? <LoaderCircle className="spin" size={15} /> : <ShieldCheck size={15} />} 验证
            </button>
          </div>
        </label>
        <p className="security-note">
          已内置 Aurora 自有 Azure 应用（ae897ba3-93b3-4998-a4fb-b80bad87b538）并已通过 Mojang
          appID 白名单审核，可直接点击“微软登录”。想用自己的应用时才需要填写：
          账户类型选“个人 Microsoft 账户”，添加 Web 平台重定向 https://www.microsoft.com/link
          并允许公共客户端流，材料见 docs/mce-review-application.md。
        </p>
        <div className="settings-actions">
          <button className="ghost-button" onClick={() => void openAzureApps()}>
            <ExternalLink size={15} /> Azure 应用注册
          </button>
          <button className="ghost-button" onClick={() => void openReview()}>
            <ExternalLink size={15} /> Minecraft 应用审核
          </button>
        </div>
        {clientCheck && <p className="security-note">{clientCheck}</p>}
        <div className="account-list">
          {accounts.length === 0 && <p className="security-note">还没有账户；未添加时会以离线身份 Player 启动。</p>}
          {accounts.map((account) => (
            <div className={`account-row ${account.active ? "active" : ""}`} key={account.id}>
              <div>
                <strong>{account.name}</strong>
                <small>{account.kind === "microsoft" ? "微软正版" : account.kind === "yggdrasil" ? "LittleSkin" : "离线"} · {account.uuid}</small>
              </div>
              {account.active ? (
                <span className="pill mini">当前</span>
              ) : (
                <button
                  className="ghost-button"
                  onClick={() => {
                    void api.setActiveAccount(account.id).then(loadAccounts).catch((error) => onNotice(String(error)));
                  }}
                >
                  切换
                </button>
              )}
              <button
                className="danger-button"
                onClick={() => {
                  void api.removeAccount(account.id).then(loadAccounts).catch((error) => onNotice(String(error)));
                }}
              >
                <Trash2 size={15} />
              </button>
            </div>
          ))}
        </div>
        <div className="inline-field">
          <input
            value={offlineName}
            onChange={(event) => setOfflineName(event.target.value)}
            placeholder="离线用户名（1-16 位字母数字）"
          />
          <button className="ghost-button" onClick={() => void addOffline()} disabled={busy === "offline"}>
            <Plus size={15} /> 添加离线账户
          </button>
          <button className="primary-button" onClick={() => void startMsLogin()} disabled={busy === "ms"}>
            {busy === "ms" ? <LoaderCircle className="spin" size={15} /> : <ShieldCheck size={15} />} 微软登录
          </button>
        </div>
        {deviceCode && (
          <div className="device-code">
            <p>
              浏览器已自动打开；若没有，请访问 <strong>{deviceCode.verification_uri}</strong> 并输入代码：
            </p>
            <div className="user-code">{deviceCode.user_code}</div>
            <div className="settings-actions">
              <button
                className="ghost-button"
                onClick={() => {
                  void navigator.clipboard
                    ?.writeText(deviceCode.user_code)
                    .then(() => onNotice("代码已复制"))
                    .catch(() => onNotice("复制失败，请手动选择代码"));
                }}
              >
                复制代码
              </button>
              <button className="ghost-button" onClick={() => void sysOpen()} disabled={busy === "poll"}>
                <ExternalLink size={15} /> 重新打开网页
              </button>
              <button className="ghost-button" onClick={() => void pollMsLogin()} disabled={busy === "poll"}>
                {busy === "poll" ? <LoaderCircle className="spin" size={15} /> : <RefreshCw size={15} />} 立即检查
              </button>
              <button className="ghost-button" onClick={() => setDeviceCode(null)}>取消登录</button>
            </div>
            <p className="security-note">代码约 15 分钟内有效；登录完成后会自动完成 Xbox 与 Minecraft 认证。</p>
            {pollMessage && <p className="security-note">{pollMessage}</p>}
            {pollHint && (
              <button
                className="ghost-button"
                onClick={() =>
                  void api.openExternal(pollHint).catch((error) => onNotice(String(error)))
                }
              >
                <ExternalLink size={15} /> 打开 Minecraft 商店
              </button>
            )}
          </div>
        )}
        <div className="external-login">
          <h4>LittleSkin 登录（外置 Yggdrasil）</h4>
          <div className="inline-field">
            <button className="primary-button" onClick={() => void startLsOauth()} disabled={lsOauthBusy}>
              {lsOauthBusy ? <LoaderCircle className="spin" size={15} /> : <ShieldCheck size={15} />} 授权登录（推荐）
            </button>
          </div>
          <p className="security-note">
            授权登录走 LittleSkin OAuth 设备代码流，不输入密码。应用回调需为
            https://open.littleskin.cn/oauth/callback（当前已配置）；若提示 invalid_client，
            说明设备代码流白名单尚未通过，可用下方按钮复制申请内容。
          </p>
          <div className="settings-actions">
            <button className="ghost-button" onClick={openLsOauthManage}>
              <ExternalLink size={15} /> LittleSkin 应用管理
            </button>
            <button className="ghost-button" onClick={copyWhitelistTemplate}>
              复制白名单申请
            </button>
          </div>
          {lsOauth && (
            <div className="device-code">
              <p>
                浏览器已打开授权页；若没有，请访问 <strong>{lsOauth.verification_uri}</strong> 并输入授权码：
              </p>
              <div className="user-code">{lsOauth.user_code}</div>
              <div className="settings-actions">
                <button className="ghost-button" onClick={copyLsOauthCode}>复制授权码</button>
                <button className="ghost-button" onClick={openLsOauth}>
                  <ExternalLink size={15} /> 重新打开授权页
                </button>
                <button
                  className="ghost-button"
                  onClick={() => {
                    setLsOauth(null);
                    setLsOauthMessage("");
                  }}
                >
                  取消授权
                </button>
              </div>
              <p className="security-note">授权码约 5 分钟内有效；在授权页选择角色后会自动完成登录。</p>
              {lsOauthMessage && <p className="security-note">{lsOauthMessage}</p>}
            </div>
          )}
          <p className="security-note">或使用 LittleSkin 账号密码（传统 Yggdrasil 登录）：</p>
          <div className="inline-field">
            <input
              value={lsUsername}
              onChange={(event) => setLsUsername(event.target.value)}
              placeholder="LittleSkin 邮箱或用户名"
              spellCheck={false}
              autoComplete="username"
            />
            <input
              type="password"
              value={lsPassword}
              onChange={(event) => setLsPassword(event.target.value)}
              placeholder="密码（只发送给 littleskin.cn，不保存）"
              autoComplete="current-password"
            />
            <button className="primary-button" onClick={() => void loginLittleskin()} disabled={lsBusy}>
              {lsBusy ? <LoaderCircle className="spin" size={15} /> : <ShieldCheck size={15} />} 登录
            </button>
          </div>
          {lsProfiles.length > 1 && (
            <div className="inline-field">
              <select value={lsSelected} onChange={(event) => setLsSelected(event.target.value)}>
                {lsProfiles.map((profile) => (
                  <option key={profile.id} value={profile.id}>
                    {profile.name}
                  </option>
                ))}
              </select>
              <button
                className="ghost-button"
                onClick={() => void finishLittleskin()}
                disabled={lsBusy || !lsPendingId}
              >
                使用该角色
              </button>
            </div>
          )}
          <p className="security-note">
            通过 authlib-injector 接入 Yggdrasil；角色与皮肤来自 littleskin.cn。密码不会保存，登录令牌使用 DPAPI 加密存放在本机。
          </p>
        </div>
      </section>

      <section className="settings-card">
        <div className="settings-heading">
          <div className="setting-icon network">
            <Globe2 size={22} />
          </div>
          <div>
            <h3>网络代理</h3>
            <p>所有 API 与资源下载都会使用这里的代理；留空则直连。</p>
          </div>
        </div>
        <label className="field-label">
          代理地址
          <input
            value={proxy}
            onChange={(event) => setProxy(event.target.value)}
            placeholder="http://127.0.0.1:10808"
            spellCheck={false}
          />
        </label>
        <div className="settings-actions">
          <button className="ghost-button" onClick={() => setProxy("http://127.0.0.1:10808")}>
            填入 10808
          </button>
          <button className="ghost-button" onClick={() => void test()} disabled={testing}>
            {testing ? <LoaderCircle className="spin" size={17} /> : <RefreshCw size={16} />} 测试代理
          </button>
          <button className="primary-button" onClick={() => void saveProxy()}>
            <Check size={17} /> 保存网络设置
          </button>
        </div>
        {testResult && <div className="inline-note">{testResult}</div>}
      </section>

      <section className="settings-card">
        <div className="settings-heading">
          <div className="setting-icon network">
            <Download size={22} />
          </div>
          <div>
            <h3>下载与共享目录</h3>
            <p>游戏文件默认下载到实例 game 目录；共享目录只复用只读的 assets、libraries 与 vanilla 版本。</p>
          </div>
        </div>
        <label className="field-label">
          下载源
          <select
            value={downloadSource.startsWith("custom:") ? "custom" : downloadSource}
            onChange={(event) => {
              const value = event.target.value;
              if (value === "custom") {
                // 默认给 Aurora 自建镜像；已存过自定义值则保留原值。
                setDownloadSource(
                  downloadSource.startsWith("custom:") ? downloadSource : "custom:https://mc.elingfeng.com",
                );
              } else {
                setDownloadSource(value);
              }
            }}
          >
            <option value="official">Mojang 官方</option>
            <option value="bmclapi">BMCLAPI 国内镜像</option>
            <option value="auto">自动（探测最优下载源）</option>
            <option value="custom">自定义镜像（MC 镜像站）</option>
          </select>
        </label>
        {downloadSource.startsWith("custom:") && (
          <label className="field-label">
            自定义镜像地址（BMCLAPI 兼容布局，镜像优先、官方兜底）
            <input
              value={downloadSource.slice("custom:".length)}
              onChange={(event) => setDownloadSource(`custom:${event.target.value.trim()}`)}
              placeholder="https://mc.elingfeng.com"
              spellCheck={false}
            />
          </label>
        )}
        <label className="field-label">
          联机共享节点（可选，逗号分隔；留空使用内置默认节点）
          <input
            value={mpPeers}
            onChange={(event) => setMpPeers(event.target.value)}
            placeholder="tcp://public.easytier.cn:11010, tcp://my-node.example:11010"
            spellCheck={false}
          />
        </label>
        <label className="field-label">
          数据目录（账户、实例索引、EasyTier 等缓存的根目录；修改后自动把现有数据复制到新位置，引导配置保留在默认位置）
          <div className="inline-field">
            <input
              value={dataDir}
              onChange={(event) => setDataDir(event.target.value)}
              placeholder="留空使用默认（C 盘 AppData\com.aurora.launcher）"
              spellCheck={false}
            />
            <button
              className="ghost-button"
              onClick={async () => {
                const path = await api.pickFolder();
                if (path) setDataDir(path);
              }}
            >
              浏览
            </button>
          </div>
        </label>
        <label className="field-label">
          实例存储目录（可选；留空时新实例放在数据目录的 instances 下，仅对新实例生效）
          <div className="inline-field">
            <input
              value={instancesDir}
              onChange={(event) => setInstancesDir(event.target.value)}
              placeholder="例如 D:\AuroraInstances"
              spellCheck={false}
            />
            <button
              className="ghost-button"
              onClick={async () => {
                const path = await api.pickFolder();
                if (path) setInstancesDir(path);
              }}
            >
              浏览
            </button>
          </div>
        </label>
        <label className="field-label">
          共享游戏目录（可选）
          <div className="inline-field">
            <input
              value={sharedGameDir}
              onChange={(event) => setSharedGameDir(event.target.value)}
              placeholder="例如 F:\\.xmcl 或 F:\\.minecraft"
              spellCheck={false}
            />
            <button className="ghost-button" onClick={() => void chooseSharedDir()}>浏览</button>
          </div>
        </label>
        <div className="settings-actions">
          <button className="primary-button" onClick={() => void saveDownload()}>
            <Check size={17} /> 保存下载设置
          </button>
        </div>
        <p className="security-note">
          共享目录用于已存在的官方游戏文件，避免重复下载数 GB 的 assets；实例的 mods、saves、config、logs 仍完全隔离。不填则每个实例独立下载。
        </p>
      </section>

      <AiCard settings={settings} onNotice={onNotice} />

      <section className="settings-card">
        <div className="settings-heading">
          <div className="setting-icon sakura">
            <Sparkles size={22} />
          </div>
          <div>
            <h3>樱花二次元主题</h3>
            <p>随机二次元背景；默认关闭。背景会加模糊与深色遮罩，加载失败自动回退默认主题。</p>
          </div>
        </div>
        <div className="toggle-row">
          <label className="toggle">
            <input
              type="checkbox"
              checked={sakuraEnabled}
              onChange={(event) => void toggleSakura(event.target.checked)}
            />
            <span>启用随机樱花背景</span>
          </label>
          {sakuraEnabled && (
            <button className="ghost-button" onClick={() => void onRefreshSakura()}>
              <RefreshCw size={15} /> 换一张
            </button>
          )}
        </div>
        <label className="field-label">
          随机图接口（可选；留空使用内置源池 dmoe.cc / paugram / alcy 随机轮换，也可换成你信任的单一接口，必须 https）
          <input
            value={sakuraSource}
            onChange={(event) => setSakuraSource(event.target.value)}
            placeholder="留空使用内置源池，或填 https://…/random.php"
            spellCheck={false}
          />
        </label>
        <p className="security-note">
          随机图接口不受本项目控制：返回的图片无法预先审核，可能包含不适宜内容；请求也会把你的 IP 暴露给该第三方服务。界面已用模糊与深色遮罩弱化背景，加载失败时自动回退默认主题；介意这些风险请保持关闭或换成自建接口。
        </p>
        {sakura && (
          <div className="sakura-preview">
            <img src={sakura} alt="樱花主题预览" />
          </div>
        )}
      </section>

      <section className="settings-card">
        <div className="settings-heading">
          <div className="setting-icon">
            <Settings2 size={22} />
          </div>
          <div>
            <h3>关于</h3>
            <p>{info?.name ?? "Aurora Launcher"} {info?.version ?? ""}</p>
          </div>
        </div>
        <div className="about-grid">
          <span>数据目录</span>
          <strong>{info?.data_dir ?? "-"}</strong>
          <span>地区限制</span>
          <strong>{info?.region_locked ? "当前发行版已限制下载" : "无限制"}</strong>
        </div>
      </section>
    </div>
  );
}

function MusicDeck({ onNotice }: { onNotice: (message: string) => void }) {
  const [tracks, setTracks] = useState<MusicTrack[]>([]);
  const [cur, setCur] = useState(-1);
  const [playing, setPlaying] = useState(false);
  const [title, setTitle] = useState("未播放");
  const [pos, setPos] = useState({ cur: 0, end: 0 });
  const [loading, setLoading] = useState(false);
  const [vol, setVol] = useState(80);

  const audioRef = useRef<HTMLAudioElement | null>(null);
  const urlRef = useRef("");
  const ctxRef = useRef<AudioContext | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const barsRef = useRef<HTMLDivElement | null>(null);
  const modeRef = useRef(0);

  const loadTrack = useCallback(
    async (index: number) => {
      const tr = tracks[index];
      if (!tr) return;
      setLoading(true);
      try {
        // 分块读回音频字节，拼成 Blob 播放。
        let offset = 0;
        const parts: Uint8Array[] = [];
        for (;;) {
          const chunk = await api.musicRead(tr.file, offset);
          if (!chunk.data) break;
          const bin = atob(chunk.data);
          const bytes = new Uint8Array(bin.length);
          for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
          parts.push(bytes);
          offset = chunk.offset;
          if (offset >= chunk.total) break;
        }
        const blob = new Blob(parts as BlobPart[]);
        if (urlRef.current) URL.revokeObjectURL(urlRef.current);
        urlRef.current = URL.createObjectURL(blob);
        const audio = audioRef.current;
        if (!audio) return;
        setCur(index);
        setTitle(tr.name);
        // 换 src 后等 canplay 再 play：立即 play 会因 readyState=0 被
        // 静默 stall（自动播放下一首失效的根因）。
        audio.src = urlRef.current;
        const onCanPlay = () => {
          audio.removeEventListener("canplay", onCanPlay);
          audio.play().catch(() => {
            setPlaying(false);
            onNotice(`已就绪：${tr.name}，点播放继续`);
          });
        };
        audio.addEventListener("canplay", onCanPlay, { once: true });
        audio.load();
      } catch (error) {
        onNotice(String(error));
      } finally {
        setLoading(false);
      }
    },
    [tracks, onNotice],
  );

  const refresh = useCallback(async () => {
    try {
      setTracks(await api.musicList());
    } catch (error) {
      onNotice(String(error));
    }
  }, [onNotice]);

  useEffect(() => {
    void refresh();
    return () => {
      if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    };
  }, [refresh]);

  const importMusic = async () => {
    try {
      const paths = await api.pickMusicFiles();
      if (!paths.length) return;
      const added = await api.musicImport(paths);
      await refresh();
      onNotice(added.length ? `已导入 ${added.length} 首音乐` : "所选文件没有可导入的音频");
    } catch (error) {
      onNotice(String(error));
    }
  };

  const removeTrack = async (file: string) => {
    try {
      await api.musicDelete(file);
      if (tracks[cur]?.file === file) {
        audioRef.current?.pause();
        setPlaying(false);
        setCur(-1);
        setTitle("未播放");
      }
      await refresh();
    } catch (error) {
      onNotice(String(error));
    }
  };

  const toggle = () => {
    const audio = audioRef.current;
    if (!audio) return;
    if (cur < 0) {
      if (tracks.length) void loadTrack(0);
      return;
    }
    if (audio.paused) {
      void audio.play();
      setPlaying(true);
    } else {
      audio.pause();
      setPlaying(false);
    }
  };

  const step = (delta: number) => {
    if (!tracks.length) return;
    const nextIndex = (cur + delta + tracks.length) % tracks.length;
    void loadTrack(nextIndex);
  };

  /* Web Audio 图 + 可视化循环 */
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;
    let raf = 0;
    let ctx: AudioContext | null = null;
    let analyser: AnalyserNode | null = null;
    const ensureGraph = () => {
      if (ctx) return;
      try {
        ctx = new AudioContext();
        const source = ctx.createMediaElementSource(audio);
        analyser = ctx.createAnalyser();
        analyser.fftSize = 2048;
        analyser.smoothingTimeConstant = 0.82;
        source.connect(analyser);
        analyser.connect(ctx.destination);
        ctxRef.current = ctx;
        analyserRef.current = analyser;
      } catch {
        ctx = null;
      }
    };
    const onPlay = () => {
      ensureGraph();
      if (ctx?.state === "suspended") void ctx.resume();
      setPlaying(true);
    };
    const onPause = () => setPlaying(false);
    const onEnded = () => {
      step(1);
      // 自动切曲 play 被 stall 时兜底重播当前曲。
      setTimeout(() => {
        if (audio.paused && !audio.ended) audio.play().catch(() => {});
      }, 500);
    };
    const onTime = () => {
      if (isFinite(audio.duration)) {
        setPos({ cur: audio.currentTime, end: audio.duration });
      }
    };
    audio.addEventListener("play", onPlay);
    audio.addEventListener("pause", onPause);
    audio.addEventListener("ended", onEnded);
    audio.addEventListener("timeupdate", onTime);

    const canvas = canvasRef.current;
    const vctx = canvas?.getContext("2d") ?? null;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    let W = 0, H = 0;
    const resize = () => {
      if (!canvas) return;
      const r = canvas.getBoundingClientRect();
      W = Math.floor(r.width); H = Math.floor(r.height);
      canvas.width = Math.floor(W * dpr); canvas.height = Math.floor(H * dpr);
      vctx?.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize();
    window.addEventListener("resize", resize);

    const freq = new Uint8Array(1024);
    const loop = (t: number) => {
      raf = requestAnimationFrame(loop);
      if (!vctx || !canvas) return;
      vctx.clearRect(0, 0, W, H);
      vctx.strokeStyle = "rgba(95,232,255,.05)";
      vctx.lineWidth = 1;
      for (let gx = 0; gx < W; gx += 44) { vctx.beginPath(); vctx.moveTo(gx, 0); vctx.lineTo(gx, H); vctx.stroke(); }
      for (let gy = 0; gy < H; gy += 44) { vctx.beginPath(); vctx.moveTo(0, gy); vctx.lineTo(W, gy); vctx.stroke(); }

      const an = analyserRef.current;
      if (an) an.getByteFrequencyData(freq);
      const playing2 = !audio.paused;
      const energy = playing2 && an ? freq.slice(2, 24).reduce((a, b) => a + b, 0) / (22 * 255) : 0.05;

      const bars = 56, gap = 3;
      const bw = (W - gap * (bars - 1)) / bars;
      for (let b = 0; b < bars; b++) {
        const lo = Math.floor(b * 2.2);
        const v = playing2 && an
          ? freq.slice(lo, lo + 3).reduce((a, x) => a + x, 0) / (3 * 255)
          : 0.05 + 0.04 * Math.sin(t / 700 + b / 4);
        const bh = Math.max(3, v * (H - 52));
        const x = b * (bw + gap), y = H - 26 - bh;
        const grad = vctx.createLinearGradient(0, y, 0, H - 26);
        grad.addColorStop(0, "rgba(95,232,255,.95)");
        grad.addColorStop(0.6, "rgba(126,150,255,.75)");
        grad.addColorStop(1, "rgba(143,124,255,.28)");
        vctx.fillStyle = grad;
        vctx.fillRect(x, y, bw, bh);
        vctx.fillStyle = "rgba(205,246,255,.7)";
        vctx.fillRect(x, y - 3, bw, 2);
      }
      vctx.fillStyle = "rgba(95,232,255,.35)";
      vctx.fillRect(0, H - 25, W, 1.4);

      if (barsRef.current) {
        Array.from(barsRef.current.children).forEach((el, i) => {
          const el2 = el as HTMLElement;
          const lo = 2 + i * 3;
          const v = playing2 && an
            ? freq.slice(lo, lo + 3).reduce((a, x) => a + x, 0) / (3 * 255)
            : 0.12;
          el2.style.height = Math.max(10, Math.min(100, v * 160)).toFixed(0) + "%";
        });
      }
      void energy;
    };
    raf = requestAnimationFrame(loop);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
      audio.removeEventListener("play", onPlay);
      audio.removeEventListener("pause", onPause);
      audio.removeEventListener("ended", onEnded);
      audio.removeEventListener("timeupdate", onTime);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tracks, cur]);

  const fmt = (sec: number) => {
    if (!isFinite(sec)) return "0:00";
    const m = Math.floor(sec / 60), s = Math.floor(sec % 60);
    return `${m}:${s < 10 ? "0" : ""}${s}`;
  };

  return (
    <div className="page-stack">
      <section className="section-heading">
        <div>
          <span className="eyebrow">SOUND DECK</span>
          <h3>音乐舱</h3>
          <p className="section-subtitle">
            导入你喜欢的本地音乐（mp3 / flac / ogg / wav 等），文件复制到启动器数据目录持久保存，仅在本机播放。
          </p>
        </div>
        <button className="primary-button" onClick={() => void importMusic()}>
          <Plus size={16} /> 导入音乐
        </button>
      </section>

      <div className="music-grid">
        <div className="music-panel">
          <div className="panel-h">
            <span className="led" />
            VISUALIZER · SPECTRUM
          </div>
          <div className="viz">
            <canvas ref={canvasRef} />
          </div>
          <div className="now">
            <div className={`disc ${playing ? "spinning" : ""}`}>✦</div>
            <div className="now-meta">
              <div className="now-title" title={title}>{title}</div>
              <div className="now-artist">
                {loading ? "加载中…" : playing ? "PLAYING" : cur >= 0 ? "PAUSED" : "STANDBY"} · LOCAL FILE
              </div>
              <div className="bars" ref={barsRef} aria-hidden="true" />
            </div>
          </div>
          <div className="ctrl">
            <button className="ghost-button" onClick={() => step(-1)} title="上一首">⏮</button>
            <button className="primary-button" onClick={toggle} title="播放 / 暂停">
              {playing ? "⏸" : "▶"}
            </button>
            <button className="ghost-button" onClick={() => step(1)} title="下一首">⏭</button>
            <div className="seek">
              <span>{fmt(pos.cur)}</span>
              <input
                type="range"
                min={0}
                max={1000}
                value={pos.end ? (pos.cur / pos.end) * 1000 : 0}
                style={{ ["--p" as string]: pos.end ? `${(pos.cur / pos.end) * 100}%` : "0%" }}
                onChange={(e) => {
                  const audio = audioRef.current;
                  if (audio && isFinite(audio.duration)) {
                    audio.currentTime = (Number(e.target.value) / 1000) * audio.duration;
                  }
                }}
              />
              <span>{fmt(pos.end)}</span>
            </div>
            <div className="vol">
              VOL
              <input
                type="range"
                min={0}
                max={100}
                value={vol}
                style={{ ["--p" as string]: `${vol}%` }}
                onChange={(e) => {
                  const v = Number(e.target.value);
                  setVol(v);
                  if (audioRef.current) audioRef.current.volume = v / 100;
                }}
              />
            </div>
          </div>
          <audio ref={audioRef} hidden />
        </div>

        <div className="music-panel">
          <div className="panel-h">
            <span className="led" />
            PLAYLIST · {tracks.length} TRACKS
          </div>
          {tracks.length === 0 ? (
            <div className="content-empty">
              曲库为空——点上方「导入音乐」选择你喜欢的音频文件
            </div>
          ) : (
            <div className="track-list">
              {tracks.map((tr, i) => (
                <div
                  key={tr.file}
                  className={`track-row ${i === cur ? "cur" : ""}`}
                  onClick={() => void loadTrack(i)}
                >
                  <span className="track-no">{i === cur ? "♪" : String(i + 1).padStart(2, "0")}</span>
                  <span className="track-name" title={tr.file}>{tr.name}</span>
                  <span className="track-size">{(tr.size / 1024 / 1024).toFixed(1)} MB</span>
                  <button
                    className="track-rm"
                    title="移除"
                    onClick={(e) => {
                      e.stopPropagation();
                      void removeTrack(tr.file);
                    }}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              ))}
            </div>
          )}
          <p className="security-note">
            音乐文件复制到数据目录 <code>music/</code> 下，仅在本机播放，不上传；上限 2000 首、单首 200 MB。
          </p>
        </div>
      </div>
    </div>
  );
}

function ChangelogPage({
  currentVersion,
  onNotice,
}: {
  currentVersion: string;
  onNotice: (message: string) => void;
}) {
  const [data, setData] = useState<Changelog | null>(null);
  const [showHistory, setShowHistory] = useState(false);

  useEffect(() => {
    api
      .changelog()
      .then(setData)
      .catch((error) => {
        setData(null);
        onNotice(String(error));
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const current = data?.entries.find((entry) => entry.current) ?? data?.entries[0];
  const history = data?.entries.filter((entry) => entry !== current) ?? [];

  const renderItems = (entry: ChangelogEntry) => {
    // 把 `path` 形式渲染成行内代码，保持更新日志里的路径可读。
    const renderText = (text: string) =>
      text.split(/(`[^`]+`)/g).map((part, index) =>
        part.startsWith("`") && part.endsWith("`") && part.length > 2 ? (
          <code key={index}>{part.slice(1, -1)}</code>
        ) : (
          <span key={index}>{part}</span>
        ),
      );

    return (
      <ul className="changelog-list">
        {entry.items.map((item, index) => {
          const lines = item.split("\n");
          const head = lines[0];
          const children = lines.slice(1).map((line) => line.replace(/^· /, ""));
          return (
            <li key={index}>
              {renderText(head)}
              {children.length > 0 && (
                <ul className="changelog-sub">
                  {children.map((child, childIndex) => (
                    <li key={childIndex}>{renderText(child)}</li>
                  ))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    );
  };

  return (
    <div className="page-stack">
      <section className="section-heading">
        <div>
          <span className="eyebrow">RELEASE NOTES</span>
          <h3>{currentVersion ? `Aurora Launcher ${currentVersion}` : "更新内容"}</h3>
          <p className="section-subtitle">
            更新说明随版本内置在启动器中，与仓库里的 RELEASE_NOTES.md 一致。
          </p>
        </div>
      </section>

      {data === null ? (
        <div className="empty-state">
          <LoaderCircle className="spin" size={22} />
          <p>正在读取更新内容…</p>
        </div>
      ) : (
        <>
          {current && (
            <article className="changelog-card current">
              <header>
                <div>
                  <span className="pill accent mini">当前版本</span>
                  <h4>v{current.version}</h4>
                </div>
                <ScrollText size={18} />
              </header>
              {current.items.length ? renderItems(current) : <p className="security-note">该版本没有记录条目。</p>}
            </article>
          )}

          {history.length > 0 && (
            <section className="changelog-history">
              <button className="text-button" onClick={() => setShowHistory((value) => !value)}>
                <History size={15} />
                {showHistory ? "收起历史版本" : `查看历史版本（${history.length}）`}
                <ChevronRight size={15} className={showHistory ? "flip" : ""} />
              </button>
              {showHistory &&
                history.map((entry) => (
                  <article className="changelog-card" key={entry.version}>
                    <header>
                      <h4>v{entry.version}</h4>
                      {entry.history && <em>历史</em>}
                    </header>
                    {renderItems(entry)}
                  </article>
                ))}
            </section>
          )}
        </>
      )}
    </div>
  );
}

function CreateInstance({
  manifest,
  onClose,
  onNotice,
  onCreated,
}: {
  manifest: VersionManifest | null;
  onClose: () => void;
  onNotice: (message: string) => void;
  onCreated: (instance: Instance) => Promise<void>;
}) {
  const [name, setName] = useState("我的新世界");
  const [version, setVersion] = useState(manifest?.latest_release ?? "1.21.1");
  const [loader, setLoader] = useState<LoaderId>("vanilla");
  const [loaderVersion, setLoaderVersion] = useState("");
  const [loaderVersions, setLoaderVersions] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [installing, setInstalling] = useState(false);
  const installProgress = useLoaderProgress(installing);
  const releases = useMemo(
    () => (manifest?.versions ?? []).filter((item) => item.kind === "release").slice(0, 120),
    [manifest],
  );
  const staticLoader = ["labymod"].includes(loader);

  useEffect(() => {
    let cancelled = false;
    setLoaderVersions([]);
    setLoaderVersion("");
    if (loader === "vanilla") return;
    api
      .loaderVersions(loader, version)
      .then((catalog) => {
        if (!cancelled) setLoaderVersions(catalog.versions);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [loader, version]);

  const create = async () => {
    setBusy(true);
    try {
      const instance = await api.createInstance({
        name: name.trim(),
        minecraftVersion: version,
        loader,
        loaderVersion: loaderVersion.trim() || undefined,
      });
      await onCreated(instance);
      if (loader === "labymod") {
        void api
          .openExternal("https://www.labymod.net/en/download")
          .catch(() => undefined);
        onNotice(
          "实例已创建。LabyMod 4 是独立启动器，已打开官方下载页；安装时选择本实例的 game 目录。",
        );
        return;
      }
      if (loader !== "vanilla") {
        setInstalling(true);
        onNotice(`正在下载并安装 ${version} 的 ${loader}，请稍候…`);
        try {
          const result = await api.installLoader(
            instance.id,
            loader,
            loaderVersion.trim(),
          );
          await onCreated(instance);
          onNotice(`已安装 ${loader} ${loaderVersion.trim() || ""}，版本 ${result.version_id}`);
        } catch (installError) {
          onNotice(
            `实例已创建，但加载器安装失败：${String(installError)}。可在实例管理中重试。`,
          );
        } finally {
          setInstalling(false);
        }
      }
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <div className="modal">
        <div className="modal-top">
          <span>新建游戏实例</span>
          <button className="icon-button" onClick={onClose}>
            <X size={17} />
          </button>
        </div>
        <div className="form-stack">
          <label className="field-label">
            实例名称
            <input value={name} onChange={(event) => setName(event.target.value)} />
          </label>
          <label className="field-label">
            Minecraft 版本
            {releases.length ? (
              <select value={version} onChange={(event) => setVersion(event.target.value)}>
                {releases.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.id}
                  </option>
                ))}
              </select>
            ) : (
              <input value={version} onChange={(event) => setVersion(event.target.value)} />
            )}
          </label>
          <label className="field-label">
            加载器
            <select value={loader} onChange={(event) => setLoader(event.target.value as LoaderId)}>
              {loaderEntries.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.label}
                </option>
              ))}
            </select>
          </label>
          {loader !== "vanilla" && (
            <label className="field-label">
              加载器版本
              {staticLoader || loaderVersions.length === 0 ? (
                <input
                  value={loaderVersion}
                  onChange={(event) => setLoaderVersion(event.target.value)}
                  placeholder="可留空，自动安装最新适配版"
                />
              ) : (
                <select
                  value={loaderVersion}
                  onChange={(event) => setLoaderVersion(event.target.value)}
                >
                  <option value="">稍后选择</option>
                  {loaderVersions.slice(0, 60).map((value) => (
                    <option key={value} value={value}>
                      {value}
                    </option>
                  ))}
                </select>
              )}
            </label>
          )}
          <p className="security-note">
            每个实例会创建独立的 game、mods、resourcepacks、shaderpacks、saves、datapacks、logs 与 downloads 目录。
          </p>
          {installing && installProgress && <LoaderProgressLine progress={installProgress} />}
          <div className="modal-actions">
            <button className="ghost-button" onClick={onClose}>
              取消
            </button>
            <button
              className="primary-button"
              onClick={() => void create()}
              disabled={busy || !name.trim() || !version.trim()}
            >
              {busy ? <LoaderCircle className="spin" size={17} /> : <Plus size={17} />} 创建实例
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export default App;
