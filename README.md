# Aurora Launcher

Windows 优先的 Minecraft 启动器，Rust + Tauri 2 + React 19：真正的游戏启动与加载器自动安装、微软正版/LittleSkin/离线登录、整合包导入导出、实例内容管理、镜像加速、EasyTier 联机。历史版本更新记录见 [RELEASE_NOTES.md](RELEASE_NOTES.md)。

## 0.6.5 更新内容

- **修复 Forge/NeoForge/Fabric 启动即退**：继承型版本 JSON（`inheritsFrom`，安装器生成）不再把原版 client jar 放上 classpath。Forge 的 BootstrapLauncher 依据 `--ignoreList` 与模块层自行装配 SRG 客户端，原版 jar 在场会被当成自动模块 `_1._20._1`，与 `minecraft` 模块导出同样的包而报 `Modules minecraft and _1._20._1 export package ...`；原版启动不受影响。

## 0.6.4 更新内容（历史）

- **自定义 MC 镜像源**：下载源新增「自定义镜像」，填镜像站地址即可（BMCLAPI 兼容布局；默认示例为自建镜像 mc.elingfeng.com）。镜像覆盖版本清单/JSON、client jar、assets、libraries、Forge/NeoForge/Fabric maven；NeoForge 自动去掉 `/releases` 前缀，镜像优先、官方兜底。
- 镜像站搭建：nginx `proxy_cache` 磁盘缓存反代官方源 + `sync_forge.py` 每日从 files.minecraftforge.net 同步 Forge 清单与安装器（官方 maven 已下架老版本文件，安装器经 USTC BMCLAPI 节点获取）。
- **安装器卡死修复**：官方安装器内部网络请求无超时，弱网会永远卡在「运行安装器」；现每次尝试限时 900 秒，超时终止并重试，错误信息带回安装器已输出的日志；关闭界面时后台安装器进程一并终止。
- **并发安装互斥**：同一实例同时只允许一个安装任务，重复点击「自动安装」返回明确提示，不再出现两个安装器并发写同一目录。
- **安装器库预置**：Forge/NeoForge/Cleanroom 安装前把 install_profile 声明的全部库经镜像候选（含自定义镜像）预置并做 SHA-1 校验，官方安装器检测到本地文件匹配即跳过其无重试的下载，弱网安装成功率显著提升。

## 0.6.3 更新内容（历史）

- **加载器安装全链路重试**：Forge/NeoForge/Fabric/OptiFine/LiteLoader/Cleanroom 的版本信息、安装器与依赖下载共 8 处裸请求全部接入统一重试助手（每地址最多 3 次，4xx 不重试），配合既有的失败库预下载恢复，网络抖动不再让安装直接失败。
- **OptiFine 安装增强**（参考 PCL-CE 的下载策略）：版本列表优先 BMCLAPI JSON，失败回退官网 HTML 解析；官网抓取带 UA/Referer 且重试。
- **发布产物反逆向加固**：`strip = symbols`、`lto = fat`、`codegen-units = 1`、`panic = abort`——exe 不再含符号与 unwind 表；代码仓库保持 MIT 开源，混淆只作用于编译产物。
- **网络诊断脚本**：`python scripts/network_check.py`（标准库实现）扫描 GitHub 官方 IP 段（本机 62/72 可用）与加载器全部端点；实测发现 BMCLAPI 版本清单在本机 SSL 握手失败——auto 下载源会自动改走官方，这也是此前安装偶发失败的根因之一。

## 0.6.2 更新内容（历史）

- **启动失败修复**：AI 伙伴的 JVM 参数 `-Daurora.ai.port` 与端口号被拆成两个参数，JVM 把端口号当主类名（"找不到或无法加载主类 8123"，exit 1 且无游戏输出）。已合并为单个 `-Dkey=value`；进程异常退出且全程无输出时，日志会给出排查提示。
- **下载自动重试**：加载器安装、游戏文件、资源、EasyTier 下载在传输中途断流时自动重试最多 3 次（4xx 除外），不再直接报"error decoding response body"。
- **樱花主题增强**：默认随机图改为三源池（dmoe.cc / paugram / alcy.cc）随机轮换，单源失败自动换下一个；顶栏新增「换一张」按钮。

