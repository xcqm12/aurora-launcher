#!/usr/bin/env python3
"""Aurora 网络诊断：GitHub 可用 IP 扫描 + 加载器/游戏下载端点可达性检查。

用法（在仓库根目录）：
    python scripts/network_check.py            # 全部检查
    python scripts/network_check.py --hosts    # 顺便给出 hosts 建议行

输出：
- GitHub 官方 IP 段（api.github.com/meta，限流时用内置兜底段）抽样 TCP 443
- 加载器安装链路各端点（Forge/NeoForge/Fabric/OptiFine/BMCLAPI/Mojang...）
  的 HTTPS 可达性，与启动器 download_source 无关，逐个直测
"""
from __future__ import annotations

import concurrent.futures
import json
import socket
import ssl
import sys
import time
import urllib.request

GITHUB_FALLBACK_SEGMENTS = [
    "140.82.112.0/20", "192.30.252.0/22", "185.199.108.0/22",
    "143.55.64.0/20", "20.205.243.166/32", "20.201.28.151/32",
]

# 加载器安装与游戏文件下载会碰到的端点（与 loaders.rs/lib.rs 保持一致）
ENDPOINTS = {
    "Forge 官方 maven": "https://maven.minecraftforge.net/maven-metadata.xml",
    "NeoForge maven": "https://maven.neoforged.net/releases/",
    "Fabric meta": "https://meta.fabricmc.net/v2/versions/loader",
    "Legacy Fabric meta": "https://meta.legacyfabric.net/v2/versions/loader",
    "OptiFine 官网": "https://optifine.net/downloads",
    "OptiFine BMCLAPI 列表": "https://bmclapi2.bangbang93.com/optifine/versionList",
    "Forge BMCLAPI 列表": "https://bmclapi2.bangbang93.com/forge/minecraft",
    "LiteLoader 元数据": "https://raw.githubusercontent.com/Mumfy/LiteLoader/1.12-SNAPSHOT/update.json",
    "Cleanroom releases": "https://api.github.com/repos/CleanroomMC/Cleanroom/releases/latest",
    "Mojang 版本清单": "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
    "Mojang 资源下载": "https://resources.download.minecraft.net/",
    "BMCLAPI 版本清单": "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json",
    "Sponge maven": "https://repo.spongepowered.org/maven/",
    "Maven Central": "https://repo1.maven.org/maven2/",
    "authlib-injector": "https://authlib-injector.yushi.moe/artifact/latest.json",
}


def github_segments() -> list[str]:
    try:
        req = urllib.request.Request(
            "https://api.github.com/meta",
            headers={"User-Agent": "AuroraLauncher-network-check"},
        )
        with urllib.request.urlopen(req, timeout=15) as resp:
            meta = json.load(resp)
        segments = [s for s in meta.get("web", []) + meta.get("api", []) if ":" not in s]
        return segments or GITHUB_FALLBACK_SEGMENTS
    except Exception:
        print("[github] api.github.com 不可达/限流，使用内置官方段")
        return GITHUB_FALLBACK_SEGMENTS


def sample_ips(segments: list[str], per_segment: int = 4) -> list[str]:
    ips: list[str] = []
    for seg in segments:
        if seg.endswith("/32"):
            ips.append(seg.split("/")[0])
            continue
        parts = seg.split("/")
        if parts[0].count(".") != 3:
            continue
        try:
            prefix, bits = parts[0], int(parts[1])
        except ValueError:
            continue
        if bits < 20:
            continue
        base = prefix.rsplit(".", 1)[0]
        ips.extend(f"{base}.{last}" for last in (3, 4, 9, 10)[:per_segment])
    return list(dict.fromkeys(ips))


def tcp_probe(ip: str, timeout: float = 1.5) -> str | None:
    try:
        with socket.create_connection((ip, 443), timeout=timeout):
            return ip
    except OSError:
        return None


def scan_github() -> list[str]:
    segments = github_segments()
    ips = sample_ips(segments)
    print(f"[github] 官方段 {len(segments)} 个，抽样 {len(ips)} 个 IP 测 TCP 443 …")
    with concurrent.futures.ThreadPoolExecutor(max_workers=24) as pool:
        alive = [ip for ip in pool.map(tcp_probe, ips) if ip]
    print(f"[github] 可用 {len(alive)}/{len(ips)}：{' '.join(alive[:8])}{' …' if len(alive) > 8 else ''}")
    return alive


def check_endpoint(name: str, url: str) -> tuple[str, str]:
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "AuroraLauncher/0.6"})
        ctx = ssl.create_default_context()
        start = time.monotonic()
        with urllib.request.urlopen(req, timeout=15, context=ctx) as resp:
            status = resp.status
            ms = int((time.monotonic() - start) * 1000)
            return name, f"HTTP {status} ({ms} ms)"
    except urllib.error.HTTPError as e:
        return name, f"HTTP {e.code}（可达但返回错误）"
    except Exception as e:
        return name, f"FAIL {type(e).__name__}: {e}"


def main() -> int:
    alive = scan_github()
    if alive:
        print("[github] 备用方案：把可用 IP 写入 hosts（scripts/github-ip-fallback.ps1 -WriteHosts）")
        if "--hosts" in sys.argv:
            print(f"建议 hosts 行：{alive[0]} github.com")

    print()
    print("[endpoints] 加载器/游戏下载端点检查：")
    failures = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        for name, result in pool.map(lambda item: check_endpoint(*item), ENDPOINTS.items()):
            mark = "OK " if not result.startswith("FAIL") else ("WARN" if "HTTPError" not in result else "WARN")
            if result.startswith("FAIL"):
                failures += 1
            print(f"  [{mark}] {name:<22} {result}")

    print()
    if failures:
        print(f"结论：{failures} 个端点不可达——对应加载器安装会失败，启动器的 BMCLAPI/镜像候选可部分兜底。")
    else:
        print("结论：所有端点可达；安装失败若复现请看启动器日志里的具体阶段。")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
