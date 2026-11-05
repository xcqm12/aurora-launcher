# Aurora Launcher v0.6.2

- 修复：AI 伙伴的 JVM 参数被拆成两个元素（`-Daurora.ai.port` 与端口号分开），JVM 把端口号当主类名导致启动即退（"找不到或无法加载主类 8123"，exit 1）；现已合并为单个 `-Daurora.ai.port=8123`。AI 服务默认开启，这是 v0.6.1 所有实例首次启动失败的原因。
- 修复：所有网络下载（加载器、游戏文件、资源、EasyTier）在传输中途断流（reqwest 报 error decoding response body）时自动重试最多 3 次（4xx 除外），错误信息改为中文说明原因。
- 改进：游戏进程异常退出且全程无输出时，日志给出排查提示（JVM 参数格式、Java 兼容性、内存设置）。
- 樱花主题增强：默认改为内置三源随机轮换（dmoe.cc / paugram / alcy.cc），单源失败自动换下一个；顶栏新增「换一张」按钮，启用背景后无需进设置页即可随机换图。

# Aurora Launcher v0.6.1（历史）

- 实例内容管理：实例管理弹窗改为分页布局，新增 8 个内容页，与原有「设置」页并列，标签显示数量角标。
  - 模组：列出 `mods/` 下的 .jar，可禁用/启用（重命名为 `.jar.disabled`，保留原文件名大小写）与删除。
  - 光影包 / 资源包：列表、删除、从磁盘导入 `.zip`，可直接打开对应目录。
  - 存档：解析 `level.dat` 显示世界名、最后游玩时间与占用空间，可打开或删除。
  - 截图：分批加载缩略图，点击查看大图，支持单张删除。
  - 原理图：列出 `.litematic` / `.schem` / `.schematic` / `.nbt`，支持导入与删除。
  - 服务器：直接读写 `servers.dat`（gzip + NBT），可改名称/地址、设置隐藏、增删条目；保存前自动备份（保留最近 3 份 `servers.dat.aurora-bak-*`）。
  - 配置：编辑 `options.txt` 与 `config/` 下的文本文件（≤1 MB），支持搜索与新建，保存时生成 `*.aurora-bak-*` 备份。
- 安全边界：内容操作按实例根目录做路径规范化校验（拒绝 `..` 与绝对路径）；删除世界/目录需二次确认；配置写入限制在 `options.txt` 与 `config/` 子树。
- 模组元数据：本地读取 jar 内的 `fabric.mod.json` / `quilt.mod.json` / `META-INF/mods.toml` / `mcmod.info`，显示真实名称、版本与加载器标签（不联网，按文件大小与修改时间缓存）。
- 内容搜索与配置编辑：各内容页支持关键字过滤（模组可按名称/ID/版本搜）；配置编辑器切换文件前会提示未保存改动，支持 Ctrl+S 保存。
- 截图预览支持 ←/→ 切换与 Esc 关闭。
- 批量与拖拽：模组支持勾选后批量启用/禁用，可按「启用优先」排序；模组/光影包/资源包/原理图/存档页支持把文件直接拖进面板导入（按扩展名过滤，逐个报告成功与失败）。
- 存档页整合导入/备份：导入世界从设置页移到「存档」页，并改为「导入 / 还原备份」；每个世界可一键备份为 zip（复用整合包的 zip 打包逻辑）。
- 修复：模组启停不再把文件名整体转小写（此前 `JEI-1.20.jar` 会变成 `jei-1.20.jar.disabled`）。

# Aurora Launcher v0.6.0（历史）

- AI 伙伴（实验）：游戏内 Companion 模组（Fabric 1.21.x），自动随玩家进入世界、观察操作过程形成记忆、`#ai` 对话与主动吐槽；单机模式有跟随实体。
- 启动器内置本地回环服务 + OpenAI 兼容 LLM 代理：模组不持有任何密钥，API Key 在启动器内 DPAPI 加密、仅 Rust 侧使用、绝不写日志；实例页一键安装模组，设置页可测试对话。
- 边界声明：「自我意识」为人格设定；伙伴不自动代玩。

# Aurora Launcher v0.5.4（历史）

