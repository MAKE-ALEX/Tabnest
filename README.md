# Tabnest · 个人书签面板

Tabnest 是一个自托管的 **Papaly 风格书签起始页**：用「面板 → 分组 → 书签卡片」整理网址，支持拖拽排序、跨分组移动、全文搜索、浏览器书签导入与导出。为个人使用设计，没有分享、协作和多用户功能。

- 后端：Rust + Axum + SQLx + SQLite（WAL）
- 前端：原生 HTML / CSS / JavaScript，无构建步骤
- 部署：单二进制或 Docker Compose，数据落在宿主机 `./data` 目录
- 访问保护：单密码登录（默认强制；仅回环监听或显式 `ALLOW_INSECURE=true` 可免密）

## 界面

| 浅色 | 深色 |
| --- | --- |
| ![浅色主题](docs/screenshot-light.png) | ![深色主题](docs/screenshot-dark.png) |

---

## 功能

| 功能 | 说明 |
| --- | --- |
| 面板 / 分组 / 书签 | 侧栏切换面板，每个面板可建多个分组，书签卡片可拖拽调整顺序 |
| 布局 | Papaly 风格壁纸、左侧图标栏、固定列瀑布流，分组长短不一会保留在各自列内，可设置 2–8 列或自动 |
| 拖拽 | 拖动卡片跨分组移动、拖动分组标题排序、拖动侧栏面板排序 |
| 添加书签 | 粘贴网址自动抓取网页标题和 favicon；也可手填标题、备注 |
| 搜索 | 标题、网址、备注、分组名全文过滤，结果按面板分组 |
| 外观 | 6 套配色（陶土 / 苔绿 / 海蓝 / 紫藤 / 樱粉 / 琥珀）× 浅色 / 深色 / 跟随系统 |
| 导入导出 | 导入 Tabnest JSON 或 Chrome/Edge/Firefox 导出的 HTML；导出 JSON（备份）或 HTML（可再导入浏览器） |
| 图标缓存 | favicon 由服务端代理下载并缓存在 `data/favicons/`，不依赖第三方图标服务 |
| 访问保护 | 单密码登录；非回环监听且未设密码时拒绝启动；会话 7 天滑动过期 |
| 快捷键 | `/` 或 `Ctrl/⌘ + K` 聚焦搜索，`n` 新建书签，`Esc` 关闭 / 清空搜索 |

右上角「主题」图标可以切换 6 套配色和明暗模式，设置只保存在当前浏览器。

![外观与配色](docs/appearance.png)

## 前置条件

- Docker Engine 24+，Docker Compose v2（BuildKit 默认开启）
- 宿主机能访问 Docker Hub、crates.io；国内网络可设置 `CARGO_REGISTRY_MIRROR` / `DEBIAN_MIRROR`
- 首次构建约 3–10 分钟；如果卡在 `apt-get update` 或 Cargo 下载，优先在 `.env` 中启用镜像（见 `.env.example` 末尾）
- `8420` 端口可用，或修改 `PORT`
- 支持 Linux x86_64 / arm64；Windows / macOS 建议用 Docker Desktop 做开发验证

## 快速开始（Docker Compose）

```bash
git clone <你的仓库地址> Tabnest
cd Tabnest

cp .env.example .env
chmod 600 .env
# 编辑 .env，至少设置：
#   APP_PASSWORD=你的访问密码
# 公网 HTTPS 还要设置：
#   BIND_HOST=127.0.0.1
#   COOKIE_SECURE=true
#   TRUSTED_PROXIES=...（见下方反向代理说明）

docker compose config
docker compose up -d --build
docker compose ps
curl -fsS http://127.0.0.1:8420/healthz
```

局域网 HTTP 访问：`http://<服务器IP>:8420`。

如果设置了 `COOKIE_SECURE=true`，必须通过 HTTPS 域名访问，否则浏览器会拒绝保存会话 Cookie，登录后会再次显示登录页。

默认配置：