## 0.6.1 更新内容（历史）

- **实例内容管理**：实例管理弹窗改为分页布局，新增 8 个内容页，与原有「设置」页并列（标签上显示数量角标）：
  - **模组**：列出 `mods/` 下的 .jar，可一键/批量禁用与启用（重命名为 `.jar.disabled`，文件名大小写保持不变）、按启用状态排序、删除；文件夹也会列出但不能启停。
  - **光影包 / 资源包**：列表、删除、从磁盘导入 `.zip`，并可直接打开对应目录。
  - **存档**：读取每个世界的 `level.dat`，显示世界名、目录名、占用空间与最后游玩时间；支持导入 / 还原备份、逐世界备份为 zip、打开世界目录或删除。
  - **截图**：缩略图墙（分批加载，最多先加载 80 张），点击查看大图，支持单张删除。
  - **原理图**：列出 `.litematic` / `.schem` / `.schematic` / `.nbt`，支持导入与删除。
  - **服务器**：直接读写 `servers.dat`（gzip + NBT），可修改名称/地址、设置隐藏、增删条目并保存；保存前自动备份原文件（`servers.dat.aurora-bak-<时间戳>`，只保留最近 3 份）。
  - **配置**：编辑实例根目录的 `options.txt` 与 `config/` 目录树下的文本文件（txt/json/toml/cfg/properties/yml 等，单个 ≤1 MB），支持搜索与新建；保存时生成 `*.aurora-bak-<时间戳>` 备份。
- **模组元数据**：本地读取 jar 里的 `fabric.mod.json` / `quilt.mod.json` / `META-INF/mods.toml` / `mcmod.info`，显示真实名称、版本与加载器标签；按文件大小与修改时间缓存，不联网。
- **搜索与编辑细节**：各内容页支持关键字过滤（模组可按名称/ID/版本搜索）；配置编辑器在切换文件前提示未保存改动，支持 Ctrl+S 保存；截图预览可用 ←/→ 切换、Esc 关闭。
- **批量与拖拽**：模组页可勾选多个模组批量启用/禁用，提供「启用优先」排序；模组/光影包/资源包/原理图/存档页支持拖拽导入（拖文件进面板即复制到对应目录，世界 zip 直接还原）。
- **存档备份**：「存档」页内置导入 / 还原备份与逐世界一键备份为 zip；设置页不再提供导入世界。
- 安全边界：所有内容操作都按实例根目录做路径规范化校验（拒绝 `..` 与绝对路径），删除目录/世界需二次确认，配置写入只允许 `options.txt` 与 `config/` 子树。
- **更新内容页**：侧边栏新增「更新内容」，用 `include_str!` 把仓库的 `RELEASE_NOTES.md` 编译进程序（安装后不依赖仓库目录），展示当前版本的更新条目并支持展开历史版本；发版时改 `RELEASE_NOTES.md` 重新构建即可同步。
- 实现：新增后端模块 `src-tauri/src/content.rs`（自带最小 NBT 解析/编码器，gzip 走 `flate2`）与 `src-tauri/src/changelog.rs`，注册 16 个命令；新增 NBT 往返、路径穿越拒绝、启停命名与更新日志解析的单元测试。

## 0.6.0 更新内容（历史）

