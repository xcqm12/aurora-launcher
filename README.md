# Aurora Launcher

Windows 优先的 Minecraft 启动器，Rust + Tauri 2 + React 19。0.3.0 完成了 PCL 风格资源中心；0.4.0 补上真正的游戏启动、加载器自动安装、微软/离线账户、整合包导入导出、镜像下载、签名与锁区网关模板。

## 0.4.1 更新内容

- 微软登录对齐 PCL2 / HMCL：使用 Minecraft 令牌的真实 `expires_in`（约 24h）而不是微软令牌的 1h；新增 `/entitlements/mcstore` 所有权检查与档案 404 的许可证判断。
- 补全 XSTS `XErr` 映射（封禁、无 Xbox 档案、地区不支持、儿童账户），校验 Xbox 与 XSTS 的 `uhs` 一致；MC 登录 429/403/503 与封禁有明确提示。
- MC access token 改为 DPAPI 加密后落盘（离线 token 除外）；刷新令牌失效时返回“请重新登录”，不静默成功。
- 设备码 UX 对齐 PCL2：自动打开浏览器、复制代码、按 `interval` 自动轮询、处理 `slow_down`、过期/拒绝停止；保留手动检查。
- 构建时可从 `AURORA_MS_CLIENT_ID` 注入 Client ID（GitHub Secret），运行时设置仍可覆盖；不内置任何第三方 Client ID。
- Worker 增加 `PATH_PREFIX`，`wrangler.toml` 预置 `qlmtest.dpdns.org/aurora/*` 路由与 v0.4.1 资产源；尚未部署。

## 0.4.0 更新内容

- **游戏启动**：解析版本 JSON（含 `inheritsFrom` 继承）、下载 client jar/libraries/asset index/objects、解压 natives、构建 classpath 与 JVM/游戏参数、选择 Java、启动进程、实时日志与停止。
- **镜像下载**：设置中可选 Mojang 官方、BMCLAPI 国内镜像或自动回退；BMCLAPI 做主机映射并按低并发请求，避免触发限流。
- **共享游戏目录**：可指定现有 `.minecraft`/`.xmcl` 作为只读 assets/libraries/vanilla 版本来源；实例的 mods、saves、config、logs 仍独立。
- **加载器自动安装**：Fabric、Legacy Fabric、Forge、NeoForge、Cleanroom 下载官方安装器并无头执行；安装后自动记录生成版本 id。OptiFine、LabyMod、LiteLoader 保留官方页面与手动导入。
- **账户**：离线账户（Java 一致的离线 UUID）与微软设备码登录（MS → Xbox Live → XSTS → Minecraft → profile），刷新令牌用 DPAPI 加密保存。
- **整合包/世界**：导入 `.mrpack`（校验 sha1/sha512、安全路径、overrides/client-overrides）与 CurseForge zip（需有效 API Key）；导入世界 zip 到 `saves`；导出 `.mrpack`。
- **代码签名**：`scripts/sign-windows.ps1` 支持 PFX 或证书存储 thumbprint；CI 在配置 Secrets 时自动签名，未配置则跳过。
- **锁区网关模板**：`server/region-gate/` 提供 Cloudflare Worker，按 `CF-IPCountry` 拦截并流式代理发布资产。

## 0.3.0 更新内容

- PCL 风格资源浏览器、热门预加载、来源/排序/版本/加载器筛选、分页、文件抽屉、收藏。
- 多版本实例隔离、代理设置、樱花主题、滚动优化、双发行版构建与自动发布。

## 已实现

- Vanilla 1.20.1 / 1.21.1 及 Fabric 1.20.1 的实际启动测试（本机现有游戏文件 + Java 17/21）。
- 每个实例独立 `instance.json`、`game/versions|libraries|assets|natives`、`mods`、`resourcepacks`、`shaderpacks`、`saves`、`datapacks`、`config`、`logs`、`downloads`。
- 启动失败会返回具体阶段（Java、版本 JSON、下载、natives、参数、进程），日志通过 Tauri 事件推送到控制台面板。
- Modrinth / CurseForge 资源搜索与校验安装；CurseForge 密钥 DPAPI 保存，不进入日志和前端。

