# 分发站页面源文件（mcdles.us.ci）

`index.html` 是分发站首页的**权威源文件**，改版后从这里同步到服务器，避免线上手改后无法回溯。

## 结构

- **介绍**：项目定位、技术栈、三个核心承诺（真启动 / 一键装加载器 / 安全边界）
- **功能**：8 张卡片，覆盖实例管理、内容管理、资源中心、账户、下载源、联机、界面、工程发布
- **教程**：5 步（下载 → 建实例 → 登录 → 装资源 → 启动联机）+ 宣传视频 + 8 条常见问题
- **下载**：标准版 / 锁区版各两个产物，附校验命令
- **文档**：README、RELEASE_NOTES、已知错误、Releases 入口

## 特效实现（纯 CSS + 原生 JS，无外部依赖）

| 效果 | 实现 |
| --- | --- |
| 极光光斑背景 | 三个 `filter:blur` 圆形 + `@keyframes` 浮动 |
| 星尘 | `<canvas>` 粒子（数量按视口面积自适应，≤90），随标签页隐藏暂停 |
| 标题流光 | `background-clip:text` + 220% 背景位移动画 |
| 滚动进度条 | 顶部 2px 渐变条，`scroll` 事件节流更新宽度 |
| 入场动画 | `IntersectionObserver` + `.reveal` 位移淡入，分级延迟 `d1/d2/d3` |
| 卡片光斑 | `pointermove` 写入 `--mx/--my`，径向渐变跟随鼠标 |
| 数字滚动 | 同类 IntersectionObserver 触发，缓动计数 |
| 星座连线 + 流星 | canvas 粒子近距连线（O(n²) 距离衰减 alpha），2.6–6.8s 随机流星带渐变尾迹 |
| 逐字标题入场 | `<h1>` 按字符拆分，translateY+rotateX+blur 依次入场，渐变字符叠加 shimmer |
| Hero 轨道装饰 | 双环旋转 + 沿轨道运行的发光卫星点 |
| 鼠标跟随辉光 | `#glow` 大径向渐变以 lerp 0.08 缓动跟随指针（仅 pointer:fine） |
| 卡片 3D tilt | hover 时按指针位置 rotateX/rotateY（±7°），离开回弹 |
| 按钮扫光/脉冲 | 主按钮周期性斜向扫光；推荐下载按钮外圈脉冲光圈 |
| 区块发光线 | `h2.sec::after` 渐变线随 reveal 从 0 展开并发光 |
| scrollspy | IntersectionObserver 高亮导航当前区块 |
| 滚动视差 | 背景光斑层随滚动 0.07 系数位移（容器留 -14% 余量防露底） |
| 无障碍 | 全部动画在 `prefers-reduced-motion: reduce` 下关闭（含逐字标题强制可见、glow 隐藏） |

## 部署

```bash
ssh -p 54125 -i ~/.ssh/mc_mirror_ed25519 root@154.222.28.228 \
  'cd /www/wwwroot/mcdles.us.ci && cp index.html index.html.bak-$(date +%Y%m%d%H%M)'
scp -P 54125 -i ~/.ssh/mc_mirror_ed25519 server/distribution-site/index.html \
  root@154.222.28.228:/www/wwwroot/mcdles.us.ci/index.html
ssh -p 54125 -i ~/.ssh/mc_mirror_ed25519 root@154.222.28.228 \
  'chown www:www /www/wwwroot/mcdles.us.ci/index.html'
```

发版时需同步更新：版本号徽章（`v0.6.7 · 2026-11-08`）、4 个下载链接与体积文案、`download/SHA256SUMS.txt`。