- **AI 伙伴（实验）**：新增游戏内 AI 伙伴。安装 Companion 模组后，伙伴随玩家自动进入世界：单机模式出现跟随实体（小七），玩家操作（挖方块、聊天、受伤、死亡、换维度）会进入它的记忆，`#ai 问题` 随时对话，它也会主动吐槽；启动器设置页可配置 OpenAI 兼容接口（默认 DeepSeek）、模型、Key 与伙伴名字，并支持对话测试。
- **架构与安全**：模组 → 启动器本地回环服务（127.0.0.1 + 令牌鉴权）→ AI 接口，**API Key 只在 Rust 侧使用并 DPAPI 加密落盘**，不进模组配置、不进日志；启动时通过 JVM 属性注入端口/令牌/玩家名。
- 诚实边界：「自我意识」是伙伴的人格设定而非真实意识；伙伴**不会替玩家操作游戏**（多人服有反作弊与封号风险），实体跟随仅单机可用。AI 伙伴需要自行在设置里填入接口 Key（默认关闭直到配置完成）。
- 实例页一键安装 Companion 模组（Fabric 1.21.x，从 v0.6.0 Release 下载 aurora-companion-1.0.0.jar）；模组源码在 `mod/companion/`。

## 0.5.4 更新内容

- **数据目录也可自定义**：设置 → 下载与目录 → “数据目录”，可浏览选择任意磁盘位置（账户、实例索引、EasyTier/注入器缓存等的根目录）。修改保存后自动把现有数据复制到新位置（原件保留作备份），引导配置 settings.json 固定留在默认位置用于定位，二者关系与迁移行为已在界面说明。实例存储目录继续作为新实例位置的进一步覆盖。
- 樱花主题、联机节点等 0.5.3 功能不变。

## 0.5.3 更新内容

- **实例存储目录可选**：设置 → 下载与目录 → “实例存储目录”，可浏览选择磁盘与目录；留空仍使用默认（C 盘 AppData）。仅对新创建的实例生效，已有实例路径不变；联机节点、共享目录设置不受影响。
- **樱花主题安全化**：随机图接口可在设置中替换（留空使用内置源池 dmoe.cc / paugram / alcy.cc 随机轮换、单源失败自动换下一个，也可换成自建/可信接口，必须 https）；顶栏有「换一张」按钮，启用背景后随时一键换图；设置页保留风险提示——接口返回的图片不受控制、请求会把 IP 暴露给第三方；背景在原有模糊基础上叠加更深的渐变遮罩。
- **失败自动回退**：樱花背景加载失败时清除背景并自动回退默认主题，提示具体原因；后端在图片格式/大小校验失败时同步清理缓存，避免下次启动重复使用坏缓存。
- 加载器安装进度条与 README 精简延续 0.5.2（见 RELEASE_NOTES.md 历史）。

## 0.5.2 更新内容

- **加载器安装全程进度条**：下载安装器（Forge/NeoForge 约 6MB）、下载 MC 本体（实例首次安装时的原版 jar）、OptiFine 官网下载、EasyTier 下载（约 30MB）全部流式上报，每 256 KiB 刷新一次；运行安装器与预下载缺失库显示阶段进度（第 n 次/共 3 次、第 i/n 个库）。
- 进度显示覆盖三处入口：创建实例对话框、实例管理“自动安装”、加载器页“安装到实例”。
- README 精简：只保留当前版本更新内容，历史版本记录移至 RELEASE_NOTES.md。

## 已实现

- Vanilla 1.20.1 / 1.21.1 及 Fabric 1.20.1 的实际启动测试（本机现有游戏文件 + Java 17/21）。
- 每个实例独立 `instance.json`、`game/versions|libraries|assets|natives`、`mods`、`resourcepacks`、`shaderpacks`、`saves`、`datapacks`、`config`、`logs`、`downloads`。
- 启动失败会返回具体阶段（Java、版本 JSON、下载、natives、参数、进程），日志通过 Tauri 事件推送到控制台面板。
- Modrinth / CurseForge 资源搜索与校验安装（流式下载 + 实时进度）；CurseForge 密钥 DPAPI 保存，不进入日志和前端。
- EasyTier 联机：创建/加入房间、房间码、虚拟 IP 展示、日志面板（实验功能，见“联机”章节）。
- LittleSkin（Yggdrasil + authlib-injector）登录：账户/密码验证、令牌刷新、多角色选择、启动注入与注入器校验下载。
- 实例内容管理：分页查看并管理 mods（可禁用/启用 .jar，本地解析名称/版本/加载器）、shaderpacks、resourcepacks、saves（解析 `level.dat` 显示世界名与最后游玩时间）、screenshots（缩略图与预览）、schematics（投影原理图，支持导入）；可直接编辑 `servers.dat` 服务器列表与 `options.txt` / `config/` 文本配置，改动前自动备份（详见上方 0.6.1 更新内容）。
- 更新内容页：启动器内置 `RELEASE_NOTES.md`，展示当前版本条目与历史版本，无需联网。