## 尚未实现 / 限制

- **微软登录需要你自己的 Azure 公共客户端 Client ID**。不存在可合法复用的公共 ID；设置页填写，未填写时只支持离线账户。
- OptiFine、LabyMod、LiteLoader 没有稳定无头安装接口，继续手动导入；不伪装自动安装。
- 服务端锁区只交付 Worker 模板，未部署；GitHub Release 资产仍公开，真正强制需要把资产放到 Worker 后面的源站。
- 本机签名使用自签名证书，只在本机信任；公开信任需要 CA 证书或云签名服务。
- CurseForge 导入/搜索需要 `console.curseforge.com` 的有效 API Key；网站 PAT 会 403。
- 1.21.1 的本地 asset index 与 JSON 声明不一致时，严格校验会重新下载；这是预期行为。

## 启动原理

1. 从实例 `launch_version_id`（或 Minecraft 版本）读取版本 JSON，递归合并父版本。
2. 解析 Java 版本要求，按 实例指定 → JAVA_HOME → 常见 JDK 目录 → PATH 选择可用 Java。
3. 校验/下载 client jar、libraries（Windows rules、natives classifier）、asset index、objects、log4j 配置。
4. 解压 natives 到 `game/natives/<version>`。
5. 替换 `${auth_player_name}`、`${classpath}`、`${natives_directory}` 等占位符，组装参数。
6. 以实例根为工作目录启动 Java，stdout/stderr 写入日志并推送事件。

## 账户

- 离线：用户名 1-16 位字母数字/下划线；UUID 使用 `OfflinePlayer:<name>` 的 MD5 并设置版本/变体位。
- 微软：Azure 应用注册选择“移动和桌面应用”，启用“允许公共客户端流”，加入 Minecraft API 权限；在设置页填 Client ID 后点击微软登录，按提示访问 `microsoft.com/link` 并输入设备码。
- 刷新令牌 DPAPI 加密存于 `%APPDATA%\com.aurora.launcher\accounts.json`；访问令牌仅内存。

## 镜像与代理

- 下载源：`official`、`bmclapi`、`auto`。BMCLAPI 使用 `bmclapi2.bangbang93.com`，映射 `/assets`、`/maven`、`/version/{id}/client` 等路径；短窗口约 10 次请求限制，故并发降为 2。
- 代理可选，留空直连；设置中的代理会用于所有 API、资源与游戏文件下载。

## 签名

本地：

```powershell
powershell -File scripts/sign-windows.ps1 -Path target/release/aurora-launcher.exe -Thumbprint <证书指纹> -TimestampUrl ""
```

CI：配置 `WINDOWS_CERT_PFX_BASE64` 与 `WINDOWS_CERT_PASSWORD` Secrets 后自动签名 EXE 与 NSIS；未配置时构建仍成功但未签名。

## 锁区网关

见 `server/region-gate/README.md`。核心事实：GitHub Release 不能按地区阻止下载；必须把资产放到你的源站，并只发布 Worker URL。客户端 `cn-restricted` 检查只是提示，可被绕过。

## 构建与发布

```powershell
npm install
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run bundle:standard
npm run bundle:cn
```

发布：

```powershell
git tag v0.4.1
git push origin v0.4.1
```

CI 会构建 standard 与 cn-restricted、可选签名、生成 SHA256SUMS 并创建 GitHub Release。

## 已知错误与处理记录

