# Aurora Launcher

Windows 优先的 Minecraft 启动器，Rust + Tauri 2 + React 19。0.3.0 完成了 PCL 风格资源中心；0.4.0 补上真正的游戏启动、加载器自动安装、微软/离线账户、整合包导入导出、镜像下载、签名与锁区网关模板；0.4.2 按 lmaml 教程对齐设备码流程细节；0.4.3 增加 Client ID 自检与 Mojang 应用审核指南；0.4.4 接入 LittleSkin（Yggdrasil + authlib-injector）登录；0.4.5 增加 LittleSkin OAuth 授权登录并消除子进程控制台窗口。

## 0.4.5 更新内容

- 新增 **LittleSkin OAuth 授权登录**（设备代码流，客户端 ID `800`）：浏览器授权并选择角色，再用 `POST https://littleskin.cn/api/yggdrasil/authserver/oauth` 换取 Minecraft 令牌；全程不需要密码，也不需要客户端 Secret。
- 授权失败给出可执行原因：应用未加入设备代码流白名单时提示把回调设为 `https://open.littleskin.cn/oauth/callback` 并发送工单；scope 冲突时提示 `Read` 与 `Select` 不能同时申请。
- 保留原有的 LittleSkin 账号密码（传统 Yggdrasil）登录作为备选。
- **控制台窗口**：Java 版本探测、加载器安装器、`taskkill` 等所有子进程统一使用 `CREATE_NO_WINDOW`；启动游戏或安装加载器时不再闪出 cmd 窗口。发布版主程序本身保持 `windows` 子系统。
- 不把图中客户端 Secret 写入仓库或二进制：设备代码流不需要它；该 Secret 已出现在对话中，建议在 LittleSkin 应用管理页重置。

## 0.4.4 更新内容

- 新增 **LittleSkin 登录**：邮箱/密码走 LittleSkin 的 Yggdrasil `authserver/authenticate`，令牌保存在本机，支持自动刷新与启动前校验；密码只发送给 littleskin.cn，不保存、不写日志。
- 启动时注入 `authlib-injector`：`-javaagent` 与预取的 Yggdrasil 元数据（Base64）一起传给 JVM，皮肤与角色来自 LittleSkin；注入器从官方 `authlib-injector.yushi.moe` 下载并按接口给出的 SHA-256 校验，缓存在应用数据目录。
- 多角色账户在设置页弹出角色选择；无角色时提示先到 littleskin.cn 创建。
- 账户列表区分“微软正版 / LittleSkin / 离线”；外置登录使用 `msa` 用户类型与 Yggdrasil 令牌，不再要求微软 Client ID。
- 启动参数中的启动器版本号改为读取 `CARGO_PKG_VERSION`，不再硬编码 0.4.1。

## 0.4.3 更新内容

- 设置页新增“验证”按钮：检查 Client ID 格式是否为 GUID，并用它请求一次设备码，提前发现应用不存在、账户类型错误、未启用公共客户端流等问题（不能替代 Mojang 审核）。
- 设置页新增“Azure 应用注册”“Minecraft 应用审核”直达按钮，说明文字改为 Web 平台重定向 + 公共客户端流 + 审核要求。
- 新增 `docs/mce-review-application.md`：申请材料清单、英文申请模板、常见拒绝原因，以及为什么不能借用 PCL2/HMCL 等他人已审核的 Client ID。
- 明确一条产品边界：不内置、不提取、不冒用任何第三方 Azure Client ID。

## 0.4.2 更新内容

- 设备码请求追加 `?mkt=zh-CN` 与 `Accept-Language: zh-CN`，错误描述由微软本地化返回。
- 设备码失败按 AADSTS 码给出修复建议：700016（应用不存在/账户类型不符）、7000218/7000215（未启用公共客户端流）、700005（权限不足）、404（未过 Minecraft 应用审核，附 aka.ms/mce-reviewappid）。
- 轮询状态新增 `terminal` 标记：除 `pending`/`slow_down` 外全部停止自动轮询；`expired_token`、`bad_verification_code`、`invalid_client` 有独立状态与中文提示，不再反复轮询已消耗的设备码。
- 无所有权/无许可证返回 `no_license`，设置页显示“打开 Minecraft 商店”按钮；支持 `verification_uri_complete` 直接打开带代码的登录页。
- README 补充 Azure 注册要点与失败排查表。

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
- LittleSkin（Yggdrasil + authlib-injector）登录：账户/密码验证、令牌刷新、多角色选择、启动注入与注入器校验下载。

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
- 微软：Azure 应用注册选择包含个人 Microsoft 账户的账户类型，添加 Web 平台重定向 `https://www.microsoft.com/link` 并启用“允许公共客户端流”；不需要在门户添加 API 权限。设置页填 Client ID 后点微软登录，按提示访问 `microsoft.com/link` 并输入设备码。
- LittleSkin：设置页填写 LittleSkin 邮箱/用户名和密码，通过 Yggdrasil 认证；多角色账户会要求选择角色。启动时自动下载并用 `-javaagent` 注入 authlib-injector，令牌过期会先刷新再启动。
- 刷新令牌、Minecraft 访问令牌与 Yggdrasil 令牌均 DPAPI 加密存于 `%APPDATA%\com.aurora.launcher\accounts.json`；密码从不落盘。

