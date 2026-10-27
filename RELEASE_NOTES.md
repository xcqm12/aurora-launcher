# Aurora Launcher v0.4.1

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
