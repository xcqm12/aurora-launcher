# Mojang 应用审核与自己的 Client ID

Aurora Launcher 的微软登录需要**你自己注册并获 Mojang 批准**的 Azure 公共客户端 Client ID。这份文档说明为什么、怎么申请、以及申请时该写什么。

## 为什么需要审核

Mojang 对访问《Minecraft: Java Edition》游戏服务 API 的第三方应用做人工审核和白名单（2023 年起）。未获批准的应用即使微软 OAuth、Xbox Live、XSTS 全部成功，也可能在 `login_with_xbox` 或 `/minecraft/profile` 阶段被拒绝（社区报告为 HTTP 403 `Invalid app registration` 或 404）。

官方入口：

- 应用注册信息：<https://aka.ms/AppRegInfo>（当前跳转到 Mojang 帮助文章 `help.minecraft.net/hc/en-us/articles/16254801392141`）
- 旧表单入口：<https://aka.ms/mce-reviewappid>

入口和表单可能调整；打不开时以 Mojang 帮助中心的最新文章为准。

## 不能做的事

不要把 PCL2、HMCL 或其他启动器已经过审的 Client ID 抄过来用。

Client ID 代表一个应用的身份：微软用它识别“是谁在请求登录”，Mojang 的审核也绑定在这个 ID 上。冒用他人 ID 属于把自己的应用伪装成别人的应用，违反 Microsoft 身份平台条款；一旦被发现，对方的应用可能被撤销或封禁，你的应用也拿不到稳定支持。这类 ID 通常还被混淆在二进制里，提取本身也不是正当的集成方式。

Aurora 不内置任何第三方 Client ID，设置页、环境变量、构建注入三种方式都只接受你自己的 ID。

## 申请前的准备

1. **Azure 应用**（<https://portal.azure.com> → Microsoft Entra ID → 应用注册）：
   - 受支持的账户类型：**任何组织目录中的账户和个人 Microsoft 账户**。只选组织账户会在 `consumers` 端点报 `AADSTS700016`。
   - 身份验证 → 添加平台 **Web**，重定向 URI 填 `https://www.microsoft.com/link`；打开“允许公共客户端流”。不要创建客户端密钥。
   - **API 权限：不需要添加任何权限**。门户里搜不到 `XboxLive.signin`、`offline_access` 是正常的，原因见下节。
   - 品牌与属性：应用名称、徽标、说明为必填；建议补主页、隐私声明、服务条款链接。
2. **可公开访问的主页与隐私政策**。GitHub 仓库 README 或 GitHub Pages 都可以；内容要说明应用做什么、收集哪些数据（本启动器只在本机保存账户令牌，不上传）。
3. **可复现的登录演示**：应用名称、版本、下载地址，最好有一段从点击“微软登录”到进入游戏的录屏或截图。
4. **稳定的联系方式**：审核方可能会邮件询问。

### 为什么“添加权限”里找不到 XboxLive.signin / offline_access

因为它们是 **OAuth scope**，不是门户里的资源权限：

- Azure 门户的“API 权限”只列出已暴露给第三方的资源权限，主要是 Microsoft Graph。Xbox Live（`https://xboxlive.com`）不在这个列表里，所以搜不到 `XboxLive.signin`。
- `offline_access` 是 OpenID Connect 的标准 scope，表示“需要刷新令牌”，门户从来不把它作为可勾选的权限。
- 启动器在设备码请求里直接写 `scope=XboxLive.signin offline_access`（见 `src-tauri/src/auth.rs`），AAD 按请求里的资源动态授权。首次登录的同意页会显示 Xbox Live 访问，以及 `offline_access` 对应的“维护你已授权数据的访问权限”。
- 不需要管理员同意；`consumers` 租户下都是个人账户，没有管理员角色。
- 不要为了“补权限”去添加 Microsoft Graph 的 `User.Read` 等无关权限，那不会让 Minecraft 登录生效。

准备好后先在启动器设置页点“验证”：它会用你的 Client ID 请求一次设备码，能立刻发现应用不存在、账户类型不对、未开公共客户端流等问题（scope 拼错时微软会返回 `invalid_scope` / AADSTS70011，启动器会提示检查应用配置）。注意这一步**不能**证明已过 Minecraft 审核。

## 提交内容（英文模板）

审核以英文为主，下面是可直接改写的模板：

```text
Application name: Aurora Launcher
Client ID: <your application (client) ID>
Publisher: <your name or team>
Website: https://github.com/<you>/aurora-launcher
Privacy policy: https://github.com/<you>/aurora-launcher/blob/main/PRIVACY.md
Contact email: <you@example.com>

What the application does:
Aurora Launcher is a Windows desktop launcher for Minecraft: Java Edition. It
downloads game files and mod loaders, manages isolated instances, and launches
the game with the player's own Microsoft account.

How Microsoft authentication is used:
The launcher uses the OAuth 2.0 device code flow (consumers tenant, scopes
XboxLive.signin and offline_access). After the user signs in, it exchanges the
Microsoft token for an Xbox Live token and XSTS token, then calls
https://api.minecraftservices.com/authentication/login_with_xbox to obtain a
Minecraft access token. It calls /entitlements/mcstore and /minecraft/profile to
verify game ownership and read the player's profile name and UUID. These values
are only used to launch the game locally.

Data handling:
Tokens are stored on the user's machine only, encrypted with Windows DPAPI.
Nothing is sent to our servers; the launcher has no backend.

Distribution:
Open source, distributed as signed Windows installers from GitHub Releases.
Source code: https://github.com/<you>/aurora-launcher
```

按实际情况改：如果加了服务端、统计或崩溃上报，必须如实写明。

## 审核通过之后

- 把 Client ID 填到设置页，或用环境变量 `AURORA_MS_CLIENT_ID`，或作为 GitHub Secret `AURORA_MS_CLIENT_ID` 在构建时注入。
- 真正验证是否过审只能走一次完整登录：设备码 → Xbox → XSTS → `login_with_xbox` → 档案。任一步被拒都会在界面显示对应错误。
- 审核通常绑定应用和 Client ID；换 ID 需要重新申请。

## 常见被拒或失败原因

| 现象 | 原因 |
| --- | --- |
| `AADSTS700016` | Client ID 错误，或应用账户类型不含个人账户 |
| `AADSTS7000218` / `7000215` | 没开“允许公共客户端流”，或缺少 Web 平台 |
| 门户里搜不到 `XboxLive.signin` | 正常现象：它是运行时 scope，不是门户权限，不需要添加 |
| `invalid_scope` | scope 拼写错误，或应用类型/租户不对 |
| `login_with_xbox` 403 `Invalid app registration` | 应用未获 Mojang 批准，或 Client ID 与申请时不一致 |
| `/minecraft/profile` 404 | 账户没有 Java 版档案或许可证；也可能是应用未过审 |
| 审核被拒 | 品牌信息不全、没有隐私政策、描述含糊、无法复现登录流程 |

## 等待审核期间

- 先用离线账户完整测试启动、mod、整合包等功能。
- 需要第三方服务器登录时，可考虑 authlib-injector / Yggdrasil 认证（与微软登录是两套体系，不涉及 Mojang 审核）。
- 不要在公开发行版里塞入未过审的 Client ID 并宣称“正版登录可用”；界面应在失败时给出明确原因。