## 尚未实现 / 限制

- **微软登录已内置自有 Client ID（ae897ba3-93b3-4998-a4fb-b80bad87b538，已过 Mojang appID 白名单）**，开箱可用；想用自己的应用时在设置页覆盖。
- OptiFine 已自动安装（官网下载 + 反射驱动无头安装）；LiteLoader 按官方元数据直装（≤1.12.2）；LabyMod 4 是独立启动器，打开官方下载页安装。
- 服务端锁区只交付 Worker 模板，未部署；GitHub Release 资产仍公开，真正强制需要把资产放到 Worker 后面的源站。
- 本机签名使用自签名证书，只在本机信任；公开信任需要 CA 证书或云签名服务。
- CurseForge 导入/搜索需要 `console.curseforge.com` 的有效 API Key；网站 PAT 会 403。
- 1.21.1 的本地 asset index 与 JSON 声明不一致时，严格校验会重新下载；这是预期行为。
- 内容管理仅做本地文件操作：模组元数据取自 jar 自带清单（解析失败的仍显示文件名），不解析模组依赖；截图缩略图先加载前 80 张，其余需单独点开；配置编辑限文本文件（≤1 MB），`servers.dat` 的图标随原文件读写但不提供图形化设置。

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

## 联机（EasyTier，实验）

