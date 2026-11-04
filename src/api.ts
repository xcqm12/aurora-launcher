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
  launch_version_id?: string | null;
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

export type AppSettings = {
  proxy: string;
  sakura_enabled: boolean;
  shared_game_dir: string;
  download_source: string;
  ms_client_id: string;
  active_account: string;
  mp_peers?: string;
  instances_dir?: string;
  data_dir?: string;
  sakura_source?: string;
};
export type ProxyTest = { ok: boolean; status: number; latency_ms: number; message: string };
export type SakuraBackground = { data_url: string; cached: boolean };
export type AccountView = {
  id: string;
  kind: string;
  name: string;
  uuid: string;
  active: boolean;
  server?: string;
};
export type DeviceCode = {
  device_code: string;
  user_code: string;
  verification_uri: string;
  verification_uri_complete?: string | null;
  expires_in: number;
  interval: number;
  message: string;
};
export type PollResult = {
  status: string;
  account?: AccountView | null;
  message: string;
  terminal: boolean;
  hint?: string | null;
};
export type ClientIdCheck = { ok: boolean; message: string };
export type YggdrasilProfile = { id: string; name: string };
export type YggdrasilLogin = {
  account?: AccountView | null;
  pending_id?: string | null;
  profiles: YggdrasilProfile[];
};
export type LittleSkinOauthDevice = {
  device_code: string;
  user_code: string;
  verification_uri: string;
  verification_uri_complete?: string | null;
  expires_in: number;
  interval: number;
};
export type MpStatus = {
  running: boolean;
  role: string;
  network_name: string;
  room_code: string;
  virtual_ip: string;
  peers_connected: number;
  log_tail: string[];
};
export type AiSettingsView = {
  enabled: boolean;
  base_url: string;
  model: string;
  key_set: boolean;
  name: string;
};
export type AiSettingsInput = {
  enabled: boolean;
  base_url: string;
  model: string;
  key?: string;
  name: string;
};
export type AiReply = { reply: string };
export type LoaderProgressPayload = {
  stage: string;
  filename: string;
  downloaded: number;
  total: number;
};
export type ResourceProgressPayload = {
  filename: string;
  downloaded: number;
  total: number;
};
export type LittleSkinOauthPoll = {
  status: string;
  terminal: boolean;
  message: string;
  account?: AccountView | null;
};
export type LoaderInstallResult = { version_id: string; installer: string; log: string };

export type ContentKind =
  | "mod"
  | "shader"
  | "resourcepack"
  | "datapack"
  | "save"
  | "screenshot"
  | "schematic"
  | "server";

export type ContentItem = {
  name: string;
  is_dir: boolean;
  size: number;
  modified: number;
  enabled?: boolean | null;
  level_name?: string | null;
  last_played?: number | null;
};

export type ContentSummary = {
  mod_count: number;
  shader_count: number;
  resourcepack_count: number;
  save_count: number;
  screenshot_count: number;
  schematic_count: number;
  server_count: number;
};

export type ServerEntry = {
  name: string;
  ip: string;
  hidden: boolean;
  icon_data_url?: string | null;
};

export type ServerInput = {
  name: string;
  ip: string;
  hidden: boolean;
  icon_base64?: string | null;
};

export type ConfigEntry = { path: string; size: number; modified: number };
export type ConfigFile = { path: string; content: string };

export type ChangelogEntry = {
  version: string;
  current: boolean;
  history: boolean;
  items: string[];
};
export type Changelog = { version: string; entries: ChangelogEntry[] };

