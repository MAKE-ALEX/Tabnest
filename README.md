# Tabnest · 个人书签面板

Tabnest 是一个自托管的 **Papaly 风格书签起始页**：用「面板 → 分组 → 书签卡片」整理网址，支持拖拽排序、跨分组移动、全文搜索、浏览器书签导入与导出。为个人使用设计，没有分享、协作和多用户功能。

- 后端：Rust + Axum + SQLx + SQLite（WAL）
- 前端：原生 HTML / CSS / JavaScript，无构建步骤
- 部署：单二进制或 Docker Compose，数据落在宿主机 `./data` 目录
- 访问保护：可选的单密码登录（`APP_PASSWORD`）

## 界面

| 浅色 | 深色 |
| --- | --- |
| ![浅色主题](docs/screenshot-light.png) | ![深色主题](docs/screenshot-dark.png) |

---

## 功能

| 功能 | 说明 |
| --- | --- |
| 面板 / 分组 / 书签 | 侧栏切换面板，每个面板可建多个分组，书签卡片可拖拽调整顺序 |
| 拖拽 | 拖动卡片跨分组移动、拖动分组头横向排序、拖动侧栏面板排序 |
| 添加书签 | 粘贴网址自动抓取网页标题和 favicon；也可手填标题、备注 |
| 搜索 | 标题、网址、备注、分组名全文过滤，结果按面板分组 |
| 外观 | 明 / 暗主题，跟随系统或手动切换；响应式，手机可用 |
| 导入导出 | 导入 Tabnest JSON 或 Chrome/Edge/Firefox 导出的 HTML；导出 JSON（备份）或 HTML（可再导入浏览器） |
| 图标缓存 | favicon 由服务端代理下载并缓存在 `data/favicons/`，不依赖第三方图标服务 |
| 可选密码 | 设置 `APP_PASSWORD` 后需要登录；会话保存在内存中，重启后重新登录 |
| 快捷键 | `/` 或 `Ctrl/⌘ + K` 聚焦搜索，`n` 新建书签，`Esc` 关闭 / 清空搜索 |

## 快速开始（Docker Compose）

```bash
cd /opt/code
cp .env.example .env      # 按需修改 PORT / APP_PASSWORD
docker compose up -d --build
```

打开 `http://<服务器地址>:8420`（当前这台机器上是 `http://192.168.20.110:8420`）。

默认配置：

- 端口：宿主机 `8420` → 容器 `8080`
- 数据：`./data/tabnest.db`（SQLite）、`./data/favicons/`
- 首次启动写入一块示例面板（`SEED_DEMO=false` 可关闭）
- 未设置密码时，任何能访问端口的人都可以读写书签

停止 / 更新：

```bash
docker compose down
docker compose up -d --build
```

### 环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `PORT` | `8420` | Compose 映射到宿主机的端口 |
| `APP_PASSWORD` | 空 | 非空时启用单密码登录 |
| `SEED_DEMO` | `true` | 空数据库首次启动时写入示例内容 |
| `BIND_ADDR` | `0.0.0.0:8080` | 容器内监听地址 |
| `DATA_DIR` | `/data` | 数据库与图标缓存目录 |
| `DB_PATH` | `$DATA_DIR/tabnest.db` | 单独指定数据库路径 |
| `RUST_LOG` | `tabnest=info,sqlx=warn` | 日志级别 |
| `CARGO_REGISTRY_MIRROR` | 空 | 构建时使用的 crates.io 镜像，如 `https://rsproxy.cn/index/`（仅 Docker 构建） |
| `DEBIAN_MIRROR` | 空 | 构建时替换的 Debian 镜像域名，如 `mirrors.aliyun.com`（仅 Docker 构建） |
| `TZ` | `Asia/Shanghai` | 时区 |

> 如果直接运行二进制，需要在 `.env` 中确认 `DATA_DIR` 与 `DB_PATH` 指向可写目录。
> 修改 `APP_PASSWORD` / `PORT` 后执行 `docker compose up -d` 让容器重建生效。

## 不用 Docker

```bash
cargo build --release
DATA_DIR=./data BIND_ADDR=127.0.0.1:8420 ./target/release/tabnest
```

需要 Rust 1.85+（已在 1.92 上验证），Linux 下需要 OpenSSL 开发包（`libssl-dev` / `openssl-devel`）。

运行测试：

```bash
cargo test
```

## 数据与备份

所有内容都在一个 SQLite 文件里（`data/tabnest.db`）。三种备份方式：

1. 侧栏底部「数据 → 导出 JSON」，适合迁移和增量备份；
2. `docker compose stop && cp data/tabnest.db backup/`；
3. 使用 `sqlite3 data/tabnest.db ".backup 'backup.db'"`（WAL 模式下推荐）。

恢复：停止容器，用备份文件替换 `data/tabnest.db`，再启动。

## 导入浏览器书签

在 Chrome / Edge 中打开 `chrome://bookmarks` → 右上角菜单 → 导出书签，得到 HTML 文件；在 Tabnest 里点侧栏底部「数据 → 导入 → 选择文件」，或直接把文件拖进对话框。

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
src/fetch.rs       网址规范化、网页标题 / favicon 抓取、HTML 实体解码
src/import.rs      Netscape 书签 HTML 解析
src/auth.rs        可选密码、Cookie 会话
src/assets.rs      内嵌前端资源与健康检查
static/            原生前端
migrations/        SQLite schema
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

## 安全说明

- 不设 `APP_PASSWORD` 时**没有鉴权**，请只在可信内网或反向代理认证之后暴露端口。
- 会话 Cookie 为 `HttpOnly; SameSite=Lax`，未设置 `Secure`，因此可以直接走 HTTP；放到公网时建议用 Caddy / Nginx 加 HTTPS。
- 添加书签时服务端会请求目标网址以抓取标题和图标。应用默认面向私人内网，未限制访问内网地址；如果暴露到公网，请注意 SSRF 风险。
- 容器以非 root 用户 `tabnest`（UID 10001）运行；入口脚本只用 root 修正 `./data` 权限，随后立即降权。

## License

MIT
