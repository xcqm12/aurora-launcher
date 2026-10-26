import { invoke } from "@tauri-apps/api/core";

export type Instance = {
  id: string;
  name: string;
  minecraft_version: string;
  loader: string;
  loader_version?: string | null;
  directory: string;
  state: string;
  java_path?: string | null;
  memory_mb: number;
  created_at: number;
};

export type LoaderInfo = { id: string; name: string; kind: string; support: string };
export type SearchResult = {
  source: string;
  id: string;
  name: string;
  description: string;
  icon_url?: string;
  downloads: number;
  url: string;
};
export type VersionManifest = {
  latest_release: string;
  latest_snapshot: string;
  versions: { id: string; kind: string; url: string; release_time: string }[];
};
export type ResourceFile = {
  filename: string;
  url: string;
  primary: boolean;
  size: number;
  hashes: Record<string, string>;
};
export type ResourceVersion = {
  id: string;
  name: string;
  version_number: string;
  game_versions: string[];
  loaders: string[];
  files: ResourceFile[];
};
export type CurseForgeFile = {
  id: number;
  mod_id: number;
  display_name: string;
  file_name: string;
  file_length: number;
  download_url?: string;
  release_type: string;
  game_versions: string[];
  mod_loader?: string;
  sha1?: string;
};

export type DiscoverItem = {
  source: string;
  id: string;
  slug: string;
  name: string;
  description: string;
  icon_url?: string | null;
  downloads: number;
  follows: number;
  updated: string;
  categories: string[];
  game_versions: string[];
  loaders: string[];
  url: string;
  kind: string;
};

export type DiscoverPage = {
  items: DiscoverItem[];
  total: number;
  offset: number;
  limit: number;
  source: string;
  note?: string | null;
};

export type LoaderCatalog = {
  id: string;
  name: string;
  game_version: string;
  versions: string[];
  installer_url?: string | null;
  installer_template?: string | null;
  page: string;
  note: string;
};

export type AppSettings = { proxy: string; sakura_enabled: boolean };
export type ProxyTest = { ok: boolean; status: number; latency_ms: number; message: string };
export type SakuraBackground = { data_url: string; cached: boolean };
export type AppInfo = {
  name: string;
  version: string;
  data_dir: string;
  curseforge_configured: boolean;
  curseforge_mode: string;
  region_locked: boolean;
};

export const api = {
  info: () => invoke<AppInfo>("app_info"),
  instances: () => invoke<Instance[]>("list_instances"),
  loaders: () => invoke<LoaderInfo[]>("list_loaders"),
  java: () => invoke<{ path: string; version: string; vendor: string }[]>("detect_java"),
  versions: () => invoke<VersionManifest>("minecraft_manifest"),
  settings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) => invoke<void>("save_settings", { settings }),
  testProxy: (proxy: string) => invoke<ProxyTest>("test_proxy", { proxy }),
  sakura: (refresh = false) => invoke<SakuraBackground>("fetch_sakura_background", { refresh }),
  createInstance: (payload: {
    name: string;
    minecraftVersion: string;
    loader: string;
    loaderVersion?: string;
  }) => invoke<Instance>("create_instance", payload),
  updateInstance: (payload: {
    id: string;
    name?: string;
    loaderVersion?: string;
    javaPath?: string;
    memoryMb?: number;
  }) => invoke<Instance>("update_instance", payload),
  openInstance: (id: string) => invoke<void>("open_instance_folder", { id }),
  deleteInstance: (id: string) => invoke<void>("delete_instance", { id }),
  openExternal: (url: string) => invoke<void>("open_external", { url }),
  discover: (payload: {
    kind: string;
    source?: string;
    query?: string;
    gameVersion?: string;
    loader?: string;
    sort?: string;
    offset?: number;
    limit?: number;
  }) => invoke<DiscoverPage>("discover_resources", payload),
  modrinth: (query: string, projectType?: string, gameVersion?: string) =>
    invoke<SearchResult[]>("search_modrinth", { query, projectType, gameVersion }),
  curseforge: (query: string, gameVersion?: string, classId?: number) =>
    invoke<SearchResult[]>("search_curseforge", { query, gameVersion, classId }),
  curseforgeFiles: (modId: number, gameVersion?: string, modLoaderType?: number) =>
    invoke<CurseForgeFile[]>("curseforge_files", { modId, gameVersion, modLoaderType }),
  installCurseForgeFile: (payload: { instanceId: string; category: string; file: CurseForgeFile }) =>
    invoke<string>("install_curseforge_file", payload),
  downloadCurseForgeFile: (payload: { instanceId: string; file: CurseForgeFile }) =>
    invoke<string>("download_curseforge_file", payload),
  installModrinthFile: (payload: { instanceId: string; category: string; file: ResourceFile }) =>
    invoke<string>("install_modrinth_file", payload),
  downloadModrinthFile: (payload: { instanceId: string; file: ResourceFile }) =>
    invoke<string>("download_modrinth_file", payload),
  modrinthVersions: (projectId: string, gameVersion?: string, loader?: string) =>
    invoke<ResourceVersion[]>("modrinth_versions", { projectId, gameVersion, loader }),
  loaderVersions: (id: string, gameVersion: string) =>
    invoke<LoaderCatalog>("loader_versions", { id, gameVersion }),
  downloadLoaderInstaller: (instanceId: string, url: string) =>
    invoke<string>("download_loader_installer", { instanceId, url }),
  setCurseForgeKey: (key: string) => invoke<void>("set_curseforge_key", { key }),
  clearCurseForgeKey: () => invoke<void>("clear_curseforge_key"),
};