- 数据目录可自定义（设置页浏览选择）：保存后自动把账户、实例索引、EasyTier/注入器缓存复制到新位置（原件保留），引导配置 settings.json 固定留在默认位置存放指针，下次启动自动定位。
- 与实例存储目录配合：数据目录是根，实例存储目录可进一步指定新实例的位置。

# Aurora Launcher v0.5.3（历史）

- 实例存储目录可选（设置 → 浏览选择），新实例不再固定写入 C 盘 AppData；已有实例路径保持不变。
- 樱花主题安全化：随机图接口可替换为自建/可信来源（https），保留“不受控内容 + IP 暴露”风险提示；背景叠加更深的渐变遮罩；加载失败自动回退默认主题并提示原因，坏缓存同步清理。

# Aurora Launcher v0.5.2（历史）

- 加载器安装全程进度条：安装器下载、MC 本体下载、OptiFine 官网下载、EasyTier 下载流式上报（每 256 KiB），运行安装器与预下载缺失库显示阶段进度；创建实例对话框、实例管理、加载器页三处入口均显示。
- README 精简为只显示当前版本更新内容，历史版本记录移至 RELEASE_NOTES.md。

# Aurora Launcher v0.5.1（历史）

- 加载器安装失败自动恢复：解析 Forge/NeoForge 安装器的失败库清单，从 BMCLAPI / Sponge 仓库 / Maven Central 预下载到实例 libraries，再自动重试安装器（最多 3 次），瞬时坏镜像（如 creeperhost 返回损坏的 trove/mixin）不再导致整个安装中止。
- 坐标解析支持带 classifier 的库；每次尝试日志保留在安装结果里。

# Aurora Launcher v0.5.0（历史）

- 模组/材质/光影/整合包文件下载改为流式并显示实时进度（百分比 + 已下载/总大小），Modrinth 与 CurseForge 均支持。
- 新增联机功能（EasyTier，对齐 PCL-CE 方案）：创建/加入房间、房间码 `网络名#密钥`、虚拟 IP 展示与联机日志；主机固定 10.144.144.1，客人 DHCP 自动取址（已实测分配 10.144.144.2/24）。
- EasyTier 首次使用自动从 GitHub Releases 下载并解包；共享节点可在设置页自定义（官方公共节点 2026-11 已下线，跨网联机需要自建/社区节点）。
- TUN 虚拟网卡需要管理员权限运行启动器。

# Aurora Launcher v0.4.10（历史）

- 自动下载源探测：auto 模式并发探测 Mojang 官方与 BMCLAPI，选响应更快的一边，缓存 10 分钟；启动与所有安装流程共用。
- Forge / NeoForge 安装器、版本元数据与 ensure_vanilla（版本清单/JSON/client jar）接入 BMCLAPI 镜像候选，失败回退官方；NeoForge 镜像去掉 /releases 前缀（实测可下载完整安装器）。
- mirror_url 新增 Forge/NeoForge maven 映射，启动期 library 下载同样受益；Fabric/GitHub/optifine.net/LiteLoader 等未镜像源自动模式直接走官方。
- 设置页“自动”文案更新为“探测最优下载源”。

# Aurora Launcher v0.4.9（历史）

- 创建实例时即可选择加载器，创建完成自动下载官方安装器并执行；失败可重试。
- OptiFine 全自动：解析 optifine.net 官网版本列表，按 `adloadx → downloadx` 官方流程下载，内置反射驱动调用 `optifine.Installer.doInstall` 无头安装到所选实例（已实测生成 1.20.1-OptiFine_HD_U_I6）。
- LiteLoader 按官方元数据直装（≤1.12.2）：versions.json + maven-metadata 解析快照，预下载 launchwrapper/asm/mixin，按官方安装器产物构造 launchwrapper 版本 JSON；官方 md5 已过期，按内容校验放行并记录日志。
- LabyMod 4 为独立启动器且官网反爬：创建后自动打开官方下载页，提示选择实例 game 目录。
- 已知边界：LiteLoader 自动安装未经真实进游戏验证（本机无 Java 8）。

# Aurora Launcher v0.4.8（历史）