1. CurseForge 403：网站 PAT 不是 API Key；需 console.curseforge.com 生成。已暴露令牌应撤销。
2. crates.io git 索引 `RPC failed / early EOF`：改用稀疏镜像 + 合并本机 crate 缓存离线构建。
3. PowerShell 缺少 `ProtectedData`：改用 `CryptProtectData` P/Invoke，已验证 Rust 可解密。
4. Tauri snake_case/camelCase：前端统一 camelCase 参数。
5. Modrinth 路由差异：使用 `project/{id}/version` 单数路径。
6. CI `generate_context!` 找不到 `dist`：必须先构建前端再跑 cargo test。
7. `spawnSync npm.cmd` EINVAL：构建脚本直接调用 tsc/vite 的 node 入口。
8. GitHub 直连间歇超时：重试推送；未做 IP 扫描。
9. 本机 PATH 默认 Java 27 不适合 MC：启动器按版本要求选择 Java 17/21，可在实例中指定。
10. Fabric 自动安装验证：生成 `fabric-loader-0.16.0-1.20.1` 并成功启动。
11. BMCLAPI 是重定向/签名层且 JSON 内仍是 Mojang URL：必须由客户端映射主机，不能缓存签名 URL。
12. OptiFine 下载依赖广告页 token、LabyMod 对非浏览器返回 403、LiteLoader 已停更：不做自动安装。
13. 微软没有第三方可用公共 Client ID：必须由使用者注册 Azure 应用。

## 发布

- 仓库：<https://github.com/xcqm12/aurora-launcher>
- 最新 Release：<https://github.com/xcqm12/aurora-launcher/releases/tag/v0.4.1>

## 微软登录（对齐 PCL2 / HMCL）

流程与 PCL2（设备码）和 HMCL（设备码/授权码）一致：`consumers` 租户 + `XboxLive.signin offline_access`，设备码 → Xbox Live → XSTS → Minecraft → 所有权 → 档案。

对齐后的行为：

- 过期时间使用 Minecraft 令牌的 `expires_in`（约 24 小时，提前 30 秒视为过期），不再用微软令牌的 1 小时。
- 调用 `/entitlements/mcstore` 检查所有权；档案 404 时再查 `/entitlements/license` 区分“有游戏无档案”与“无许可证”。
- XSTS `XErr` 完整映射：封禁 / 无 Xbox 档案 / 地区不支持 / 儿童账户；校验 Xbox 与 XSTS 的 `uhs` 一致。
- MC 登录 429/403/503 与 `ACCOUNT_SUSPENDED` 有明确提示；刷新失败返回“凭据已过期，请重新登录”。
- 设备码自动打开浏览器并复制代码，按 `interval` 自动轮询，处理 `slow_down`（+5s）、过期、拒绝；保留手动“立即检查”。
- MC access token 与刷新令牌均 DPAPI 加密落盘；日志不打印任何令牌。

Client ID 配置（三选一，均不内置第三方 ID）：

1. 设置页填写（保存到 settings.json）。
2. 环境变量 `AURORA_MS_CLIENT_ID`。
3. 构建时注入：GitHub Secret `AURORA_MS_CLIENT_ID`，CI 传入 `--env`，编译进二进制（`option_env!`）。

Azure 注册要点：应用类型选“移动和桌面应用”，启用“允许公共客户端流”，不填客户端密钥；在 API 权限中加入 Minecraft 所需权限（新应用需向 Microsoft 申请）。

## Cloudflare 锁区网关（qlmtest.dpdns.org）

- 域名已托管在 Cloudflare（NS：fred/magali.ns.cloudflare.com），是独立 zone，不是 dpdns.org 的子记录。
- `server/region-gate/` 预置了 `qlmtest.dpdns.org/aurora/*` 路由与 v0.4.1 Release 资产回源，`PATH_PREFIX=/aurora`。
- 本地已验证：CN 请求 403，其他地区 200 且回源到正确的 Release 资产 URL。
- **尚未部署**：需要 Cloudflare 凭据。你提供的 32 位 Key 是 Global API Key（需配邮箱、全账号权限，不推荐），建议在 Cloudflare 面板创建 Scoped Token（Workers Scripts:Edit、Zone Workers Routes:Edit、DNS:Edit、Zone:Read，范围限 qlmtest.dpdns.org），然后：

```powershell
$env:CLOUDFLARE_API_TOKEN = "<token>"
npx wrangler login   # 或直接用上面的 token
npx wrangler deploy
```

- 部署后，锁区下载链接为 `https://qlmtest.dpdns.org/aurora/<文件名>`；GitHub Release 原链接仍公开，真正强制必须只发网关链接。
- 若使用 Global API Key，需同时提供账号邮箱（`CLOUDFLARE_EMAIL`），并自行承担全账号权限风险；用后建议在面板轮换该 Key。