## 镜像与代理

- 下载源：`official`、`bmclapi`、`auto`。BMCLAPI 使用 `bmclapi2.bangbang93.com`，映射 `/assets`、`/maven`、`/version/{id}/client` 等路径；短窗口约 10 次请求限制，故并发降为 2。
- 代理可选，留空直连；设置中的代理会用于所有 API、资源与游戏文件下载。

## 签名

本机使用自签名代码签名证书 `CN=SevenZeroMeowTeam`（Code Signing EKU，到期 2027-10-22）：私钥在 `Cert:\CurrentUser\My`，公钥已导入本机（LocalMachine）的“受信任根证书颁发机构”和“受信任发布者”，因此在这台电脑上签名状态为“有效”。

- 新机器或更换证书后，以管理员身份导入信任（脚本可重复执行）：

```powershell
powershell -File scripts/trust-codesign-cert.ps1 -Thumbprint <证书指纹>
```

- 签名使用 SHA-256 摘要与 RFC3161 时间戳，签完自动执行 `signtool verify /pa`：

```powershell
powershell -File scripts/sign-windows.ps1 -Path <exe 列表> -Thumbprint <证书指纹>
```

- 时间戳服务不可达时加 `-TimestampUrl ""`；脚本会自动查找 Windows SDK 的 signtool，找不到会直接报错。
- 复核签名与哈希：

```powershell
Get-AuthenticodeSignature <exe> | Format-List Status, SignerCertificate
sha256sum -c SHA256SUMS.txt
```

现实限制：

- 自签名证书只在安装了该证书的电脑上显示可信，其他电脑仍会提示“未知发布者”，也不会积累 SmartScreen 信誉；面向公众分发需要购买 OV/EV 代码签名证书。
- 仓库未配置 `WINDOWS_CERT_PFX_BASE64` / `WINDOWS_CERT_PASSWORD`，CI 产物默认未签名。发布流程是：本地构建 standard/cn → 签名 → `gh release upload --clobber` 覆盖 CI 资产 → 重新生成并上传 `SHA256SUMS.txt`。
- Tauri 的 NSIS 卸载器在安装时生成，当前签名覆盖安装包与主程序，卸载器没有签名。

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
- 最新 Release：<https://github.com/xcqm12/aurora-launcher/releases/tag/v0.4.5>

## 微软登录（对齐 PCL2 / HMCL / lmaml 教程）

流程与 PCL2（设备码）、HMCL（设备码/授权码）以及 lmaml 教程第二章一致：`consumers` 租户 + `XboxLive.signin offline_access`，设备码 → Xbox Live → XSTS → Minecraft → 所有权 → 档案。

对齐后的行为：

- 过期时间使用 Minecraft 令牌的 `expires_in`（约 24 小时，提前 30 秒视为过期），不再用微软令牌的 1 小时。
- 调用 `/entitlements/mcstore` 检查所有权；档案 404 时再查 `/entitlements/license` 区分“有游戏无档案”与“无许可证”。
- XSTS `XErr` 完整映射：封禁 / 无 Xbox 档案 / 地区不支持 / 儿童账户；校验 Xbox 与 XSTS 的 `uhs` 一致。
- MC 登录 429/403/503 与 `ACCOUNT_SUSPENDED` 有明确提示；刷新失败返回“凭据已过期，请重新登录”。
- 设备码请求带 `mkt=zh-CN`；自动打开浏览器并复制代码，按 `interval` 自动轮询，`slow_down` 加 5 秒，过期/拒绝/设备码复用立即停止。
- 无许可证时返回 `no_license`，界面提供“打开 Minecraft 商店”；有游戏但无档案时提示先创建档案。
- MC access token 与刷新令牌均 DPAPI 加密落盘；日志不打印任何令牌。

Client ID 配置（三选一，均不内置第三方 ID）：

