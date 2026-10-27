# Aurora Launcher

Windows 优先的 Minecraft 启动器原型，Rust + Tauri 2 + React 19。界面按 PCL 风格重做：左侧资源/安装包分类、顶部搜索与版本/加载器筛选、自动预加载热门列表、纵向资源列表、文件安装抽屉与滚轮滚动。

## 0.3.0 更新内容

- 相对 0.2.0：樱花主题“换一张”手动刷新；资源收藏本地保存；实例卡显示独立目录路径；全部请求复用统一 HTTP 客户端并走设置页代理；修复 CI 先测试后构建前端的顺序问题。
- 资源页重构为 PCL 风格：左侧分类、来源/排序/版本/加载器筛选、热门列表自动预加载、分页加载更多。
- 模组、光影、资源包、整合包、数据包、世界均可在空搜索时自动拉取列表；文件面板显示全部可安装文件。
- Modrinth 安装支持 SHA-512/SHA-1 校验；CurseForge 安装支持 SHA-1 与官方 CDN 白名单。
- 每个实例独立目录：`instance.json`、`game/versions`、`game/libraries`、`game/assets`、`mods`、`resourcepacks`、`shaderpacks`、`saves`、`datapacks`、`config`、`logs`、`downloads`。
- 新建实例可选 Mojang 正式版本与加载器版本；支持打开实例目录、删除确认。
- 加载器页接入官方版本源：Fabric / Legacy Fabric meta、Forge maven、NeoForge maven；OptiFine / Cleanroom / LabyMod / LiteLoader 提供官方页面与手动导入说明。
- 网络代理设置（预置 `http://127.0.0.1:10808`）与“测试代理”按钮；API 与下载统一走该代理。
- 樱花二次元主题：默认关闭，开启后从 `https://www.dmoe.cc/random.php` 获取随机背景，加模糊与深色遮罩，失败回退默认主题。
- 侧边栏与主内容独立滚动，资源列表、文件抽屉、版本列表均支持鼠标滚轮；自定义滚动条。
- 双发行版：标准版与 `cn-restricted` 地区限制版。限制版基于 Windows 地区设置判断，仅禁用下载，浏览不受影响；这是客户端限制，可被绕过，不是服务端强制。
- EXE 与 NSIS 安装包自动构建、SHA-256 校验和与 GitHub Release 自动发布。

## 已实现

- 实例管理、独立目录、代理设置、樱花主题、加载器版本目录与安装器下载。
- 统一资源发现接口：Modrinth + CurseForge，支持 kind / source / query / 版本 / 加载器 / 排序 / offset / limit。
- Modrinth：搜索、版本列表、按实例版本/加载器过滤、校验后安装；整合包/世界下载到 `downloads`。
- CurseForge：搜索、文件列表、校验后安装；需要 `console.curseforge.com` 生成的有效 API Key。
- CurseForge 密钥：环境变量、DPAPI 本地加密、HTTPS 代理三种模式；不写入日志和前端。
- 安全下载：仅 HTTPS、来源域名白名单、512 MiB 上限、临时文件 + 原子替换、哈希校验。

## 尚未实现

- Minecraft 游戏本体下载、资源索引、natives 解压、启动参数与进程启动。
- Forge / NeoForge / Fabric 安装器的自动执行（当前只下载安装器）。
- OptiFine / Cleanroom / LabyMod / LiteLoader 自动安装。
- 微软正版 OAuth 登录。
- 整合包 / 世界自动导入（当前下载到实例 `downloads`）。
- 真正的服务端地区强制（GitHub Release 不能按地区限制下载）。

## 安全与密钥

- 不要把 API Key 提交到 Git、截图或分享给他人。
- 桌面客户端中的密钥无法靠混淆真正保密；发布版应使用 `AURORA_CURSEFORGE_PROXY` 指向你的 HTTPS 代理，由服务端注入密钥并限流。
- 本地 DPAPI 密钥路径：`%APPDATA%\com.aurora.launcher\curseforge.key`；地区限制版标识符不同，路径为 `com.aurora.launcher.cn`。
- 运行 `aurora-launcher.exe --self-test` 可检查密钥是否可解密、代理/环境变量是否配置（只输出状态，不输出密钥）。
- 本项目不分发 Minecraft 游戏文件或未经授权的资源。