- 端口：宿主机 `8420` → 容器 `8080`，默认绑定 `0.0.0.0`
- 数据：`./data/tabnest.db`（SQLite）、`./data/favicons/`
- 首次启动写入一块示例面板（`SEED_DEMO=false` 可关闭）
- **同一个 `./data` 只能运行一个 Tabnest 容器**；SQLite 不适合多副本 / 多容器同时写
- **未设置密码且监听非回环地址时，程序会拒绝启动**；如需无鉴权运行必须显式设置 `ALLOW_INSECURE=true`
- `.env` 不会进入 Git / 镜像；不要把密码提交到仓库

### 通过 HTTPS / 反向代理访问

推荐只让反向代理访问 Tabnest，不要把 `8420` 直接暴露到公网。

同机 Caddy 示例：

```caddyfile
tabnest.example.com {
    reverse_proxy 127.0.0.1:8420
}
```

`.env` 对应配置：

```dotenv
BIND_HOST=127.0.0.1
COOKIE_SECURE=true
# 根据实际代理来源调整；Docker 端口映射常见网桥为 172.16.0.0/12
TRUSTED_PROXIES=127.0.0.1/32,172.16.0.0/12
```

如果 Caddy 也在 Docker 网络中，反向代理目标写 `tabnest:8080`，`TRUSTED_PROXIES` 填该 Docker 网络的实际 CIDR。

Nginx 反向代理时至少需要：

```nginx
client_max_body_size 25m;
proxy_set_header Host $host;
proxy_set_header X-Forwarded-Proto $scheme;
proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
proxy_pass http://127.0.0.1:8420;
```

否则大体积 JSON / HTML 导入可能被 Nginx 默认的 1MB 限制拦截。

Cloudflare Tunnel：

- 设置 `COOKIE_SECURE=true`
- `TRUSTED_PROXIES` 填 cloudflared 容器所在网段
- 源站端口用防火墙限制，只允许 cloudflared / 反向代理访问
- 不要设置旧的 `TRUST_PROXY`，除非你明确知道它在信任任意对端

> 登录后仍回到登录页，先检查地址栏是不是 HTTPS，以及 `.env` 是否设置了 `COOKIE_SECURE=true`。HTTP + Secure Cookie 一定会失败。

停止 / 更新：

```bash
# 升级前先备份 data/tabnest.db
docker compose down
git pull
docker compose up -d --build
docker compose ps
```

数据库迁移会在启动时自动执行；升级不要删除 `./data`。

### 环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `BIND_HOST` | `0.0.0.0` | 宿主机监听地址；同机反向代理建议 `127.0.0.1`，不要将端口直接暴露公网 |
| `PORT` | `8420` | Compose 映射到宿主机的端口 |
| `CONTAINER_NAME` | `tabnest` | Compose 容器名；同机运行多个实例时改掉 |
| `IMAGE_NAME` | `tabnest:latest` | Compose 构建 / 使用的镜像名 |
| `APP_PASSWORD` | 空 | 非空时启用单密码登录；为空且监听非回环地址时拒绝启动 |
| `ALLOW_INSECURE` | `false` | 明确允许无密码运行（危险） |
| `ALLOW_PRIVATE_NETWORK` | `false` | 允许服务端抓取内网 / 本机地址（SSRF 风险，默认关闭） |
| `COOKIE_SECURE` | `false` | 会话 Cookie 加 `Secure` 并使用 `__Host-` 前缀；HTTPS 反向代理后开启 |
| `TRUST_PROXY` | `false` | 旧兼容开关：信任任意对端的转发头；建议改用 `TRUSTED_PROXIES` |
| `TRUSTED_PROXIES` | 空 | 可信代理 CIDR 列表（逗号分隔），如 `127.0.0.1/32,172.16.0.0/12`；仅这些网段可提供 `CF-Connecting-IP` / XFF |
| `SEED_DEMO` | `true` | 空数据库首次启动时写入示例内容 |
| `BIND_ADDR` | `0.0.0.0:8080` | 容器内监听地址（一般不用改） |
| `DATA_DIR` | `/data` | 数据库与图标缓存目录（容器内路径） |
| `DB_PATH` | `$DATA_DIR/tabnest.db` | 单独指定数据库路径；仅直接运行二进制时通常需要 |
| `RUST_LOG` | `tabnest=info,sqlx=warn` | 日志级别 |
| `CARGO_REGISTRY_MIRROR` | 空 | 构建时使用的 crates.io 镜像，如 `https://rsproxy.cn/index/`（仅 Docker 构建） |
| `DEBIAN_MIRROR` | 空 | 构建时替换的 Debian 镜像域名，如 `mirrors.aliyun.com`（仅 Docker 构建） |
| `TZ` | `Asia/Shanghai` | 时区；容器内已安装 tzdata |