1. 设置页填写（保存到 settings.json）。
2. 环境变量 `AURORA_MS_CLIENT_ID`。
3. 构建时注入：GitHub Secret `AURORA_MS_CLIENT_ID`，CI 传入 `--env`，编译进二进制（`option_env!`）。

Azure 注册要点（按教程顺序）：

1. 账户类型选“任何组织目录中的账户和个人 Microsoft 账户”。只选组织账户会在 `consumers` 端点返回 `AADSTS700016`。
2. “身份验证”里添加平台 **Web**，重定向 URI 填 `https://www.microsoft.com/link`；打开“允许公共客户端流”。不要创建客户端密钥。
3. “品牌与属性”中应用名称、徽标、说明为必填，否则可能注册失败。
4. **不需要**在“API 权限”里添加任何权限。`XboxLive.signin` 属于 Xbox Live 服务（`https://xboxlive.com`），`offline_access` 是 OIDC 标准 scope，两者都在登录请求的 `scope` 参数里动态申请；门户的“添加权限”列表里搜不到它们是正常的，不是配置遗漏。不要为了“凑权限”去加 Microsoft Graph 的 `User.Read` 等无关项。
5. 未通过审核的 Client ID 可能直接返回 404，可在 <https://aka.ms/mce-reviewappid> 提交应用审核。
6. 公开分发的启动器不要把 Client ID 硬编码进仓库，用上面的 Secret 注入方式。

关于 `scope` 与门户权限的区别：

- Azure 门户的“API 权限”只列出暴露给第三方的资源权限（主要是 Microsoft Graph）。Xbox Live 不在其中，所以搜不到 `XboxLive.signin` 是设计如此，不是漏配。
- 代码在设备码请求里写 `scope=XboxLive.signin offline_access`（`src-tauri/src/auth.rs`），AAD 按请求里的资源动态授权；首次登录的同意页会显示 Xbox Live 访问，以及 `offline_access` 对应的“维护你已授权数据的访问权限”，后者用于拿刷新令牌。
- 不需要管理员同意：`consumers` 租户下都是个人账户，没有管理员角色。

登录失败排查：

| 现象 | 原因与处理 |
| --- | --- |
| 设置页提示“未配置微软 Client ID” | 三处都为空：设置页、`AURORA_MS_CLIENT_ID`、构建注入。 |
| `AADSTS700016` | Client ID 拼错，或应用账户类型不含个人账户。 |
| `AADSTS7000218` / `7000215` | 未启用“允许公共客户端流”，或缺少 Web 平台。 |
| 添加权限里搜不到 `XboxLive.signin` / `offline_access` | 正常：它们是运行时 scope，不是门户权限，不需要添加（见下方说明）。 |
| HTTP 404 | Client ID 未过 Minecraft 应用审核，去 aka.ms/mce-reviewappid 申请。 |
| `bad_verification_code` | 设备码已被使用或换了 Client ID，重新点击登录。 |
| `no_license` | 账户没有 Java 版所有权或 Game Pass 过期，界面按钮可跳转商店。 |
| `no_profile` | 有游戏但没有 Java 档案，先到 minecraft.net 创建用户名。 |
| `XErr 2148916233` | 微软账户没有 Xbox 档案，先到 xbox.com 创建。 |

### Mojang 应用审核

未获 Mojang 批准的 Client ID，即使微软登录成功，也可能在 `login_with_xbox` 或档案查询阶段被拒（社区报告为 403 `Invalid app registration` / 404）。官方入口：<https://aka.ms/AppRegInfo>（当前跳转 Mojang 帮助文章），旧表单：<https://aka.ms/mce-reviewappid>。

- 申请材料、英文模板与常见拒绝原因见 [docs/mce-review-application.md](docs/mce-review-application.md)。
- 设置页“验证”只检查 Client ID 是否能走个人账户设备码流程，**不代表**已过 Minecraft 审核；审核结果只有在完整登录拉取档案时才体现。
- 不内置、不提取、不冒用 PCL2/HMCL 等他人已审核的 Client ID：Client ID 是应用身份，冒用违反微软条款，也可能连累对方应用被撤销。

## LittleSkin 外置登录

LittleSkin 是 Yggdrasil（authlib-injector）体系，和微软登录互不影响：