- **内置 Aurora 自有 Azure 应用的 Client ID**（ae897ba3-93b3-4998-a4fb-b80bad87b538）。该应用于 2026-10-31 通过 Mojang appID 白名单审核（Mojang Enforcement 邮件确认），微软正版登录开箱即用，不再要求先注册 Azure。
- 设置页 Client ID 留空即用内置 ID；“验证”按钮在留空时检测内置 ID；设置页、环境变量、构建注入三种覆盖方式不变。
- 说明与 README 更新：Client ID 非机密可内置；客户端 Secret 绝不入库（设备码流程不需要）。
- LittleSkin 设备代码流白名单仍未通过（实测仍 invalid_client），密码登录不受影响。

# Aurora Launcher v0.4.7（历史）

- LittleSkin OAuth 授权登录申请 `offline_access`，OAuth 刷新令牌与 Yggdrasil 令牌一起 DPAPI 加密保存。
- Yggdrasil 令牌失效且 `authserver/refresh` 失败时，自动用 OAuth 刷新令牌重新签发 Minecraft 令牌，避免重复扫码；两者都失败才提示重新授权。
- UUID 统一转为无符号形式再请求 `authserver/oauth`，并补充单元测试。
- 设备代码流白名单仍未通过时，授权与刷新接口都会返回 `invalid_client`，界面与 README 保持对应提示。

# Aurora Launcher v0.4.6（历史）

- 设置页 LittleSkin 授权登录新增「LittleSkin 应用管理」与「复制白名单申请」按钮：一键打开 OAuth 应用配置页，或把应用名称、客户端 ID 800、Yggdrasil 权限列表与申请理由复制到剪贴板。
- 说明文字更新为当前状态：回调已配置为 https://open.littleskin.cn/oauth/callback，但设备代码流白名单仍需邮件申请；未通过时接口返回 invalid_client。
- README 记录实测结果，区分“改回调”与“通过白名单”两件事。

# Aurora Launcher v0.4.5（历史）

- 新增 LittleSkin OAuth 授权登录（设备代码流，客户端 ID 800）：浏览器授权并选角色，再用 `authserver/oauth` 换取 Minecraft 令牌，无需密码与客户端 Secret。
- 授权失败会提示可执行原因：未加入设备代码流白名单（回调需设为 https://open.littleskin.cn/oauth/callback 并发送工单）、scope 冲突、授权过期等；初次通过多为测试模式，仅创建者可授权。
- 保留 LittleSkin 账号密码（传统 Yggdrasil）登录作为备选。
- 修复控制台窗口闪现：Java 版本探测、加载器安装器、taskkill 等子进程统一使用 `CREATE_NO_WINDOW`；发布版主程序保持 windows 子系统。
- 不把客户端 Secret 写入仓库或二进制；图中 Secret 已在对话中暴露，建议在 LittleSkin 应用管理页重置。

# Aurora Launcher v0.4.4（历史）

- 新增 LittleSkin 登录（Yggdrasil）：设置页用 LittleSkin 邮箱/密码登录，令牌 DPAPI 保存；启动前先 validate，失效自动 refresh，失败提示重新登录。
- 启动时注入 authlib-injector：从官方接口读取版本与 SHA-256，校验后缓存到应用数据目录，传 `-javaagent` 与 Base64 元数据；接口不可达时使用缓存。
- 多角色账户在设置页选择角色；待选会话 10 分钟过期，令牌不经过前端。
- 账户列表区分“LittleSkin”；启动参数里的启动器版本改为编译期读取，不再硬编码。
- 密码只通过 HTTPS 发送给 littleskin.cn，不保存、不写日志。

# Aurora Launcher v0.4.3（历史）

第三方启动器的微软登录需要**自己注册并通过 Mojang 审核**的 Client ID。本版把这条路径做成可操作的功能，并明确不冒用他人凭据：

