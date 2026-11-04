<p align="center">
  <a href="README.md">English</a> · <b>简体中文</b>
</p>

<p align="center">
  <img src="assets/social/social-preview.zh-CN.png" alt="Weave Office：一个应用打开所有文档，AI 可以直接调用" width="100%">
</p>

<h1 align="center">Weave Office</h1>

<p align="center">
  <b>一个应用，打开所有文档；AI 可以直接调用。</b><br>
  用 Rust 写的开源桌面办公应用：文字、表格、演示、PDF、Markdown、代码与富文本，<br>
  在同一条文件标签栏里打开、编辑、保存。同一个程序也是命令行工具，AI 代理可以直接调用它来读写文档。
</p>

<p align="center">
  <img alt="Rust 1.95" src="https://img.shields.io/badge/Rust-1.95-orange?logo=rust">
  <img alt="平台 macOS" src="https://img.shields.io/badge/%E5%B9%B3%E5%8F%B0-macOS-lightgrey?logo=apple">
  <img alt="UI egui" src="https://img.shields.io/badge/UI-egui-4b8bbe">
  <img alt="AI 友好的命令行" src="https://img.shields.io/badge/AI%20%E5%8F%8B%E5%A5%BD-CLI%20%2B%20JSON-7c5cff">
  <a href="LICENSE"><img alt="许可证 MIT" src="https://img.shields.io/badge/%E8%AE%B8%E5%8F%AF%E8%AF%81-MIT-blue"></a>
  <img alt="状态 开发中" src="https://img.shields.io/badge/%E7%8A%B6%E6%80%81-%E5%BC%80%E5%8F%91%E4%B8%AD-yellow">
</p>

<p align="center">
  <a href="#功能">功能</a> ·
  <a href="#命令行给-ai-和脚本用">命令行</a> ·
  <a href="#从源码构建">构建</a> ·
  <a href="#项目结构">项目结构</a> ·
  <a href="#文档">文档</a>
</p>

> [!NOTE]
> 项目仍在积极开发中，还不是可以日常替代成熟办公套件的版本。复杂文档的格式保真、输入法和部分高级流程仍在打磨，当前进度和已知问题见[验收记录](docs/acceptance.md)。

## 为什么做 Weave Office

两个核心特点：**多合一**和 **AI 友好**。

我们每天在 Word、Excel、PowerPoint、PDF 阅读器、Markdown 笔记和代码编辑器之间来回切换，每个应用都有自己的窗口、菜单、设置和保存逻辑。Weave Office 想把它们收进**一个真正的应用**：