## 已知错误与处理记录

1. **CurseForge 403**：从网站复制的个人访问令牌（PAT）不是 API Key，官方接口返回 403。需在 <https://console.curseforge.com/> 生成 API Key，或在设置中使用代理模式。已暴露的令牌应撤销。
2. **crates.io 索引中断**：`git fetch` 报 `RPC failed / early EOF`。改用稀疏索引 + 镜像，并把本机已有 `.crate` 缓存合并后离线构建；`.cargo/config.toml` 为本地未跟踪镜像配置。
3. **PowerShell 找不到 ProtectedData**：改用 `CryptProtectData` 的 P/Invoke 脚本 `scripts/save-curseforge-key.ps1`，已验证 Rust 端可解密。
4. **Tauri 参数命名**：Rust snake_case 参数在 JS 侧需 camelCase（如 `minecraftVersion`、`gameVersion`），已统一。
5. **Modrinth 路由差异**：本环境 `project/{id}/versions` 返回 404，使用官方 `project/{id}/version`（单数）路径。
6. **代理出口限流**：经 10808 访问 GitHub 首次出现 403（共享出口触发未认证配额），直连与重试均正常；未做 IP 扫描。
7. **随机图风险**：`dmoe.cc/random.php` 返回无审核随机图片，可能包含不适宜内容，且会把 IP 暴露给第三方；因此默认关闭、加模糊遮罩。
8. **构建输出路径**：工作区根 `target/` 才是实际产物目录，CI 已修正；CI 中同时构建 standard 与 cn 两个 target 目录。
9. **前端 npm 安装中断**：大包下载出现 `ECONNRESET`，重试后完成；`npm run build` 直接调用本地 `tsc`/`vite` 二进制以避免 Windows shim 问题。
10. **CI 中 `generate_context!` 找不到 dist**：`cargo test` 在前端构建前执行会触发宏 panic；CI 已改为先构建前端再跑测试，并让 tag 指向包含该修复的提交。
11. **GitHub 直连抖动**：推送时段 github.com 的默认 DNS IP 超时（443 连接超时），稍后自动恢复；未做 IP 扫描，仅重试推送。

## 发布

- 仓库：<https://github.com/xcqm12/aurora-launcher>
- Release v0.3.0：<https://github.com/xcqm12/aurora-launcher/releases/tag/v0.3.0>
- Release v0.2.0：<https://github.com/xcqm12/aurora-launcher/releases/tag/v0.2.0>

## 开发与构建

```powershell
npm install
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri dev
```

本地打包：

```powershell
npm run bundle:standard  # target/release/bundle/nsis
npm run bundle:cn        # target/cn/release/bundle/nsis
```

CI 在 tag `v*` 时构建两个变体、生成 SHA-256 并发布 GitHub Release：

```powershell
git tag v0.3.0
git push origin v0.3.0
```

## 代理与地区限制

- 设置中的代理会用于 Mojang、Modrinth、CurseForge、背景图与资源下载。
- 本地测试：`curl -x http://127.0.0.1:10808 https://api.github.com/zen`。
- `cn-restricted` 版仅在编译时嵌入 `AURORA_REGION_LOCK=CN`，启动后读取 Windows 地区名称，若为 `CN` 则禁用下载。它不影响浏览、实例管理与手动导入，也不能阻止用户通过改系统设置或使用标准版绕过。

## 发布产物

- `Aurora-Launcher-0.3.0-standard-x64-setup.exe`
- `Aurora-Launcher-0.3.0-cn-restricted-x64-setup.exe`
- 对应便携版 EXE 与 `SHA256SUMS.txt`

安装包未签名，首次运行可能出现 SmartScreen 提示。