- 设置页新增“验证”：检查 Client ID 是否为合法 GUID，并用它请求一次微软设备码，提前发现应用不存在（AADSTS700016）、未启用公共客户端流（7000218/7000215）、权限不足（65001）等问题。
- 设置页新增“Azure 应用注册”“Minecraft 应用审核”按钮；说明文字更新为 Web 平台重定向 `https://www.microsoft.com/link` + 允许公共客户端流 + 审核要求。
- 新增 `docs/mce-review-application.md`：申请前准备、可直接改写的英文申请模板、审核通过后的配置方式、常见拒绝原因。
- 明确产品边界：不内置、不提取、不冒用 PCL2/HMCL 等他人已审核的 Client ID；审核结果只会在完整登录拉取档案时体现，验证按钮不冒充审核结果。

# Aurora Launcher v0.4.2（历史）

参照 lmaml 教程第二章《微软 OAuth 登录》补齐设备码流程的细节：

- 设备码请求带 `?mkt=zh-CN` 与 `Accept-Language: zh-CN`，微软返回中文提示与本地化错误描述。
- 设备码接口失败时按 AADSTS 错误码给出可执行建议：应用不存在（700016）、未启用公共客户端流（7000218/7000215）、权限不足（700005）、未过 Minecraft 应用审核（HTTP 404，附 aka.ms/mce-reviewappid）。
- 轮询结果区分终态（`pending`/`slow_down` 之外全部停止自动轮询）：过期、拒绝、设备码被复用、Client ID 无效都有独立状态与中文提示，不再无限轮询已被消耗的设备码。
- 无游戏所有权/无许可证时返回 `no_license` 状态并在设置页显示“打开 Minecraft 商店”按钮（对齐教程中的商店跳转）。
- 支持微软返回的 `verification_uri_complete`，有则直接打开带代码的登录链接。
- README 补充 Azure 注册要点：账户类型、Web 平台重定向 `https://www.microsoft.com/link`、允许公共客户端流、品牌信息必填与审核表单。

# Aurora Launcher v0.4.1（历史）

- 微软登录修正：MC 令牌过期时间、所有权检查、XErr 映射、uhs 校验、access token DPAPI 加密。
- 设备码自动打开浏览器、复制代码、自动轮询与 slow_down/过期处理。
- 构建时可通过 `AURORA_MS_CLIENT_ID` 注入 Client ID；仍不内置第三方 ID。
- Worker 支持 `/aurora` 路径前缀，预置 `qlmtest.dpdns.org/aurora/*` 路由（未部署）。

# Aurora Launcher v0.4.0（历史）

## 核心更新

- 真正启动 Minecraft：版本继承、client/libraries/assets 下载、natives 解压、classpath/参数、Java 选择、进程启动、实时日志与停止。
- 已用本机 1.20.1、1.21.1 与 Fabric 1.20.1 实测启动；Fabric 自动安装生成 `fabric-loader-0.16.0-1.20.1` 并可启动。
- 镜像下载：官方 / BMCLAPI / 自动；BMCLAPI 主机映射与低并发。
- 共享游戏目录：只复用只读 assets/libraries/vanilla 版本，实例 mods/saves/config 仍隔离。
- 加载器自动安装：Fabric、Legacy Fabric、Forge、NeoForge、Cleanroom；OptiFine、LabyMod、LiteLoader 手动导入。
- 微软设备码登录（需自己的 Azure Client ID）与离线账户；刷新令牌 DPAPI 加密。
- `.mrpack` / CurseForge zip 导入、世界导入、`.mrpack` 导出。
- Windows 签名脚本（PFX 或证书 thumbprint）与 CI 可选签名。
- Cloudflare Worker 锁区网关模板（按 CF-IPCountry 拦截并代理资产）。

## 安全

- CurseForge 密钥与微软刷新令牌均 DPAPI 保存，不写入日志或前端。
- 下载仅 HTTPS 与来源白名单，校验哈希/大小，临时文件原子替换。
- 不内置任何第三方 Azure Client ID；不代管云账号或签名证书。

## 已知限制

- 微软登录未配置 Client ID 时只支持离线账户。
- 服务端锁区未部署；GitHub Release 资产本身仍公开。
- 本机签名为自签名证书，仅本机信任。
- OptiFine/LabyMod/LiteLoader 没有可靠无头安装接口。
- 安装包未配置正式证书时不会签名，可能出现 SmartScreen 提示。