> `DATA_DIR` / `DB_PATH` 是容器内路径，不是宿主机路径。宿主机数据目录由 Compose 的 `./data:/data` 决定。
> 密码包含 `#`、`$`、空格等字符时用单引号包住，例如 `APP_PASSWORD='p@ss#...'`。
> 修改 `APP_PASSWORD`、`PORT`、`BIND_HOST` 后执行 `docker compose up -d` 让容器重建生效。

## 不用 Docker

```bash
cargo build --release
APP_PASSWORD='你的密码' DATA_DIR=./data BIND_ADDR=127.0.0.1:8420 ./target/release/tabnest
```

需要 Rust 1.85+（已在 1.92 上验证），Linux 下需要 OpenSSL 开发包（`libssl-dev` / `openssl-devel`）。

运行测试：

```bash
cargo test
```

## 部署到 Cloudflare Workers

同一份代码也能跑在 Cloudflare Workers 上：`worker/` 是 Workers 入口，`crates/core` 放服务端和边缘端共用的纯逻辑（网址规范化、标题/图标解析、书签导入、签名 Cookie）。

**与自托管版的差异**

| 项目 | 自托管（Docker / 二进制） | Cloudflare Workers |
| --- | --- | --- |
| 存储 | 本地 SQLite 文件 | Cloudflare D1 |
| 登录会话 | 进程内 Map | HMAC 签名 Cookie（`SESSION_SECRET`） |
| 登录限速 | 全局精确 | 每个 isolate 内存，多实例时是近似值 |
| favicon | 服务端磁盘缓存 | 仅浏览器 / CDN 缓存 |
| 抓取网页元数据 | `reqwest` + DNS 钉扎防 SSRF | `fetch` + 字面量内网地址拦截（无法钉扎 DNS） |
| 静态资源 | `include_str!` 编进二进制 | Cloudflare Static Assets，`static/_headers` 提供 CSP |

**步骤**

```bash
cd worker
npm install

# 1. 建库，把返回的 database_id 填进 wrangler.toml
npx wrangler d1 create tabnest

# 2. 配置密码与会话密钥（不要写进 wrangler.toml）
npx wrangler secret put APP_PASSWORD
npx wrangler secret put SESSION_SECRET   # 可省略，省略时由密码派生

# 3. 部署
npm run deploy
```

首次请求会自动执行 `worker/schema.sql`（`CREATE TABLE IF NOT EXISTS`）并按 `SEED_DEMO` 写入示例面板，不需要手动迁移。

本地调试：

```bash
cd worker
cp .dev.vars.example .dev.vars
npm run dev            # http://localhost:8787，D1 由 Miniflare 本地模拟
```

备份与恢复：

```bash
cd worker
npm run db:export      # 导出 tabnest-backup.sql
```

> Workers 免费额度是每天 10 万次请求、D1 免费 5 GB；favicon 代理与元数据抓取都算子请求。未设置 `APP_PASSWORD` 时任何人都能读写，请务必配置。
>
> Workers 版的会话是无状态签名 Cookie：退出登录只会清掉浏览器里的 Cookie，已泄露的令牌在 7 天有效期内仍然可用（自托管版退出即服务端失效）。共用设备上建议缩短 `SESSION_TTL_SECS` 或更换 `SESSION_SECRET` 强制全员下线。

## 数据与备份

所有内容都在一个 SQLite 文件里（`data/tabnest.db`）。三种备份方式：

1. 右上角「数据」图标 → 导出 JSON，适合迁移和增量备份；
2. `mkdir -p backup && docker compose stop && cp data/tabnest.db backup/`；
3. 使用 `sqlite3 data/tabnest.db ".backup 'backup.db'"`（WAL 模式下推荐，可在运行中执行）。