- 原理与 PCL-CE 相同：每个玩家运行一个 `easytier-core` 节点，相同网络名 + 密钥的节点组成虚拟局域网；共享节点负责打洞（直连）或中继（绕行）。
- 使用：实例页“联机”卡片 → 房主点“创建房间”（生成 `aurora-xxxxxx#密钥`），启动 Minecraft 对局域网开放；朋友填房间码点“加入房间”，卡片上会显示各自的虚拟 IP（主机固定 10.144.144.1，客人 DHCP 自动分配），多人联机时直连该 IP:端口。
- 节点配置：设置页“联机共享节点”，逗号分隔；留空使用内置候选（`public.easytier.cn/.top`、`kkrainbow.top`——官方节点 2026-11 已下线，需要自建或社区节点才能跨网联机）。
- 权限：EasyTier 需要 TUN 虚拟网卡，Windows 下必须以管理员身份运行启动器，否则进程启动失败（日志有提示）。
- 安全：房间码含网络密钥，只发给要一起玩的人；联机日志只在本机显示，不上传。
- 首次使用会从 GitHub 下载 EasyTier（约 30MB）到 `%APPDATA%\com.aurora.launcher\easytier\`。

## 镜像与代理

- 下载源：`official`（Mojang/官方源）、`bmclapi`（BMCLAPI 国内镜像）、`auto`（默认建议：同时探测两个源，取响应更快者，结果缓存 10 分钟；实测在境内会自动选中 BMCLAPI，海外直连环境自动选中官方）。
- MC 本体（版本清单/JSON/client jar/libraries/assets）与 Forge、NeoForge 安装器及其 maven 依赖均支持镜像：BMCLAPI 映射 `/assets`、`/maven`、`/version/{id}/client`，Forge/NeoForge 的 maven 请求会去掉 NeoForge 的 `/releases` 前缀（已实测镜像可下载完整安装器）。
- 不参与镜像（官方源本就可达或 BMCLAPI 未提供）：Fabric/Legacy Fabric 的 maven、Cleanroom 的 GitHub Releases、OptiFine 的 optifine.net（BMCLAPI 的 OptiFine 接口只提供列表不提供文件）、LiteLoader 的官方 maven。这些源自动模式下直接走官方。
- BMCLAPI 短窗口约 10 次请求限制，游戏文件并发降为 2。
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
- CI 已配置 `WINDOWS_CERT_PFX_BASE64` / `WINDOWS_CERT_PASSWORD`（2026-11-04 起）：tag 构建自动签名，Release 的 `SHA256SUMS.txt` 由签名后的产物生成，发布时不再需要本地签名覆盖（本地签名仅作 Secrets 失效时的兜底）。
- Tauri 的 NSIS 卸载器在安装时生成，签名覆盖安装包与主程序，卸载器没有签名。

CI：`WINDOWS_CERT_PFX_BASE64` 与 `WINDOWS_CERT_PASSWORD` Secrets 已配置，构建自动签名 4 个 exe；密钥更新方式是重新导出 PFX 后 `gh secret set` 覆盖。证书 2027-10-22 到期，续期后同样操作。

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
8. GitHub 直连间歇超时：先重试推送（多数情况直接恢复）；仍失败用 `scripts/github-ip-fallback.ps1` 从官方 IP 段抽样测 TCP 443，`-WriteHosts` 把可用 IP 写入 hosts（标记行可撤销，备份 hosts.aurora-bak）。注意 `http.curloptResolve` 在 Windows 的 schannel 后端不可用，hosts 是唯一干净的指定 IP 方式。
9. 本机 PATH 默认 Java 27 不适合 MC：启动器按版本要求选择 Java 17/21，可在实例中指定。
10. Fabric 自动安装验证：生成 `fabric-loader-0.16.0-1.20.1` 并成功启动。
11. BMCLAPI 是重定向/签名层且 JSON 内仍是 Mojang URL：必须由客户端映射主机，不能缓存签名 URL。
12. OptiFine 下载依赖广告页 token（已带 UA/Referer 解析 `adloadx → downloadx`，token 过期会重试失败提示）、LabyMod 官网反爬无法直下（改为打开下载页）、LiteLoader 已停更且官方 md5 过期（按内容校验放行并记录日志）。
13. 微软不存在可合法复用的第三方公共 Client ID：Aurora 现已内置自有 Azure 应用（2026-10-31 过 Mojang 审核），第三方启动器仍需自行注册。
14. LiteLoader 自动安装未经真实进游戏验证：本机没有 Java 8（LiteLoader 仅支持 ≤1.12.2），下载、依赖解析与版本 JSON 构造均已按官方安装器产物校验。
15. Forge 安装器优先走 maven.creeperhost.net，该镜像偶发坏文件（trove/mixin SHA-1 校验失败即删，安装中止）：已实现失败库解析 + BMCLAPI/Sponge/Maven Central 预下载 + 自动重试（最多 3 次）；瞬时损坏通常第二次尝试即成功。
16. 读写 `servers.dat` / `level.dat` 需要一个最小 NBT 编解码器，自写时踩了两个坑：Compound 的字段名本身已带 u16 长度前缀（不能再多读一次长度，否则报“数据不完整”）；列表值自身的标签是 `TAG_List(9)`，而不是元素的标签类型（写错会把列表编码成 Compound）。已加 NBT 往返测试覆盖。
17. 模组启停最初按小写文件名重命名，会把 `JEI-1.20.jar` 变成 `jei-1.20.jar.disabled`：改为忽略大小写匹配后缀、保留原文件名（禁用只追加 `.disabled`，启用只去掉 `.disabled`），并加大小写保持测试。
18. 游戏启动即退（exit 1 无输出）：AI 伙伴的 JVM 参数 `-Daurora.ai.port` 与值被拆成两个参数，JVM 把端口号当主类名（`java -Dfoo 8123 -version` 会报"找不到或无法加载主类 8123"，本机已复现验证）。修复为单个 `-Dkey=value`；smoke 测试此前传空 ai_args 所以没暴露。
19. 下载中途断流（reqwest error decoding response body）：代理断流/CDN 掐连接，此前直接失败。已在所有流式下载（加载器/游戏文件/资源/EasyTier）统一加重试（每地址最多 3 次，600ms·n 退避，4xx 不重试），并给共享下载助手加了单测覆盖的友好错误文案。
20. Forge 1.20.1 启动即退 `Modules minecraft and _1._20._1 export package net.minecraft.data`：原版 client jar 被放上 classpath，BootstrapLauncher 按 `ignoreList`（只含 `minecraft.jar`）过滤，`1.20.1.jar` 成为自动模块 `_1._20._1`，与 SRG 客户端模块 `minecraft` 包导出冲突。修复（0.6.5）：带 `inheritsFrom` 的版本 JSON（Forge/NeoForge/Fabric 安装器生成）一律不放原版 jar——Forge 的 SRG 客户端不在 libraries 里（首次修复误判了这点，靠 client library 检测会漏），由安装器 patch 体系在运行时生成。

## 发布

- 仓库：<https://github.com/xcqm12/aurora-launcher>
- 最新 Release：<https://github.com/xcqm12/aurora-launcher/releases/tag/v0.6.4>
- 分发下载页：<http://mcdles.us.ci/>

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

Client ID 配置：

**已内置 Aurora 自有 Azure 应用的 Client ID**（显示名 `aurora-launcher`，ID `ae897ba3-93b3-4998-a4fb-b80bad87b538`，账户类型为所有 Microsoft 账户）。该应用于 2026-10-31 收到 Mojang Enforcement 邮件，确认已加入 appID 允许列表（API 白名单），因此**开箱即可使用微软登录**，无需任何配置。设置页的 Client ID 输入框留空即使用内置 ID；仍然支持三处覆盖：

1. 设置页填写自己的 Client ID（保存到 settings.json，优先级最高）。
2. 环境变量 `AURORA_MS_CLIENT_ID`。
3. 构建时注入：GitHub Secret `AURORA_MS_CLIENT_ID`，CI 传入 `--env`，编译进二进制（`option_env!`）。

Azure 注册要点（给自己申请新应用时参考）：

1. 账户类型选“任何组织目录中的账户和个人 Microsoft 账户”。只选组织账户会在 `consumers` 端点返回 `AADSTS700016`。
2. “身份验证”里添加平台 **Web**，重定向 URI 填 `https://www.microsoft.com/link`；打开“允许公共客户端流”。不要创建客户端密钥。
3. “品牌与属性”中应用名称、徽标、说明为必填，否则可能注册失败。
4. **不需要**在“API 权限”里添加任何权限。`XboxLive.signin` 属于 Xbox Live 服务（`https://xboxlive.com`），`offline_access` 是 OIDC 标准 scope，两者都在登录请求的 `scope` 参数里动态申请；门户的“添加权限”列表里搜不到它们是正常的，不是配置遗漏。不要为了“凑权限”去加 Microsoft Graph 的 `User.Read` 等无关项。
5. 未通过审核的 Client ID 可能直接返回 404，可在 <https://aka.ms/mce-reviewappid> 提交应用审核。
6. Client ID 本身不是机密，可以内置；但**客户端密钥（Secret）绝不能写入仓库**——公共客户端设备码流程不需要它。

