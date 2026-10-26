# Aurora Launcher v0.2.0

## 重点更新

- PCL 风格资源浏览器：左侧分类、搜索、来源/排序/版本/加载器筛选、空搜索自动预加载、分页加载更多。
- 模组、光影、资源包、整合包、数据包与世界均可浏览；文件面板显示全部可安装文件。
- 多版本实例隔离：每个实例独立 `game/`、`mods`、`resourcepacks`、`shaderpacks`、`saves`、`datapacks`、`config`、`logs`、`downloads`。
- 加载器版本目录：Fabric / Legacy Fabric / Forge / NeoForge 官方版本源与安装器下载；OptiFine / Cleanroom / LabyMod / LiteLoader 提供官方页面。
- 网络代理设置与测试（预置 `http://127.0.0.1:10808`）。
- 樱花二次元主题，默认关闭，支持换图与失败回退。
- 标准版与 `cn-restricted` 地区限制版。

## 安全

- CurseForge 密钥使用 Windows DPAPI 保存，不进入日志或前端。
- 下载仅允许 HTTPS 与官方域名，校验文件大小与哈希，使用临时文件原子替换。
- 地区限制版是客户端检查，可被绕过，不是服务端强制。

## 已知问题

- 游戏本体启动、微软登录、加载器安装器自动执行、整合包/世界自动导入仍未完成。
- CurseForge 需要 `console.curseforge.com` 的有效 API Key；网站个人令牌会返回 403。
- 安装包未签名，可能出现 SmartScreen 提示。