export type ModMeta = {
  title?: string | null;
  version?: string | null;
  mod_id?: string | null;
  loader?: string | null;
  description?: string | null;
};
export type ImportResult = {
  kind: string;
  name: string;
  files: number;
  overrides: number;
  loader?: string | null;
  loader_version?: string | null;
  minecraft?: string | null;
};
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
  launchInstance: (instanceId: string) => invoke<string>("launch_instance", { instanceId }),
  killInstance: (instanceId: string) => invoke<void>("kill_instance", { instanceId }),
  launchStatus: (instanceId: string) => invoke<string>("get_launch_status", { instanceId }),
  instanceLog: (instanceId: string) => invoke<string[]>("get_instance_log", { instanceId }),
  installLoader: (instanceId: string, loader: string, version: string) =>
    invoke<LoaderInstallResult>("install_loader", { instanceId, loader, version }),
  importPack: (instanceId: string, path: string) =>
    invoke<ImportResult>("import_instance_pack", { instanceId, path }),
  importWorld: (instanceId: string, path: string) =>
    invoke<string>("import_instance_world", { instanceId, path }),
  exportPack: (instanceId: string, path: string) =>
    invoke<string>("export_instance_pack", { instanceId, path }),
  pickPackFile: () => invoke<string | null>("pick_pack_file"),
  pickWorldFile: () => invoke<string | null>("pick_world_file"),
  pickExportFile: (defaultName: string) => invoke<string | null>("pick_export_file", { defaultName }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  pickJavaFile: () => invoke<string | null>("pick_java_file"),
  accounts: () => invoke<AccountView[]>("list_accounts"),
  addOfflineAccount: (name: string) => invoke<AccountView>("add_offline_account", { name }),
  setActiveAccount: (id: string) => invoke<void>("set_active_account", { id }),
  removeAccount: (id: string) => invoke<void>("remove_account", { id }),
  msStartLogin: () => invoke<DeviceCode>("ms_start_login"),
  msPollLogin: (deviceCode: string) => invoke<PollResult>("ms_poll_login", { deviceCode }),
  msCheckClientId: (clientId: string) => invoke<ClientIdCheck>("ms_check_client_id", { clientId }),
  yggdrasilLogin: (server: string, username: string, password: string) =>
    invoke<YggdrasilLogin>("yggdrasil_login", { server, username, password }),
  yggdrasilSelect: (pendingId: string, profileId: string) =>
    invoke<AccountView>("yggdrasil_select", { pendingId, profileId }),
  littleskinOauthStart: () => invoke<LittleSkinOauthDevice>("littleskin_oauth_start"),
  aiSettings: () => invoke<AiSettingsView>("ai_get_settings"),
  aiSaveSettings: (settings: AiSettingsInput) =>
    invoke<AiSettingsView>("ai_save_settings", { settings }),
  aiTestChat: (message: string) => invoke<AiReply>("ai_test_chat", { message }),
  aiInstallCompanion: (instanceId: string) =>
    invoke<string>("ai_install_companion", { instanceId }),
  mpHost: () => invoke<MpStatus>("mp_host"),
  mpJoin: (code: string) => invoke<MpStatus>("mp_join", { code }),
  mpStop: () => invoke<MpStatus>("mp_stop"),
  mpStatus: () => invoke<MpStatus>("mp_status"),
  littleskinOauthPoll: (deviceCode: string) =>
    invoke<LittleSkinOauthPoll>("littleskin_oauth_poll", { deviceCode }),
  contentSummary: (instanceId: string) =>
    invoke<ContentSummary>("instance_content_summary", { instanceId }),
  contentList: (instanceId: string, kind: ContentKind) =>
    invoke<ContentItem[]>("instance_content_list", { instanceId, kind }),
  contentDelete: (instanceId: string, kind: ContentKind, name: string) =>
    invoke<void>("instance_content_delete", { instanceId, kind, name }),
  contentOpenFolder: (instanceId: string, kind: ContentKind) =>
    invoke<void>("instance_content_open_folder", { instanceId, kind }),
  contentUpload: (instanceId: string, kind: ContentKind, source: string) =>
    invoke<string>("instance_content_upload", { instanceId, kind, source }),
  pickContentFile: (kind: ContentKind) => invoke<string | null>("pick_content_file", { kind }),
  modToggle: (instanceId: string, name: string, enable: boolean) =>
    invoke<void>("instance_mod_toggle", { instanceId, name, enable }),
  modToggleMany: (instanceId: string, names: string[], enable: boolean) =>
    invoke<number>("instance_mod_toggle_many", { instanceId, names, enable }),
  modMetadata: (instanceId: string) =>
    invoke<Record<string, ModMeta>>("instance_mod_metadata", { instanceId }),
  exportWorld: (instanceId: string, name: string, path: string) =>
    invoke<string>("instance_save_export", { instanceId, name, path }),
  pickSaveExport: (defaultName: string) =>
    invoke<string | null>("pick_save_export", { defaultName }),
  imagePreview: (instanceId: string, kind: ContentKind, name: string) =>
    invoke<string>("instance_image_preview", { instanceId, kind, name }),
  saveOpen: (instanceId: string, name: string) =>
    invoke<void>("instance_save_open", { instanceId, name }),
  serverList: (instanceId: string) => invoke<ServerEntry[]>("instance_server_list", { instanceId }),
  serverSet: (instanceId: string, servers: ServerInput[]) =>
    invoke<void>("instance_server_set", { instanceId, servers }),
  configList: (instanceId: string) => invoke<ConfigEntry[]>("instance_config_list", { instanceId }),
  configRead: (instanceId: string, path: string) =>
    invoke<ConfigFile>("instance_config_read", { instanceId, path }),
  configWrite: (instanceId: string, path: string, content: string) =>
    invoke<void>("instance_config_write", { instanceId, path, content }),
  changelog: () => invoke<Changelog>("app_changelog"),
};