关于 `scope` 与门户权限的区别：

- Azure 门户的“API 权限”只列出暴露给第三方的资源权限（主要是 Microsoft Graph）。Xbox Live 不在其中，所以搜不到 `XboxLive.signin` 是设计如此，不是漏配。
- 代码在设备码请求里写 `scope=XboxLive.signin offline_access`（`src-tauri/src/auth.rs`），AAD 按请求里的资源动态授权；首次登录的同意页会显示 Xbox Live 访问，以及 `offline_access` 对应的“维护你已授权数据的访问权限”，后者用于拿刷新令牌。
- 不需要管理员同意：`consumers` 租户下都是个人账户，没有管理员角色。

登录失败排查：

| 现象 | 原因与处理 |
| --- | --- |
| `AADSTS700016` | Client ID 拼错，或应用账户类型不含个人账户（设置页填了自定义 ID 时检查）。 |
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

1. `POST https://open.littleskin.cn/oauth/device_code`，`client_id=800`，scope 为 `openid offline_access Yggdrasil.PlayerProfiles.Select Yggdrasil.MinecraftToken.Create`（`Read` 与 `Select` 互斥，不能同时申请）。
2. 浏览器打开授权页并输入授权码，在页面上选择角色；启动器每 `interval` 秒轮询 `POST https://open.littleskin.cn/oauth/token`（`grant_type=urn:ietf:params:oauth:grant-type:device_code`）。
3. 从返回的 `id_token` 读取 `selectedProfile`，再带 OAuth 访问令牌调用 `POST https://littleskin.cn/api/yggdrasil/authserver/oauth`（body `{"uuid":"<角色 id>"}`），拿到与 Yggdrasil 登录相同结构的 `accessToken` / `clientToken`。
4. 令牌保存方式与密码登录一致，启动时照常 `validate`/`refresh` 并注入 authlib-injector。授权时申请 `offline_access`，OAuth 刷新令牌 DPAPI 加密保存；Yggdrasil 令牌失效时先用它换 OAuth 令牌并重新签发 Minecraft 令牌，避免每次重新扫码。