- API 根：`https://littleskin.cn/api/yggdrasil/`。启动器调用 `authserver/authenticate`、`authserver/refresh`、`authserver/validate`，并读取该根的元数据（`meta.serverName`、`skinDomains`、签名公钥）。
- 启动时 JVM 参数：
  - `-javaagent:<缓存目录>\authlib-injector-<版本>.jar=https://littleskin.cn/api/yggdrasil/`
  - `-Dauthlibinjector.yggdrasil.prefetched=<元数据的 Base64>`
  注入器第一次使用会从 `https://authlib-injector.yushi.moe/artifact/latest.json` 获取版本、下载地址和 SHA-256，校验后缓存到 `%APPDATA%\com.aurora.launcher\authlib\`；接口不可达时使用已有缓存。
- 安全：密码只通过 HTTPS 发送给 littleskin.cn，不保存、不写日志；Yggdrasil 令牌与 `clientToken` 保存在 `accounts.json`，访问令牌 DPAPI 加密。启动前先 `validate`，失效则 `refresh`，失败会提示重新登录。
- 多角色：`authenticate` 未返回 `selectedProfile` 且有多个角色时，设置页显示角色下拉框；待选会话 10 分钟过期，不会把令牌传给前端。
- 实测：用 Java 21 加载 `authlib-injector 1.2.8` 与真实 LittleSkin 元数据，代理正确识别认证服务器；登录成功与否需要你自己的 LittleSkin 账户验证。

### LittleSkin OAuth 授权登录（设备代码流）

设置页「授权登录（推荐）」使用 LittleSkin 的 OAuth 设备代码流，不收集密码：

1. `POST https://open.littleskin.cn/oauth/device_code`，`client_id=800`，scope 为 `openid Yggdrasil.PlayerProfiles.Select Yggdrasil.MinecraftToken.Create`（`Read` 与 `Select` 互斥，不能同时申请）。
2. 浏览器打开授权页并输入授权码，在页面上选择角色；启动器每 `interval` 秒轮询 `POST https://open.littleskin.cn/oauth/token`（`grant_type=urn:ietf:params:oauth:grant-type:device_code`）。
3. 从返回的 `id_token` 读取 `selectedProfile`，再带 OAuth 访问令牌调用 `POST https://littleskin.cn/api/yggdrasil/authserver/oauth`（body `{"uuid":"<角色 id>"}`），拿到与 Yggdrasil 登录相同结构的 `accessToken` / `clientToken`。
4. 令牌保存方式与密码登录一致，启动时照常 `validate`/`refresh` 并注入 authlib-injector。

**使用前需要在 LittleSkin 配置（否则会提示 `invalid_client`）**：

- 在 <https://littleskin.cn/user/oauth/manage> 把 OAuth 应用（客户端 ID 800）的回调 URL 改为 `https://open.littleskin.cn/oauth/callback`。
- 用绑定邮箱向 `support@littlesk.in` 发工单，标题「申请 OAuth 设备代码流白名单」，正文写应用名称、客户端 ID 800 和 Yggdrasil 权限列表。初次通过通常是测试模式，仅应用创建者能授权；正式使用需再次申请解除。
- 设备代码流**不需要客户端 Secret**，所以本项目不内置 Secret。图中的 Secret 已出现在对话里，建议在应用管理页重置。

限制：界面只提供 LittleSkin 预设，后端 `normalize_server` 支持其他 https Yggdrasil 服务器；微软正版账户与外置登录账户是两个独立账户类型，不能混用。

## Cloudflare 锁区网关（qlmtest.dpdns.org）

- 域名已托管在 Cloudflare（NS：fred/magali.ns.cloudflare.com），是独立 zone，不是 dpdns.org 的子记录。
- `server/region-gate/` 预置了 `qlmtest.dpdns.org/aurora/*` 路由与 v0.4.5 Release 资产回源，`PATH_PREFIX=/aurora`。
- 本地已验证：CN 请求 403，其他地区 200 且回源到正确的 Release 资产 URL。
- **尚未部署**：需要 Cloudflare 凭据。你提供的 32 位 Key 是 Global API Key（需配邮箱、全账号权限，不推荐），建议在 Cloudflare 面板创建 Scoped Token（Workers Scripts:Edit、Zone Workers Routes:Edit、DNS:Edit、Zone:Read，范围限 qlmtest.dpdns.org），然后：

```powershell
$env:CLOUDFLARE_API_TOKEN = "<token>"
npx wrangler login   # 或直接用上面的 token
npx wrangler deploy
```

- 部署后，锁区下载链接为 `https://qlmtest.dpdns.org/aurora/<文件名>`；GitHub Release 原链接仍公开，真正强制必须只发网关链接。
- 若使用 Global API Key，需同时提供账号邮箱（`CLOUDFLARE_EMAIL`），并自行承担全账号权限风险；用后建议在面板轮换该 Key。
