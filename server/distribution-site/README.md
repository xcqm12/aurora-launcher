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
| 无障碍 | 全部动画在 `prefers-reduced-motion: reduce` 下关闭 |

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