**LittleSkin 侧配置状态**：

- 回调 URL 已配置为 `https://open.littleskin.cn/oauth/callback`（2026-10-30 确认），符合设备代码流要求。
- 但白名单仍未通过：实测 `POST https://open.littleskin.cn/oauth/device_code` 仍返回 `{"error":"invalid_client","error_description":"Client was not found or not whitelisted"}`。白名单与回调是两项独立要求，改回调不会自动获得授权。
- 用绑定邮箱向 `support@littlesk.in` 发工单，标题「申请 OAuth 设备代码流白名单」，正文写应用名称、客户端 ID 800 和 Yggdrasil 权限列表（`Yggdrasil.MinecraftToken.Create`、`Yggdrasil.PlayerProfiles.Select`）。初次通过通常是测试模式，仅应用创建者能授权；正式使用需再次申请解除。
- 设置页「复制白名单申请」按钮可直接生成上述邮件正文；「LittleSkin 应用管理」按钮打开应用配置页。
- 设备代码流**不需要客户端 Secret**，所以本项目不内置 Secret。图中的 Secret 已出现在对话里，建议在应用管理页重置。

限制：界面只提供 LittleSkin 预设，后端 `normalize_server` 支持其他 https Yggdrasil 服务器；微软正版账户与外置登录账户是两个独立账户类型，不能混用。

## Cloudflare 锁区网关（qlmtest.dpdns.org）

- 域名已托管在 Cloudflare（NS：fred/magali.ns.cloudflare.com），是独立 zone，不是 dpdns.org 的子记录。
- `server/region-gate/` 预置了 `qlmtest.dpdns.org/aurora/*` 路由与 v0.6.4 Release 资产回源，`PATH_PREFIX=/aurora`。
- 本地已验证：CN 请求 403，其他地区 200 且回源到正确的 Release 资产 URL。
- **尚未部署**：需要 Cloudflare 凭据。你提供的 32 位 Key 是 Global API Key（需配邮箱、全账号权限，不推荐），建议在 Cloudflare 面板创建 Scoped Token（Workers Scripts:Edit、Zone Workers Routes:Edit、DNS:Edit、Zone:Read，范围限 qlmtest.dpdns.org），然后：

```powershell
$env:CLOUDFLARE_API_TOKEN = "<token>"
npx wrangler login   # 或直接用上面的 token
npx wrangler deploy
```

- 部署后，锁区下载链接为 `https://qlmtest.dpdns.org/aurora/<文件名>`；GitHub Release 原链接仍公开，真正强制必须只发网关链接。
- 若使用 Global API Key，需同时提供账号邮箱（`CLOUDFLARE_EMAIL`），并自行承担全账号权限风险；用后建议在面板轮换该 Key。