恢复：

```bash
docker compose stop
cp /path/to/backup.db data/tabnest.db
rm -f data/tabnest.db-wal data/tabnest.db-shm
docker compose start
```

如果只迁移 `tabnest.db`，记得目标机器上的 UID 10001 需要有写权限；入口脚本会尝试自动修正 `./data` 权限。favicon 缓存可以不迁移，缺失的图标会按需重新抓取。

## 导入浏览器书签

在 Chrome / Edge 中打开 `chrome://bookmarks` → 右上角菜单 → 导出书签，得到 HTML 文件；在 Tabnest 里点右上角「数据」图标 → 导入 → 选择文件，或直接把文件拖进对话框。

映射规则：

- 一级文件夹 → 面板
- 二级及更深文件夹 → 分组（更深层级用 ` / ` 连接）
- 文件夹里的直接书签 → 该文件夹对应面板的「全部」分组
- 重复网址（已存在数据库中的）自动跳过

## 开发

```bash
cargo check
cargo test
cargo run
```

前端没有打包工具：修改 `static/index.html`、`static/style.css`、`static/app.js` 后重启二进制即可（资源通过 `include_str!` 编译进程序，改完需要重新编译）。

### 目录结构

```
src/main.rs        启动、日志、优雅退出
src/api.rs         JSON API、排序、导入导出接口
src/db.rs          SQLite 连接、建表、示例数据、批量导入
src/fetch.rs       网页标题 / favicon 抓取（解析逻辑在 crates/core）
src/auth.rs        可选密码、Cookie 会话
src/assets.rs      内嵌前端资源与健康检查
crates/core/       服务端与 Workers 共用的纯逻辑（URL、HTML 解析、导入、签名 Cookie）
worker/            Cloudflare Workers 入口（D1 + Static Assets）
static/            原生前端
migrations/        SQLite schema（自托管）
```

### API 摘要（均需登录，若启用密码）

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/state` | 一次返回全部面板 / 分组 / 书签 |
| POST | `/api/boards` | 新建面板 `{name}` |
| PATCH/DELETE | `/api/boards/{id}` | 重命名 / 删除面板 |
| PUT | `/api/boards/order` | 面板排序 `{ids}` |
| POST | `/api/columns` | 新建分组 `{board_id, name}` |
| PATCH/DELETE | `/api/columns/{id}` | 重命名 / 删除分组 |
| PUT | `/api/columns/order` | 分组排序 `{board_id, ids}` |
| POST | `/api/links` | 新建书签 `{column_id, url, title?, note?, icon?}` |
| PATCH/DELETE | `/api/links/{id}` | 编辑 / 删除书签 |
| PUT | `/api/links/arrange` | 跨分组 / 组内排序 `{columns:[{column_id, link_ids}]}` |
| GET | `/api/meta?url=` | 抓取标题与图标 |
| GET | `/api/favicon?u=` | favicon 代理（磁盘缓存） |
| POST | `/api/import/json`、`/api/import/html` | 导入（原始文本 body） |
| GET | `/api/export/json`、`/api/export/html` | 导出下载 |
| POST | `/api/login`、`/api/logout` | 登录 / 退出 |

## 常见问题

### 容器启动后 unhealthy / 一直重启

```bash
docker compose ps
docker compose logs --tail=100 tabnest
```

常见原因：

- 没有设置 `APP_PASSWORD`，且 `BIND_ADDR` 不是回环地址：程序会拒绝启动
- `data/` 目录权限不足：`sudo chown -R 10001:10001 data`
- `PORT` 被占用：修改 `.env` 的 `PORT` 或 `BIND_HOST`
- 构建阶段无法访问 Docker Hub / crates.io：配置 `.env` 里的镜像变量后重新构建

### 登录后仍回到登录页

- 地址栏必须是 `https://`，且 `.env` 设置 `COOKIE_SECURE=true`；HTTP 下 Secure Cookie 会被浏览器拒绝
- 清理该站点的 Cookie 后重试
- 确认没有同时打开多个旧版本标签页

