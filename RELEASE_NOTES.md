# Aurora Launcher v0.4.6

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
