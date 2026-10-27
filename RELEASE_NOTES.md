# Aurora Launcher v0.3.0

## 新增

- 樱花主题“换一张”支持手动刷新背景并即时缓存。
- 加载器安装器下载记录与实例 downloads 目录提示；安装器域名白名单覆盖 Fabric / Legacy Fabric / Forge / NeoForge 官方 maven。
- 资源列表收藏（本地保存）、来源徽标、分类标签、兼容版本与更新时间展示。
- 实例卡显示独立目录路径与内存配置；创建实例可预设加载器版本。
- 设置页显示数据目录与当前发行版的地区限制状态。

## 优化

- Modrinth facets 合并为单次请求，修复合并筛选失效问题。
- 全部 API/下载请求统一走设置页代理并复用同一 HTTP 客户端（45s 超时、15s 连接超时）。
- 主内容、侧边栏、文件抽屉、版本列表独立滚动，新增自定义滚动条样式。
- 空搜索自动预加载热门列表；分页加载更多；加载中状态提示。
- CI：先构建前端再执行 cargo test，tag 与版本号一致性校验，SHA256SUMS 随 Release 发布。

## 修复

- `cargo test` 触发 `generate_context!` panic：CI 现保证 dist 先于任何 cargo 命令存在。
- `spawnSync npm.cmd` 在 Windows 上的 EINVAL：构建脚本直接调用 tsc/vite 的 node 入口。
- 本地死代理阻断 git 推送：推送命令显式覆盖 `http.proxy`（用户环境问题，未写入仓库配置）。

## 安全

- CurseForge 密钥 DPAPI 加密保存；日志与前端不回显密钥。
- 下载仅允许 HTTPS 与官方域名白名单（forgecdn.net / modrinth.com / 官方 loader maven），512 MiB 上限，临时文件原子替换。
- `cn-restricted` 版为客户端地区检查（Windows 地区名称），仅禁用下载，可被绕过；不是服务端强制。

## 已知限制

- 游戏本体启动、微软登录、加载器安装器自动执行、整合包/世界自动导入仍未实现。
- CurseForge 需要有效 API Key（console.curseforge.com 生成）；网站个人令牌返回 403。
- 安装包未签名，首次运行有 SmartScreen 提示。

---

# Aurora Launcher v0.2.0

- PCL 风格资源浏览器、多版本实例隔离、加载器版本目录、代理设置、樱花主题首发。