### 页面样式没有更新

静态资源由二进制内嵌，更新后需要重新构建镜像：

```bash
docker compose up -d --build
```

浏览器仍显示旧页面时按 `Ctrl+Shift+R` 强制刷新；也可以临时访问 `/g` 入口绕过旧页面缓存。

### 反向代理后所有用户共用一个登录限速额度

把实际代理来源网段加入 `TRUSTED_PROXIES`，例如：

```dotenv
TRUSTED_PROXIES=127.0.0.1/32,172.16.0.0/12
```

不要为了省事设置 `TRUST_PROXY=true`，它信任任意对端的转发头。

### 如何完全重置数据

确认不再需要旧数据后：

```bash
docker compose down
rm -rf data
docker compose up -d
```

这会删除数据库和 favicon 缓存，重新生成示例数据（如果 `SEED_DEMO=true`）。

## 安全说明

- **默认拒绝无密码暴露**：`APP_PASSWORD` 为空且 `BIND_ADDR` 不是回环地址时，程序启动即报错。只有显式设置 `ALLOW_INSECURE=true` 才会继续。
- **SSRF 防护**：`/api/meta`、`/api/favicon`、新建书签的自动抓取都会解析域名并检查 IP，默认拒绝回环、私网、链路本地、CGNAT、组播等地址；重定向逐跳校验，DNS 结果固定到已校验的 IP，避免 DNS rebinding。确需收藏内网地址时设置 `ALLOW_PRIVATE_NETWORK=true`。
- **favicon 只接受位图**：PNG / JPEG / GIF / WebP / ICO；SVG 被拒绝，响应带 `nosniff`、`Content-Security-Policy: default-src 'none'; sandbox`，避免同源 SVG XSS。
- **登录限速**：同一 IP 10 分钟内失败 5 次后锁定 30 秒，继续失败按 60 秒 / 2 分钟 / … / 15 分钟递增；返回 429 + `Retry-After`。位于 Cloudflare / 反向代理后时，通过可信代理网段读取 `CF-Connecting-IP`（XFF 只取最右侧非代理跳），避免所有访客共用一个边缘 IP 的额度。密码比较先做 SHA-256 再恒定时间比较，不泄露长度。
- **会话**：7 天滑动过期，Cookie 为 `HttpOnly; SameSite=Lax`；启用 `COOKIE_SECURE=true` 后使用 `__Host-tabnest_session`（强制 Secure、Path=/、无 Domain）。
- **CSRF**：所有写接口校验 `Origin` 与 `Sec-Fetch-Site`；跨站或同站不同端口（例如 `:3000` → `:8420`）的写请求返回 403。
- **安全响应头**：全局 CSP（脚本与样式均仅允许同源，主题引导已拆到 `/theme-init.js`）、`X-Frame-Options: DENY`、`Referrer-Policy: no-referrer`、`Permissions-Policy` 等；HTTPS 请求额外发送 HSTS。
- **缓存语义**：`/api/*` JSON 响应统一 `Cache-Control: no-store`（favicon 除外）；favicon 使用 `private` + `Vary: Cookie`。favicon 磁盘缓存最多保留 1000 个文件，超出后按修改时间淘汰。
- **部署建议**：公网或不可信网络请用 Caddy / Nginx 或 Cloudflare Tunnel 终止 TLS 并转发到 `8420`，设置 `COOKIE_SECURE=true`，并用 `TRUSTED_PROXIES` 限定可信代理网段（例如 Cloudflare Tunnel 所在 Docker 网络），不要直接打开旧的 `TRUST_PROXY`；修改 `APP_PASSWORD` 或重启容器后旧会话会失效（会话只存在内存中）。源站端口应只允许 Cloudflare / 反向代理访问，避免绕过 WAF 直接打 IP。
- 容器以非 root 用户 `tabnest`（UID 10001）运行；入口脚本只用 root 修正 `./data` 权限，随后立即降权。
- 依赖库（tokio / sqlx / openssl 等）内部仍会使用 `unsafe` 与 C 库交互；应用代码通过 `#![forbid(unsafe_code)]` 禁止手写 `unsafe`。

## License

MIT