- **一条文件标签栏**：合同、报价表、路演稿、发票 PDF、笔记和配置文件可以混排在同一行，同类文件也能开多个；切走再切回，光标、选区、滚动、缩放、撤销历史和未保存内容都还在。
- **一套公共系统**：新建、打开、保存、另存为、关闭、退出、最近文件、自动保存、打印和弹窗都由同一个宿主管理，而不是四个 App 各管各的。
- **AI 可以直接调用**：同一个程序就是命令行工具，AI 代理不用打开窗口，按路径读取、修改、导出文档，结果以 JSON 返回（见[命令行](#命令行给-ai-和脚本用)）。
- **共用底层**：字体、整形、光栅化、PDF 输出、图元文件解析等底层能力只保留一份，安装包更小，各编辑器表现也更一致。

## 功能

### 文档格式一览

| 类型 | 打开 | 新建时保存为 | 编辑器 |
|---|---|---|---|
| 文字 | `.docx` `.docm` `.doc` `.odt` `.rtf` | `.docx` | 基于 WordCraft 引擎 |
| 表格 | `.xlsx` `.xlsm` `.xls` `.ods` `.csv` | `.xlsx` | 基于 GridCraft 引擎 |
| 演示 | `.pptx` `.pptm` `.ppt` `.odp` | `.pptx` | 基于 DeckCraft 引擎 |
| PDF | `.pdf` | `.pdf` | 基于 PdfCraft 引擎 |
| Markdown | `.md` `.markdown` `.mdx` 等 | `.md` | 自研（`crates/text-editor`） |
| 纯文本与代码 | 20 多种语言，以及内容是文本的未知扩展名 | `.txt` `.json` `.yaml` `.toml` `.xml` `.sql` `.sh` `.py` `.js` `.ts` `.css` `.go` `.rs` | 自研（`crates/text-editor`） |
| 富文本（HTML） | `.html` `.htm` `.xhtml` | `.html` | 自研（`crates/rich-editor`），开发中 |

点击标签栏的 **＋** 打开新建面板，分三组列出所有可新建的格式：「Office 文档」（文字、演示、表格、PDF）、「文本与网页」（Markdown、纯文本、富文本）和「代码与数据」（JSON、YAML、TOML、XML、SQL、Shell、Python、JavaScript、TypeScript、CSS、Go、Rust）。面板里的每种格式都能编辑，并按原格式保存。

### 四合一办公：文字、表格、演示、PDF

- **文字**：段落与字符格式、样式、表格、图片、页眉页脚、分页；读取文档里的 WMF/EMF 图片；导出 PDF 与打印。
- **表格**：多工作表、公式、单元格格式、边框与合并；中英文混排按基线对齐；导出 PDF（中文字体会嵌入）与 CSV。
- **演示**：幻灯片、形状、文本框、图片、版式；放映与导出。
- **PDF**：不只是阅读器。可以直接改已有正文和图片、添加文字与图片、批注（便笺、文本框、标记、形状和回复）、填写文字，以及插入、删除、重排、旋转、裁切、提取和拆分页面。能力边界见 [PDF 编辑能力](docs/pdf-capabilities.md)。

四个编辑器整合在同一个进程里：

- **统一功能区**：功能区标签行由宿主统一绘制，各编辑器只负责下方的命令，切换文件类型时操作逻辑一致。
- **统一字体**：开放字体只打包一份，系统字体只扫描一次；四个编辑器共用一套界面字体，切换标签页不再重新装载字体。
- **自动保存与崩溃恢复**：宿主定时把未保存的修改写入恢复目录，意外退出后重新启动即可找回。
- **统一文件与弹窗**：打开、保存、文件夹选择等使用操作系统原生面板；确认、错误、设置、打印等由 Weave 自建，并且弹窗打开期间不会误切换或误关标签，外部打开的文件会排队等待。
- **macOS 原生体验**：保留系统的红黄绿窗口按钮、圆角和阴影，标题栏里直接放文件标签；接入应用菜单、快捷键、Finder 打开文件和系统退出流程。

### Markdown：接近 Obsidian 的实时预览

- **实时预览**：标题、粗体、链接等格式就地呈现，只有光标所在行才显示 `#`、`**`、`[]()` 这类标记；Markdown 表格显示边框、列对齐和单元格自动换行，点击表格可编辑源码；另有源码视图和阅读视图。
- **笔记功能**：任务复选框、`[[双链]]`、反向链接、标签、大纲。
- **左侧栏**：文件树、全文搜索、大纲、标签、反向链接。
- **命令面板**（⇧⌘P）和**快速打开**（⌥⌘O）。
- **插入工具**：图片、脚注、标注块（callout）、公式、属性（front matter）。
- **专注模式**和**打字机滚动**。
- 导出 PDF 和打印借用文字引擎的 Markdown 导入。

### 代码与纯文本编辑器

- **语法高亮**：Rust、JavaScript、TypeScript、Python、Go、C/C++、C#、Java、Kotlin、Swift、PHP、Ruby、Lua、Shell、PowerShell、SQL、Dockerfile、Makefile、JSON、YAML、TOML、INI、HTML、CSS 等。
- **编辑工具**：行号、查找替换、在文件夹中搜索、转到行、行操作（移动、复制、删除、合并），排序、去重、大小写转换、清理行尾空格、缩进转换，以及 **JSON 格式化**。
- **保留编码**：按原来的编码（包括 GBK、UTF-16、带 BOM 的文件）和换行符写回；没有改动的文件保存后一个字节都不变。扩展名不认识但内容是文本的文件，也会按纯文本打开。

### 富文本编辑器（开发中）

面向公众号排版这类场景的所见即所得 HTML 编辑器，体验对标 135 编辑器：

- **打开任意 HTML 文件**，在**设计视图**（排好版的页面）和**源码视图**（HTML 源码，带高亮）之间切换，两边始终一致。
- **在设计视图里直接编辑**：点击放置光标、拖动或 Shift 点击选择、双击选词、三击选段，支持输入法；复制同时带 HTML 和纯文本，粘贴保留行内样式并去掉脚本。
- **开始选项卡**：段落样式、字号、粗体、斜体、下划线、删除线、文字颜色和高亮、清除格式、对齐、行距、首行缩进。
- **保真**：不认识的标签、注释、`<head>`、`<script>`、`<style>` 原样保留；没改动的文件原样写回；撤销、保存和源码视图都基于同一份文本。
- 自研 HTML 解析和 CSS 子集排版，不引入新的第三方依赖。
- 路线图：样式库组件、一键换色、图片 → 复制到公众号 → 打磨。

### 命令行：给 AI 和脚本用

**这是 Weave Office 的第二个核心特点：对 AI 友好。** 同一个可执行文件也是命令行工具：第一个参数是动词时不开窗口，直接在四个编辑器的引擎上读写文档。设计目标是让 AI 代理能可靠地操作办公文档。

```sh
weave-office create 报价.xlsx
weave-office set 报价.xlsx /Sheet1/B2 value=1280
weave-office add 合同.docx /body --type paragraph text="甲乙双方……"
weave-office view 路演.pptx outline
weave-office export 合同.docx 合同.pdf
weave-office help agent      # 打印给 AI 读的完整使用说明
```

- 动词：`create` `info` `view` `get` `set` `add` `remove` `commands` `run` `batch` `export` `help`。
- 按路径定位：`/body/p[3]`、`/Sheet1/A1:C10`、`/slide[2]/shape[1]`、`/page[4]`。
- `--json` 输出统一的 `{"ok":…,"data"|"error":…}` 信封，错误带错误码、修改建议和最相近的候选命令。
- `batch` 全部成功才写盘；保存先写临时文件再改名覆盖，文件要么是完整新版本，要么保持原样。

完整说明见 [docs/cli.md](docs/cli.md)。

## 从源码构建

需要 Rust 1.95.0（`rust-toolchain.toml` 已固定）和 Python 3。字体与翻译等大型构建输入放在外置盘上，用环境变量 `WEAVE_DEV_VOLUME` 指向它的挂载点：

```sh
export WEAVE_DEV_VOLUME=/Volumes/MyDrive

python3 scripts/prepare-fonts.py
python3 scripts/prepare-document-fonts.py
python3 scripts/prepare-i18n.py
python3 scripts/cargo.py run --locked --offline
```

构建输出写到 `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/target`。只想试命令行，不需要准备字体：

```sh
cargo run -p weave-cli --example weave -- help
cargo test -p weave-cli
```

打包 macOS `.app` 见[打包说明](packaging/README.md)，字体输入见[字体说明](docs/fonts.md)。

## 项目结构

```
src/                  宿主：窗口、文件标签、功能区、弹窗、自动保存与恢复、编辑器适配
crates/
  workspace/          文件类型、标签、未保存状态与保存顺序
  ribbon/             宿主绘制的统一功能区
  font-assets/        共享的开放字体
  system-fonts/       系统字体只扫描一次
  pdf-backend/        PDF 引擎适配
  text-editor/        Markdown 与代码编辑器
  rich-editor/        富文本（HTML）编辑器
  cli/                命令行 weave-office <动词>
vendor/               四个上游编辑器的固定快照（文字、表格、演示、PDF）
integration/          早期集成实验，仅作参考
packaging/ scripts/   打包与构建脚本
tests/                宿主回归测试
```

## 文档

- [产品目标与实施规划](PLAN.md)
- [验收记录](docs/acceptance.md)：自动测试、实际操作验证与剩余工作
- [命令行](docs/cli.md) · [PDF 编辑能力](docs/pdf-capabilities.md) · [字体](docs/fonts.md) · [界面语言](docs/i18n.md)
- [上游项目、固定版本与接手说明](vendor/README.md)
- [应用图标](assets/app-icon/README.md) · [打包说明](packaging/README.md)

## 致谢与许可

Weave Office 以 [MIT 许可证](LICENSE) 开源。

四种办公文档的编辑能力来自 [WordCraft](https://github.com/storytold/wordcraft)、[GridCraft](https://github.com/storytold/gridcraft)、[DeckCraft](https://github.com/storytold/deckcraft) 和 [PdfCraft](https://github.com/storytold/pdfcraft)，以固定快照形式放在 `vendor/` 下。上游源码以 MIT OR Apache-2.0 授权，各自的 LICENSE、NOTICE 和素材署名保留在对应目录中；Weave Office 使用自己的应用名称和图标。命令行的设计思路参考了 [OfficeCLI](https://github.com/iOfficeAI/OfficeCLI)，不包含其代码。

感谢 [LINUX DO](https://linux.do) 社区的支持与反馈。
