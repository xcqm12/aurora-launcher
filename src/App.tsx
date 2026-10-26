import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  Boxes,
  Brush,
  Check,
  ChevronRight,
  Clock,
  Database,
  Download,
  ExternalLink,
  Feather,
  Flame,
  FolderOpen,
  Gamepad2,
  Globe2,
  Heart,
  History,
  Layers3,
  LoaderCircle,
  Monitor,
  PackageOpen,
  Plus,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
  Sparkles,
  Trash2,
  Wrench,
  X,
} from "lucide-react";
import {
  api,
  AppInfo,
  AppSettings,
  CurseForgeFile,
  DiscoverItem,
  Instance,
  LoaderCatalog,
  ResourceFile,
  ResourceVersion,
  VersionManifest,
} from "./api";

type View = "home" | "browse" | "instances" | "loaders" | "settings";
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
  const [settings, setSettings] = useState<AppSettings>({ proxy: "", sakura_enabled: false });
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
  };

  return (
    <div className={`app-shell ${sakura ? "with-sakura" : ""}`}>
      {sakura && (
        <div
          className="sakura-layer"
          style={{ backgroundImage: `url(${sakura})` }}
          aria-hidden="true"
        />
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
          />
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
            {visible.map((entry) => (
              <div className="file-row" key={entry.key}>
                <div>
                  <strong>{entry.title}</strong>
                  <small>{entry.detail}</small>
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
            ))}
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

  return (
    <div className="page-stack">
      <section className="section-heading">
        <div>
          <span className="eyebrow">ISOLATED INSTANCES</span>
          <h3>{instances.length ? `${instances.length} 个游戏实例` : "还没有游戏实例"}</h3>
          <p className="section-subtitle">
            每个实例拥有独立的 mods、资源包、光影、世界、日志与游戏目录。
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
                  <button className="launch-button" disabled title="游戏启动链路尚未接入">
                    {activeId === instance.id ? "当前实例" : "选择实例"}
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
    </div>
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
}: {
  focus: LoaderId;
  setFocus: (id: LoaderId) => void;
  instances: Instance[];
  activeId: string;
  manifest: VersionManifest | null;
  onNotice: (message: string) => void;
  onOpenExternal: (url: string) => void;
  onCreate: () => void;
}) {
  const active = instances.find((item) => item.id === activeId);
  const [gameVersion, setGameVersion] = useState("");
  const [catalog, setCatalog] = useState<LoaderCatalog | null>(null);
  const [loading, setLoading] = useState(false);
  const [downloading, setDownloading] = useState<string | null>(null);
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
                <button
                  className="ghost-button"
                  disabled={downloading === version || !installerUrl(version)}
                  onClick={() => void downloadInstaller(version)}
                >
                  {downloading === version ? <LoaderCircle className="spin" size={15} /> : <Download size={15} />}
                  下载安装器
                </button>
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
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState("");

  useEffect(() => {
    setProxy(settings.proxy);
    setSakuraEnabled(settings.sakura_enabled);
  }, [settings]);

  const saveKey = async () => {
    setBusy(true);
    try {
      await api.setCurseForgeKey(key);
      setKey("");
      await onRefreshInfo();
      onNotice("CurseForge API Key 已加密保存");
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy(false);
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

  const toggleSakura = async (enabled: boolean) => {
    setSakuraEnabled(enabled);
    await onSave({ ...settings, sakura_enabled: enabled });
    if (enabled) {
      await onRefreshSakura();
    }
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
          <button className="primary-button" onClick={() => void saveKey()} disabled={busy || key.trim().length < 20}>
            {busy ? <LoaderCircle className="spin" size={17} /> : <Check size={17} />} 保存 Key
          </button>
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
          <div className="setting-icon sakura">
            <Sparkles size={22} />
          </div>
          <div>
            <h3>樱花二次元主题</h3>
            <p>从 dmoe.cc 随机图片接口获取背景；默认关闭。</p>
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
        <p className="security-note">
          该接口返回不受控的随机图片，可能包含不适宜内容；请求会将你的 IP 暴露给第三方。背景会加模糊与深色遮罩，加载失败时自动回退默认主题。
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
  const releases = useMemo(
    () => (manifest?.versions ?? []).filter((item) => item.kind === "release").slice(0, 120),
    [manifest],
  );
  const staticLoader = ["optifine", "cleanroom", "labymod", "liteloader"].includes(loader);

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
                  placeholder="可留空，稍后手动导入"
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
